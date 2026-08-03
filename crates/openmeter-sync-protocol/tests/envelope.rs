use openmeter_sync_protocol::{
    open, seal, AccountHistoryV1, DailyUsageV1, EnvelopeMeta, HistoryKey, HistoryPayloadV1,
    ModelUsageV1, ProtocolError,
};

fn payload() -> HistoryPayloadV1 {
    HistoryPayloadV1 {
        accounts: vec![AccountHistoryV1 {
            provider_id: "claude".to_string(),
            record_id: "claude@abcd1234".to_string(),
            days: vec![DailyUsageV1 {
                day: "2026-08-03".to_string(),
                cost: 1.25,
                tokens: 500.0,
                models: vec![ModelUsageV1 {
                    model: "claude-opus-4-8".to_string(),
                    cost: 1.25,
                    tokens: 500.0,
                }],
                unpriced_models: vec![],
            }],
        }],
    }
}

#[test]
fn round_trip_preserves_normalized_history_only() {
    let key = HistoryKey::from_bytes([7; 32]);
    let meta = EnvelopeMeta::new("device-a", 4, 1_800_000_000_000).unwrap();

    let envelope = seal(&key, meta.clone(), &payload()).unwrap();

    assert_ne!(envelope.ciphertext, serde_json::to_vec(&payload()).unwrap());
    assert_eq!(open(&key, &envelope).unwrap(), payload());
    assert_eq!(envelope.meta, meta);
}

#[test]
fn changing_authenticated_revision_rejects_the_envelope() {
    let key = HistoryKey::from_bytes([7; 32]);
    let mut envelope = seal(
        &key,
        EnvelopeMeta::new("device-a", 4, 1_800_000_000_000).unwrap(),
        &payload(),
    )
    .unwrap();

    envelope.meta.revision = 5;

    assert_eq!(open(&key, &envelope), Err(ProtocolError::Authentication));
}

#[test]
fn unknown_or_sensitive_payload_fields_are_rejected() {
    let value = serde_json::json!({
        "accounts": [{
            "provider_id": "claude",
            "record_id": "claude@abcd1234",
            "label": "Company",
            "days": []
        }]
    });

    assert!(serde_json::from_value::<HistoryPayloadV1>(value).is_err());
}
