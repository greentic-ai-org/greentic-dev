use anyhow::Result;

use crate::passthrough::install_all_delegated_tools;

pub fn install(latest: bool, locale: &str) -> Result<()> {
    eprintln!("Installing Greentic development/bootstrap tools.");
    eprintln!("For customer-approved pinned releases, use `gtc install`.");
    install_all_delegated_tools(latest, locale)
}
