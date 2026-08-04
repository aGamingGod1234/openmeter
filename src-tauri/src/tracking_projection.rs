use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use chrono::{DateTime, NaiveDate, Utc};
use openmeter_sync_protocol::{HistoryPayloadV1, QuotaSnapshotV2, TrackingPayloadV2, UsageEventV2};
use serde::Serialize;

const DEFAULT_REFRESH_INTERVAL_MS: i64 = 300_000;
const RESET_DISAGREEMENT_MS: i64 = 300_000;

#[derive(Debug, Clone)]
pub struct PeerTrackingState {
    pub current: TrackingPayloadV2,
    pub last_good: Option<TrackingPayloadV2>,
    pub received_at_ms: i64,
}

impl PeerTrackingState {
    pub fn current(payload: TrackingPayloadV2, received_at_ms: i64) -> Self {
        Self {
            current: payload,
            last_good: None,
            received_at_ms,
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackingDashboard {
    pub generated_at_ms: i64,
    pub devices: Vec<TrackingDevice>,
    pub days: Vec<TrackingDay>,
    pub quotas: Vec<TrackingQuota>,
    pub legacy_present: bool,
    pub unique_events: usize,
    pub quarantined_devices: Vec<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrackingDevice {
    pub device_id: String,
    pub label: String,
    pub generated_at_ms: i64,
    pub received_at_ms: i64,
    pub quarantined: bool,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackingDay {
    pub day: String,
    pub device_id: String,
    pub device_label: String,
    pub provider_id: String,
    pub model: String,
    pub tokens: f64,
    pub cost: f64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct TrackingQuota {
    pub provider_id: String,
    pub record_id: String,
    pub metric_id: String,
    pub used_percent: f64,
    pub remaining: Option<f64>,
    pub limit: Option<f64>,
    pub resets_at_ms: Option<i64>,
    pub observed_at_ms: i64,
    pub source_device_id: String,
    pub source_device_label: String,
    pub disagreement: bool,
}

#[derive(Clone)]
struct AcceptedPayload {
    payload: TrackingPayloadV2,
    received_at_ms: i64,
}

#[derive(Clone)]
struct OwnedEvent {
    event: UsageEventV2,
    device_id: String,
    device_label: String,
}

pub fn project_tracking(
    local: TrackingPayloadV2,
    peers: &[PeerTrackingState],
    legacy: &[HistoryPayloadV1],
    today: &str,
    now_ms: i64,
) -> TrackingDashboard {
    let mut accepted = vec![AcceptedPayload {
        payload: local,
        received_at_ms: now_ms,
    }];
    let mut quarantined = BTreeSet::new();

    let mut ordered_peers = peers.to_vec();
    ordered_peers.sort_by_key(|peer| peer.received_at_ms);
    for peer in ordered_peers {
        if payload_collides(&peer.current, &accepted) {
            quarantined.insert(peer.current.device.device_id.clone());
            if let Some(last_good) = peer.last_good {
                if !payload_collides(&last_good, &accepted) {
                    accepted.push(AcceptedPayload {
                        payload: last_good,
                        received_at_ms: peer.received_at_ms,
                    });
                }
            }
            continue;
        }
        accepted.push(AcceptedPayload {
            payload: peer.current,
            received_at_ms: peer.received_at_ms,
        });
    }

    let tombstones: HashSet<&str> = accepted
        .iter()
        .flat_map(|candidate| candidate.payload.tombstones.iter())
        .map(|tombstone| tombstone.event_id.as_str())
        .collect();
    let mut events = BTreeMap::<String, OwnedEvent>::new();
    for candidate in &accepted {
        for event in &candidate.payload.events {
            if tombstones.contains(event.event_id.as_str()) {
                continue;
            }
            events
                .entry(event.event_id.clone())
                .or_insert_with(|| OwnedEvent {
                    event: event.clone(),
                    device_id: candidate.payload.device.device_id.clone(),
                    device_label: candidate.payload.device.label.clone(),
                });
        }
    }

    let coverage: HashSet<(String, String, String)> = events
        .values()
        .filter_map(|owned| {
            event_day(&owned.event).map(|day| {
                (
                    owned.event.provider_id.clone(),
                    owned.event.record_id.clone(),
                    day,
                )
            })
        })
        .collect();
    let mut rows = aggregate_events(events.values(), today);
    let legacy_rows = aggregate_legacy(legacy, &coverage, today);
    let legacy_present = !legacy_rows.is_empty();
    rows.extend(legacy_rows);
    rows.sort_by(|left, right| {
        left.day
            .cmp(&right.day)
            .then_with(|| left.device_id.cmp(&right.device_id))
            .then_with(|| left.provider_id.cmp(&right.provider_id))
            .then_with(|| left.model.cmp(&right.model))
    });

    let mut devices: Vec<TrackingDevice> = accepted
        .iter()
        .map(|candidate| TrackingDevice {
            device_id: candidate.payload.device.device_id.clone(),
            label: candidate.payload.device.label.clone(),
            generated_at_ms: candidate.payload.device.generated_at_ms,
            received_at_ms: candidate.received_at_ms,
            quarantined: quarantined.contains(&candidate.payload.device.device_id),
        })
        .collect();
    for peer in peers {
        if quarantined.contains(&peer.current.device.device_id)
            && !devices
                .iter()
                .any(|device| device.device_id == peer.current.device.device_id)
        {
            devices.push(TrackingDevice {
                device_id: peer.current.device.device_id.clone(),
                label: peer.current.device.label.clone(),
                generated_at_ms: peer.current.device.generated_at_ms,
                received_at_ms: peer.received_at_ms,
                quarantined: true,
            });
        }
    }
    devices.sort_by(|left, right| {
        left.label
            .cmp(&right.label)
            .then_with(|| left.device_id.cmp(&right.device_id))
    });

    TrackingDashboard {
        generated_at_ms: now_ms,
        devices,
        days: rows,
        quotas: select_quotas(&accepted, now_ms),
        legacy_present,
        unique_events: events.len(),
        quarantined_devices: quarantined.into_iter().collect(),
    }
}

fn payload_collides(candidate: &TrackingPayloadV2, accepted: &[AcceptedPayload]) -> bool {
    let known: HashMap<&str, &UsageEventV2> = accepted
        .iter()
        .flat_map(|payload| payload.payload.events.iter())
        .map(|event| (event.event_id.as_str(), event))
        .collect();
    candidate.events.iter().any(|event| {
        known
            .get(event.event_id.as_str())
            .is_some_and(|existing| *existing != event)
    })
}

fn aggregate_events<'a>(
    events: impl Iterator<Item = &'a OwnedEvent>,
    today: &str,
) -> Vec<TrackingDay> {
    let mut rows = BTreeMap::<(String, String, String, String, String), (f64, f64)>::new();
    for owned in events {
        let Some(day) = event_day(&owned.event) else {
            continue;
        };
        if !in_window(&day, today) {
            continue;
        }
        let key = (
            day,
            owned.device_id.clone(),
            owned.device_label.clone(),
            owned.event.provider_id.clone(),
            owned.event.model.clone(),
        );
        let totals = rows.entry(key).or_default();
        totals.0 += owned.event.tokens.total;
        totals.1 += owned.event.cost;
    }
    rows.into_iter()
        .map(
            |((day, device_id, device_label, provider_id, model), (tokens, cost))| TrackingDay {
                day,
                device_id,
                device_label,
                provider_id,
                model,
                tokens,
                cost,
            },
        )
        .collect()
}

fn aggregate_legacy(
    payloads: &[HistoryPayloadV1],
    coverage: &HashSet<(String, String, String)>,
    today: &str,
) -> Vec<TrackingDay> {
    let mut rows = Vec::new();
    for payload in payloads {
        for account in &payload.accounts {
            for day in &account.days {
                if !in_window(&day.day, today)
                    || coverage.contains(&(
                        account.provider_id.clone(),
                        account.record_id.clone(),
                        day.day.clone(),
                    ))
                {
                    continue;
                }
                if day.models.is_empty() {
                    rows.push(TrackingDay {
                        day: day.day.clone(),
                        device_id: "legacy".into(),
                        device_label: "Legacy sync".into(),
                        provider_id: account.provider_id.clone(),
                        model: "unattributed".into(),
                        tokens: day.tokens,
                        cost: day.cost,
                    });
                } else {
                    rows.extend(day.models.iter().map(|model| TrackingDay {
                        day: day.day.clone(),
                        device_id: "legacy".into(),
                        device_label: "Legacy sync".into(),
                        provider_id: account.provider_id.clone(),
                        model: model.model.clone(),
                        tokens: model.tokens,
                        cost: model.cost,
                    }));
                }
            }
        }
    }
    rows
}

fn select_quotas(payloads: &[AcceptedPayload], now_ms: i64) -> Vec<TrackingQuota> {
    let mut groups = BTreeMap::<(String, String, String), Vec<(&QuotaSnapshotV2, &str)>>::new();
    for payload in payloads {
        for quota in &payload.payload.quotas {
            if quota.source_device_id != payload.payload.device.device_id {
                continue;
            }
            let max_age = DEFAULT_REFRESH_INTERVAL_MS.saturating_mul(2);
            if quota.observed_at_ms > now_ms || now_ms - quota.observed_at_ms > max_age {
                continue;
            }
            groups
                .entry((
                    quota.provider_id.clone(),
                    quota.record_id.clone(),
                    quota.metric_id.clone(),
                ))
                .or_default()
                .push((quota, payload.payload.device.label.as_str()));
        }
    }
    groups
        .into_values()
        .filter_map(|mut candidates| {
            candidates.sort_by_key(|(quota, _)| quota.observed_at_ms);
            let (selected, label) = *candidates.last()?;
            let disagreement = candidates.iter().any(|(candidate, _)| {
                (candidate.used_percent - selected.used_percent).abs() > 5.0
                    || reset_differs(candidate.resets_at_ms, selected.resets_at_ms)
            });
            Some(TrackingQuota {
                provider_id: selected.provider_id.clone(),
                record_id: selected.record_id.clone(),
                metric_id: selected.metric_id.clone(),
                used_percent: selected.used_percent,
                remaining: selected.remaining,
                limit: selected.limit,
                resets_at_ms: selected.resets_at_ms,
                observed_at_ms: selected.observed_at_ms,
                source_device_id: selected.source_device_id.clone(),
                source_device_label: label.to_string(),
                disagreement,
            })
        })
        .collect()
}

fn reset_differs(left: Option<i64>, right: Option<i64>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left.abs_diff(right) > RESET_DISAGREEMENT_MS as u64,
        (None, None) => false,
        _ => true,
    }
}

fn event_day(event: &UsageEventV2) -> Option<String> {
    DateTime::<Utc>::from_timestamp_millis(event.occurred_at_ms)
        .map(|timestamp| timestamp.date_naive().format("%Y-%m-%d").to_string())
}

fn in_window(day: &str, today: &str) -> bool {
    let Ok(day) = NaiveDate::parse_from_str(day, "%Y-%m-%d") else {
        return false;
    };
    let Ok(today) = NaiveDate::parse_from_str(today, "%Y-%m-%d") else {
        return false;
    };
    day <= today && day > today - chrono::Duration::days(30)
}
