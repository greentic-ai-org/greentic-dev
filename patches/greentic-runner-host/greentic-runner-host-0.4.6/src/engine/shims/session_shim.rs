use super::super::error::GResult;
use super::super::host::{SessionHost, SessionKey, SessionSnapshot};
use async_trait::async_trait;
use parking_lot::RwLock;
use std::collections::HashMap;
use std::time::{Duration, Instant};

#[derive(Clone)]
pub struct SessionEntry {
    pub snapshot: SessionSnapshot,
    pub cas: u64,
    pub expires_at: Option<Instant>,
}

impl SessionEntry {
    fn new(snapshot: SessionSnapshot) -> Self {
        let ttl = snapshot.ttl;
        Self {
            snapshot,
            cas: 0,
            expires_at: Some(Instant::now() + ttl),
        }
    }
}

#[derive(Default)]
pub struct InMemorySessionHost {
    store: RwLock<HashMap<SessionKey, SessionEntry>>,
}

impl InMemorySessionHost {
    pub fn new() -> Self {
        Self {
            store: RwLock::new(HashMap::new()),
        }
    }

    fn apply_ttl(entry: &mut SessionEntry) {
        let ttl = entry.snapshot.ttl;
        entry.expires_at = Some(Instant::now() + ttl);
    }

    fn is_expired(entry: &SessionEntry) -> bool {
        entry
            .expires_at
            .map(|exp| Instant::now() > exp)
            .unwrap_or(false)
    }
}

#[async_trait]
impl SessionHost for InMemorySessionHost {
    async fn get(&self, key: &SessionKey) -> GResult<Option<SessionSnapshot>> {
        let mut guard = self.store.write();
        if let Some(entry) = guard.get_mut(key) {
            if Self::is_expired(entry) {
                guard.remove(key);
                return Ok(None);
            }
            let mut snapshot = entry.snapshot.clone();
            snapshot.revision = entry.cas;
            return Ok(Some(snapshot));
        }
        Ok(None)
    }

    async fn put(&self, mut snapshot: SessionSnapshot) -> GResult<()> {
        snapshot.revision = 0;
        let mut entry = SessionEntry::new(snapshot);
        Self::apply_ttl(&mut entry);
        self.store.write().insert(entry.snapshot.key.clone(), entry);
        Ok(())
    }

    async fn update_cas(
        &self,
        mut snapshot: SessionSnapshot,
        expected_revision: u64,
    ) -> GResult<bool> {
        let mut guard = self.store.write();
        if let Some(entry) = guard.get_mut(&snapshot.key) {
            if entry.cas != expected_revision {
                return Ok(false);
            }
            entry.cas = expected_revision.saturating_add(1);
            snapshot.revision = entry.cas;
            entry.snapshot = snapshot;
            Self::apply_ttl(entry);
            return Ok(true);
        }
        Ok(false)
    }

    async fn delete(&self, key: &SessionKey) -> GResult<()> {
        self.store.write().remove(key);
        Ok(())
    }

    async fn touch(&self, key: &SessionKey, ttl: Duration) -> GResult<()> {
        let mut guard = self.store.write();
        if let Some(entry) = guard.get_mut(key) {
            entry.snapshot.ttl = ttl;
            entry.expires_at = Some(Instant::now() + ttl);
        }
        Ok(())
    }
}
