use anyhow::Result;

use crate::config;
use crate::delegate::packc::PackcDelegate;
use greentic_dev::cli::PackNewArgs;

pub fn run_new(args: &PackNewArgs) -> Result<()> {
    let config = config::load()?;
    let delegate = PackcDelegate::from_config(&config)?;
    delegate.run_new(&args.passthrough)
}
