use openmeter_sync_protocol::{
    derive_tracking_key, open_tracking, seal_tracking, tracking_event_id, DeviceDescriptorV2,
    EnvelopeMeta, ProtocolError, TokenUsageV2, TrackingPayloadV2, UsageEventV2,
};

#[test]
fn tracking_envelope_round_trip_preserves_only_normalized_facts() {
    let recovery = [7_u8; 32];
    let key = derive_tracking_key(&recovery).unwrap();
    let event_id = tracking_event_id(
        &key,
        "codex",
        "account.opaque",
        br#"{"native_id":"turn-42","prompt":"never serialize me"}"#,
    )
    .unwrap();
    assert_eq!(
        event_id,
        "evt_a89adf675d062d755f4b4a44abf987f6615df5b9dd540f8f29f6774d618455bd"
    );
    let payload = payload(&event_id);
    let envelope = seal_tracking(
        &key,
        EnvelopeMeta::tracking_v2("device-a", 1, 1_800_000_000_000).unwrap(),
        &payload,
    )
    .unwrap();

    let opened = open_tracking(&key, &envelope).unwrap();

    assert_eq!(opened, payload);
    let encoded = serde_json::to_string(&opened).unwrap();
    assert!(!encoded.contains("turn-42"));
    assert!(!encoded.contains("never serialize me"));
}

#[test]
fn authenticated_revision_change_is_rejected() {
    let key = derive_tracking_key(&[7_u8; 32]).unwrap();
    let event_id = tracking_event_id(&key, "codex", "account.opaque", b"turn-42").unwrap();
    let mut envelope = seal_tracking(
        &key,
        EnvelopeMeta::tracking_v2("device-a", 1, 1_800_000_000_000).unwrap(),
        &payload(&event_id),
    )
    .unwrap();
    envelope.meta.revision = 2;

    assert_eq!(
        open_tracking(&key, &envelope).unwrap_err(),
        ProtocolError::Authentication
    );
}

#[test]
fn uppercase_or_duplicate_event_ids_are_rejected() {
    let key = derive_tracking_key(&[7_u8; 32]).unwrap();
    let event_id = tracking_event_id(&key, "codex", "account.opaque", b"turn-42").unwrap();
    let mut uppercase = payload(&event_id);
    uppercase.events[0].event_id = format!("evt_{}", event_id[4..].to_ascii_uppercase());
    let meta = EnvelopeMeta::tracking_v2("device-a", 1, 1_800_000_000_000).unwrap();
    assert!(seal_tracking(&key, meta, &uppercase).is_err());

    let mut duplicate = payload(&event_id);
    duplicate.events.push(duplicate.events[0].clone());
    let meta = EnvelopeMeta::tracking_v2("device-a", 1, 1_800_000_000_000).unwrap();
    assert!(seal_tracking(&key, meta, &duplicate).is_err());
}

#[test]
fn unknown_or_sensitive_payload_fields_are_rejected() {
    let key = derive_tracking_key(&[7_u8; 32]).unwrap();
    let event_id = tracking_event_id(&key, "codex", "account.opaque", b"turn-42").unwrap();
    let mut value = serde_json::to_value(payload(&event_id)).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("credentials".into(), serde_json::json!("secret"));

    assert!(serde_json::from_value::<TrackingPayloadV2>(value).is_err());
}

fn payload(event_id: &str) -> TrackingPayloadV2 {
    TrackingPayloadV2 {
        device: DeviceDescriptorV2::new("device-a", "Laptop", 1_800_000_000_000, "0.6.0").unwrap(),
        events: vec![UsageEventV2::new(
            event_id,
            "codex",
            "account.opaque",
            1_800_000_000_000,
            "gpt-5",
            TokenUsageV2 {
                input: 10.0,
                output: 5.0,
                cached: 2.0,
                reasoning: 1.0,
                total: 18.0,
            },
            0.25,
            "catalog-2026-08-04",
            "codex-log",
        )
        .unwrap()],
        quotas: vec![],
        tombstones: vec![],
        retained_from_day: "2026-05-07".into(),
    }
}
