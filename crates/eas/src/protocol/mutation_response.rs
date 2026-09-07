use crate::wbxml::{Element, Node};
use crate::{EasError, Result};

pub(super) fn malformed(message: &str) -> EasError {
    EasError::Protocol(message.into())
}

pub(super) fn child<'a>(
    parent: &'a Element,
    namespace: &str,
    name: &str,
) -> Result<Option<&'a Element>> {
    let mut values =
        parent.children().filter(|value| value.namespace == namespace && value.name == name);
    let first = values.next();
    if values.next().is_some() {
        return Err(malformed("duplicate mutation acknowledgement field"));
    }
    Ok(first)
}

pub(super) fn text(parent: &Element, namespace: &str, name: &str) -> Result<Option<String>> {
    child(parent, namespace, name)?
        .map(|value| {
            if value.content.iter().any(|node| !matches!(node, Node::Text(_))) {
                return Err(malformed("mutation acknowledgement field is not text"));
            }
            Ok(value.text_content())
        })
        .transpose()
}

pub(super) fn required_text(parent: &Element, namespace: &str, name: &str) -> Result<String> {
    text(parent, namespace, name)?
        .filter(|value| !value.is_empty())
        .ok_or_else(|| malformed("missing mutation acknowledgement field"))
}

pub(super) fn status(parent: &Element, namespace: &str, allowed: &[u16]) -> Result<u16> {
    let value = required_text(parent, namespace, "Status")?
        .parse::<u16>()
        .map_err(|_| malformed("invalid mutation acknowledgement status"))?;
    if !allowed.contains(&value) {
        return Err(malformed("unsupported or ambiguous mutation acknowledgement status"));
    }
    Ok(value)
}
