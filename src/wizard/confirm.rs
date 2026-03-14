use std::io::{self, IsTerminal, Write};

use anyhow::{Result, bail};

use crate::i18n;

pub fn ensure_execute_allowed(
    summary: &str,
    yes: bool,
    non_interactive: bool,
    locale: &str,
) -> Result<()> {
    if yes {
        return Ok(());
    }

    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    if !interactive {
        if non_interactive {
            return Ok(());
        }
        bail!(
            "{}",
            i18n::t(locale, "runtime.wizard.confirm.error.non_interactive")
        );
    }

    eprintln!("{summary}");
    eprint!("{}", i18n::t(locale, "runtime.wizard.confirm.prompt"));
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let accepted = matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    if accepted {
        Ok(())
    } else {
        bail!(
            "{}",
            i18n::t(locale, "runtime.wizard.confirm.error.canceled")
        )
    }
}
