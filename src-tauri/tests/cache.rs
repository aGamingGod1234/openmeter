use std::path::PathBuf;

use openmeter_lib::accounts::{AccountContext, AccountSource};
use openmeter_lib::cache::SnapshotCache;
use openmeter_lib::providers::{Metric, ProviderSnapshot};

#[test]
fn account_and_credential_stamps_isolate_fresh_entries() {
    let personal = AccountContext::default_for("claude").unwrap();
    let work = AccountContext::named("claude", "work", "Work").unwrap();
    let mut cache = SnapshotCache::default();
    cache.insert(snapshot(&personal, "credential-a", 1_000, 2_000, 25.0));

    assert!(cache.fresh("claude", "credential-a", 1_500).is_some());
    assert!(cache.fresh("claude--work", "credential-a", 1_500).is_none());
    assert!(cache.fresh("claude", "credential-b", 1_500).is_none());
    assert!(cache.last_good("claude", "credential-b").is_none());

    cache.insert(snapshot(&work, "credential-work", 1_000, 2_000, 40.0));
    assert_eq!(
        cache
            .fresh("claude--work", "credential-work", 1_500)
            .unwrap()
            .metrics[0]
            .used_percent,
        Some(40.0)
    );
}

#[test]
fn stable_account_identity_rejects_a_cache_entry_from_a_reassigned_card() {
    let original = AccountContext::identified(
        "claude",
        "organization:personal",
        Some("Personal"),
        AccountSource::default_home("claude-home"),
    )
    .unwrap();
    let replacement = AccountContext::identified(
        "claude",
        "organization:company",
        Some("Company"),
        AccountSource::default_home("claude-home"),
    )
    .unwrap();
    let mut cache = SnapshotCache::default();
    cache.insert(snapshot(&original, "same-credential", 1_000, 2_000, 25.0));

    assert!(cache
        .fresh_for_account(&original, "same-credential", 1_500)
        .is_some());
    assert!(cache
        .fresh_for_account(&replacement, "same-credential", 1_500)
        .is_none());
    assert!(cache
        .last_good_for_account(&replacement, "same-credential")
        .is_none());
}

#[test]
fn expired_entries_are_available_only_as_last_good() {
    let account = AccountContext::default_for("codex").unwrap();
    let mut cache = SnapshotCache::default();
    cache.insert(snapshot(&account, "credential-c", 1_000, 2_000, 42.0));

    assert!(cache.fresh("codex", "credential-c", 2_000).is_none());
    assert!(cache.last_good("codex", "credential-c").is_some());
}

#[test]
fn versioned_cache_round_trips_all_account_entries() {
    let path = temp_cache_path();
    let personal = AccountContext::default_for("claude").unwrap();
    let work = AccountContext::named("claude", "work", "Work").unwrap();
    let mut cache = SnapshotCache::default();
    cache.insert(snapshot(&personal, "credential-a", 1_000, 2_000, 25.0));
    cache.insert(snapshot(&work, "credential-work", 1_100, 2_100, 40.0));

    cache.write(&path).expect("write cache");
    cache.write(&path).expect("atomically replace cache");
    let loaded = SnapshotCache::read(&path).expect("read cache");
    assert_eq!(loaded.entries().len(), 2);
    assert!(loaded
        .last_good("claude--work", "credential-work")
        .is_some());

    let raw = std::fs::read_to_string(&path).unwrap();
    assert!(raw.contains("\"version\": 1"));
    let _ = std::fs::remove_file(path);
}

fn snapshot(
    account: &AccountContext,
    stamp: &str,
    fetched_at: i64,
    expires_at: i64,
    used: f64,
) -> ProviderSnapshot {
    ProviderSnapshot::ok(
        &account.card_id,
        &account.display_name,
        Some("Pro".to_string()),
        vec![Metric::progress("Session", used, None)],
    )
    .with_cache_identity(account, stamp, fetched_at, expires_at)
}

fn temp_cache_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "openmeter-cache-test-{}-{}.json",
        std::process::id(),
        std::thread::current().name().unwrap_or("thread")
    ))
}
