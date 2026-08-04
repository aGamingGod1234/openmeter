use std::error::Error;
use std::time::{SystemTime, UNIX_EPOCH};

use openmeter_sync_protocol::{
    derive_tracking_key, seal_tracking, tracking_event_id, DeviceDescriptorV2, EnvelopeMeta,
    TokenUsageV2, TrackingPayloadV2, UsageEventV2,
};
use serde_json::json;

fn main() -> Result<(), Box<dyn Error>> {
    let mut arguments = std::env::args().skip(1);
    let device_a = arguments.next().ok_or("missing first device id")?;
    let device_b = arguments.next().ok_or("missing second device id")?;
    if arguments.next().is_some() {
        return Err("usage: tracking_fixture <device-a> <device-b>".into());
    }

    // This fixed key exists only to make disposable acceptance fixtures reproducible.
    // It is unrelated to every production recovery key and is never printed.
    let key = derive_tracking_key(&[0x42_u8; 32])?;
    let now_ms = i64::try_from(SystemTime::now().duration_since(UNIX_EPOCH)?.as_millis())?;
    let common = tracking_event_id(&key, "synthetic", "fixture", b"shared-event")?;
    let unique = tracking_event_id(&key, "synthetic", "fixture", b"unique-event")?;

    let envelope_a = seal_tracking(
        &key,
        EnvelopeMeta::tracking_v2(&device_a, 1, now_ms)?,
        &payload(&device_a, "Synthetic A", now_ms, [&common, &unique])?,
    )?;
    let envelope_b = seal_tracking(
        &key,
        EnvelopeMeta::tracking_v2(&device_b, 1, now_ms)?,
        &payload(&device_b, "Synthetic B", now_ms, [&common])?,
    )?;

    println!(
        "{}",
        serde_json::to_string(&json!({
            "envelope_a": envelope_a,
            "envelope_b": envelope_b,
            "expected_unique_events": 2
        }))?
    );
    Ok(())
}

fn payload<'a>(
    device_id: &str,
    label: &str,
    now_ms: i64,
    event_ids: impl IntoIterator<Item = &'a String>,
) -> Result<TrackingPayloadV2, Box<dyn Error>> {
    let events = event_ids
        .into_iter()
        .enumerate()
        .map(|(index, event_id)| {
            UsageEventV2::new(
                event_id,
                "synthetic",
                "fixture",
                now_ms + i64::try_from(index).unwrap_or_default(),
                "fixture-model",
                TokenUsageV2 {
                    input: 10.0,
                    output: 5.0,
                    cached: 0.0,
                    reasoning: 0.0,
                    total: 15.0,
                },
                0.01,
                "fixture-v1",
                "synthetic-fixture",
            )
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(TrackingPayloadV2 {
        device: DeviceDescriptorV2::new(device_id, label, now_ms, "fixture-v2")?,
        events,
        quotas: vec![],
        tombstones: vec![],
        retained_from_day: "2026-05-07".into(),
    })
}
