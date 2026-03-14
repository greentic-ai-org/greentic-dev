use anyhow::Result;

use crate::passthrough::install_all_delegated_tools;

pub fn install(latest: bool, locale: &str) -> Result<()> {
    install_all_delegated_tools(latest, locale)
}
