use anyhow::Result;

use crate::passthrough::install_all_delegated_tools;

pub fn install(latest: bool) -> Result<()> {
    install_all_delegated_tools(latest)
}
