use std::collections::BTreeSet;

use chrono::{DateTime, SecondsFormat, Utc};

use super::tree::{element, push_text};
use crate::wbxml::{Element, decode, encode};
use crate::{EasError, OofAudience, OofMessage, OofSettings, OofState, Result};

/// Builds a read-only Settings/Oof Get request for plain-text reply content.
pub fn build_oof_get() -> Result<Vec<u8>> {
    let mut get = element("Settings", "Get");
    push_text(&mut get, "Settings", "BodyType", "Text");
    envelope(get)
}

/// Builds a Settings/Oof Set request with explicit audience selection.
pub fn build_oof_set(settings: &OofSettings) -> Result<Vec<u8>> {
    validate(settings)?;
    let mut set = element("Settings", "Set");
    push_text(
        &mut set,
        "Settings",
        "OofState",
        match settings.state {
            OofState::Disabled => "0",
            OofState::Enabled => "1",
            OofState::Scheduled => "2",
        },
    );
    for (name, date) in [("StartTime", settings.starts_at), ("EndTime", settings.ends_at)] {
        if let Some(date) = date {
            push_text(
                &mut set,
                "Settings",
                name,
                date.to_rfc3339_opts(SecondsFormat::Millis, true),
            );
        }
    }
    for message in &settings.messages {
        let mut item = element("Settings", "OofMessage");
        item.push(element("Settings", audience_name(message.audience)));
        push_text(&mut item, "Settings", "Enabled", if message.enabled { "1" } else { "0" });
        if let Some(message) = &message.message {
            push_text(&mut item, "Settings", "ReplyMessage", message);
            push_text(&mut item, "Settings", "BodyType", "Text");
        }
        set.push(item);
    }
    envelope(set)
}

/// Parses both Settings and Oof statuses before accepting the returned settings.
pub fn parse_oof_get(data: &[u8]) -> Result<OofSettings> {
    let root = root(data)?;
    if status(&root)? != 1 {
        return Err(invalid());
    }
    let oof = child(&root, "Oof")?.ok_or_else(invalid)?;
    if status(oof)? != 1 {
        return Err(invalid());
    }
    let get = child(oof, "Get")?.ok_or_else(invalid)?;
    let state = match text(get, "OofState")?.as_deref() {
        Some("0") => OofState::Disabled,
        Some("1") => OofState::Enabled,
        Some("2") => OofState::Scheduled,
        _ => return Err(invalid()),
    };
    let mut audiences = BTreeSet::new();
    let mut messages = Vec::new();
    for item in
        get.children().filter(|item| item.namespace == "Settings" && item.name == "OofMessage")
    {
        let message = parse_message(item)?;
        if !audiences.insert(message.audience) {
            return Err(invalid());
        }
        messages.push(message);
    }
    let settings = OofSettings {
        state,
        starts_at: date(get, "StartTime")?,
        ends_at: date(get, "EndTime")?,
        messages,
    };
    if settings.state == OofState::Scheduled && !valid_interval(&settings) {
        return Err(invalid());
    }
    Ok(settings)
}

/// Returns an explicit Settings or Oof failure status; malformed acknowledgements fail parsing.
pub fn parse_oof_set(data: &[u8]) -> Result<u16> {
    let root = root(data)?;
    let outer = status(&root)?;
    if outer != 1 {
        return Ok(outer);
    }
    status(child(&root, "Oof")?.ok_or_else(invalid)?)
}

fn envelope(operation: Element) -> Result<Vec<u8>> {
    let mut root = element("Settings", "Settings");
    let mut oof = element("Settings", "Oof");
    oof.push(operation);
    root.push(oof);
    encode(&root)
}

fn validate(settings: &OofSettings) -> Result<()> {
    let mut audiences = BTreeSet::new();
    let valid_dates = if settings.state == OofState::Scheduled {
        valid_interval(settings)
    } else {
        settings.starts_at.is_none() && settings.ends_at.is_none()
    };
    if !valid_dates
        || settings.messages.iter().any(|message| {
            !audiences.insert(message.audience)
                || message.is_html
                || message
                    .message
                    .as_ref()
                    .is_some_and(|text| text.len() > 65_536 || text.contains('\0'))
        })
    {
        return Err(EasError::InvalidConfiguration("invalid out-of-office settings".into()));
    }
    Ok(())
}

fn valid_interval(settings: &OofSettings) -> bool {
    settings.starts_at.zip(settings.ends_at).is_some_and(|(start, end)| start < end)
}

fn parse_message(item: &Element) -> Result<OofMessage> {
    let mut audience = None;
    for candidate in
        [OofAudience::Internal, OofAudience::ExternalKnown, OofAudience::ExternalUnknown]
    {
        if child(item, audience_name(candidate))?.is_some() && audience.replace(candidate).is_some()
        {
            return Err(invalid());
        }
    }
    let enabled = match text(item, "Enabled")?.as_deref() {
        Some("0") => false,
        Some("1") => true,
        _ => return Err(invalid()),
    };
    let message = text(item, "ReplyMessage")?;
    let is_html = match text(item, "BodyType")?.as_deref() {
        Some("HTML") => true,
        Some("Text") => false,
        None if message.is_none() => false,
        _ => return Err(invalid()),
    };
    if message.as_ref().is_some_and(|value| value.len() > 65_536) {
        return Err(invalid());
    }
    Ok(OofMessage { audience: audience.ok_or_else(invalid)?, enabled, message, is_html })
}

fn audience_name(audience: OofAudience) -> &'static str {
    match audience {
        OofAudience::Internal => "AppliesToInternal",
        OofAudience::ExternalKnown => "AppliesToExternalKnown",
        OofAudience::ExternalUnknown => "AppliesToExternalUnknown",
    }
}

fn root(data: &[u8]) -> Result<Element> {
    let root = decode(data)?.ok_or_else(invalid)?;
    if root.namespace != "Settings" || root.name != "Settings" {
        return Err(invalid());
    }
    Ok(root)
}

fn child<'a>(parent: &'a Element, name: &str) -> Result<Option<&'a Element>> {
    let mut children =
        parent.children().filter(|item| item.namespace == "Settings" && item.name == name);
    let first = children.next();
    if children.next().is_some() {
        return Err(invalid());
    }
    Ok(first)
}

fn text(parent: &Element, name: &str) -> Result<Option<String>> {
    child(parent, name)?
        .map(|child| {
            if child.children().next().is_some() {
                Err(invalid())
            } else {
                Ok(child.text_content())
            }
        })
        .transpose()
}

fn status(parent: &Element) -> Result<u16> {
    let status =
        text(parent, "Status")?.ok_or_else(invalid)?.parse::<u16>().map_err(|_| invalid())?;
    // MS-ASCMD 2.2.3.177.15: the Oof domain is narrower than the Settings domain.
    // Common codes from 2.2.2 are also defined for EAS 14.1; reserved codes are not acknowledgements.
    let valid = matches!(status, 101..=156 | 160..=174)
        || match parent.name.as_str() {
            "Settings" => matches!(status, 1..=7),
            "Oof" => matches!(status, 1 | 2 | 5 | 6),
            _ => false,
        };
    if valid { Ok(status) } else { Err(invalid()) }
}

fn date(parent: &Element, name: &str) -> Result<Option<DateTime<Utc>>> {
    text(parent, name)?
        .map(|value| {
            DateTime::parse_from_rfc3339(&value)
                .map(|date| date.with_timezone(&Utc))
                .map_err(|_| invalid())
        })
        .transpose()
}

fn invalid() -> EasError {
    EasError::Protocol("Exchange returned invalid or unsuccessful out-of-office settings".into())
}

#[cfg(test)]
mod tests;
