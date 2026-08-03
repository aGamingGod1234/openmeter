use openmeter_lib::accounts::{AccountContext, AccountRegistry};
use openmeter_lib::contracts::{serialize_limits, serialize_limits_with_registry, serialize_usage};
use openmeter_lib::providers::{Metric, ProviderSnapshot};

const FETCHED_AT: i64 = 1_783_906_770_000;
const EXPIRES_AT: i64 = 1_783_907_070_000;
const GENERATED_AT: i64 = 1_783_906_800_000;
const RESET_AT: i64 = 1_783_922_400_000;

#[test]
fn account_rename_is_resolved_when_serialized_without_rewriting_cached_snapshot() {
    let account = AccountContext::named("claude", "work", "Old").unwrap();
    let mut registry = AccountRegistry::from_accounts(vec![account.clone()]).unwrap();
    let snapshot = ProviderSnapshot::ok(
        &account.card_id,
        "Claude",
        None,
        vec![Metric::progress("Session", 25.0, None)],
    )
    .with_cache_identity(&account, "credential-work", FETCHED_AT, EXPIRES_AT);
    registry.rename("claude--work", "Company").unwrap();

    let wire = serialize_limits_with_registry(&[snapshot.clone()], GENERATED_AT, &registry);

    assert_eq!(snapshot.name, "Claude — Old");
    assert_eq!(
        wire["providers"]["claude--work"]["displayName"],
        "Claude — Company"
    );
}

#[test]
fn limits_contract_is_account_aware_scalar_and_fixture_stable() {
    let codex = AccountContext::default_for("codex").unwrap();
    let work = AccountContext::named("codex", "work", "Work").unwrap();
    let snapshot = ProviderSnapshot::ok(
        "codex",
        "Codex",
        Some("Pro 20x".to_string()),
        vec![
            Metric::progress("Weekly", 42.0, Some("58% left".to_string()))
                .with_reset(Some(RESET_AT), Some(604_800_000)),
            Metric::text("Today", "$5.17 · 9.2M tokens".to_string()),
        ],
    )
    .with_cache_identity(&codex, "credential-codex", FETCHED_AT, EXPIRES_AT);
    let failed = ProviderSnapshot::error(
        "codex--work",
        "Codex — Work",
        "work account unavailable".to_string(),
    )
    .with_cache_identity(&work, "credential-work", FETCHED_AT, EXPIRES_AT);

    let wire = serialize_limits(&[snapshot.clone(), failed], GENERATED_AT);
    assert_eq!(wire["schema"], "openusage.limits.v1");
    assert_eq!(wire["providers"]["codex"]["providerId"], "codex");
    assert_eq!(wire["providers"]["codex"]["accountId"], "default");
    assert_eq!(
        wire["providers"]["codex"]["resources"]["weekly"]["unit"],
        "percent"
    );
    assert_eq!(
        wire["providers"]["codex"]["resources"]["weekly"]["used"],
        42.0
    );
    assert!(wire["providers"]["codex"]["resources"]
        .get("Today")
        .is_none());
    assert_eq!(wire["errors"][0]["providerId"], "codex--work");

    let expected: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/limits-codex.json")).unwrap();
    assert_eq!(
        serde_json::to_string(&wire).unwrap(),
        serde_json::to_string(&expected).unwrap()
    );

    let usage = serialize_usage(&[snapshot]);
    assert_eq!(usage[0]["providerId"], "codex");
    assert_eq!(usage[0]["lines"][0]["used"], 42.0);
    assert_eq!(usage[0]["lines"][1]["label"], "Today");
}

#[test]
fn expiry_and_snapshot_staleness_are_reflected_without_losing_last_good_data() {
    let account = AccountContext::default_for("claude").unwrap();
    let mut snapshot = ProviderSnapshot::ok(
        "claude",
        "Claude",
        None,
        vec![Metric::progress("Session", 25.0, None)],
    )
    .with_cache_identity(&account, "credential-a", 1_000, 2_000);
    snapshot.stale = true;
    snapshot.warning = Some("rate limited".to_string());

    let wire = serialize_limits(&[snapshot], 1_500);
    assert_eq!(wire["providers"]["claude"]["stale"], true);
    assert_eq!(
        wire["providers"]["claude"]["expiresAt"],
        "1970-01-01T00:00:02.000Z"
    );
    assert_eq!(wire["errors"][0]["message"], "rate limited");
}
