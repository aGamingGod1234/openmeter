use axum::body::Body;
use axum::http::{header, Request, StatusCode};
use http_body_util::BodyExt;
use openmeter_sync_hub::{router, Hub, Store};
use openmeter_sync_protocol::{
    EncryptedEnvelope, EnvelopeMeta, MAX_ENVELOPE_BYTES, MAX_ENVELOPE_WIRE_BYTES,
};
use serde_json::{json, Value};
use tower::ServiceExt;

#[tokio::test]
async fn enrollment_is_single_use_and_upload_requires_device_bearer() {
    let db_path = temp_db();
    let hub = Hub::new(Store::open(&db_path).unwrap(), [3; 32]);
    let now = now_ms();
    let token = hub.create_enrollment(now).unwrap();
    let app = router(hub);

    let enrolled = send_json(&app, "/v1/enroll", None, json!({"token": token})).await;
    assert_eq!(enrolled.0, StatusCode::OK);
    let device_id = enrolled.1["device_id"].as_str().unwrap();
    let credential = enrolled.1["credential"].as_str().unwrap();

    let reused = send_json(&app, "/v1/enroll", None, json!({"token": token})).await;
    assert_eq!(reused.0, StatusCode::UNAUTHORIZED);

    let envelope = EncryptedEnvelope {
        meta: EnvelopeMeta::new(device_id, 1, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![9; 32],
    };
    let upload_path = format!("/v1/devices/{device_id}/envelope");
    let unauthenticated = send_json(
        &app,
        &upload_path,
        None,
        serde_json::to_value(&envelope).unwrap(),
    )
    .await;
    assert_eq!(unauthenticated.0, StatusCode::UNAUTHORIZED);
    let uploaded = send_json(
        &app,
        &upload_path,
        Some(credential),
        serde_json::to_value(&envelope).unwrap(),
    )
    .await;
    assert_eq!(uploaded.0, StatusCode::NO_CONTENT);

    let revisions = send_json(&app, "/v1/revisions", Some(credential), Value::Null).await;
    assert_eq!(revisions.0, StatusCode::OK);
    assert_eq!(revisions.1, json!({"history": 1, "tracking": null}));

    cleanup(db_path);
}

#[tokio::test]
async fn oversized_body_is_rejected_before_json_parsing() {
    let path = temp_db();
    let hub = Hub::new(Store::open(&path).unwrap(), [3; 32]);
    let app = router(hub);
    let request = Request::builder()
        .method("POST")
        .uri("/v1/enroll")
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(vec![b'x'; MAX_ENVELOPE_WIRE_BYTES + 1]))
        .unwrap();

    let response = app.oneshot(request).await.unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert!(response
        .headers()
        .get(header::ACCESS_CONTROL_ALLOW_ORIGIN)
        .is_none());
    cleanup(path);
}

#[tokio::test]
async fn maximum_valid_ciphertext_fits_the_wire_body_limit() {
    let db_path = temp_db();
    let hub = Hub::new(Store::open(&db_path).unwrap(), [3; 32]);
    let token = hub.create_enrollment(now_ms()).unwrap();
    let app = router(hub);
    let enrolled = send_json(&app, "/v1/enroll", None, json!({"token": token})).await;
    let device_id = enrolled.1["device_id"].as_str().unwrap();
    let credential = enrolled.1["credential"].as_str().unwrap();
    let envelope = EncryptedEnvelope {
        meta: EnvelopeMeta::new(device_id, 1, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![255; MAX_ENVELOPE_BYTES],
    };

    let uploaded = send_json(
        &app,
        &format!("/v1/devices/{device_id}/envelope"),
        Some(credential),
        serde_json::to_value(envelope).unwrap(),
    )
    .await;

    assert_eq!(uploaded.0, StatusCode::NO_CONTENT);
    cleanup(db_path);
}

async fn send_json(
    app: &axum::Router,
    uri: &str,
    bearer: Option<&str>,
    value: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(if uri == "/v1/revisions" {
            "GET"
        } else if uri.contains("/envelope") {
            "PUT"
        } else {
            "POST"
        })
        .uri(uri)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(bearer) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {bearer}"));
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(value.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    let value = if bytes.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&bytes)
            .unwrap_or_else(|_| Value::String(String::from_utf8_lossy(&bytes).into_owned()))
    };
    (status, value)
}

fn temp_db() -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-sync-api-{}-{}.db",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn cleanup(path: std::path::PathBuf) {
    let _ = std::fs::remove_file(path);
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}
