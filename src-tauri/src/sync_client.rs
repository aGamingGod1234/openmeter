use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use openmeter_sync_protocol::{EncryptedEnvelope, MAX_ENVELOPE_BYTES, TRACKING_SCHEMA};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::{Client, StatusCode, Url};
use serde::{Deserialize, Serialize};

const REQUEST_TIMEOUT: Duration = Duration::from_secs(15);
const MAX_BACKOFF: Duration = Duration::from_secs(30 * 60);
const MAX_PULL_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PendingEnvelope(Option<EncryptedEnvelope>);

impl PendingEnvelope {
    pub fn replace(&mut self, envelope: EncryptedEnvelope) {
        if self
            .0
            .as_ref()
            .is_none_or(|current| envelope.meta.revision > current.meta.revision)
        {
            self.0 = Some(envelope);
        }
    }

    pub fn current(&self) -> Option<&EncryptedEnvelope> {
        self.0.as_ref()
    }

    pub fn clear(&mut self) {
        self.0 = None;
    }

    pub fn load(path: &Path) -> Result<Self, SyncError> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let bytes = std::fs::read(path).map_err(|_| SyncError::PendingStorage)?;
        serde_json::from_slice(&bytes).map_err(|_| SyncError::PendingStorage)
    }

    pub fn save(&self, path: &Path) -> Result<(), SyncError> {
        let bytes = serde_json::to_vec(self).map_err(|_| SyncError::PendingStorage)?;
        crate::platform::atomic_write(path, &bytes).map_err(|_| SyncError::PendingStorage)
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct RetryState {
    failures: u32,
}

impl RetryState {
    pub fn after_failures(failures: u32) -> Self {
        Self { failures }
    }

    pub fn maximum_delay(&self) -> Duration {
        let exponent = self.failures.min(16);
        Duration::from_secs(5_u64.saturating_mul(1_u64 << exponent)).min(MAX_BACKOFF)
    }

    pub fn record_failure(&mut self) -> Duration {
        self.failures = self.failures.saturating_add(1);
        let maximum = self.maximum_delay().as_millis() as u64;
        if maximum == 0 {
            return Duration::ZERO;
        }
        let jitter = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos() as u64 % (maximum + 1))
            .unwrap_or(maximum);
        Duration::from_millis(jitter)
    }

    pub fn record_success(&mut self) {
        self.failures = 0;
    }
}

#[derive(Debug, Deserialize)]
pub struct Enrollment {
    pub device_id: String,
    pub credential: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingEnvelopeRecord {
    pub received_at_ms: i64,
    pub envelope: EncryptedEnvelope,
}

pub struct SyncClient {
    http: Client,
    hub: Url,
    pending_path: PathBuf,
}

impl SyncClient {
    pub fn new(
        hub_url: &str,
        allowed_host: &str,
        pending_path: PathBuf,
    ) -> Result<Self, SyncError> {
        let hub = Url::parse(hub_url).map_err(|_| SyncError::Configuration)?;
        let host_matches = hub.host_str() == Some(allowed_host);
        let scheme_allowed = hub.scheme() == "http" || hub.scheme() == "https";
        if !host_matches
            || !scheme_allowed
            || hub.username() != ""
            || hub.password().is_some()
            || hub.path() != "/"
            || hub.query().is_some()
            || hub.fragment().is_some()
        {
            return Err(SyncError::Configuration);
        }
        let http = Client::builder()
            .timeout(REQUEST_TIMEOUT)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .map_err(|_| SyncError::Configuration)?;
        Ok(Self {
            http,
            hub,
            pending_path,
        })
    }

    pub fn pending_path(&self) -> &Path {
        &self.pending_path
    }

    pub async fn enroll(&self, token: &str) -> Result<Enrollment, SyncError> {
        let response = self
            .http
            .post(self.endpoint("v1/enroll")?)
            .header(CONTENT_TYPE, "application/json")
            .json(&serde_json::json!({ "token": token }))
            .send()
            .await
            .map_err(|_| SyncError::Network)?;
        response
            .error_for_status()
            .map_err(map_status)?
            .json()
            .await
            .map_err(|_| SyncError::InvalidResponse)
    }

    pub async fn push(
        &self,
        device_id: &str,
        credential: &str,
        envelope: &EncryptedEnvelope,
    ) -> Result<(), SyncError> {
        if !valid_device_id(device_id)
            || envelope.meta.device_id != device_id
            || envelope.ciphertext.len() > MAX_ENVELOPE_BYTES
        {
            return Err(SyncError::InvalidEnvelope);
        }
        let response = self
            .http
            .put(self.endpoint(&format!("v1/devices/{device_id}/envelope"))?)
            .bearer_auth(credential)
            .json(envelope)
            .send()
            .await
            .map_err(|_| SyncError::Network)?;
        response.error_for_status().map_err(map_status)?;
        Ok(())
    }

    pub async fn pull(&self, credential: &str) -> Result<Vec<EncryptedEnvelope>, SyncError> {
        let response = self
            .http
            .get(self.endpoint("v1/envelopes")?)
            .header(AUTHORIZATION, format!("Bearer {credential}"))
            .send()
            .await
            .map_err(|_| SyncError::Network)?
            .error_for_status()
            .map_err(map_status)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PULL_BYTES as u64)
        {
            return Err(SyncError::InvalidResponse);
        }
        let bytes = response.bytes().await.map_err(|_| SyncError::Network)?;
        if bytes.len() > MAX_PULL_BYTES {
            return Err(SyncError::InvalidResponse);
        }
        serde_json::from_slice(&bytes).map_err(|_| SyncError::InvalidResponse)
    }

    pub async fn push_tracking(
        &self,
        device_id: &str,
        credential: &str,
        envelope: &EncryptedEnvelope,
    ) -> Result<(), SyncError> {
        if !valid_device_id(device_id)
            || envelope.meta.device_id != device_id
            || envelope.meta.schema != TRACKING_SCHEMA
            || envelope.nonce.len() != 24
            || envelope.ciphertext.len() > MAX_ENVELOPE_BYTES
        {
            return Err(SyncError::InvalidEnvelope);
        }
        let response = self
            .http
            .put(self.endpoint(&format!("v2/devices/{device_id}/envelope"))?)
            .bearer_auth(credential)
            .json(envelope)
            .send()
            .await
            .map_err(|_| SyncError::Network)?;
        response.error_for_status().map_err(map_status)?;
        Ok(())
    }

    pub async fn pull_tracking(
        &self,
        credential: &str,
    ) -> Result<Vec<TrackingEnvelopeRecord>, SyncError> {
        let response = self
            .http
            .get(self.endpoint("v2/envelopes")?)
            .header(AUTHORIZATION, format!("Bearer {credential}"))
            .send()
            .await
            .map_err(|_| SyncError::Network)?
            .error_for_status()
            .map_err(map_status)?;
        if response
            .content_length()
            .is_some_and(|length| length > MAX_PULL_BYTES as u64)
        {
            return Err(SyncError::InvalidResponse);
        }
        let bytes = response.bytes().await.map_err(|_| SyncError::Network)?;
        if bytes.len() > MAX_PULL_BYTES {
            return Err(SyncError::InvalidResponse);
        }
        let records: Vec<TrackingEnvelopeRecord> =
            serde_json::from_slice(&bytes).map_err(|_| SyncError::InvalidResponse)?;
        if records.iter().any(|record| {
            record.received_at_ms < 0
                || record.envelope.meta.schema != TRACKING_SCHEMA
                || !valid_device_id(&record.envelope.meta.device_id)
                || record.envelope.nonce.len() != 24
                || record.envelope.ciphertext.len() > MAX_ENVELOPE_BYTES
        }) {
            return Err(SyncError::InvalidResponse);
        }
        Ok(records)
    }

    pub async fn revoke(&self, device_id: &str, credential: &str) -> Result<(), SyncError> {
        if !valid_device_id(device_id) {
            return Err(SyncError::Configuration);
        }
        let response = self
            .http
            .delete(self.endpoint(&format!("v1/devices/{device_id}"))?)
            .bearer_auth(credential)
            .send()
            .await
            .map_err(|_| SyncError::Network)?;
        response.error_for_status().map_err(map_status)?;
        Ok(())
    }

    fn endpoint(&self, relative: &str) -> Result<Url, SyncError> {
        self.hub
            .join(relative)
            .map_err(|_| SyncError::Configuration)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncError {
    Configuration,
    Network,
    Authentication,
    Conflict,
    RateLimited,
    InvalidEnvelope,
    InvalidResponse,
    PendingStorage,
    HubUnavailable,
}

impl fmt::Display for SyncError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            Self::Configuration => "sync configuration is invalid",
            Self::Network => "sync hub could not be reached",
            Self::Authentication => "sync device authentication failed",
            Self::Conflict => "sync revision is out of date",
            Self::RateLimited => "sync upload was rate limited",
            Self::InvalidEnvelope => "sync envelope is invalid",
            Self::InvalidResponse => "sync hub returned an invalid response",
            Self::PendingStorage => "encrypted pending sync state is unavailable",
            Self::HubUnavailable => "sync hub is unavailable",
        };
        formatter.write_str(message)
    }
}

impl std::error::Error for SyncError {}

fn map_status(error: reqwest::Error) -> SyncError {
    match error.status() {
        Some(StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) => SyncError::Authentication,
        Some(StatusCode::CONFLICT) => SyncError::Conflict,
        Some(StatusCode::TOO_MANY_REQUESTS) => SyncError::RateLimited,
        Some(StatusCode::BAD_REQUEST | StatusCode::PAYLOAD_TOO_LARGE) => SyncError::InvalidEnvelope,
        Some(_) => SyncError::HubUnavailable,
        None => SyncError::Network,
    }
}

fn valid_device_id(device_id: &str) -> bool {
    !device_id.is_empty()
        && device_id.len() <= 64
        && device_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
}
