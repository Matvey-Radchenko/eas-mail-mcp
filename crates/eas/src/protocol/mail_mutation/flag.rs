use chrono::{DateTime, Utc};
use std::collections::BTreeMap;

use super::{EasError, Element, Result, element};
use crate::wbxml::Node;

// MS-ASEMAIL Email schema sequence; each supported field can occur at most once.
const FIELDS: [(&str, &str); 13] = [
    ("Tasks", "Subject"),
    ("Email", "Status"),
    ("Email", "FlagType"),
    ("Tasks", "DateCompleted"),
    ("Email", "CompleteTime"),
    ("Tasks", "StartDate"),
    ("Tasks", "DueDate"),
    ("Tasks", "UtcStartDate"),
    ("Tasks", "UtcDueDate"),
    ("Tasks", "ReminderSet"),
    ("Tasks", "ReminderTime"),
    ("Tasks", "OrdinalDate"),
    ("Tasks", "SubOrdinalDate"),
];

pub(super) fn build(
    status: u8,
    previous: Option<&Element>,
    updated_at: DateTime<Utc>,
) -> Result<Element> {
    if status > 2 {
        return Err(EasError::InvalidConfiguration("invalid mail flag status".into()));
    }
    let mut result = element("Email", "Flag");
    if status == 0 {
        return Ok(result);
    }
    let mut fields = existing_fields(previous)?;
    validate_dates(&fields)?;
    let already_complete =
        fields.get(&("Email", "Status")).is_some_and(|node| node.text_content() == "1");
    if status == 1 {
        let completed = [("Tasks", "DateCompleted"), ("Email", "CompleteTime")];
        if !already_complete
            || !completed
                .iter()
                .all(|key| fields.get(key).is_some_and(|v| !v.text_content().is_empty()))
        {
            fields.insert(
                ("Tasks", "DateCompleted"),
                Element::text(
                    "Tasks",
                    "DateCompleted",
                    updated_at.format("%Y-%m-%dT00:00:00.000Z").to_string(),
                ),
            );
            fields.insert(
                ("Email", "CompleteTime"),
                Element::text(
                    "Email",
                    "CompleteTime",
                    updated_at.format("%Y-%m-%dT%H:%M:00.000Z").to_string(),
                ),
            );
        }
    } else {
        fields.remove(&("Tasks", "DateCompleted"));
        fields.remove(&("Email", "CompleteTime"));
    }
    fields.insert(("Email", "Status"), Element::text("Email", "Status", status.to_string()));
    fields
        .entry(("Email", "FlagType"))
        .or_insert_with(|| Element::text("Email", "FlagType", "Flag for follow up"));
    for key in FIELDS {
        if let Some(value) = fields.remove(&key) {
            result.push(value);
        }
    }
    Ok(result)
}

fn existing_fields(
    previous: Option<&Element>,
) -> Result<BTreeMap<(&'static str, &'static str), Element>> {
    let mut fields = BTreeMap::new();
    let Some(previous) = previous else {
        return Ok(fields);
    };
    if previous.namespace != "Email" || previous.name != "Flag" {
        return Err(unsupported());
    }
    for node in &previous.content {
        let Node::Element(child) = node else {
            return Err(unsupported());
        };
        let key = FIELDS
            .iter()
            .find(|(ns, name)| *ns == child.namespace && *name == child.name)
            .copied()
            .ok_or_else(unsupported)?;
        if !child.content.iter().all(|node| matches!(node, Node::Text(_)))
            || fields.insert(key, child.clone()).is_some()
        {
            return Err(unsupported());
        }
    }
    Ok(fields)
}

fn validate_dates(fields: &BTreeMap<(&str, &str), Element>) -> Result<()> {
    // MS-ASEMAIL DueDate: all four dates must be present or all NULL. Preserve no-date flags.
    let count = ["StartDate", "DueDate", "UtcStartDate", "UtcDueDate"]
        .iter()
        .filter(|name| fields.get(&("Tasks", **name)).is_some_and(|v| !v.text_content().is_empty()))
        .count();
    if count != 0 && count != 4 {
        return Err(unsupported());
    }
    Ok(())
}

fn unsupported() -> EasError {
    EasError::FeatureUnavailable("existing flag parameters cannot be preserved safely".into())
}

#[cfg(test)]
mod tests;
