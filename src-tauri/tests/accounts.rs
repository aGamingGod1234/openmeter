use std::path::PathBuf;

use openmeter_lib::accounts::{AccountContext, AccountRegistry, AccountSource};

#[test]
fn sources_with_the_same_identity_merge_into_one_stable_account() {
    let mut registry = AccountRegistry::from_accounts(vec![AccountContext::identified(
        "claude",
        "organization:acme",
        Some("Work"),
        AccountSource::default_home("claude-home"),
    )
    .expect("valid discovered account")])
    .unwrap();

    registry
        .attach_source(
            "organization:acme",
            AccountSource::directory("claude-work", PathBuf::from(r"D:\AI\claude-work")).unwrap(),
        )
        .unwrap();

    assert_eq!(registry.accounts().len(), 1);
    let account = &registry.accounts()[0];
    assert_eq!(account.card_id, "claude");
    assert_eq!(account.identity_key.as_deref(), Some("organization:acme"));
    assert_eq!(account.sources.len(), 2);
    assert!(account.sources[0].holds_default_source);
    assert!(!account.sources[1].holds_default_source);
}

#[test]
fn default_and_named_accounts_have_stable_card_ids() {
    let default = AccountContext::default_for("claude").expect("valid default account");
    let work = AccountContext::named("claude", "work", "Work").expect("valid named account");

    assert_eq!(default.account_id.as_str(), "default");
    assert_eq!(default.card_id, "claude");
    assert_eq!(work.account_id.as_str(), "work");
    assert_eq!(work.card_id, "claude--work");
    assert_eq!(work.display_name, "Claude — Work");
}

#[test]
fn invalid_ids_and_duplicate_card_ids_are_rejected() {
    assert!(AccountContext::named("claude", "bad id", "Work").is_err());
    assert!(AccountContext::named("claude", "default", "Work").is_err());

    let first = AccountContext::named("claude", "work", "Work").unwrap();
    let duplicate = AccountContext::named("claude", "work", "Different label").unwrap();
    assert!(AccountRegistry::from_accounts(vec![first, duplicate]).is_err());
}

#[test]
fn matching_is_exact_for_cards_and_family_wide_for_providers() {
    let registry = AccountRegistry::from_accounts(vec![
        AccountContext::default_for("claude").unwrap(),
        AccountContext::named("claude", "work", "Work").unwrap(),
        AccountContext::default_for("codex").unwrap(),
    ])
    .unwrap();

    assert_eq!(registry.match_token("claude").len(), 2);
    assert_eq!(registry.match_token("claude--work").len(), 1);
    assert_eq!(registry.match_token("codex").len(), 1);
    assert!(registry.match_token("work").is_empty());
    assert!(registry.match_token("unknown").is_empty());
}

#[test]
fn account_crud_preserves_card_identity_and_removes_only_the_selected_account() {
    let mut registry = AccountRegistry::default();
    let work = AccountContext::named("claude", "work", "Work").unwrap();
    registry.upsert(work.clone()).unwrap();
    let renamed = AccountContext::named("claude", "work", "Company").unwrap();
    registry.upsert(renamed).unwrap();
    assert_eq!(registry.accounts().len(), 1);
    assert_eq!(registry.accounts()[0].card_id, work.card_id);
    assert!(registry.remove("claude--work").unwrap());
    assert!(registry.accounts().is_empty());
    assert!(!registry.remove("claude--work").unwrap());
}

#[test]
fn versioned_registry_round_trips_without_credentials() {
    let path = temp_registry_path();
    let registry = AccountRegistry::from_accounts(vec![
        AccountContext::default_for("claude").unwrap(),
        AccountContext::named("claude", "work", "Work").unwrap(),
    ])
    .unwrap();

    registry.save(&path).expect("save registry");
    registry.save(&path).expect("atomically replace registry");
    let raw = std::fs::read_to_string(&path).expect("read registry");
    assert!(raw.contains("\"version\": 1"));
    assert!(!raw.to_lowercase().contains("token"));
    assert!(!raw.to_lowercase().contains("secret"));
    assert!(!raw.to_lowercase().contains("api_key"));

    let loaded = AccountRegistry::load(&path).expect("load registry");
    assert_eq!(loaded.accounts(), registry.accounts());

    let _ = std::fs::remove_file(path);
}

fn temp_registry_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-accounts-test-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("thread")
    ))
}
