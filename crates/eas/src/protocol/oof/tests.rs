use super::*;

#[test]
fn get_requests_text_and_disable_preserves_messages() -> anyhow::Result<()> {
    let get = decode(&build_oof_get()?)?.ok_or_else(|| anyhow::anyhow!("missing request"))?;
    assert_eq!(get.namespace, "Settings");
    assert_eq!(
        get.descendant("Settings", "BodyType").map(Element::text_content).as_deref(),
        Some("Text")
    );
    let request = OofSettings {
        state: OofState::Disabled,
        starts_at: None,
        ends_at: None,
        messages: Vec::new(),
    };
    let set =
        decode(&build_oof_set(&request)?)?.ok_or_else(|| anyhow::anyhow!("missing request"))?;
    assert_eq!(
        set.descendant("Settings", "OofState").map(Element::text_content).as_deref(),
        Some("0")
    );
    assert!(set.descendant("Settings", "OofMessage").is_none());
    Ok(())
}

#[test]
fn scheduled_get_parses_utc_and_all_audiences() -> anyhow::Result<()> {
    let mut get = element("Settings", "Get");
    push_text(&mut get, "Settings", "OofState", "2");
    push_text(&mut get, "Settings", "StartTime", "2026-09-07T09:00:00+02:00");
    push_text(&mut get, "Settings", "EndTime", "2026-09-11T17:00:00+02:00");
    for audience in
        [OofAudience::Internal, OofAudience::ExternalKnown, OofAudience::ExternalUnknown]
    {
        get.push(message(audience, "1", "Away"));
    }
    let parsed = parse_oof_get(&response("1", "1", Some(get))?)?;
    assert_eq!(parsed.state, OofState::Scheduled);
    assert_eq!(parsed.messages.len(), 3);
    assert_eq!(
        parsed.starts_at.map(|date| date.to_rfc3339()).as_deref(),
        Some("2026-09-07T07:00:00+00:00")
    );
    let set = decode(&build_oof_set(&parsed)?)?.ok_or_else(|| anyhow::anyhow!("missing set"))?;
    assert_eq!(
        set.descendant("Settings", "StartTime").map(Element::text_content).as_deref(),
        Some("2026-09-07T07:00:00.000Z")
    );
    Ok(())
}

#[test]
fn malformed_gets_and_duplicate_audiences_are_rejected() -> anyhow::Result<()> {
    let mut duplicate = element("Settings", "Get");
    push_text(&mut duplicate, "Settings", "OofState", "1");
    duplicate.push(message(OofAudience::Internal, "1", "one"));
    duplicate.push(message(OofAudience::Internal, "1", "two"));
    assert!(parse_oof_get(&response("1", "1", Some(duplicate))?).is_err());
    for state in ["2", "9"] {
        let mut get = element("Settings", "Get");
        push_text(&mut get, "Settings", "OofState", state);
        assert!(parse_oof_get(&response("1", "1", Some(get))?).is_err());
    }
    assert!(parse_oof_get(&response("2", "1", None)?).is_err());
    assert!(parse_oof_get(&response("1", "2", None)?).is_err());
    let mut get = element("Settings", "Get");
    push_text(&mut get, "Settings", "OofState", "0");
    push_text(&mut get, "Settings", "OofState", "1");
    assert!(parse_oof_get(&response("1", "1", Some(get))?).is_err());
    Ok(())
}

#[test]
fn set_status_preserves_explicit_rejection_and_requires_both_status_levels() -> anyhow::Result<()> {
    assert_eq!(parse_oof_set(&response("1", "1", None)?)?, 1);
    assert_eq!(parse_oof_set(&response("1", "2", None)?)?, 2);
    assert_eq!(parse_oof_set(&response("3", "1", None)?)?, 3);
    let mut root = element("Settings", "Settings");
    push_text(&mut root, "Settings", "Status", "1");
    assert!(parse_oof_set(&encode(&root)?).is_err());
    Ok(())
}

#[test]
fn undefined_and_reserved_statuses_are_not_definite_acknowledgements() -> anyhow::Result<()> {
    for value in [0, 8, 99, 100, 157, 159, 175, 255, 999] {
        let status = value.to_string();
        assert!(parse_oof_set(&response(&status, "1", None)?).is_err());
        assert!(parse_oof_set(&response("1", &status, None)?).is_err());
    }
    for value in [3, 4, 7] {
        assert!(parse_oof_set(&response("1", &value.to_string(), None)?).is_err());
    }
    for value in [2, 5, 6, 101, 156, 160, 174] {
        assert_eq!(parse_oof_set(&response("1", &value.to_string(), None)?)?, value);
    }
    Ok(())
}

#[test]
fn set_rejects_invalid_schedule_and_duplicate_audiences() {
    let mut settings = OofSettings {
        state: OofState::Scheduled,
        starts_at: None,
        ends_at: None,
        messages: Vec::new(),
    };
    assert!(build_oof_set(&settings).is_err());
    settings.state = OofState::Enabled;
    let item = OofMessage {
        audience: OofAudience::Internal,
        enabled: true,
        message: Some("away".into()),
        is_html: false,
    };
    settings.messages = vec![item.clone(), item];
    assert!(build_oof_set(&settings).is_err());
}

fn message(audience: OofAudience, enabled: &str, text: &str) -> Element {
    let mut item = element("Settings", "OofMessage");
    item.push(element("Settings", audience_name(audience)));
    push_text(&mut item, "Settings", "Enabled", enabled);
    push_text(&mut item, "Settings", "ReplyMessage", text);
    push_text(&mut item, "Settings", "BodyType", "Text");
    item
}

fn response(outer: &str, inner: &str, get: Option<Element>) -> Result<Vec<u8>> {
    let mut root = element("Settings", "Settings");
    push_text(&mut root, "Settings", "Status", outer);
    let mut oof = element("Settings", "Oof");
    push_text(&mut oof, "Settings", "Status", inner);
    if let Some(get) = get {
        oof.push(get);
    }
    root.push(oof);
    encode(&root)
}
