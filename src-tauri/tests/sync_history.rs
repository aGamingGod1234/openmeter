use std::collections::HashSet;
use std::path::PathBuf;

use openmeter_lib::accounts::{AccountContext, AccountRegistry, AccountSource};
use openmeter_lib::spend::{DailySpend, ModelSpend, ProviderSpend, Window};
use openmeter_lib::sync_history::{export_history, merge_peer_history};
use openmeter_sync_protocol::{AccountHistoryV1, DailyUsageV1, HistoryPayloadV1};

#[test]
fn export_contains_totals_but_no_labels_credentials_errors_or_paths() {
    let account = AccountContext::identified(
        "claude",
        "stable-account@example.com",
        Some("Company"),
        AccountSource::directory(
            "claude-company",
            PathBuf::from(r"C:\Users\Alice\Company\.claude"),
        )
        .unwrap(),
    )
    .unwrap();
    let registry = AccountRegistry::from_accounts(vec![account.clone()]).unwrap();
    let payload = export_history(
        &[local_spend(&account.card_id, &account.display_name)],
        &registry,
        &HashSet::from(["claude".to_string()]),
    );
    let json = serde_json::to_string(&payload).unwrap();

    assert_eq!(payload.accounts[0].days[0].cost, 1.25);
    for forbidden in ["Company", "access_token", r"C:\Users", "HTTP 429"] {
        assert!(!json.contains(forbidden), "leaked {forbidden}: {json}");
    }
}

#[test]
fn peer_history_adds_to_totals_without_creating_a_local_card() {
    let local_account = AccountContext::default_for("claude").unwrap();
    let registry = AccountRegistry::from_accounts(vec![local_account.clone()]).unwrap();
    let local = vec![local_spend(
        &local_account.card_id,
        &local_account.display_name,
    )];
    let peer = HistoryPayloadV1 {
        accounts: vec![AccountHistoryV1 {
            provider_id: "claude".into(),
            record_id: "remote.opaque.record".into(),
            days: vec![DailyUsageV1 {
                day: "2026-08-03".into(),
                cost: 2.5,
                tokens: 200.0,
                models: vec![],
                unpriced_models: vec![],
            }],
        }],
    };

    let combined = merge_peer_history(
        &local,
        &[peer],
        &registry,
        &HashSet::from(["claude".to_string()]),
        "2026-08-03",
    );

    assert_eq!(combined.cards.len(), local.len());
    assert_eq!(combined.total_spend.len(), 2);
    assert!(combined
        .total_spend
        .iter()
        .any(|row| row.id == "claude@remote.opaque.record" && row.today.cost == 2.5));
}

fn local_spend(id: &str, name: &str) -> ProviderSpend {
    ProviderSpend {
        id: id.into(),
        name: name.into(),
        today: Window {
            cost: 1.25,
            tokens: 100.0,
            models: vec![ModelSpend {
                model: "claude-sonnet".into(),
                cost: 1.25,
                tokens: 100.0,
            }],
        },
        yesterday: Window::default(),
        last30: Window {
            cost: 1.25,
            tokens: 100.0,
            models: vec![],
        },
        trend: vec![0.0; 30],
        unpriced: 0,
        unpriced_models: vec![],
        daily: vec![DailySpend {
            day: "2026-08-03".into(),
            cost: 1.25,
            tokens: 100.0,
            models: vec![ModelSpend {
                model: "claude-sonnet".into(),
                cost: 1.25,
                tokens: 100.0,
            }],
            unpriced_models: vec![],
        }],
        tracking_events: vec![],
    }
}
