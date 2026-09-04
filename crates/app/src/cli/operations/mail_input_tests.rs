use clap::Parser;

use super::super::mail_args::MailCommand;
use super::*;

#[derive(Parser)]
struct Arguments {
    #[command(subcommand)]
    command: MailCommand,
}

#[test]
fn attach_flags_are_repeatable_absolute_and_exclusive_with_json() -> anyhow::Result<()> {
    let args = Arguments::try_parse_from([
        "mail",
        "send",
        "--account",
        "a",
        "--to",
        "x@y",
        "--subject",
        "s",
        "--body",
        "b",
        "--attach",
        "first.pdf",
        "--attach",
        "second.bin",
    ])?;
    let MailCommand::Send(args) = args.command else { anyhow::bail!("expected send") };
    let (input, _) = send(args)?;
    assert_eq!(input.attachments.len(), 2);
    assert!(input.attachments.iter().all(|a| std::path::Path::new(&a.path).is_absolute()));
    let args =
        Arguments::try_parse_from(["mail", "send", "--input", "missing.json", "--attach", "f"])?;
    let MailCommand::Send(args) = args.command else { anyhow::bail!("expected send") };
    assert!(send(args).is_err_and(|e| e.envelope.code == ErrorCode::ValidationFailed));
    Ok(())
}

#[test]
fn reply_and_forward_accept_new_attachments() -> anyhow::Result<()> {
    let args = Arguments::try_parse_from(["mail", "reply", "ref", "--body", "b", "--attach", "f"])?;
    let MailCommand::Reply(args) = args.command else { anyhow::bail!("expected reply") };
    assert_eq!(reply(args)?.0.attachments.len(), 1);
    let args = Arguments::try_parse_from([
        "mail", "forward", "ref", "--to", "x@y", "--body", "b", "--attach", "f",
    ])?;
    let MailCommand::Forward(args) = args.command else { anyhow::bail!("expected forward") };
    assert_eq!(forward(args)?.0.attachments.len(), 1);
    Ok(())
}
