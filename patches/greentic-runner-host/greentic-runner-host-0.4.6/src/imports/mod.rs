use crate::oauth::add_oauth_broker_to_linker;
use anyhow::Result;
use wasmtime_wasi::p2;

use crate::runtime_wasmtime::Linker;

use crate::pack::ComponentState;

pub fn register_all(linker: &mut Linker<ComponentState>, oauth_enabled: bool) -> Result<()> {
    p2::add_to_linker_sync(linker)?;
    greentic_interfaces_host::host_import::v0_6::add_to_linker(linker, |state| state)?;
    greentic_interfaces_host::host_import::v0_2::add_to_linker(linker, |state| state)?;
    if oauth_enabled {
        add_oauth_broker_to_linker(linker)?;
    }
    Ok(())
}
