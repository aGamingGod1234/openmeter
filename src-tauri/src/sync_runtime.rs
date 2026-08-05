use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use openmeter_sync_protocol::{
    derive_tracking_key, open, seal, seal_tracking, DeviceDescriptorV2, EnvelopeMeta, HistoryKey,
    HistoryPayloadV1, ProtocolError, TrackingPayloadV2,
};
use serde::Serialize;
use serde_json::{json, Value};

use crate::credential_store::{
    CredentialStore, SecretVec, DEVICE_CREDENTIAL_TARGET, HISTORY_KEY_TARGET,
};
use crate::sync_client::{PendingEnvelope, SyncClient};
use crate::sync_tracking::PeerEnvelopeCache;
use crate::tracking_projection::TrackingDashboard;

const HUB_URL: &str = "http://100.90.87.7:6740";
const HUB_HOST: &str = "100.90.87.7";
const SYNC_INTERVAL: Duration = Duration::from_secs(5 * 60);

static SYNCING: AtomicBool = AtomicBool::new(false);

#[derive(Debug, Clone, Serialize)]
pub struct SyncStatus {
    pub enabled: bool,
    pub hub_url: String,
    pub device_id: Option<String>,
    pub last_success_ms: Option<i64>,
    pub has_history_key: bool,
    pub has_device_credential: bool,
    pub tracking_last_success_ms: Option<i64>,
    pub device_label: String,
    pub protocol_version: String,
    pub devices: Vec<SyncDeviceSummary>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SyncDeviceSummary {
    pub device_id: String,
    pub label: String,
    pub protocol_version: String,
    pub client_version: String,
    pub last_generated_ms: i64,
    pub last_received_ms: i64,
    pub state: String,
}

#[derive(Debug, Serialize)]
pub struct SyncEnrollmentResult {
    pub status: SyncStatus,
    pub recovery_key: Option<String>,
}

#[tauri::command]
pub fn sync_status() -> Result<SyncStatus, String> {
    status_from_config(&crate::load_config())
}

#[tauri::command]
pub fn fetch_tracking() -> TrackingDashboard {
    dashboard()
        .lock()
        .ok()
        .and_then(|current| current.clone())
        .unwrap_or_else(empty_dashboard)
}

#[tauri::command]
pub fn sync_set_device_label(label: String) -> Result<SyncStatus, String> {
    let label = label.trim();
    if label.is_empty() || label.len() > 32 || label.chars().any(char::is_control) {
        return Err("device label must be between 1 and 32 printable characters".to_string());
    }
    let config = crate::set_config(json!({ "syncDeviceLabel": label }))?;
    status_from_config(&config)
}

#[tauri::command]
pub async fn sync_enroll(token: String) -> Result<SyncEnrollmentResult, String> {
    let token = token.trim();
    if token.len() < 20 || token.len() > 256 {
        return Err("enrollment token is invalid".to_string());
    }
    let client = client()?;
    let enrollment = client
        .enroll(token)
        .await
        .map_err(|error| error.to_string())?;
    CredentialStore::write(
        DEVICE_CREDENTIAL_TARGET,
        &SecretVec::new(enrollment.credential.into_bytes()),
    )
    .map_err(|error| error.to_string())?;

    let (history_key, recovery_key) =
        match CredentialStore::read(HISTORY_KEY_TARGET).map_err(|error| error.to_string())? {
            Some(existing) if existing.expose().len() == 32 => (None, None),
            _ => {
                let bytes = HistoryKey::random_bytes();
                CredentialStore::write(HISTORY_KEY_TARGET, &SecretVec::new(bytes.to_vec()))
                    .map_err(|error| error.to_string())?;
                (Some(bytes), Some(URL_SAFE_NO_PAD.encode(bytes)))
            }
        };
    if let Some(mut bytes) = history_key {
        bytes.fill(0);
    }
    let cfg = crate::set_config(json!({
        "syncEnabled": true,
        "syncHubUrl": HUB_URL,
        "syncDeviceId": enrollment.device_id,
        "syncRevision": 0,
        "syncLastSuccess": 0,
        "syncTrackingRevision": 0,
        "syncTrackingLastSuccess": 0,
        "syncDeviceLabel": default_device_label(),
    }))?;
    Ok(SyncEnrollmentResult {
        status: status_from_config(&cfg)?,
        recovery_key,
    })
}

#[tauri::command]
pub fn sync_import_recovery(recovery_key: String) -> Result<(), String> {
    let mut bytes = URL_SAFE_NO_PAD
        .decode(recovery_key.trim())
        .map_err(|_| "recovery key is invalid".to_string())?;
    if bytes.len() != 32 {
        bytes.fill(0);
        return Err("recovery key is invalid".to_string());
    }
    let result = CredentialStore::write(HISTORY_KEY_TARGET, &SecretVec::new(bytes.clone()))
        .map_err(|error| error.to_string());
    bytes.fill(0);
    result
}

#[tauri::command]
pub async fn sync_now() -> Result<SyncStatus, String> {
    let _guard = SyncGuard::acquire()?;
    sync_once().await
}

#[tauri::command]
pub async fn sync_revoke_device(device_id: String) -> Result<SyncStatus, String> {
    let cfg = crate::config_with_defaults(crate::load_config());
    let current = config_string(&cfg, "syncDeviceId").unwrap_or_default();
    if device_id != current || device_id.is_empty() {
        return Err("only this device can be revoked from the app".to_string());
    }
    let credential = device_credential()?;
    client()?
        .revoke(&device_id, credential.expose_str()?)
        .await
        .map_err(|error| error.to_string())?;
    disable_sync()
}

#[tauri::command]
pub fn sync_disable() -> Result<SyncStatus, String> {
    disable_sync()
}

pub fn spawn_scheduler() {
    tauri::async_runtime::spawn(async {
        tokio::time::sleep(Duration::from_secs(30)).await;
        loop {
            if crate::load_config()
                .get("syncEnabled")
                .and_then(Value::as_bool)
                .unwrap_or(false)
            {
                if let Err(error) = sync_now().await {
                    crate::note_config_error(&format!("sync scheduler: {error}"));
                }
            }
            tokio::time::sleep(SYNC_INTERVAL).await;
        }
    });
}

pub fn peer_history() -> Vec<HistoryPayloadV1> {
    peers()
        .lock()
        .map(|peers| peers.clone())
        .unwrap_or_default()
}

pub fn enabled_provider_ids(config: &Value) -> HashSet<String> {
    let disabled: HashSet<&str> = config
        .get("disabled")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect();
    crate::providers::provider_catalog()
        .iter()
        .filter(|provider| !disabled.contains(provider.id))
        .map(|provider| provider.id.to_string())
        .collect()
}

async fn sync_once() -> Result<SyncStatus, String> {
    let cfg = crate::config_with_defaults(crate::load_config());
    if !cfg
        .get("syncEnabled")
        .and_then(Value::as_bool)
        .unwrap_or(false)
    {
        return Err("sync is disabled".to_string());
    }
    let device_id = config_string(&cfg, "syncDeviceId")
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "sync device is not enrolled".to_string())?;
    let local_history_revision = cfg
        .get("syncRevision")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let local_tracking_revision = cfg
        .get("syncTrackingRevision")
        .and_then(Value::as_u64)
        .unwrap_or(0);
    let device_label = config_string(&cfg, "syncDeviceLabel")
        .filter(|label| !label.trim().is_empty())
        .unwrap_or_else(default_device_label);
    let credential = device_credential()?;
    let client = client()?;
    let remote_revisions = client.revisions(credential.expose_str()?).await;
    let history_revision = reconciled_next_revision(
        local_history_revision,
        remote_revisions.is_ok(),
        remote_revisions.as_ref().ok().and_then(|state| state.history),
    );
    let tracking_revision = reconciled_next_revision(
        local_tracking_revision,
        remote_revisions.is_ok(),
        remote_revisions.as_ref().ok().and_then(|state| state.tracking),
    );
    let mut key_bytes = history_key()?;
    let key = HistoryKey::from_bytes(key_bytes);
    let tracking_key = derive_tracking_key(&key_bytes).map_err(|error| error.to_string())?;
    key_bytes.fill(0);
    let registry =
        crate::accounts::AccountRegistry::load(&crate::account_registry_path()).unwrap_or_default();
    let cursor_csv = crate::providers::cursor::fetch_usage_csv().await;
    let environment = crate::environment::launch_environment().clone();
    let registry_for_scan = registry.clone();
    let local = tauri::async_runtime::spawn_blocking(move || {
        crate::spend::collect_for_accounts(cursor_csv, &registry_for_scan, &environment)
    })
    .await
    .map_err(|_| "local history scan failed".to_string())?;
    let payload =
        crate::sync_history::export_history(&local, &registry, &enabled_provider_ids(&cfg));
    let now_ms = chrono::Utc::now().timestamp_millis();
    let meta = EnvelopeMeta::new(&device_id, history_revision, now_ms)
        .map_err(|error| error.to_string())?;
    let envelope = seal(&key, meta, &payload).map_err(|error| error.to_string())?;
    let history_result = publish_history(
        &client,
        &device_id,
        credential.expose_str()?,
        envelope,
        &key,
    )
    .await;

    let events =
        crate::tracking_events::export_tracking_events(&local, &registry, &tracking_key, now_ms)?;
    let previous_local = PeerEnvelopeCache::load(&tracking_local_path())
        .ok()
        .and_then(|cache| cache.opened(&tracking_key).into_iter().next())
        .map(|peer| peer.current);
    let completed_providers: HashSet<String> = events
        .iter()
        .map(|event| event.provider_id.clone())
        .collect();
    let tombstones = crate::sync_tracking::next_tombstones(
        previous_local.as_ref(),
        &events,
        &completed_providers,
        now_ms,
    );
    let retained_from_day = (chrono::Utc::now().date_naive() - chrono::Duration::days(89))
        .format("%Y-%m-%d")
        .to_string();
    let mut local_tracking = TrackingPayloadV2 {
        device: DeviceDescriptorV2::new(
            &device_id,
            &device_label,
            now_ms,
            env!("CARGO_PKG_VERSION"),
        )
        .map_err(|error| error.to_string())?,
        events,
        quotas: crate::sync_tracking::local_quotas(),
        tombstones,
        retained_from_day,
    };
    let tracking_meta = EnvelopeMeta::tracking_v2(&device_id, tracking_revision, now_ms)
        .map_err(|error| error.to_string())?;
    let tracking_envelope = seal_tracking_with_retention(
        &tracking_key,
        tracking_meta,
        &mut local_tracking,
        now_ms,
    )?;
    let local_path = tracking_local_path();
    let mut local_cache = PeerEnvelopeCache::load(&local_path).unwrap_or_default();
    local_cache
        .replace_and_save(
            vec![crate::sync_client::TrackingEnvelopeRecord {
                received_at_ms: now_ms,
                envelope: tracking_envelope.clone(),
            }],
            &tracking_key,
            &local_path,
        )
        .map_err(|error| error.to_string())?;
    let tracking_result = publish_tracking(
        &client,
        &device_id,
        credential.expose_str()?,
        tracking_envelope,
        &tracking_key,
        &local_tracking,
    )
    .await;

    let peer_tracking = tracking_result
        .as_ref()
        .cloned()
        .unwrap_or_else(|_| load_tracking_peers(&tracking_key));
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    let projected = crate::tracking_projection::project_tracking(
        local_tracking,
        &peer_tracking,
        &peer_history(),
        &today,
        now_ms,
    );
    if let Ok(mut current) = dashboard().lock() {
        *current = Some(projected);
    }

    let mut patch = serde_json::Map::new();
    if history_result.is_ok() {
        patch.insert("syncRevision".into(), json!(history_revision));
        patch.insert("syncLastSuccess".into(), json!(now_ms));
    }
    if tracking_result.is_ok() {
        patch.insert("syncTrackingRevision".into(), json!(tracking_revision));
        patch.insert("syncTrackingLastSuccess".into(), json!(now_ms));
    }
    let updated = crate::set_config(Value::Object(patch))?;
    let status = status_from_config(&updated)?;
    match (history_result, tracking_result) {
        (Ok(()), Ok(_)) => Ok(status),
        (Err(history), Ok(_)) => Err(format!("history sync failed: {history}")),
        (Ok(()), Err(tracking)) => Err(format!("tracking sync failed: {tracking}")),
        (Err(history), Err(tracking)) => Err(format!(
            "history sync failed: {history}; tracking sync failed: {tracking}"
        )),
    }
}

async fn publish_history(
    client: &SyncClient,
    device_id: &str,
    credential: &str,
    envelope: openmeter_sync_protocol::EncryptedEnvelope,
    key: &HistoryKey,
) -> Result<(), String> {
    let mut pending =
        PendingEnvelope::load(client.pending_path()).map_err(|error| error.to_string())?;
    if pending.current().is_some_and(|current| {
        current.meta.schema != openmeter_sync_protocol::HISTORY_SCHEMA
            || current.meta.device_id != device_id
            || current.meta.revision != envelope.meta.revision
    }) {
        pending.clear();
    }
    pending.replace(envelope);
    pending
        .save(client.pending_path())
        .map_err(|error| error.to_string())?;
    let upload = pending
        .current()
        .ok_or_else(|| "pending history state is empty".to_string())?;
    client
        .push(device_id, credential, upload)
        .await
        .map_err(|error| error.to_string())?;
    pending.clear();
    if let Err(error) = pending.save(client.pending_path()) {
        eprintln!("[openmeter] uploaded history pending marker was not cleared: {error}");
    }
    let Ok(envelopes) = client.pull(credential).await else {
        return Ok(());
    };
    let opened: Vec<HistoryPayloadV1> = envelopes
        .iter()
        .filter_map(|envelope| open(key, envelope).ok())
        .collect();
    if let Ok(mut cached) = peers().lock() {
        *cached = opened;
    }
    Ok(())
}

async fn publish_tracking(
    client: &SyncClient,
    device_id: &str,
    credential: &str,
    envelope: openmeter_sync_protocol::EncryptedEnvelope,
    key: &openmeter_sync_protocol::TrackingKey,
    baseline: &TrackingPayloadV2,
) -> Result<Vec<crate::tracking_projection::PeerTrackingState>, String> {
    let pending_path = tracking_pending_path();
    let mut pending = PendingEnvelope::load(&pending_path).map_err(|error| error.to_string())?;
    if pending.current().is_some_and(|current| {
        current.meta.schema != openmeter_sync_protocol::TRACKING_SCHEMA
            || current.meta.device_id != device_id
            || current.meta.revision != envelope.meta.revision
    }) {
        pending.clear();
    }
    pending.replace(envelope);
    pending
        .save(&pending_path)
        .map_err(|error| error.to_string())?;
    let upload = pending
        .current()
        .ok_or_else(|| "pending tracking state is empty".to_string())?;
    client
        .push_tracking(device_id, credential, upload)
        .await
        .map_err(|error| error.to_string())?;
    pending.clear();
    if let Err(error) = pending.save(&pending_path) {
        eprintln!("[openmeter] uploaded tracking pending marker was not cleared: {error}");
    }

    let peer_path = tracking_peer_path();
    let mut cache = PeerEnvelopeCache::load(&peer_path).map_err(|error| error.to_string())?;
    let Ok(incoming) = client.pull_tracking(credential).await else {
        return Ok(cache.opened(key));
    };
    cache
        .replace_collision_safe_and_save(incoming, key, &peer_path, baseline)
        .map_err(|error| error.to_string())
}

fn load_tracking_peers(
    key: &openmeter_sync_protocol::TrackingKey,
) -> Vec<crate::tracking_projection::PeerTrackingState> {
    PeerEnvelopeCache::load(&tracking_peer_path())
        .map(|cache| cache.opened(key))
        .unwrap_or_default()
}

fn disable_sync() -> Result<SyncStatus, String> {
    CredentialStore::delete(DEVICE_CREDENTIAL_TARGET).map_err(|error| error.to_string())?;
    CredentialStore::delete(HISTORY_KEY_TARGET).map_err(|error| error.to_string())?;
    if let Ok(mut cached) = peers().lock() {
        cached.clear();
    }
    if let Ok(mut current) = dashboard().lock() {
        *current = None;
    }
    for path in [
        tracking_pending_path(),
        tracking_peer_path(),
        tracking_local_path(),
    ] {
        let _ = std::fs::remove_file(path);
    }
    let cfg = crate::set_config(json!({
        "syncEnabled": false,
        "syncDeviceId": "",
        "syncRevision": 0,
        "syncLastSuccess": 0,
        "syncTrackingRevision": 0,
        "syncTrackingLastSuccess": 0,
    }))?;
    status_from_config(&cfg)
}

fn client() -> Result<SyncClient, String> {
    SyncClient::new(
        HUB_URL,
        HUB_HOST,
        crate::providers::config_dir().join("sync-pending.json"),
    )
    .map_err(|error| error.to_string())
}

fn status_from_config(config: &Value) -> Result<SyncStatus, String> {
    let now_ms = chrono::Utc::now().timestamp_millis();
    let devices = dashboard()
        .lock()
        .ok()
        .and_then(|current| current.clone())
        .map(|dashboard| {
            dashboard
                .devices
                .into_iter()
                .map(|device| SyncDeviceSummary {
                    device_id: device.device_id,
                    label: device.label,
                    protocol_version: "v2".to_string(),
                    client_version: device.client_version.clone(),
                    last_generated_ms: device.generated_at_ms,
                    last_received_ms: device.received_at_ms,
                    state: if device.quarantined {
                        "quarantined"
                    } else if device.client_version != env!("CARGO_PKG_VERSION") {
                        "needs_upgrade"
                    } else if now_ms.saturating_sub(device.received_at_ms) > 30 * 60 * 1_000 {
                        "offline"
                    } else if now_ms.saturating_sub(device.received_at_ms) > 10 * 60 * 1_000 {
                        "delayed"
                    } else {
                        "current"
                    }
                    .to_string(),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(SyncStatus {
        enabled: config
            .get("syncEnabled")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        hub_url: HUB_URL.to_string(),
        device_id: config_string(config, "syncDeviceId").filter(|value| !value.is_empty()),
        last_success_ms: config
            .get("syncLastSuccess")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0),
        has_history_key: CredentialStore::read(HISTORY_KEY_TARGET)
            .map_err(|error| error.to_string())?
            .is_some(),
        has_device_credential: CredentialStore::read(DEVICE_CREDENTIAL_TARGET)
            .map_err(|error| error.to_string())?
            .is_some(),
        tracking_last_success_ms: config
            .get("syncTrackingLastSuccess")
            .and_then(Value::as_i64)
            .filter(|value| *value > 0),
        device_label: config_string(config, "syncDeviceLabel")
            .filter(|label| !label.is_empty())
            .unwrap_or_else(default_device_label),
        protocol_version: "v1+v2".to_string(),
        devices,
    })
}

fn config_string(config: &Value, key: &str) -> Option<String> {
    config.get(key).and_then(Value::as_str).map(str::to_string)
}

fn reconciled_next_revision(
    local_revision: u64,
    remote_was_read: bool,
    remote_revision: Option<u64>,
) -> u64 {
    if remote_was_read {
        remote_revision.unwrap_or(0).saturating_add(1)
    } else {
        local_revision.saturating_add(1)
    }
}

fn seal_tracking_with_retention(
    key: &openmeter_sync_protocol::TrackingKey,
    meta: EnvelopeMeta,
    payload: &mut TrackingPayloadV2,
    now_ms: i64,
) -> Result<openmeter_sync_protocol::EncryptedEnvelope, String> {
    loop {
        match seal_tracking(key, meta.clone(), payload) {
            Ok(envelope) => return Ok(envelope),
            Err(ProtocolError::TooLarge) if payload.events.len() > 1 => {
                let remove = (payload.events.len() / 8).max(1);
                payload.events.drain(..remove);
                payload.retained_from_day = payload
                    .events
                    .first()
                    .and_then(|event| chrono::DateTime::from_timestamp_millis(event.occurred_at_ms))
                    .map(|time| time.format("%Y-%m-%d").to_string())
                    .unwrap_or_else(|| {
                        chrono::DateTime::from_timestamp_millis(now_ms)
                            .map(|time| time.format("%Y-%m-%d").to_string())
                            .unwrap_or_else(|| "1970-01-01".to_string())
                    });
            }
            Err(ProtocolError::TooLarge) if !payload.tombstones.is_empty() => {
                payload
                    .tombstones
                    .sort_by_key(|tombstone| tombstone.removed_at_ms);
                let remove = (payload.tombstones.len() / 8).max(1);
                payload.tombstones.drain(..remove);
            }
            Err(error) => return Err(error.to_string()),
        }
    }
}

fn device_credential() -> Result<SecretText, String> {
    CredentialStore::read(DEVICE_CREDENTIAL_TARGET)
        .map_err(|error| error.to_string())?
        .map(SecretText)
        .ok_or_else(|| "sync device credential is unavailable".to_string())
}

fn history_key() -> Result<[u8; 32], String> {
    let secret = CredentialStore::read(HISTORY_KEY_TARGET)
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "sync history key is unavailable".to_string())?;
    secret
        .expose()
        .try_into()
        .map_err(|_| "sync history key is invalid".to_string())
}

struct SecretText(SecretVec);

impl SecretText {
    fn expose_str(&self) -> Result<&str, String> {
        std::str::from_utf8(self.0.expose())
            .map_err(|_| "sync device credential is invalid".to_string())
    }
}

struct SyncGuard;

impl SyncGuard {
    fn acquire() -> Result<Self, String> {
        SYNCING
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| Self)
            .map_err(|_| "sync is already running".to_string())
    }
}

impl Drop for SyncGuard {
    fn drop(&mut self) {
        SYNCING.store(false, Ordering::Release);
    }
}

fn tracking_pending_path() -> std::path::PathBuf {
    crate::providers::config_dir().join("sync-pending-v2.json")
}

fn tracking_peer_path() -> std::path::PathBuf {
    crate::providers::config_dir().join("sync-peers-v2.json")
}

fn tracking_local_path() -> std::path::PathBuf {
    crate::providers::config_dir().join("sync-local-v2.json")
}

fn default_device_label() -> String {
    std::env::var("COMPUTERNAME")
        .ok()
        .map(|label| label.trim().chars().take(32).collect::<String>())
        .filter(|label| !label.is_empty() && !label.chars().any(char::is_control))
        .unwrap_or_else(|| "This device".to_string())
}

fn empty_dashboard() -> TrackingDashboard {
    TrackingDashboard {
        generated_at_ms: chrono::Utc::now().timestamp_millis(),
        devices: vec![],
        days: vec![],
        quotas: vec![],
        legacy_present: false,
        unique_events: 0,
        quarantined_devices: vec![],
    }
}

fn dashboard() -> &'static Mutex<Option<TrackingDashboard>> {
    static DASHBOARD: OnceLock<Mutex<Option<TrackingDashboard>>> = OnceLock::new();
    DASHBOARD.get_or_init(|| Mutex::new(None))
}

fn peers() -> &'static Mutex<Vec<HistoryPayloadV1>> {
    static PEERS: OnceLock<Mutex<Vec<HistoryPayloadV1>>> = OnceLock::new();
    PEERS.get_or_init(|| Mutex::new(Vec::new()))
}

#[cfg(test)]
mod tests {
    use super::reconciled_next_revision;

    #[test]
    fn revision_recovery_uses_the_hub_after_a_local_rollback() {
        assert_eq!(reconciled_next_revision(128, true, Some(135)), 136);
    }

    #[test]
    fn revision_recovery_starts_at_one_after_a_hub_reset() {
        assert_eq!(reconciled_next_revision(128, true, None), 1);
    }

    #[test]
    fn revision_recovery_falls_back_to_local_state_when_pull_is_unavailable() {
        assert_eq!(reconciled_next_revision(128, false, None), 129);
    }
}
