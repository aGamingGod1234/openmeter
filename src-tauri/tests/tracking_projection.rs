use openmeter_lib::tracking_projection::{project_tracking, PeerTrackingState};
use openmeter_sync_protocol::{
    AccountHistoryV1, DailyUsageV1, DeviceDescriptorV2, HistoryPayloadV1, ModelUsageV1,
    QuotaSnapshotV2, TokenUsageV2, TombstoneV2, TrackingPayloadV2, UsageEventV2,
};

const NOW: i64 = 1_800_000_000_000;

#[test]
fn identical_events_collapse_but_distinct_events_remain() {
    let shared = event('a', 18.0);
    let local = payload("device-a", "Laptop", vec![shared.clone()], vec![]);
    let peer = payload(
        "device-b",
        "Desktop",
        vec![shared, event('b', 19.0)],
        vec![],
    );

    let dashboard = project_tracking(
        local,
        &[PeerTrackingState::current(peer, NOW)],
        &[],
        "2027-01-15",
        NOW,
    );

    assert_eq!(dashboard.unique_events, 2);
    assert_eq!(
        dashboard.days.iter().map(|row| row.tokens).sum::<f64>(),
        37.0
    );
}

#[test]
fn an_event_id_content_collision_quarantines_the_later_device() {
    let local = payload("device-a", "Laptop", vec![event('a', 18.0)], vec![]);
    let mut collision = event('a', 18.0);
    collision.cost = 999.0;
    let peer = payload("device-b", "Desktop", vec![collision], vec![]);

    let dashboard = project_tracking(
        local,
        &[PeerTrackingState::current(peer, NOW)],
        &[],
        "2027-01-15",
        NOW,
    );

    assert_eq!(dashboard.unique_events, 1);
    assert_eq!(dashboard.quarantined_devices, vec!["device-b"]);
}

#[test]
fn a_quarantined_device_uses_its_prior_last_good_projection() {
    let local = payload("device-a", "Laptop", vec![event('a', 18.0)], vec![]);
    let mut collision = event('a', 18.0);
    collision.cost = 999.0;
    let current = payload("device-b", "Desktop", vec![collision], vec![]);
    let last_good = payload("device-b", "Desktop", vec![event('b', 19.0)], vec![]);

    let dashboard = project_tracking(
        local,
        &[PeerTrackingState {
            current,
            last_good: Some(last_good),
            received_at_ms: NOW,
        }],
        &[],
        "2027-01-15",
        NOW,
    );

    assert_eq!(dashboard.unique_events, 2);
    assert_eq!(dashboard.quarantined_devices, vec!["device-b"]);
}

#[test]
fn tombstones_remove_events_before_daily_aggregation() {
    let removed = event('a', 18.0);
    let mut local = payload("device-a", "Laptop", vec![removed.clone()], vec![]);
    local.events.clear();
    local.tombstones.push(TombstoneV2 {
        event_id: removed.event_id.clone(),
        removed_at_ms: NOW,
    });
    let peer = payload("device-b", "Desktop", vec![removed], vec![]);

    let dashboard = project_tracking(
        local,
        &[PeerTrackingState::current(peer, NOW)],
        &[],
        "2027-01-15",
        NOW,
    );

    assert_eq!(dashboard.unique_events, 0);
    assert!(dashboard.days.is_empty());
}

#[test]
fn newest_fresh_quota_wins_and_disagreement_is_reported() {
    let local = payload(
        "device-a",
        "Laptop",
        vec![],
        vec![quota("device-a", 30.0, NOW - 60_000, NOW + 3_600_000)],
    );
    let peer = payload(
        "newest-device",
        "Desktop",
        vec![],
        vec![quota("newest-device", 41.0, NOW - 10_000, NOW + 4_000_000)],
    );

    let dashboard = project_tracking(
        local,
        &[PeerTrackingState::current(peer, NOW)],
        &[],
        "2027-01-15",
        NOW,
    );

    assert_eq!(dashboard.quotas[0].source_device_id, "newest-device");
    assert!(dashboard.quotas[0].disagreement);
}

#[test]
fn quotas_older_than_two_sync_intervals_are_ignored() {
    let local = payload(
        "device-a",
        "Laptop",
        vec![],
        vec![quota("device-a", 30.0, NOW - 600_001, NOW + 3_600_000)],
    );

    let dashboard = project_tracking(local, &[], &[], "2027-01-15", NOW);

    assert!(dashboard.quotas.is_empty());
}

#[test]
fn legacy_rows_disappear_only_for_matching_v2_account_days() {
    let local = payload("device-a", "Laptop", vec![event('a', 18.0)], vec![]);
    let legacy = HistoryPayloadV1 {
        accounts: vec![AccountHistoryV1 {
            provider_id: "codex".into(),
            record_id: "record-a".into(),
            days: vec![
                legacy_day("2027-01-15", 10.0),
                legacy_day("2027-01-14", 20.0),
            ],
        }],
    };

    let dashboard = project_tracking(local, &[], &[legacy], "2027-01-15", NOW);

    assert!(dashboard.legacy_present);
    assert!(!dashboard
        .days
        .iter()
        .any(|row| row.device_id == "legacy" && row.day == "2027-01-15"));
    assert!(dashboard
        .days
        .iter()
        .any(|row| row.device_id == "legacy" && row.day == "2027-01-14"));
}

fn payload(
    device_id: &str,
    label: &str,
    events: Vec<UsageEventV2>,
    quotas: Vec<QuotaSnapshotV2>,
) -> TrackingPayloadV2 {
    TrackingPayloadV2 {
        device: DeviceDescriptorV2::new(device_id, label, NOW, "test").unwrap(),
        events,
        quotas,
        tombstones: vec![],
        retained_from_day: "2026-10-17".into(),
    }
}

fn event(suffix: char, tokens: f64) -> UsageEventV2 {
    UsageEventV2::new(
        format!("evt_{}", suffix.to_string().repeat(64)),
        "codex",
        "record-a",
        NOW,
        "gpt-5",
        TokenUsageV2 {
            input: tokens,
            output: 0.0,
            cached: 0.0,
            reasoning: 0.0,
            total: tokens,
        },
        tokens / 100.0,
        "catalog-test",
        "normalized-local",
    )
    .unwrap()
}

fn quota(device_id: &str, used_percent: f64, observed_at_ms: i64, reset: i64) -> QuotaSnapshotV2 {
    QuotaSnapshotV2 {
        provider_id: "codex".into(),
        record_id: "record-a".into(),
        metric_id: "weekly".into(),
        used_percent,
        remaining: None,
        limit: None,
        resets_at_ms: Some(reset),
        observed_at_ms,
        period_ms: Some(300_000),
        source_device_id: device_id.into(),
    }
}

fn legacy_day(day: &str, tokens: f64) -> DailyUsageV1 {
    DailyUsageV1 {
        day: day.into(),
        cost: tokens / 100.0,
        tokens,
        models: vec![ModelUsageV1 {
            model: "gpt-5".into(),
            cost: tokens / 100.0,
            tokens,
        }],
        unpriced_models: vec![],
    }
}
