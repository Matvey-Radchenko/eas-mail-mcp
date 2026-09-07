use super::*;

fn now() -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339("2026-09-04T10:15:42Z")?.with_timezone(&Utc))
}

#[test]
fn completing_preserves_every_supported_field_and_records_required_dates_in_schema_order()
-> anyhow::Result<()> {
    let mut old = element("Email", "Flag");
    for (ns, name) in FIELDS.into_iter().rev() {
        let value = match name {
            "Status" => "2",
            _ => "existing",
        };
        old.push(Element::text(ns, name, value));
    }
    let completed = build(1, Some(&old), now()?)?;
    let keys =
        completed.children().map(|v| (v.namespace.as_str(), v.name.as_str())).collect::<Vec<_>>();
    assert_eq!(keys, FIELDS);
    for child in completed.children() {
        let expected = match child.name.as_str() {
            "Status" => "1",
            "DateCompleted" => "2026-09-04T00:00:00.000Z",
            "CompleteTime" => "2026-09-04T10:15:00.000Z",
            _ => "existing",
        };
        assert_eq!(child.text_content(), expected);
    }
    let later = now()? + chrono::Duration::days(1);
    assert_eq!(build(1, Some(&completed), later)?, completed);
    Ok(())
}

#[test]
fn activating_an_undated_flag_does_not_create_a_due_date_and_reopening_removes_completion()
-> anyhow::Result<()> {
    let active = build(2, Some(&element("Email", "Flag")), now()?)?;
    assert_eq!(active.children().count(), 2);
    assert!(active.child("Tasks", "DueDate").is_none());
    let completed = build(1, Some(&active), now()?)?;
    assert_eq!(build(2, Some(&completed), now()?)?, active);
    assert!(build(0, Some(&completed), now()?)?.content.is_empty());
    Ok(())
}

#[test]
fn unsupported_duplicate_and_incomplete_parameters_fail_before_writing() -> anyhow::Result<()> {
    for bad in [
        Element::text("Tasks", "Importance", "1"),
        Element::text("Tasks", "UtcDueDate", "2026-10-01T00:00:00.000Z"),
    ] {
        let mut old = element("Email", "Flag");
        old.push(bad);
        assert!(matches!(build(2, Some(&old), now()?), Err(EasError::FeatureUnavailable(_))));
    }
    let mut duplicate = element("Email", "Flag");
    duplicate.push(Element::text("Email", "Status", "1"));
    duplicate.push(Element::text("Email", "Status", "2"));
    assert!(build(2, Some(&duplicate), now()?).is_err());
    Ok(())
}
