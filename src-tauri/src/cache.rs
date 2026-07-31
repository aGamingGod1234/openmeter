use std::io;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::providers::ProviderSnapshot;

const CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SnapshotCache {
    version: u32,
    entries: Vec<ProviderSnapshot>,
}

impl Default for SnapshotCache {
    fn default() -> Self {
        Self {
            version: CACHE_VERSION,
            entries: Vec::new(),
        }
    }
}

impl SnapshotCache {
    pub fn entries(&self) -> &[ProviderSnapshot] {
        &self.entries
    }

    pub fn insert(&mut self, snapshot: ProviderSnapshot) {
        if snapshot.status != "ok"
            || snapshot.card_id.is_empty()
            || snapshot.credential_stamp.is_empty()
            || snapshot.expires_at < snapshot.fetched_at
        {
            return;
        }
        self.entries
            .retain(|entry| entry.card_id != snapshot.card_id);
        self.entries.push(snapshot);
    }

    pub fn fresh(
        &self,
        card_id: &str,
        credential_stamp: &str,
        now: i64,
    ) -> Option<&ProviderSnapshot> {
        self.matching(card_id, credential_stamp)
            .filter(|snapshot| now < snapshot.expires_at)
    }

    pub fn last_good(&self, card_id: &str, credential_stamp: &str) -> Option<&ProviderSnapshot> {
        self.matching(card_id, credential_stamp)
    }

    pub fn read(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                let cache: Self = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
                    .map_err(|error| format!("parse {}: {error}", path.display()))?;
                cache.validate()?;
                Ok(cache)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("read {}: {error}", path.display())),
        }
    }

    pub fn write(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("serialize snapshot cache: {error}"))?;
        crate::platform::atomic_write(path, &bytes)
            .map_err(|error| format!("save {}: {error}", path.display()))
    }

    fn matching(&self, card_id: &str, credential_stamp: &str) -> Option<&ProviderSnapshot> {
        self.entries
            .iter()
            .filter(|snapshot| {
                snapshot.card_id == card_id
                    && snapshot.credential_stamp == credential_stamp
                    && snapshot.status == "ok"
            })
            .max_by_key(|snapshot| snapshot.fetched_at)
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != CACHE_VERSION {
            return Err(format!(
                "unsupported snapshot cache version {}; expected {CACHE_VERSION}",
                self.version
            ));
        }
        for snapshot in &self.entries {
            if snapshot.card_id.is_empty() || snapshot.credential_stamp.is_empty() {
                return Err("snapshot cache entry has no card or credential stamp".to_string());
            }
            if snapshot.id != snapshot.card_id {
                return Err(format!(
                    "snapshot id '{}' does not match card id '{}'",
                    snapshot.id, snapshot.card_id
                ));
            }
            if snapshot.expires_at < snapshot.fetched_at {
                return Err(format!(
                    "snapshot '{}' expires before it was fetched",
                    snapshot.card_id
                ));
            }
        }
        Ok(())
    }
}
