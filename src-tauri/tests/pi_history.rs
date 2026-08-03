use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::PathBuf;

use openmeter_lib::accounts::{AccountContext, AccountRegistry, AccountSource};
use openmeter_lib::environment::EnvironmentSnapshot;
use openmeter_lib::spend::{collect_for_accounts, collect_pi_events};

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

#[test]
fn pi_events_fold_into_total_spend_under_the_stable_account_card() {
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
    let registry = AccountRegistry::from_accounts(vec![owner.clone()]).unwrap();
    let mut variables = BTreeMap::new();
    variables.insert(
        "PI_CODING_AGENT_SESSION_DIR".to_string(),
        OsString::from(root.as_os_str()),
    );
    let environment = EnvironmentSnapshot::from_values(root.clone(), variables);

    let spend = collect_for_accounts(None, &registry, &environment);
    let claude = spend
        .iter()
        .find(|entry| entry.id == owner.card_id)
        .expect("Pi-only Claude spend entry");

    assert_eq!(claude.name, "Claude — Company");
    assert_eq!(claude.last30.cost, 0.5);
    assert_eq!(claude.last30.tokens, 150.0);

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
