use eas_mail_protocol::{MailFields, MailSearchQuery, Patch};

use crate::model::{MailSearchFilters, MailSearchInput};
use crate::{AppError, ErrorCode, Result};

pub(super) fn query(input: &MailSearchInput) -> Result<MailSearchQuery> {
    let filters = &input.filters;
    if input.query.len() > 4096 {
        return Err(validation("search text exceeds 4096 bytes"));
    }
    for address in [filters.from.as_deref(), filters.to.as_deref()].into_iter().flatten() {
        if !valid_address(address) {
            return Err(validation("from and to filters require one exact SMTP address"));
        }
    }
    let duration = filters
        .received_after
        .zip(filters.received_before)
        .map(|(after, before)| before.signed_duration_since(after));
    if duration.is_some_and(|value| value <= chrono::Duration::zero()) {
        return Err(validation("received_before must be after received_after"));
    }
    if input.query.trim().is_empty()
        && duration.is_none_or(|value| value > chrono::Duration::days(31))
    {
        return Err(validation(
            "search without text requires both date bounds spanning at most 31 days",
        ));
    }
    if filters.folder_ids.len() > 100
        || filters
            .folder_ids
            .iter()
            .any(|id| id.is_empty() || id.len() > 8192 || id.chars().any(char::is_control))
    {
        return Err(validation("folder_ids must contain at most 100 valid folder identifiers"));
    }
    Ok(MailSearchQuery {
        text: input.query.clone(),
        folder_ids: filters.folder_ids.clone(),
        received_after: filters.received_after,
        received_before: filters.received_before,
        conversation_id: None,
    })
}

pub(super) fn matches(fields: &MailFields, filters: &MailSearchFilters) -> Option<bool> {
    let checks = [
        address_matches(&fields.sender, filters.from.as_deref()),
        address_matches(&fields.recipients, filters.to.as_deref()),
        filters.is_read.map_or(Some(true), |expected| match fields.is_read {
            Patch::Value(value) => Some(value == expected),
            Patch::Missing => None,
        }),
        filters.has_attachments.map_or(Some(true), |expected| match &fields.attachments {
            Patch::Value(value) => Some(value.is_empty() != expected),
            Patch::Missing => None,
        }),
        date_matches(fields, filters),
    ];
    if checks.contains(&Some(false)) {
        Some(false)
    } else if checks.contains(&None) {
        None
    } else {
        Some(true)
    }
}

fn date_matches(fields: &MailFields, filters: &MailSearchFilters) -> Option<bool> {
    if filters.received_after.is_none() && filters.received_before.is_none() {
        return Some(true);
    }
    match fields.received_at {
        Patch::Value(Some(value)) => Some(
            filters.received_after.is_none_or(|after| value > after)
                && filters.received_before.is_none_or(|before| value < before),
        ),
        _ => None,
    }
}

fn address_matches(header: &Patch<String>, expected: Option<&str>) -> Option<bool> {
    let Some(expected) = expected else {
        return Some(true);
    };
    let Patch::Value(header) = header else {
        return None;
    };
    addresses(header).map(|values| values.iter().any(|value| value.eq_ignore_ascii_case(expected)))
}

fn addresses(header: &str) -> Option<Vec<&str>> {
    let mut quoted = false;
    let mut escaped = false;
    let mut angle = false;
    let mut start = 0;
    let mut output = Vec::new();
    for (position, character) in header.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        match character {
            '\\' if quoted => escaped = true,
            '"' => quoted = !quoted,
            '<' if !quoted => angle = true,
            '>' if !quoted => angle = false,
            ',' | ';' if !quoted && !angle => {
                if let Some(value) = address(header.get(start..position)?)? {
                    output.push(value);
                }
                start = position + character.len_utf8();
            }
            _ => {}
        }
    }
    if quoted || angle || escaped {
        return None;
    }
    if let Some(value) = address(header.get(start..)?)? {
        output.push(value);
    }
    Some(output)
}

fn address(value: &str) -> Option<Option<&str>> {
    let value = value.trim();
    if value.is_empty() {
        return Some(None);
    }
    let value = if let Some((_, rest)) = value.rsplit_once('<') {
        rest.strip_suffix('>')?.trim()
    } else {
        value
    };
    valid_address(value).then_some(Some(value))
}

fn valid_address(value: &str) -> bool {
    let Some((local, domain)) = value.split_once('@') else {
        return false;
    };
    value.len() <= 254
        && !local.is_empty()
        && !domain.is_empty()
        && !domain.contains('@')
        && !value.chars().any(|character| {
            character.is_whitespace()
                || character.is_control()
                || matches!(character, '<' | '>' | '"' | '(' | ')' | ',' | ';')
        })
}

fn validation(message: &'static str) -> AppError {
    AppError::new(ErrorCode::ValidationFailed, message)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_addresses_handle_quoted_display_names_and_case() {
        let header =
            Patch::Value("\"Last, First\" <First@Example.com>, Other <other@example.com>".into());
        assert_eq!(address_matches(&header, Some("first@example.com")), Some(true));
        assert_eq!(address_matches(&header, Some("first@example.co")), Some(false));
    }

    #[test]
    fn missing_is_not_false_and_a_known_mismatch_still_excludes() {
        let filters = MailSearchFilters { is_read: Some(false), ..Default::default() };
        assert_eq!(matches(&MailFields::default(), &filters), None);
        let fields = MailFields { is_read: Patch::Value(true), ..Default::default() };
        assert_eq!(matches(&fields, &filters), Some(false));
    }

    #[test]
    fn textless_queries_require_a_bounded_period() {
        let mut input = MailSearchInput::default();
        assert!(query(&input).is_err());
        input.filters.received_after = Some(chrono::DateTime::UNIX_EPOCH);
        input.filters.received_before =
            Some(chrono::DateTime::UNIX_EPOCH + chrono::Duration::days(31));
        assert!(query(&input).is_ok());
        input.filters.received_before =
            Some(chrono::DateTime::UNIX_EPOCH + chrono::Duration::days(32));
        assert!(query(&input).is_err());
    }
}
