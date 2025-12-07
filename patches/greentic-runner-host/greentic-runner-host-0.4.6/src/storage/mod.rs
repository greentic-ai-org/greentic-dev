pub mod session;
pub mod state;

use std::sync::Arc;

use crate::engine::host::{SessionHost, StateHost};
pub use session::DynSessionStore;
pub use state::DynStateStore;

pub fn new_session_store() -> DynSessionStore {
    session::new_session_store()
}

pub fn new_state_store() -> DynStateStore {
    state::new_state_store()
}

pub fn session_host_from(store: DynSessionStore) -> Arc<dyn SessionHost> {
    session::session_host_from(store)
}

pub fn state_host_from(store: DynStateStore) -> Arc<dyn StateHost> {
    state::state_host_from(store)
}
