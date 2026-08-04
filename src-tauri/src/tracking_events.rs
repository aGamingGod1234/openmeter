use std::collections::BTreeMap;

use openmeter_sync_protocol::{tracking_event_id, TokenUsageV2, TrackingKey, UsageEventV2};

use crate::accounts::AccountRegistry;
use crate::spend::{ProviderSpend, RawSpendEvent};

const TRACKING_WINDOW_MS: i64 = 90 * 24 * 60 * 60 * 1_000;

pub fn export_tracking_events(
    spend: &[ProviderSpend],
    registry: &AccountRegistry,
    key: &TrackingKey,
    now_ms: i64,
) -> Result<Vec<UsageEventV2>, String> {
    let cutoff = now_ms.saturating_sub(TRACKING_WINDOW_MS);
    let pricing_stamp =
        crate::redaction::credential_stamp(crate::pricing::catalog_stamp().as_bytes());
    let pricing_version = format!("catalog-{}", &pricing_stamp[..32]);
    let mut unique = BTreeMap::<String, UsageEventV2>::new();

    for provider in spend {
        let (provider_id, record_id) = identity_for(provider, registry);
        for raw in provider
            .tracking_events
            .iter()
            .filter(|event| event.occurred_at_ms >= cutoff && event.occurred_at_ms <= now_ms)
        {
            let canonical = canonical_source(raw);
            let event_id = tracking_event_id(key, &provider_id, &record_id, &canonical)
                .map_err(|error| error.to_string())?;
            let event = UsageEventV2::new(
                &event_id,
                &provider_id,
                &record_id,
                raw.occurred_at_ms,
                normalized_model(&raw.model),
                TokenUsageV2 {
                    input: raw.input,
                    output: raw.output,
                    cached: raw.cached,
                    reasoning: raw.reasoning,
                    total: raw.total_tokens,
                },
                raw.cost,
                &pricing_version,
                "normalized-local",
            )
            .map_err(|error| error.to_string())?;
            if let Some(existing) = unique.get(&event_id) {
                if existing != &event {
                    return Err("local tracking event identity collision".to_string());
                }
            } else {
                unique.insert(event_id, event);
            }
        }
    }

    let mut events: Vec<UsageEventV2> = unique.into_values().collect();
    events.sort_by(|left, right| {
        left.occurred_at_ms
            .cmp(&right.occurred_at_ms)
            .then_with(|| left.event_id.cmp(&right.event_id))
    });
    Ok(events)
}

fn identity_for(provider: &ProviderSpend, registry: &AccountRegistry) -> (String, String) {
    if let Some(account) = registry
        .accounts()
        .iter()
        .find(|account| account.card_id == provider.id)
    {
        return (account.provider_id.clone(), account.identity_stamp());
    }
    let provider_id = provider_family(&provider.id).to_string();
    let record_id =
        crate::redaction::credential_stamp(format!("{provider_id}\0{}", provider.id).as_bytes());
    (provider_id, record_id)
}

fn provider_family(card_id: &str) -> &str {
    card_id
        .split_once("--")
        .or_else(|| card_id.split_once('@'))
        .map_or(card_id, |(provider, _)| provider)
}

fn normalized_model(model: &str) -> &str {
    if model.trim().is_empty() {
        "unattributed"
    } else {
        model
    }
}

fn canonical_source(event: &RawSpendEvent) -> Vec<u8> {
    if let Some(source_id) = event.source_id.as_deref().filter(|id| !id.is_empty()) {
        let mut bytes = Vec::with_capacity(9 + source_id.len());
        bytes.extend_from_slice(b"native\0");
        bytes.extend_from_slice(source_id.as_bytes());
        return bytes;
    }
    let mut bytes = Vec::with_capacity(96 + event.model.len());
    bytes.extend_from_slice(b"derived\0");
    bytes.extend_from_slice(&event.occurred_at_ms.to_le_bytes());
    bytes.extend_from_slice(&(event.model.len() as u64).to_le_bytes());
    bytes.extend_from_slice(event.model.as_bytes());
    for value in [
        event.cost,
        event.input,
        event.output,
        event.cached,
        event.reasoning,
        event.total_tokens,
    ] {
        bytes.extend_from_slice(&value.to_bits().to_le_bytes());
    }
    bytes
}
