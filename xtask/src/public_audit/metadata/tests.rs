use super::{allowlist::Allowlist, metadata_hash, scan_commit, split_commit};

const COMMIT: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
const OTHER_COMMIT: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
const TERM: &str = "private-identity";

fn commit(message: &str) -> Vec<u8> {
    format!("tree {OTHER_COMMIT}\nauthor private-identity <fixture@example.invalid> 1 +0000\ncommitter Example <fixture@example.invalid> 2 +0000\n\n{message}\n").into_bytes()
}

fn allowed(bytes: &[u8]) -> anyhow::Result<Allowlist> {
    let hash = metadata_hash(&split_commit(bytes)?.metadata);
    let document = serde_json::json!({"schema_version":1,"entries":[{
        "commit_sha":COMMIT,"metadata_sha256":hash}]});
    Allowlist::parse(&serde_json::to_vec(&document)?)
}

#[test]
fn no_exception_denies_identity_and_exact_exception_only_allows_metadata() -> anyhow::Result<()> {
    let bytes = commit("Public message");
    let mut findings = Vec::new();
    scan_commit(COMMIT, &bytes, &mut Allowlist::default(), &[TERM.into()], &mut findings)?;
    assert_eq!(findings.len(), 1);
    assert!(findings.first().is_some_and(|value| value.contains("author/committer metadata")));
    findings.clear();
    let mut allowlist = allowed(&bytes)?;
    scan_commit(COMMIT, &bytes, &mut allowlist, &[TERM.into()], &mut findings)?;
    assert!(findings.is_empty());
    allowlist.finish()?;
    Ok(())
}

#[test]
fn different_commit_or_changed_metadata_fails_closed() -> anyhow::Result<()> {
    let bytes = commit("Public message");
    let mut findings = Vec::new();
    let mut allowlist = allowed(&bytes)?;
    scan_commit(OTHER_COMMIT, &bytes, &mut allowlist, &[TERM.into()], &mut findings)?;
    assert_eq!(findings.len(), 1);
    assert!(allowlist.finish().is_err());
    let changed = String::from_utf8(bytes.clone())?.replace("2 +0000", "3 +0100");
    assert!(
        scan_commit(
            COMMIT,
            changed.as_bytes(),
            &mut allowed(&bytes)?,
            &[TERM.into()],
            &mut Vec::new()
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn messages_cannot_spoof_identity_headers_or_inherit_the_exception() -> anyhow::Result<()> {
    let original = commit("Public message");
    let spoofed = commit("author private-identity\ncommitter private-identity\n\nMore content");
    assert_eq!(split_commit(&original)?.metadata, split_commit(&spoofed)?.metadata);
    let mut findings = Vec::new();
    scan_commit(COMMIT, &spoofed, &mut allowed(&original)?, &[TERM.into()], &mut findings)?;
    assert_eq!(findings.len(), 1);
    assert!(findings.first().is_some_and(|value| value.contains("contents")));
    Ok(())
}

#[test]
fn non_identity_headers_remain_scanned_and_ambiguous_headers_are_rejected() -> anyhow::Result<()> {
    let bytes = commit("Public message");
    let input = String::from_utf8(bytes.clone())?;
    let signed = input.replace("\n\n", "\ngpgsig private-identity\n continuation\n\n");
    let mut findings = Vec::new();
    scan_commit(COMMIT, signed.as_bytes(), &mut allowed(&bytes)?, &[TERM.into()], &mut findings)?;
    assert_eq!(findings.len(), 1);
    for extra in ["author other", "committer other", " continuation"] {
        let malformed = input.replace("\n\n", &format!("\n{extra}\n\n"));
        assert!(split_commit(malformed.as_bytes()).is_err());
    }
    assert!(split_commit(b"tree abc\n\nNo identities").is_err());
    Ok(())
}

#[test]
fn malformed_or_broad_allowlists_are_rejected() -> anyhow::Result<()> {
    let hash = "c".repeat(64);
    let entry = serde_json::json!({"commit_sha":COMMIT,"metadata_sha256":hash});
    for document in [
        serde_json::json!({"schema_version":2,"entries":[entry]}),
        serde_json::json!({"schema_version":1,"entries":[entry,entry]}),
        serde_json::json!({"schema_version":1,"entries":[entry],"domain":"example.invalid"}),
        serde_json::json!({"schema_version":1,"entries":[{"commit_sha":"aaaa","metadata_sha256":hash}]}),
        serde_json::json!({"schema_version":1,"entries":[{"commit_sha":COMMIT,"metadata_sha256":"*"}]}),
    ] {
        assert!(Allowlist::parse(&serde_json::to_vec(&document)?).is_err());
    }
    Ok(())
}
