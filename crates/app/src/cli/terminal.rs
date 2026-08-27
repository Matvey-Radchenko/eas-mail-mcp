use std::io::{IsTerminal as _, Write as _};

use zeroize::Zeroizing;

use crate::{AppError, ErrorCode, Result};

pub(super) trait Terminal {
    fn is_interactive(&self) -> bool;
    fn input(&mut self, label: &str, default: Option<&str>) -> Result<String>;
    fn password(&mut self, label: &str) -> Result<Zeroizing<String>>;
    fn message(&mut self, message: &str) -> Result<()>;

    fn confirm(&mut self, label: &str, default: bool) -> Result<bool> {
        let suffix = if default { "Y/n" } else { "y/N" };
        loop {
            let answer = self.input(&format!("{label} [{suffix}]"), None)?;
            match answer.trim().to_ascii_lowercase().as_str() {
                "" => return Ok(default),
                "y" | "yes" => return Ok(true),
                "n" | "no" => return Ok(false),
                _ => self.message("Enter yes or no")?,
            }
        }
    }

    fn select(&mut self, label: &str, options: &[String], default: usize) -> Result<usize> {
        if options.is_empty() || default >= options.len() {
            return Err(AppError::new(
                ErrorCode::ValidationFailed,
                "terminal selection has no valid options",
            ));
        }
        self.message(label)?;
        for (index, option) in options.iter().enumerate() {
            self.message(&format!("  {}. {option}", index + 1))?;
        }
        let default_value = (default + 1).to_string();
        loop {
            let answer = self.input("Select", Some(&default_value))?;
            let selected = answer.parse::<usize>().ok().and_then(|value| value.checked_sub(1));
            if let Some(selected) = selected.filter(|value| *value < options.len()) {
                return Ok(selected);
            }
            self.message("Enter one of the listed numbers")?;
        }
    }
}

pub(super) struct StdioTerminal {
    interactive: bool,
}

impl StdioTerminal {
    pub(super) fn detect() -> Self {
        Self { interactive: std::io::stdin().is_terminal() }
    }

    pub(super) fn disable_interaction(&mut self) {
        self.interactive = false;
    }
}

impl Terminal for StdioTerminal {
    fn is_interactive(&self) -> bool {
        self.interactive
    }

    fn input(&mut self, label: &str, default: Option<&str>) -> Result<String> {
        require_interactive(self.interactive)?;
        let default_label = default.map_or_else(String::new, |value| format!(" [{value}]"));
        let mut stderr = std::io::stderr().lock();
        write!(stderr, "{label}{default_label}: ")
            .and_then(|()| stderr.flush())
            .map_err(|_| terminal_error("cannot write terminal prompt"))?;
        let mut value = String::new();
        std::io::stdin()
            .read_line(&mut value)
            .map_err(|_| terminal_error("cannot read terminal input"))?;
        let value = value.trim().to_owned();
        Ok(if value.is_empty() { default.unwrap_or_default().to_owned() } else { value })
    }

    fn password(&mut self, label: &str) -> Result<Zeroizing<String>> {
        require_interactive(self.interactive)?;
        rpassword::prompt_password(format!("{label}: "))
            .map(Zeroizing::new)
            .map_err(|_| terminal_error("cannot read terminal password"))
    }

    fn message(&mut self, message: &str) -> Result<()> {
        writeln!(std::io::stderr().lock(), "{message}")
            .map_err(|_| terminal_error("cannot write terminal output"))
    }
}

pub(super) fn require_interactive(interactive: bool) -> Result<()> {
    if interactive {
        Ok(())
    } else {
        Err(AppError::new(ErrorCode::InteractiveRequired, "interactive terminal input is required")
            .remediation("Run this command in a terminal or provide every required argument"))
    }
}

pub(super) fn confirm_controlling_tty(label: &str) -> Result<bool> {
    if std::io::stdin().is_terminal() {
        return StdioTerminal::detect().confirm(label, false);
    }
    confirm_platform_terminal(label)
}

#[cfg(unix)]
fn confirm_platform_terminal(label: &str) -> Result<bool> {
    let mut terminal = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/tty")
        .map_err(|_| interaction_error())?;
    let reader = terminal.try_clone().map_err(|_| interaction_error())?;
    confirm_io(std::io::BufReader::new(reader), &mut terminal, label)
}

#[cfg(windows)]
fn confirm_platform_terminal(label: &str) -> Result<bool> {
    let input =
        std::fs::OpenOptions::new().read(true).open("CONIN$").map_err(|_| interaction_error())?;
    let mut output =
        std::fs::OpenOptions::new().write(true).open("CONOUT$").map_err(|_| interaction_error())?;
    confirm_io(std::io::BufReader::new(input), &mut output, label)
}

#[cfg(not(any(unix, windows)))]
fn confirm_platform_terminal(_: &str) -> Result<bool> {
    Err(interaction_error())
}

fn confirm_io(
    mut reader: impl std::io::BufRead,
    writer: &mut impl std::io::Write,
    label: &str,
) -> Result<bool> {
    loop {
        write!(writer, "{label} [y/N]: ")
            .and_then(|()| writer.flush())
            .map_err(|_| terminal_error("cannot write terminal prompt"))?;
        let mut answer = String::new();
        reader.read_line(&mut answer).map_err(|_| terminal_error("cannot read terminal input"))?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "n" | "no" => return Ok(false),
            "y" | "yes" => return Ok(true),
            _ => writeln!(writer, "Enter yes or no")
                .map_err(|_| terminal_error("cannot write terminal prompt"))?,
        }
    }
}

fn interaction_error() -> AppError {
    AppError::new(
        ErrorCode::InteractiveRequired,
        "write confirmation requires a controlling terminal or --yes",
    )
    .remediation("Review the operation and pass --yes only for explicit automation")
}

fn terminal_error(message: &str) -> AppError {
    AppError::new(ErrorCode::StorageError, message)
}

#[cfg(test)]
pub(super) mod testing {
    use std::collections::VecDeque;

    use super::*;

    pub(crate) struct ScriptedTerminal {
        answers: VecDeque<String>,
        passwords: VecDeque<String>,
        pub(crate) transcript: Vec<String>,
    }

    impl ScriptedTerminal {
        pub(crate) fn new(answers: &[&str], passwords: &[&str]) -> Self {
            Self {
                answers: answers.iter().map(|value| (*value).to_owned()).collect(),
                passwords: passwords.iter().map(|value| (*value).to_owned()).collect(),
                transcript: Vec::new(),
            }
        }
    }

    impl Terminal for ScriptedTerminal {
        fn is_interactive(&self) -> bool {
            true
        }

        fn input(&mut self, label: &str, default: Option<&str>) -> Result<String> {
            self.transcript.push(format!("prompt:{label}"));
            let value = self.answers.pop_front().ok_or_else(|| {
                AppError::new(ErrorCode::InteractiveRequired, "scripted answer is missing")
            })?;
            Ok(if value.is_empty() { default.unwrap_or_default().to_owned() } else { value })
        }

        fn password(&mut self, label: &str) -> Result<Zeroizing<String>> {
            self.transcript.push(format!("password:{label}"));
            self.passwords.pop_front().map(Zeroizing::new).ok_or_else(|| {
                AppError::new(ErrorCode::InteractiveRequired, "scripted password is missing")
            })
        }

        fn message(&mut self, message: &str) -> Result<()> {
            self.transcript.push(format!("message:{message}"));
            Ok(())
        }
    }

    #[test]
    fn selections_and_confirmations_retry_invalid_answers() -> anyhow::Result<()> {
        let mut terminal = ScriptedTerminal::new(&["invalid", "yes", "9", "2"], &[]);
        assert!(terminal.confirm("Continue", false)?);
        assert_eq!(terminal.select("Choose", &["first".into(), "second".into()], 0)?, 1);
        assert!(terminal.transcript.iter().any(|line| line.contains("Enter yes or no")));
        assert!(
            terminal.transcript.iter().any(|line| line.contains("Enter one of the listed numbers"))
        );
        let error = require_interactive(false).err().ok_or_else(|| {
            anyhow::anyhow!("non-interactive prompt unexpectedly passed validation")
        })?;
        assert_eq!(error.envelope.code, ErrorCode::InteractiveRequired);
        Ok(())
    }

    #[test]
    fn disabled_stdio_terminal_never_reads_interactive_input() -> anyhow::Result<()> {
        let mut terminal = StdioTerminal::detect();
        terminal.disable_interaction();
        assert!(!terminal.is_interactive());

        let input_error = terminal.input("Value", None).err().ok_or_else(|| {
            anyhow::anyhow!("disabled terminal unexpectedly accepted plain input")
        })?;
        assert_eq!(input_error.envelope.code, ErrorCode::InteractiveRequired);
        let password_error = terminal
            .password("Password")
            .err()
            .ok_or_else(|| anyhow::anyhow!("disabled terminal unexpectedly accepted a password"))?;
        assert_eq!(password_error.envelope.code, ErrorCode::InteractiveRequired);
        Ok(())
    }

    #[test]
    fn controlling_terminal_confirmation_accepts_retries_and_declines_eof() -> anyhow::Result<()> {
        let mut accepted = Vec::new();
        assert!(confirm_io("invalid\nyes\n".as_bytes(), &mut accepted, "Execute")?);
        assert!(String::from_utf8(accepted)?.contains("Enter yes or no"));

        let mut declined = Vec::new();
        assert!(!confirm_io("".as_bytes(), &mut declined, "Execute")?);
        Ok(())
    }
}
