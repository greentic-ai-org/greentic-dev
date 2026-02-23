use std::io::{self, IsTerminal, Write};

use anyhow::{Result, bail};

pub fn ensure_execute_allowed(summary: &str, yes: bool, non_interactive: bool) -> Result<()> {
    if yes {
        return Ok(());
    }

    let interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    if !interactive {
        if non_interactive {
            return Ok(());
        }
        bail!(
            "refusing to execute in non-interactive mode without confirmation. Re-run with `--execute --yes` or `--execute --non-interactive`."
        );
    }

    eprintln!("{summary}");
    eprint!("Execute plan? [y/N]: ");
    io::stderr().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let accepted = matches!(input.trim().to_ascii_lowercase().as_str(), "y" | "yes");
    if accepted {
        Ok(())
    } else {
        bail!("execution canceled by user")
    }
}
