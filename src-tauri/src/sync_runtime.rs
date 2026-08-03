use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use openmeter_sync_protocol::{open, seal, EnvelopeMeta, HistoryKey, HistoryPayloadV1};
use serde::Serialize;
use serde_json::{json, Value};

use crate::credential_store::{
    CredentialStore, SecretVec, DEVICE_CREDENTIAL_TARGET, HISTORY_KEY_TARGET,
};
use crate::sync_client::{PendingEnvelope, SyncClient};

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
                let _ = sync_now().await;
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
    let revision = cfg
        .get("syncRevision")
        .and_then(Value::as_u64)
        .unwrap_or(0)
        .saturating_add(1);
    let credential = device_credential()?;
    let mut key_bytes = history_key()?;
    let key = HistoryKey::from_bytes(key_bytes);
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
    let meta = EnvelopeMeta::new(&device_id, revision, chrono::Utc::now().timestamp_millis())
        .map_err(|error| error.to_string())?;
    let envelope = seal(&key, meta, &payload).map_err(|error| error.to_string())?;
    let mut pending =
        PendingEnvelope::load(client()?.pending_path()).map_err(|error| error.to_string())?;
    pending.replace(envelope);
    pending
        .save(client()?.pending_path())
        .map_err(|error| error.to_string())?;
    let upload = pending
        .current()
        .ok_or_else(|| "pending sync state is empty".to_string())?;
    let client = client()?;
    client
        .push(&device_id, credential.expose_str()?, upload)
        .await
        .map_err(|error| error.to_string())?;
    pending.clear();
    pending
        .save(client.pending_path())
        .map_err(|error| error.to_string())?;

    let envelopes = client
        .pull(credential.expose_str()?)
        .await
        .map_err(|error| error.to_string())?;
    let opened: Vec<HistoryPayloadV1> = envelopes
        .iter()
        .filter_map(|envelope| open(&key, envelope).ok())
        .collect();
    if let Ok(mut cached) = peers().lock() {
        *cached = opened;
    }
    let cfg = crate::set_config(json!({
        "syncRevision": revision,
        "syncLastSuccess": chrono::Utc::now().timestamp_millis(),
    }))?;
    status_from_config(&cfg)
}

fn disable_sync() -> Result<SyncStatus, String> {
    CredentialStore::delete(DEVICE_CREDENTIAL_TARGET).map_err(|error| error.to_string())?;
    CredentialStore::delete(HISTORY_KEY_TARGET).map_err(|error| error.to_string())?;
    if let Ok(mut cached) = peers().lock() {
        cached.clear();
    }
    let cfg = crate::set_config(json!({
        "syncEnabled": false,
        "syncDeviceId": "",
        "syncRevision": 0,
        "syncLastSuccess": 0,
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
    })
}

fn config_string(config: &Value, key: &str) -> Option<String> {
    config.get(key).and_then(Value::as_str).map(str::to_string)
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

fn peers() -> &'static Mutex<Vec<HistoryPayloadV1>> {
    static PEERS: OnceLock<Mutex<Vec<HistoryPayloadV1>>> = OnceLock::new();
    PEERS.get_or_init(|| Mutex::new(Vec::new()))
}
