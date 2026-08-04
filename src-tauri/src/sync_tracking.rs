use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

use openmeter_sync_protocol::{
    open_tracking, QuotaSnapshotV2, TombstoneV2, TrackingKey, TrackingPayloadV2, UsageEventV2,
    MAX_ENVELOPE_BYTES, TRACKING_SCHEMA,
};
use serde::{Deserialize, Serialize};

use crate::sync_client::{SyncError, TrackingEnvelopeRecord};
use crate::tracking_projection::PeerTrackingState;

const MAX_PEER_CACHE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PeerEnvelopeCache(Vec<TrackingEnvelopeRecord>);

impl PeerEnvelopeCache {
    pub fn load(path: &Path) -> Result<Self, SyncError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path).map_err(|_| SyncError::PendingStorage)?;
        if bytes.len() > MAX_PEER_CACHE_BYTES {
            return Err(SyncError::PendingStorage);
        }
        let records: Vec<TrackingEnvelopeRecord> =
            serde_json::from_slice(&bytes).map_err(|_| SyncError::PendingStorage)?;
        Ok(Self(
            records.into_iter().filter(structurally_valid).collect(),
        ))
    }

    pub fn records(&self) -> &[TrackingEnvelopeRecord] {
        &self.0
    }

    pub fn opened(&self, key: &TrackingKey) -> Vec<PeerTrackingState> {
        self.0
            .iter()
            .filter_map(|record| {
                let payload = opened_record(record, key)?;
                Some(PeerTrackingState::current(payload, record.received_at_ms))
            })
            .collect()
    }

    pub fn replace_and_save(
        &mut self,
        incoming: Vec<TrackingEnvelopeRecord>,
        key: &TrackingKey,
        path: &Path,
    ) -> Result<(), SyncError> {
        let previous: BTreeMap<String, TrackingEnvelopeRecord> = self
            .0
            .iter()
            .filter(|record| opened_record(record, key).is_some())
            .map(|record| (record.envelope.meta.device_id.clone(), record.clone()))
            .collect();
        let mut newest = BTreeMap::<String, TrackingEnvelopeRecord>::new();
        for record in incoming {
            let device_id = record.envelope.meta.device_id.clone();
            let replace = newest.get(&device_id).is_none_or(|current| {
                (record.received_at_ms, record.envelope.meta.revision)
                    > (current.received_at_ms, current.envelope.meta.revision)
            });
            if replace {
                newest.insert(device_id, record);
            }
        }

        let next: Vec<TrackingEnvelopeRecord> = newest
            .into_iter()
            .filter_map(|(device_id, record)| {
                if opened_record(&record, key).is_some() {
                    Some(record)
                } else {
                    previous.get(&device_id).cloned()
                }
            })
            .collect();
        let bytes = serde_json::to_vec(&next).map_err(|_| SyncError::PendingStorage)?;
        if bytes.len() > MAX_PEER_CACHE_BYTES {
            return Err(SyncError::PendingStorage);
        }
        crate::platform::atomic_write(path, &bytes).map_err(|_| SyncError::PendingStorage)?;
        self.0 = next;
        Ok(())
    }

    pub fn replace_collision_safe_and_save(
        &mut self,
        incoming: Vec<TrackingEnvelopeRecord>,
        key: &TrackingKey,
        path: &Path,
        baseline: &TrackingPayloadV2,
    ) -> Result<Vec<PeerTrackingState>, SyncError> {
        let previous: BTreeMap<String, (TrackingEnvelopeRecord, TrackingPayloadV2)> = self
            .0
            .iter()
            .filter_map(|record| {
                let payload = opened_record(record, key)?;
                Some((
                    record.envelope.meta.device_id.clone(),
                    (record.clone(), payload),
                ))
            })
            .collect();
        let mut known: BTreeMap<String, UsageEventV2> = baseline
            .events
            .iter()
            .map(|event| (event.event_id.clone(), event.clone()))
            .collect();
        let mut ordered = incoming;
        ordered.sort_by_key(|record| record.received_at_ms);
        let mut saved = Vec::new();
        let mut states = Vec::new();
        for record in ordered {
            let device_id = record.envelope.meta.device_id.clone();
            let Some(current) = opened_record(&record, key) else {
                if let Some((prior_record, prior_payload)) = previous.get(&device_id) {
                    saved.push(prior_record.clone());
                    states.push(PeerTrackingState::current(
                        prior_payload.clone(),
                        prior_record.received_at_ms,
                    ));
                }
                continue;
            };
            let collision = current.events.iter().any(|event| {
                known
                    .get(&event.event_id)
                    .is_some_and(|existing| existing != event)
            });
            if collision {
                let last_good = previous.get(&device_id).map(|(_, payload)| payload.clone());
                if let Some((prior_record, _)) = previous.get(&device_id) {
                    saved.push(prior_record.clone());
                }
                states.push(PeerTrackingState {
                    current,
                    last_good,
                    received_at_ms: record.received_at_ms,
                });
                continue;
            }
            for event in &current.events {
                known
                    .entry(event.event_id.clone())
                    .or_insert_with(|| event.clone());
            }
            let received_at_ms = record.received_at_ms;
            saved.push(record);
            states.push(PeerTrackingState::current(current, received_at_ms));
        }
        let bytes = serde_json::to_vec(&saved).map_err(|_| SyncError::PendingStorage)?;
        if bytes.len() > MAX_PEER_CACHE_BYTES {
            return Err(SyncError::PendingStorage);
        }
        crate::platform::atomic_write(path, &bytes).map_err(|_| SyncError::PendingStorage)?;
        self.0 = saved;
        Ok(states)
    }
}

fn structurally_valid(record: &TrackingEnvelopeRecord) -> bool {
    record.received_at_ms >= 0
        && record.envelope.meta.schema == TRACKING_SCHEMA
        && record.envelope.meta.device_id.len() <= 64
        && !record.envelope.meta.device_id.is_empty()
        && record.envelope.nonce.len() == 24
        && record.envelope.ciphertext.len() <= MAX_ENVELOPE_BYTES
}

fn opened_record(record: &TrackingEnvelopeRecord, key: &TrackingKey) -> Option<TrackingPayloadV2> {
    if !structurally_valid(record) {
        return None;
    }
    let payload = open_tracking(key, &record.envelope).ok()?;
    (payload.device.device_id == record.envelope.meta.device_id).then_some(payload)
}

pub fn replace_local_quotas(
    snapshots: &[crate::providers::Snapshot],
    registry: &crate::accounts::AccountRegistry,
) {
    let config = crate::config_with_defaults(crate::load_config());
    let Some(device_id) = config
        .get("syncDeviceId")
        .and_then(serde_json::Value::as_str)
        .filter(|value| !value.is_empty())
    else {
        return;
    };
    let normalized = normalized_quotas(
        snapshots,
        registry,
        device_id,
        chrono::Utc::now().timestamp_millis(),
    );
    if let Ok(mut current) = quotas().lock() {
        *current = normalized;
    }
}

pub fn local_quotas() -> Vec<QuotaSnapshotV2> {
    quotas()
        .lock()
        .map(|current| current.clone())
        .unwrap_or_default()
}

pub fn normalized_quotas(
    snapshots: &[crate::providers::Snapshot],
    registry: &crate::accounts::AccountRegistry,
    device_id: &str,
    now_ms: i64,
) -> Vec<QuotaSnapshotV2> {
    let mut output = Vec::new();
    for snapshot in snapshots
        .iter()
        .filter(|snapshot| snapshot.status == "ok" && !snapshot.stale)
    {
        let Some(account) = registry
            .accounts()
            .iter()
            .find(|account| account.card_id == snapshot.card_id)
        else {
            continue;
        };
        for (ordinal, metric) in snapshot
            .metrics
            .iter()
            .filter(|metric| metric.kind == "progress")
            .enumerate()
        {
            let Some(used_percent) = metric
                .used_percent
                .filter(|value| value.is_finite() && (0.0..=100.0).contains(value))
            else {
                continue;
            };
            output.push(QuotaSnapshotV2 {
                provider_id: account.provider_id.clone(),
                record_id: account.identity_stamp(),
                metric_id: metric_id(&metric.label, ordinal),
                used_percent,
                remaining: None,
                limit: None,
                resets_at_ms: metric.resets_at,
                observed_at_ms: snapshot.fetched_at.max(0).min(now_ms),
                period_ms: metric.period_ms,
                source_device_id: device_id.to_string(),
            });
        }
    }
    output.sort_by(|left, right| {
        left.provider_id
            .cmp(&right.provider_id)
            .then_with(|| left.record_id.cmp(&right.record_id))
            .then_with(|| left.metric_id.cmp(&right.metric_id))
    });
    output
}

fn metric_id(label: &str, ordinal: usize) -> String {
    let normalized = label
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    if normalized.is_empty() {
        format!("metric-{ordinal}")
    } else {
        normalized.chars().take(128).collect()
    }
}

fn quotas() -> &'static Mutex<Vec<QuotaSnapshotV2>> {
    static QUOTAS: OnceLock<Mutex<Vec<QuotaSnapshotV2>>> = OnceLock::new();
    QUOTAS.get_or_init(|| Mutex::new(Vec::new()))
}

pub fn next_tombstones(
    previous: Option<&TrackingPayloadV2>,
    current_events: &[UsageEventV2],
    completed_providers: &HashSet<String>,
    now_ms: i64,
) -> Vec<TombstoneV2> {
    let cutoff = now_ms.saturating_sub(90 * 24 * 60 * 60 * 1_000);
    let current: HashSet<&str> = current_events
        .iter()
        .map(|event| event.event_id.as_str())
        .collect();
    let mut tombstones = BTreeMap::<String, TombstoneV2>::new();
    if let Some(previous) = previous {
        for tombstone in &previous.tombstones {
            if tombstone.removed_at_ms >= cutoff && !current.contains(tombstone.event_id.as_str()) {
                tombstones.insert(tombstone.event_id.clone(), tombstone.clone());
            }
        }
        for event in &previous.events {
            if completed_providers.contains(&event.provider_id)
                && !current.contains(event.event_id.as_str())
            {
                tombstones.insert(
                    event.event_id.clone(),
                    TombstoneV2 {
                        event_id: event.event_id.clone(),
                        removed_at_ms: now_ms,
                    },
                );
            }
        }
    }
    tombstones.into_values().collect()
}
