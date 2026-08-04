use std::time::Duration;

use std::io::{Read, Write};
use std::net::TcpListener;

use openmeter_lib::sync_client::{PendingEnvelope, RetryState, SyncClient, TrackingEnvelopeRecord};
use openmeter_sync_protocol::{EncryptedEnvelope, EnvelopeMeta};

#[test]
fn newest_pending_snapshot_replaces_older_and_backoff_is_capped() {
    let mut queue = PendingEnvelope::default();
    queue.replace(envelope(4));
    queue.replace(envelope(5));

    assert_eq!(queue.current().unwrap().meta.revision, 5);
    assert!(RetryState::after_failures(20).maximum_delay() <= Duration::from_secs(30 * 60));
}

#[test]
fn stale_pending_snapshot_cannot_replace_a_newer_revision() {
    let mut queue = PendingEnvelope::default();
    queue.replace(envelope(5));
    queue.replace(envelope(4));

    assert_eq!(queue.current().unwrap().meta.revision, 5);
}

fn envelope(revision: u64) -> EncryptedEnvelope {
    EncryptedEnvelope {
        meta: EnvelopeMeta::new("device-test", revision, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![9; 32],
    }
}

#[tokio::test]
async fn tracking_transport_uses_exact_v2_paths_and_receipt_records() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let tracking = tracking_envelope(1);
    let response_body = serde_json::to_string(&vec![TrackingEnvelopeRecord {
        received_at_ms: 1_800_000_000_100,
        envelope: tracking.clone(),
    }])
    .unwrap();
    let server = std::thread::spawn(move || {
        let first = serve_once(&listener, "204 No Content", "");
        let second = serve_once(&listener, "200 OK", &response_body);
        (first, second)
    });
    let client = SyncClient::new(
        &format!("http://{address}/"),
        "127.0.0.1",
        std::env::temp_dir().join("unused-sync-pending.json"),
    )
    .unwrap();

    client
        .push_tracking("device-test", "device-test.secret", &tracking)
        .await
        .unwrap();
    let pulled = client.pull_tracking("device-test.secret").await.unwrap();
    let (first, second) = server.join().unwrap();

    assert!(first.starts_with("PUT /v2/devices/device-test/envelope HTTP/1.1"));
    assert!(second.starts_with("GET /v2/envelopes HTTP/1.1"));
    assert_eq!(pulled[0].received_at_ms, 1_800_000_000_100);
}

#[tokio::test]
async fn tracking_push_rejects_a_mismatched_device_before_network_io() {
    let client = SyncClient::new(
        "http://127.0.0.1:9/",
        "127.0.0.1",
        std::env::temp_dir().join("unused-sync-pending.json"),
    )
    .unwrap();

    let error = client
        .push_tracking("another-device", "credential", &tracking_envelope(1))
        .await
        .unwrap_err();

    assert_eq!(
        error,
        openmeter_lib::sync_client::SyncError::InvalidEnvelope
    );
}

fn tracking_envelope(revision: u64) -> EncryptedEnvelope {
    EncryptedEnvelope {
        meta: EnvelopeMeta::tracking_v2("device-test", revision, 1_800_000_000_000).unwrap(),
        nonce: vec![7; 24],
        ciphertext: vec![9; 32],
    }
}

fn serve_once(listener: &TcpListener, status: &str, body: &str) -> String {
    let (mut stream, _) = listener.accept().unwrap();
    let mut bytes = vec![0_u8; 16 * 1024];
    let read = stream.read(&mut bytes).unwrap();
    let request = String::from_utf8_lossy(&bytes[..read]).into_owned();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(response.as_bytes()).unwrap();
    request
}
