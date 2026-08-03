use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use axum::extract::{DefaultBodyLimit, Path, State};
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use hmac::{Hmac, Mac};
use openmeter_sync_protocol::{EncryptedEnvelope, MAX_ENVELOPE_BYTES};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::Sha256;
use tower_http::timeout::TimeoutLayer;

use crate::{HubError, PutError, Store};

const ENROLLMENT_LIFETIME_MS: i64 = 15 * 60 * 1_000;
const UPLOAD_INTERVAL_MS: i64 = 60 * 1_000;

#[derive(Clone)]
pub struct Hub {
    store: Arc<Store>,
    pepper: [u8; 32],
    uploads: Arc<Mutex<HashMap<String, i64>>>,
}

impl Hub {
    pub fn new(store: Store, pepper: [u8; 32]) -> Self {
        Self {
            store: Arc::new(store),
            pepper,
            uploads: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn create_enrollment(&self, now_ms: i64) -> Result<String, HubError> {
        let mut token = [0_u8; 32];
        rand::thread_rng().fill_bytes(&mut token);
        let token = URL_SAFE_NO_PAD.encode(token);
        self.store.create_enrollment(
            credential_hash(&self.pepper, token.as_bytes()),
            now_ms.saturating_add(ENROLLMENT_LIFETIME_MS),
        )?;
        Ok(token)
    }

    pub fn store(&self) -> &Store {
        &self.store
    }
}

pub fn router(hub: Hub) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/v1/enroll", post(enroll))
        .route("/v1/envelopes", get(list_envelopes))
        .route("/v1/devices/{device_id}/envelope", put(put_envelope))
        .route("/v1/devices/{device_id}", delete(revoke_device))
        .layer(DefaultBodyLimit::max(MAX_ENVELOPE_BYTES))
        .layer(TimeoutLayer::with_status_code(
            StatusCode::REQUEST_TIMEOUT,
            Duration::from_secs(15),
        ))
        .with_state(hub)
}

async fn health() -> StatusCode {
    StatusCode::NO_CONTENT
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnrollmentRequest {
    token: String,
}

#[derive(Serialize)]
struct EnrollmentResponse {
    device_id: String,
    credential: String,
}

async fn enroll(
    State(hub): State<Hub>,
    Json(request): Json<EnrollmentRequest>,
) -> Result<Json<EnrollmentResponse>, ApiError> {
    let now = now_ms()?;
    let token_hash = credential_hash(&hub.pepper, request.token.as_bytes());
    if !hub.store.consume_enrollment(token_hash, now)? {
        return Err(ApiError::Unauthorized);
    }

    let mut id_bytes = [0_u8; 16];
    let mut secret_bytes = [0_u8; 32];
    rand::thread_rng().fill_bytes(&mut id_bytes);
    rand::thread_rng().fill_bytes(&mut secret_bytes);
    let device_id = format!("device-{}", encode_hex(&id_bytes));
    let secret = URL_SAFE_NO_PAD.encode(secret_bytes);
    let credential = format!("{device_id}.{secret}");
    hub.store.enroll_device(
        &device_id,
        credential_hash(&hub.pepper, credential.as_bytes()),
        now,
    )?;

    Ok(Json(EnrollmentResponse {
        device_id,
        credential,
    }))
}

async fn put_envelope(
    State(hub): State<Hub>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
    Json(envelope): Json<EncryptedEnvelope>,
) -> Result<StatusCode, ApiError> {
    authenticate(&hub, &headers, Some(&device_id))?;
    let now = now_ms()?;
    let mut uploads = hub.uploads.lock().map_err(|_| ApiError::Unavailable)?;
    if uploads
        .get(&device_id)
        .is_some_and(|previous| now.saturating_sub(*previous) < UPLOAD_INTERVAL_MS)
    {
        return Err(ApiError::TooManyRequests);
    }
    hub.store.put(&device_id, &envelope, now)?;
    uploads.insert(device_id, now);
    Ok(StatusCode::NO_CONTENT)
}

async fn list_envelopes(
    State(hub): State<Hub>,
    headers: HeaderMap,
) -> Result<Json<Vec<EncryptedEnvelope>>, ApiError> {
    let device_id = authenticate(&hub, &headers, None)?;
    Ok(Json(hub.store.list_active(&device_id, now_ms()?)?))
}

async fn revoke_device(
    State(hub): State<Hub>,
    Path(device_id): Path<String>,
    headers: HeaderMap,
) -> Result<StatusCode, ApiError> {
    authenticate(&hub, &headers, Some(&device_id))?;
    hub.store.revoke(&device_id, now_ms()?)?;
    Ok(StatusCode::NO_CONTENT)
}

fn authenticate(
    hub: &Hub,
    headers: &HeaderMap,
    expected_device: Option<&str>,
) -> Result<String, ApiError> {
    let authorization = headers
        .get(header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .ok_or(ApiError::Unauthorized)?;
    let (device_id, _) = authorization
        .split_once('.')
        .ok_or(ApiError::Unauthorized)?;
    if expected_device.is_some_and(|expected| expected != device_id) {
        return Err(ApiError::Unauthorized);
    }
    let device = hub
        .store
        .devices()?
        .into_iter()
        .find(|device| device.device_id == device_id)
        .ok_or(ApiError::Unauthorized)?;
    let mut verifier =
        Hmac::<Sha256>::new_from_slice(&hub.pepper).map_err(|_| ApiError::Unavailable)?;
    verifier.update(authorization.as_bytes());
    verifier
        .verify_slice(&device.credential_hash)
        .map_err(|_| ApiError::Unauthorized)?;
    if device.revoked {
        return Err(ApiError::Forbidden);
    }
    Ok(device_id.to_owned())
}

fn credential_hash(pepper: &[u8; 32], credential: &[u8]) -> [u8; 32] {
    let mut mac = Hmac::<Sha256>::new_from_slice(pepper).expect("HMAC accepts any key length");
    mac.update(credential);
    mac.finalize().into_bytes().into()
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        encoded.push(HEX[(byte >> 4) as usize] as char);
        encoded.push(HEX[(byte & 0x0f) as usize] as char);
    }
    encoded
}

fn now_ms() -> Result<i64, ApiError> {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| ApiError::Unavailable)?
        .as_millis();
    i64::try_from(millis).map_err(|_| ApiError::Unavailable)
}

#[derive(Debug)]
enum ApiError {
    BadRequest,
    Unauthorized,
    Forbidden,
    Conflict,
    TooManyRequests,
    Unavailable,
}

impl From<HubError> for ApiError {
    fn from(error: HubError) -> Self {
        match error {
            HubError::InvalidRecord => Self::BadRequest,
            HubError::UnknownDevice => Self::Unauthorized,
            HubError::Database => Self::Unavailable,
        }
    }
}

impl From<PutError> for ApiError {
    fn from(error: PutError) -> Self {
        match error {
            PutError::DeviceMismatch | PutError::InvalidEnvelope => Self::BadRequest,
            PutError::Replay | PutError::RevisionGap => Self::Conflict,
            PutError::UnknownDevice => Self::Unauthorized,
            PutError::Revoked => Self::Forbidden,
            PutError::Database => Self::Unavailable,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, code) = match self {
            Self::BadRequest => (StatusCode::BAD_REQUEST, "bad_request"),
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            Self::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            Self::Conflict => (StatusCode::CONFLICT, "revision_conflict"),
            Self::TooManyRequests => (StatusCode::TOO_MANY_REQUESTS, "rate_limited"),
            Self::Unavailable => (StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        };
        (status, Json(json!({ "error": code }))).into_response()
    }
}
