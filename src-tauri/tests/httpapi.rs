use openmeter_lib::accounts::{AccountContext, AccountRegistry};
use openmeter_lib::httpapi::ApiState;
use openmeter_lib::providers::{Metric, ProviderSnapshot};
use tiny_http::Method;

fn registry() -> AccountRegistry {
    AccountRegistry::from_accounts(vec![
        AccountContext::default_for("codex").unwrap(),
        AccountContext::named("codex", "work", "Work").unwrap(),
        AccountContext::default_for("claude").unwrap(),
    ])
    .unwrap()
}

fn snapshots() -> Vec<ProviderSnapshot> {
    let default = AccountContext::default_for("codex").unwrap();
    let work = AccountContext::named("codex", "work", "Work").unwrap();
    vec![
        ProviderSnapshot::ok(
            "codex",
            "Codex",
            Some("Plus".to_string()),
            vec![Metric::progress("Weekly", 42.0, None)],
        )
        .with_cache_identity(&default, "credential-default", 1_000, 2_000),
        ProviderSnapshot::error(
            "codex--work",
            "Codex - Work",
            "account unavailable".to_string(),
        )
        .with_cache_identity(&work, "credential-work", 1_000, 2_000),
    ]
}

#[test]
fn routes_usage_and_limits_by_provider_family_or_exact_card() {
    let state = ApiState::new(registry());
    state.publish(&snapshots(), 1_500);

    let all_usage = state.route(&Method::Get, "/v1/usage?ignored=yes");
    assert_eq!(all_usage.status, 200);
    assert_eq!(all_usage.body.as_array().unwrap().len(), 2);

    let family_usage = state.route(&Method::Get, "/v1/usage/codex");
    assert_eq!(family_usage.status, 200);
    assert_eq!(family_usage.body.as_array().unwrap().len(), 2);

    let exact_usage = state.route(&Method::Get, "/v1/usage/codex--work");
    assert_eq!(exact_usage.status, 200);
    assert_eq!(exact_usage.body.as_array().unwrap().len(), 1);
    assert_eq!(exact_usage.body[0]["providerId"], "codex--work");

    let family_limits = state.route(&Method::Get, "/v1/limits/codex");
    assert_eq!(family_limits.status, 200);
    assert!(family_limits.body["providers"].get("codex").is_some());
    assert_eq!(family_limits.body["errors"][0]["providerId"], "codex--work");

    let exact_limits = state.route(&Method::Get, "/v1/limits/codex--work");
    assert_eq!(exact_limits.status, 200);
    assert!(exact_limits.body["providers"]
        .as_object()
        .unwrap()
        .is_empty());
    assert_eq!(exact_limits.body["errors"].as_array().unwrap().len(), 1);
}

#[test]
fn routes_resolve_the_registry_label_instead_of_the_cached_snapshot_name() {
    let mut accounts = registry();
    accounts.rename("codex--work", "Company").unwrap();
    let state = ApiState::new(accounts);
    state.publish(&snapshots(), 1_500);

    let response = state.route(&Method::Get, "/v1/usage/codex--work");

    assert_eq!(response.status, 200);
    assert_eq!(response.body[0]["displayName"], "Codex — Company");
}

#[test]
fn known_accounts_without_snapshots_are_empty_but_unknown_ids_are_404() {
    let state = ApiState::new(registry());
    state.publish(&snapshots(), 1_500);

    let known_usage = state.route(&Method::Get, "/v1/usage/claude");
    assert_eq!(known_usage.status, 200);
    assert!(known_usage.body.as_array().unwrap().is_empty());

    let known_limits = state.route(&Method::Get, "/v1/limits/claude");
    assert_eq!(known_limits.status, 200);
    assert!(known_limits.body["providers"]
        .as_object()
        .unwrap()
        .is_empty());
    assert!(known_limits.body["errors"].as_array().unwrap().is_empty());

    let unknown = state.route(&Method::Get, "/v1/usage/unknown");
    assert_eq!(unknown.status, 404);
    assert_eq!(unknown.body["error"], "provider_not_found");
}

#[test]
fn protocol_semantics_are_bounded_and_do_not_enable_browser_cors() {
    let state = ApiState::new(registry());
    state.publish(&snapshots(), 1_500);

    let options = state.route(&Method::Options, "/v1/limits");
    assert_eq!(options.status, 204);

    let wrong_method = state.route(&Method::Post, "/v1/usage");
    assert_eq!(wrong_method.status, 405);
    assert_eq!(wrong_method.body["error"], "method_not_allowed");

    let missing = state.route(&Method::Get, "/not-an-api-route");
    assert_eq!(missing.status, 404);
    assert_eq!(missing.body["error"], "not_found");

    for response in [options, wrong_method, missing] {
        assert!(response
            .headers
            .iter()
            .all(|(name, _)| !name.eq_ignore_ascii_case("Access-Control-Allow-Origin")));
    }

    let saturated = ApiState::with_max_in_flight(registry(), 0);
    let busy = saturated.route(&Method::Get, "/v1/usage");
    assert_eq!(busy.status, 503);
    assert_eq!(busy.body["error"], "server_busy");
}
