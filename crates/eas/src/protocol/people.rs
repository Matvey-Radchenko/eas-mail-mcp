use crate::wbxml::{Element, decode, encode};
use crate::{EasError, Result};

use super::tree::{direct_text, element, push_text};

/// Minimal directory identity; unrelated GAL properties are discarded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPerson {
    /// Directory display name.
    pub name: String,
    /// SMTP address supplied by the directory.
    pub email: String,
}

/// One bounded GAL Search response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectoryPage {
    /// Entries in server order.
    pub items: Vec<DirectoryPerson>,
    /// Total matches reported by Exchange.
    pub total: usize,
}

/// Builds a prefix/ANR directory query without fetching contacts or calendars.
pub fn build_people_search(query: &str, limit: usize) -> Result<Vec<u8>> {
    if query.trim().is_empty()
        || query.chars().count() > 256
        || query.chars().any(char::is_control)
        || !(1..=50).contains(&limit)
    {
        return Err(EasError::InvalidConfiguration("invalid directory search input".into()));
    }
    let mut root = element("Search", "Search");
    let mut store = element("Search", "Store");
    push_text(&mut store, "Search", "Name", "GAL");
    push_text(&mut store, "Search", "Query", query.trim());
    let mut options = element("Search", "Options");
    push_text(&mut options, "Search", "Range", format!("0-{}", limit - 1));
    store.push(options);
    root.push(store);
    encode(&root)
}

/// Parses only bounded display-name/address pairs and validates both status levels.
pub fn parse_people_search(data: &[u8], limit: usize) -> Result<DirectoryPage> {
    let root = decode(data)?.ok_or_else(invalid)?;
    if root.namespace != "Search" || root.name != "Search" || !(1..=50).contains(&limit) {
        return Err(invalid());
    }
    require_success(&root)?;
    let store = root
        .child("Search", "Response")
        .and_then(|response| response.child("Search", "Store"))
        .ok_or_else(invalid)?;
    require_success(store)?;
    let mut items = Vec::new();
    for result in
        store.children().filter(|child| child.namespace == "Search" && child.name == "Result")
    {
        if items.len() == limit {
            return Err(invalid());
        }
        let properties = result.child("Search", "Properties").ok_or_else(invalid)?;
        let name = direct_text(properties, "GAL", "DisplayName").unwrap_or_default();
        let email = direct_text(properties, "GAL", "EmailAddress").ok_or_else(invalid)?;
        if name.chars().count() > 1024 || email.len() > 320 || !valid_email(&email) {
            return Err(invalid());
        }
        items.push(DirectoryPerson { name, email });
    }
    let total = direct_text(store, "Search", "Total")
        .map(|value| value.parse::<usize>().map_err(|_| invalid()))
        .transpose()?
        .unwrap_or(items.len());
    if total < items.len() {
        return Err(invalid());
    }
    Ok(DirectoryPage { items, total })
}

fn require_success(element: &Element) -> Result<()> {
    if direct_text(element, "Search", "Status").as_deref() != Some("1") {
        return Err(invalid());
    }
    Ok(())
}

fn valid_email(value: &str) -> bool {
    !value.chars().any(|character| character.is_control() || character.is_whitespace())
        && value.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
}

fn invalid() -> EasError {
    EasError::Protocol("Exchange returned an invalid or unsuccessful GAL search".into())
}
