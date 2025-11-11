use anyhow::Result;

use crate::config;
use crate::delegate::component::ComponentDelegate;
use greentic_dev::cli::ComponentPassthroughArgs;

pub fn run_passthrough(args: &ComponentPassthroughArgs) -> Result<()> {
    let config = config::load()?;
    let delegate = ComponentDelegate::from_config(&config)?;
    delegate.run_passthrough(&args.passthrough)
}
