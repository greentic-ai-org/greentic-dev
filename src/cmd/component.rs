use anyhow::Result;

use crate::config;
use crate::delegate::component::ComponentDelegate;

pub fn run_passthrough(args: &[String]) -> Result<()> {
    let config = config::load()?;
    let delegate = ComponentDelegate::from_config(&config)?;
    delegate.run_passthrough(args)
}
