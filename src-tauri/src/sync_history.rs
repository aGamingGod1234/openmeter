use std::collections::HashSet;

use openmeter_sync_protocol::{AccountHistoryV1, DailyUsageV1, HistoryPayloadV1, ModelUsageV1};

use crate::accounts::AccountRegistry;
use crate::spend::{self, DailySpend, ModelSpend, ProviderSpend};

#[derive(Clone)]
pub struct CombinedHistory {
    /// Only locally configured accounts become cards.
    pub cards: Vec<ProviderSpend>,
    /// Aggregate spend surfaces also include normalized peer history.
    pub total_spend: Vec<ProviderSpend>,
}

pub fn export_history(
    local: &[ProviderSpend],
    registry: &AccountRegistry,
    enabled_providers: &HashSet<String>,
) -> HistoryPayloadV1 {
    let accounts = local
        .iter()
        .filter_map(|provider| {
            let account = registry
                .accounts()
                .iter()
                .find(|account| account.card_id == provider.id);
            let provider_id = account
                .map(|account| account.provider_id.as_str())
                .unwrap_or_else(|| provider_family(&provider.id));
            if account.is_some_and(|account| !account.enabled)
                || !enabled_providers.contains(provider_id)
            {
                return None;
            }
            Some(AccountHistoryV1 {
                provider_id: provider_id.to_string(),
                record_id: account.map_or_else(
                    || {
                        crate::redaction::credential_stamp(
                            format!("{provider_id}\0{}", provider.id).as_bytes(),
                        )
                    },
                    crate::accounts::AccountContext::identity_stamp,
                ),
                days: provider.daily.iter().map(export_day).collect(),
            })
        })
        .collect();
    HistoryPayloadV1 { accounts }
}

fn provider_family(card_id: &str) -> &str {
    card_id
        .split_once("--")
        .or_else(|| card_id.split_once('@'))
        .map_or(card_id, |(provider, _)| provider)
}

pub fn merge_peer_history(
    local: &[ProviderSpend],
    peers: &[HistoryPayloadV1],
    registry: &AccountRegistry,
    enabled_providers: &HashSet<String>,
    today: &str,
) -> CombinedHistory {
    let cards = local.to_vec();
    let mut total_spend = local.to_vec();
    for peer in peers {
        for account in &peer.accounts {
            if !enabled_providers.contains(&account.provider_id) {
                continue;
            }
            let daily: Vec<DailySpend> = account
                .days
                .iter()
                .filter(|row| in_window(&row.day, today))
                .map(import_day)
                .collect();
            if daily.is_empty() {
                continue;
            }
            let local_card_id = registry
                .accounts()
                .iter()
                .find(|candidate| {
                    candidate.enabled
                        && candidate.provider_id == account.provider_id
                        && candidate.identity_stamp() == account.record_id
                })
                .map(|candidate| candidate.card_id.as_str());
            let synthetic_id = format!("{}@{}", account.provider_id, account.record_id);
            let target_id = local_card_id.unwrap_or(&synthetic_id);
            let incoming = spend::from_daily(
                target_id,
                &format!("{} (synced)", account.provider_id),
                daily,
                today,
            );
            if let Some(existing) = total_spend.iter_mut().find(|row| row.id == target_id) {
                spend::merge_provider_spend(existing, incoming);
            } else {
                total_spend.push(incoming);
            }
        }
    }
    CombinedHistory { cards, total_spend }
}

fn export_day(day: &DailySpend) -> DailyUsageV1 {
    DailyUsageV1 {
        day: day.day.clone(),
        cost: day.cost,
        tokens: day.tokens,
        models: day
            .models
            .iter()
            .map(|model| ModelUsageV1 {
                model: model.model.clone(),
                cost: model.cost,
                tokens: model.tokens,
            })
            .collect(),
        unpriced_models: day.unpriced_models.clone(),
    }
}

fn import_day(day: &DailyUsageV1) -> DailySpend {
    DailySpend {
        day: day.day.clone(),
        cost: day.cost,
        tokens: day.tokens,
        models: day
            .models
            .iter()
            .map(|model| ModelSpend {
                model: model.model.clone(),
                cost: model.cost,
                tokens: model.tokens,
            })
            .collect(),
        unpriced_models: day.unpriced_models.clone(),
    }
}

fn in_window(day: &str, today: &str) -> bool {
    let Ok(day) = chrono::NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
        return false;
    };
    let Ok(today) = chrono::NaiveDate::parse_from_str(today, "%Y-%m-%d") else {
        return false;
    };
    day <= today && day > today - chrono::Duration::days(30)
}
