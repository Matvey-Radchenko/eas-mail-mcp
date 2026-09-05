use super::mutation_response::{malformed, status};
use crate::wbxml::{Node, decode};
use crate::{Command, EasError, MutationResult, Result};

// MS-ASCMD 2.2.2: explicit EAS 14.1 rejection reasons applicable to composing mail.
// Unknown server errors (110/111), already-sent ClientId (118), state loss (132),
// and unmodeled or later-version codes cannot prove this message was not sent.
const DEFINITE_REJECTIONS: &[u16] = &[
    101, 102, 103, 104, 105, 106, 107, 108, 109, 112, 113, 114, 115, 116, 117, 119, 120, 121, 122,
    123, 124, 125, 126, 127, 128, 129, 130, 131, 137, 138, 139, 140, 141, 142, 143, 144, 145, 146,
    147, 148, 150, 166, 167, 168, 171, 172, 177,
];

pub(crate) fn parse_for(data: &[u8], command: Command) -> Result<MutationResult> {
    parse(data, Some(command))
}

pub(super) fn parse(data: &[u8], expected: Option<Command>) -> Result<MutationResult> {
    // MS-ASCMD 2.2.3.177.14/16: successful SendMail/SmartReply/SmartForward
    // responses have no XML body; a body contains a failure reason, never status 1.
    if data.is_empty() {
        return Ok(MutationResult { status: 1, sync_key: None, server_id: None });
    }
    let root = decode(data)?.ok_or_else(|| malformed("empty compose WBXML document"))?;
    if root.namespace != "ComposeMail"
        || !matches!(root.name.as_str(), "SendMail" | "SmartReply" | "SmartForward")
        || expected.is_some_and(|command| root.name != command.name())
    {
        return Err(malformed("compose response identifies a different command"));
    }
    if !matches!(root.content.as_slice(), [Node::Element(child)]
        if child.namespace == "ComposeMail" && child.name == "Status")
    {
        return Err(malformed("compose response must contain exactly one direct status"));
    }
    let status = status(&root, "ComposeMail", DEFINITE_REJECTIONS)?;
    if status == 140 {
        return Err(EasError::AccountRemoteWipe);
    }
    Ok(MutationResult { status, sync_key: None, server_id: None })
}
