use std::path::PathBuf;

use openmeter_lib::accounts::{AccountContext, AccountRegistry, AccountSource};
use openmeter_lib::spend::{ProviderSpend, RawSpendEvent, Window};
use openmeter_lib::tracking_events::export_tracking_events;
use openmeter_sync_protocol::derive_tracking_key;

#[test]
fn equivalent_normalized_events_ignore_local_names_and_paths() {
    let account = AccountContext::identified(
        "codex",
        "stable-account@example.com",
        Some("Alice Work"),
        AccountSource::directory(
            "codex-work",
            PathBuf::from(r"C:\Users\Alice\Secrets\.codex"),
        )
        .unwrap(),
    )
    .unwrap();
    let registry = AccountRegistry::from_accounts(vec![account.clone()]).unwrap();
    let key = derive_tracking_key(&[9_u8; 32]).unwrap();
    let first = export_tracking_events(
        &[spend(&account.card_id, "Alice Work", 18.0)],
        &registry,
        &key,
        1_800_000_100_000,
    )
    .unwrap();
    let second = export_tracking_events(
        &[spend(&account.card_id, "Renamed On Desktop", 18.0)],
        &registry,
        &key,
        1_800_000_100_000,
    )
    .unwrap();

    assert_eq!(first[0].event_id, second[0].event_id);
    let json = serde_json::to_string(&first).unwrap();
    for forbidden in [
        "Alice",
        "Secrets",
        r"C:\Users",
        "stable-account@example.com",
    ] {
        assert!(!json.contains(forbidden), "leaked {forbidden}: {json}");
    }
}

#[test]
fn distinct_normalized_usage_remains_distinct() {
    let account = AccountContext::default_for("codex").unwrap();
    let registry = AccountRegistry::from_accounts(vec![account.clone()]).unwrap();
    let key = derive_tracking_key(&[9_u8; 32]).unwrap();
    let events = export_tracking_events(
        &[
            spend(&account.card_id, "Codex", 18.0),
            spend(&account.card_id, "Codex", 19.0),
        ],
        &registry,
        &key,
        1_800_000_100_000,
    )
    .unwrap();

    assert_eq!(events.len(), 2);
    assert_ne!(events[0].event_id, events[1].event_id);
}

#[test]
fn native_source_ids_disambiguate_equal_usage_and_deduplicate_replays() {
    let account = AccountContext::default_for("codex").unwrap();
    let registry = AccountRegistry::from_accounts(vec![account.clone()]).unwrap();
    let key = derive_tracking_key(&[9_u8; 32]).unwrap();
    let mut first = spend(&account.card_id, "Codex", 18.0);
    first.tracking_events[0].source_id = Some("native-event-a".into());
    let replay = first.clone();
    let mut second = spend(&account.card_id, "Codex", 18.0);
    second.tracking_events[0].source_id = Some("native-event-b".into());

    let events =
        export_tracking_events(&[first, replay, second], &registry, &key, 1_800_000_100_000)
            .unwrap();

    assert_eq!(events.len(), 2);
    assert_ne!(events[0].event_id, events[1].event_id);
    let json = serde_json::to_string(&events).unwrap();
    assert!(!json.contains("native-event-"));
}

fn spend(id: &str, name: &str, tokens: f64) -> ProviderSpend {
    ProviderSpend {
        id: id.into(),
        name: name.into(),
        today: Window::default(),
        yesterday: Window::default(),
        last30: Window::default(),
        trend: vec![0.0; 30],
        unpriced: 0,
        unpriced_models: vec![],
        daily: vec![],
        tracking_events: vec![RawSpendEvent {
            source_id: None,
            occurred_at_ms: 1_800_000_000_000,
            model: "gpt-5".into(),
            cost: 0.25,
            input: 0.0,
            output: 0.0,
            cached: 0.0,
            reasoning: 0.0,
            total_tokens: tokens,
        }],
    }
}
