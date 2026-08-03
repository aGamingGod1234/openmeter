use std::path::PathBuf;

use openmeter_lib::accounts::{AccountContext, AccountSource};
use openmeter_lib::spend::collect_pi_events;

#[test]
fn pi_events_are_owned_by_the_account_holding_the_default_source() {
    let root = temp_root();
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("session.jsonl"),
        include_str!("fixtures/pi-session.jsonl"),
    )
    .unwrap();
    let owner = AccountContext::identified(
        "claude",
        "organization:company",
        Some("Company"),
        AccountSource::default_home("claude-home"),
    )
    .unwrap();

    let events = collect_pi_events(&root, &owner);

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].record_id, owner.card_id);
    assert_eq!(events[0].provider_id, "claude");
    assert_eq!(events[0].model, "claude-opus-4-8");
    assert_eq!(events[0].tokens, 150);
    assert_eq!(events[0].carried_cost, Some(0.5));

    let _ = std::fs::remove_dir_all(root);
}

fn temp_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-pi-history-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
