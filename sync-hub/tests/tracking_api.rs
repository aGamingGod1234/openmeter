use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use openmeter_sync_hub::{router, Hub, Store};
use openmeter_sync_protocol::{EncryptedEnvelope, EnvelopeMeta};
use serde_json::{json, Value};
use tower::ServiceExt;

#[test]
fn tracking_storage_is_additive_to_an_existing_v1_database() {
    let path = temp_db();
    let store = Store::open(&path).unwrap();
    store.enroll_device("device-a", [1; 32], 900).unwrap();
    store.put("device-a", &v1("device-a", 1), 1_000).unwrap();
    drop(store);

    let store = Store::open(&path).unwrap();
    assert_eq!(store.list_active("other", 1_001).unwrap().len(), 1);
    assert_eq!(store.list_active_tracking("other", 1_001).unwrap().len(), 0);
    store
        .put_tracking("device-a", &v2("device-a", 1), 1_002)
        .unwrap();
    assert_eq!(store.list_active("other", 1_003).unwrap().len(), 1);
    let tracking = store.list_active_tracking("other", 1_003).unwrap();
    assert_eq!(tracking.len(), 1);
    assert_eq!(tracking[0].received_at_ms, 1_002);
    assert_eq!(tracking[0].envelope.meta.revision, 1);

    cleanup(path);
}

#[test]
fn envelope_schemas_cannot_cross_storage_versions() {
    let path = temp_db();
    let store = Store::open(&path).unwrap();
    store.enroll_device("device-a", [1; 32], 900).unwrap();

    assert_eq!(
        store.put_tracking("device-a", &v1("device-a", 1), 1_000),
        Err(openmeter_sync_hub::PutError::InvalidEnvelope)
    );
    assert_eq!(
        store.put("device-a", &v2("device-a", 1), 1_000),
        Err(openmeter_sync_hub::PutError::InvalidEnvelope)
    );

    cleanup(path);
}

#[tokio::test]
async fn v1_and_v2_upload_independently_and_v2_lists_receipt_metadata() {
    let path = temp_db();
    let hub = Hub::new(Store::open(&path).unwrap(), [3; 32]);
    let token_a = hub.create_enrollment(now_ms()).unwrap();
    let token_b = hub.create_enrollment(now_ms()).unwrap();
    let app = router(hub);
    let enrolled_a =
        request_json(&app, "POST", "/v1/enroll", None, json!({"token": token_a})).await;
    let enrolled_b =
        request_json(&app, "POST", "/v1/enroll", None, json!({"token": token_b})).await;
    let device_a = enrolled_a.1["device_id"].as_str().unwrap();
    let credential_a = enrolled_a.1["credential"].as_str().unwrap();
    let credential_b = enrolled_b.1["credential"].as_str().unwrap();

    let uploaded_v1 = request_json(
        &app,
        "PUT",
        &format!("/v1/devices/{device_a}/envelope"),
        Some(credential_a),
        serde_json::to_value(v1(device_a, 1)).unwrap(),
    )
    .await;
    let uploaded_v2 = request_json(
        &app,
        "PUT",
        &format!("/v2/devices/{device_a}/envelope"),
        Some(credential_a),
        serde_json::to_value(v2(device_a, 1)).unwrap(),
    )
    .await;
    assert_eq!(uploaded_v1.0, StatusCode::NO_CONTENT);
    assert_eq!(uploaded_v2.0, StatusCode::NO_CONTENT);

    let listed = request_json(
        &app,
        "GET",
        "/v2/envelopes",
        Some(credential_b),
        Value::Null,
    )
    .await;
    assert_eq!(listed.0, StatusCode::OK);
    assert_eq!(listed.1.as_array().unwrap().len(), 1);
    assert!(listed.1[0]["received_at_ms"].as_i64().is_some());
    assert_eq!(
        listed.1[0]["envelope"]["meta"]["schema"],
        "openmeter.tracking.v2"
    );

    cleanup(path);
}

async fn request_json(
    app: &axum::Router,
    method: &str,
    uri: &str,
    bearer: Option<&str>,
    value: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(bearer) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    let body = if method == "GET" {
        Body::empty()
    } else {
        Body::from(value.to_string())
    };
    let response = app
        .clone()
        .oneshot(request.body(body).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes).unwrap()
    };
    (status, value)
}

fn v1(device_id: &str, revision: u64) -> EncryptedEnvelope {
    EncryptedEnvelope {
        meta: EnvelopeMeta::new(device_id, revision, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![9; 32],
    }
}

fn v2(device_id: &str, revision: u64) -> EncryptedEnvelope {
    EncryptedEnvelope {
        meta: EnvelopeMeta::tracking_v2(device_id, revision, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![9; 32],
    }
}

fn temp_db() -> PathBuf {
    static NEXT_DB: AtomicU64 = AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "openmeter-sync-tracking-{}-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos(),
        NEXT_DB.fetch_add(1, Ordering::Relaxed)
    ))
}

fn cleanup(path: PathBuf) {
    let _ = std::fs::remove_file(&path);
    let _ = std::fs::remove_file(path.with_extension("db-shm"));
    let _ = std::fs::remove_file(path.with_extension("db-wal"));
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
