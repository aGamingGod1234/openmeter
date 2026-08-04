use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::model::{validate_day, validate_id, validate_text, validate_total, ProtocolError};

const MAX_EVENTS: usize = 100_000;
const MAX_QUOTAS: usize = 4_096;
const MAX_TOMBSTONES: usize = 100_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrackingPayloadV2 {
    pub device: DeviceDescriptorV2,
    pub events: Vec<UsageEventV2>,
    pub quotas: Vec<QuotaSnapshotV2>,
    pub tombstones: Vec<TombstoneV2>,
    pub retained_from_day: String,
}

impl TrackingPayloadV2 {
    pub(crate) fn validate(&self) -> Result<(), ProtocolError> {
        self.device.validate()?;
        validate_day(&self.retained_from_day)?;
        if self.events.len() > MAX_EVENTS {
            return Err(ProtocolError::Invalid("too many tracking events"));
        }
        if self.quotas.len() > MAX_QUOTAS {
            return Err(ProtocolError::Invalid("too many quota snapshots"));
        }
        if self.tombstones.len() > MAX_TOMBSTONES {
            return Err(ProtocolError::Invalid("too many tombstones"));
        }
        let mut event_ids = HashSet::with_capacity(self.events.len());
        for event in &self.events {
            event.validate()?;
            if !event_ids.insert(event.event_id.as_str()) {
                return Err(ProtocolError::Invalid("duplicate tracking event"));
            }
        }
        for quota in &self.quotas {
            quota.validate()?;
        }
        let mut tombstone_ids = HashSet::with_capacity(self.tombstones.len());
        for tombstone in &self.tombstones {
            tombstone.validate()?;
            if !tombstone_ids.insert(tombstone.event_id.as_str()) {
                return Err(ProtocolError::Invalid("duplicate tombstone"));
            }
            if event_ids.contains(tombstone.event_id.as_str()) {
                return Err(ProtocolError::Invalid("event is also tombstoned"));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceDescriptorV2 {
    pub device_id: String,
    pub label: String,
    pub generated_at_ms: i64,
    pub client_version: String,
}

impl DeviceDescriptorV2 {
    pub fn new(
        device_id: impl Into<String>,
        label: impl Into<String>,
        generated_at_ms: i64,
        client_version: impl Into<String>,
    ) -> Result<Self, ProtocolError> {
        let descriptor = Self {
            device_id: device_id.into(),
            label: label.into(),
            generated_at_ms,
            client_version: client_version.into(),
        };
        descriptor.validate()?;
        Ok(descriptor)
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        validate_id(&self.device_id, 64)?;
        validate_label(&self.label)?;
        validate_text(&self.client_version, 32)?;
        if self.generated_at_ms < 0 {
            return Err(ProtocolError::Invalid("invalid device generation time"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TokenUsageV2 {
    pub input: f64,
    pub output: f64,
    pub cached: f64,
    pub reasoning: f64,
    pub total: f64,
}

impl TokenUsageV2 {
    fn validate(&self) -> Result<(), ProtocolError> {
        for value in [
            self.input,
            self.output,
            self.cached,
            self.reasoning,
            self.total,
        ] {
            validate_total(value)?;
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UsageEventV2 {
    pub event_id: String,
    pub provider_id: String,
    pub record_id: String,
    pub occurred_at_ms: i64,
    pub model: String,
    pub tokens: TokenUsageV2,
    pub cost: f64,
    pub pricing_version: String,
    pub source_kind: String,
}

impl UsageEventV2 {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        event_id: impl Into<String>,
        provider_id: impl Into<String>,
        record_id: impl Into<String>,
        occurred_at_ms: i64,
        model: impl Into<String>,
        tokens: TokenUsageV2,
        cost: f64,
        pricing_version: impl Into<String>,
        source_kind: impl Into<String>,
    ) -> Result<Self, ProtocolError> {
        let event = Self {
            event_id: event_id.into(),
            provider_id: provider_id.into(),
            record_id: record_id.into(),
            occurred_at_ms,
            model: model.into(),
            tokens,
            cost,
            pricing_version: pricing_version.into(),
            source_kind: source_kind.into(),
        };
        event.validate()?;
        Ok(event)
    }

    fn validate(&self) -> Result<(), ProtocolError> {
        validate_event_id(&self.event_id)?;
        validate_id(&self.provider_id, 64)?;
        validate_id(&self.record_id, 128)?;
        validate_text(&self.model, 256)?;
        validate_text(&self.pricing_version, 64)?;
        validate_id(&self.source_kind, 64)?;
        self.tokens.validate()?;
        validate_total(self.cost)?;
        if self.occurred_at_ms < 0 {
            return Err(ProtocolError::Invalid("invalid event time"));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuotaSnapshotV2 {
    pub provider_id: String,
    pub record_id: String,
    pub metric_id: String,
    pub used_percent: f64,
    pub remaining: Option<f64>,
    pub limit: Option<f64>,
    pub resets_at_ms: Option<i64>,
    pub observed_at_ms: i64,
    pub period_ms: Option<i64>,
    pub source_device_id: String,
}

impl QuotaSnapshotV2 {
    fn validate(&self) -> Result<(), ProtocolError> {
        validate_id(&self.provider_id, 64)?;
        validate_id(&self.record_id, 128)?;
        validate_id(&self.metric_id, 128)?;
        validate_total(self.used_percent)?;
        if self.used_percent > 100.0 {
            return Err(ProtocolError::Invalid("invalid quota percentage"));
        }
        for value in [self.remaining, self.limit].into_iter().flatten() {
            validate_total(value)?;
        }
        if self.observed_at_ms < 0
            || self.resets_at_ms.is_some_and(|value| value < 0)
            || self.period_ms.is_some_and(|value| value <= 0)
        {
            return Err(ProtocolError::Invalid("invalid quota time"));
        }
        validate_id(&self.source_device_id, 64)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TombstoneV2 {
    pub event_id: String,
    pub removed_at_ms: i64,
}

impl TombstoneV2 {
    fn validate(&self) -> Result<(), ProtocolError> {
        validate_event_id(&self.event_id)?;
        if self.removed_at_ms < 0 {
            return Err(ProtocolError::Invalid("invalid tombstone time"));
        }
        Ok(())
    }
}

fn validate_label(value: &str) -> Result<(), ProtocolError> {
    if value.trim().is_empty() || value.len() > 32 || value.chars().any(char::is_control) {
        return Err(ProtocolError::Invalid("invalid device label"));
    }
    Ok(())
}

fn validate_event_id(value: &str) -> Result<(), ProtocolError> {
    let Some(hex) = value.strip_prefix("evt_") else {
        return Err(ProtocolError::Invalid("invalid event identifier"));
    };
    if hex.len() != 64
        || !hex
            .bytes()
            .all(|byte| byte.is_ascii_digit() || matches!(byte, b'a'..=b'f'))
    {
        return Err(ProtocolError::Invalid("invalid event identifier"));
    }
    Ok(())
}
