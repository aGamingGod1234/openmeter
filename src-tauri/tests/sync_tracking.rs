use openmeter_lib::sync_client::TrackingEnvelopeRecord;
use openmeter_lib::sync_tracking::{next_tombstones, normalized_quotas, PeerEnvelopeCache};
use openmeter_lib::{
    accounts::{AccountContext, AccountRegistry},
    providers::{Metric, Snapshot},
};
use openmeter_sync_protocol::{
    derive_tracking_key, seal_tracking, DeviceDescriptorV2, EnvelopeMeta, TokenUsageV2,
    TrackingPayloadV2, UsageEventV2,
};
use std::collections::HashSet;

#[test]
fn peer_cache_atomically_replaces_and_reopens_only_encrypted_records() {
    let path = temp_file("peer-cache");
    let key = derive_tracking_key(&[7_u8; 32]).unwrap();
    let mut cache = PeerEnvelopeCache::default();
    cache
        .replace_and_save(vec![record("device-a", "Private Laptop", 1)], &key, &path)
        .unwrap();

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(!raw.contains("Private Laptop"));
    let reopened = PeerEnvelopeCache::load(&path).unwrap();
    let peers = reopened.opened(&key);
    assert_eq!(peers.len(), 1);
    assert_eq!(peers[0].current.device.label, "Private Laptop");
    assert_eq!(peers[0].received_at_ms, 1_800_000_000_101);

    cleanup(path);
}

#[test]
fn invalid_new_peer_record_keeps_the_prior_last_good_envelope() {
    let path = temp_file("last-good");
    let key = derive_tracking_key(&[7_u8; 32]).unwrap();
    let mut cache = PeerEnvelopeCache::default();
    cache
        .replace_and_save(vec![record("device-a", "Laptop", 1)], &key, &path)
        .unwrap();
    let mut invalid = record("device-a", "Laptop", 2);
    invalid.envelope.ciphertext[0] ^= 0xff;

    cache.replace_and_save(vec![invalid], &key, &path).unwrap();

    assert_eq!(cache.opened(&key)[0].current.device.label, "Laptop");
    assert_eq!(cache.records()[0].envelope.meta.revision, 1);
    cleanup(path);
}

#[test]
fn quota_capture_keeps_only_normalized_progress_facts() {
    let account = AccountContext::default_for("codex").unwrap();
    let registry = AccountRegistry::from_accounts(vec![account.clone()]).unwrap();
    let snapshots = vec![Snapshot {
        id: "codex".into(),
        provider_id: "codex".into(),
        account_id: "default".into(),
        card_id: account.card_id,
        credential_stamp: "private-credential-stamp".into(),
        account_identity_stamp: "private-account-stamp".into(),
        fetched_at: 1_800_000_000_000,
        expires_at: 1_800_000_300_000,
        name: "Alice Work".into(),
        plan: Some("Secret Enterprise Plan".into()),
        status: "ok".into(),
        error: None,
        metrics: vec![
            Metric::progress("Weekly Window", 42.0, Some("private detail".into()))
                .with_reset(Some(1_800_100_000_000), Some(604_800_000)),
            Metric::text("Balance", "$50".into()),
        ],
        stale: false,
        warning: None,
    }];

    let quotas = normalized_quotas(&snapshots, &registry, "device-a", 1_800_000_000_100);

    assert_eq!(quotas.len(), 1);
    assert_eq!(quotas[0].metric_id, "weekly-window");
    let json = serde_json::to_string(&quotas).unwrap();
    for private in ["Alice", "Enterprise", "private detail", "$50", "credential"] {
        assert!(!json.contains(private));
    }
}

#[test]
fn tombstones_are_emitted_only_for_completed_provider_scans() {
    let removed = usage_event('a', "codex");
    let untouched = usage_event('b', "claude");
    let previous = TrackingPayloadV2 {
        device: DeviceDescriptorV2::new("device-a", "Laptop", 1_800_000_000_000, "test").unwrap(),
        events: vec![removed.clone(), untouched],
        quotas: vec![],
        tombstones: vec![],
        retained_from_day: "2026-10-17".into(),
    };

    let tombstones = next_tombstones(
        Some(&previous),
        &[],
        &HashSet::from(["codex".to_string()]),
        1_800_000_100_000,
    );

    assert_eq!(tombstones.len(), 1);
    assert_eq!(tombstones[0].event_id, removed.event_id);
}

#[test]
fn collision_safe_cache_retains_the_previous_peer_envelope() {
    let path = temp_file("collision-safe");
    let key = derive_tracking_key(&[7_u8; 32]).unwrap();
    let baseline = payload_with_events("device-local", "Local", vec![usage_event('a', "codex")]);
    let prior = record_payload(
        payload_with_events("device-peer", "Peer", vec![usage_event('b', "codex")]),
        1,
    );
    let mut collision = usage_event('a', "codex");
    collision.cost = 9.99;
    let current = record_payload(
        payload_with_events("device-peer", "Peer", vec![collision]),
        2,
    );
    let mut cache = PeerEnvelopeCache::default();
    cache.replace_and_save(vec![prior], &key, &path).unwrap();

    let states = cache
        .replace_collision_safe_and_save(vec![current], &key, &path, &baseline)
        .unwrap();

    assert!(states[0].last_good.is_some());
    assert_eq!(
        PeerEnvelopeCache::load(&path).unwrap().records()[0]
            .envelope
            .meta
            .revision,
        1
    );
    cleanup(path);
}

fn record(device_id: &str, label: &str, revision: u64) -> TrackingEnvelopeRecord {
    record_payload(
        TrackingPayloadV2 {
            device: DeviceDescriptorV2::new(device_id, label, 1_800_000_000_000, "test").unwrap(),
            events: vec![],
            quotas: vec![],
            tombstones: vec![],
            retained_from_day: "2026-10-17".into(),
        },
        revision,
    )
}

fn record_payload(payload: TrackingPayloadV2, revision: u64) -> TrackingEnvelopeRecord {
    let key = derive_tracking_key(&[7_u8; 32]).unwrap();
    let device_id = payload.device.device_id.clone();
    TrackingEnvelopeRecord {
        received_at_ms: 1_800_000_000_100 + revision as i64,
        envelope: seal_tracking(
            &key,
            EnvelopeMeta::tracking_v2(&device_id, revision, 1_800_000_000_000).unwrap(),
            &payload,
        )
        .unwrap(),
    }
}

fn payload_with_events(
    device_id: &str,
    label: &str,
    events: Vec<UsageEventV2>,
) -> TrackingPayloadV2 {
    TrackingPayloadV2 {
        device: DeviceDescriptorV2::new(device_id, label, 1_800_000_000_000, "test").unwrap(),
        events,
        quotas: vec![],
        tombstones: vec![],
        retained_from_day: "2026-10-17".into(),
    }
}

fn usage_event(suffix: char, provider_id: &str) -> UsageEventV2 {
    UsageEventV2::new(
        format!("evt_{}", suffix.to_string().repeat(64)),
        provider_id,
        "record-a",
        1_800_000_000_000,
        "model",
        TokenUsageV2 {
            input: 1.0,
            output: 0.0,
            cached: 0.0,
            reasoning: 0.0,
            total: 1.0,
        },
        0.01,
        "catalog-test",
        "normalized-local",
    )
    .unwrap()
}

fn temp_file(label: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-{label}-{}-{}.json",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}

fn cleanup(path: std::path::PathBuf) {
    let _ = std::fs::remove_file(&path);
}
