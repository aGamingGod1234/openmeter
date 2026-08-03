use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MAX_ENVELOPE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_PAYLOAD_BYTES: usize = 8 * 1024 * 1024;
pub const HISTORY_SCHEMA: &str = "openmeter.history.v1";

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("invalid sync payload: {0}")]
    Invalid(&'static str),
    #[error("sync payload serialization failed")]
    Serialization,
    #[error("sync payload compression failed")]
    Compression,
    #[error("sync envelope authentication failed")]
    Authentication,
    #[error("sync payload exceeds its size limit")]
    TooLarge,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EnvelopeMeta {
    pub schema: String,
    pub device_id: String,
    pub revision: u64,
    pub generated_at_ms: i64,
}

impl EnvelopeMeta {
    pub fn new(
        device_id: impl Into<String>,
        revision: u64,
        generated_at_ms: i64,
    ) -> Result<Self, ProtocolError> {
        let meta = Self {
            schema: HISTORY_SCHEMA.to_string(),
            device_id: device_id.into(),
            revision,
            generated_at_ms,
        };
        meta.validate()?;
        Ok(meta)
    }

    pub(crate) fn validate(&self) -> Result<(), ProtocolError> {
        if self.schema != HISTORY_SCHEMA {
            return Err(ProtocolError::Invalid("unsupported schema"));
        }
        validate_id(&self.device_id, 64)?;
        if self.revision == 0 {
            return Err(ProtocolError::Invalid("revision must be positive"));
        }
        if self.generated_at_ms < 0 {
            return Err(ProtocolError::Invalid(
                "generation time must be nonnegative",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryPayloadV1 {
    pub accounts: Vec<AccountHistoryV1>,
}

impl HistoryPayloadV1 {
    pub(crate) fn validate(&self) -> Result<(), ProtocolError> {
        if self.accounts.len() > 512 {
            return Err(ProtocolError::Invalid("too many accounts"));
        }
        let mut accounts = HashSet::new();
        let mut rows = HashSet::new();
        for account in &self.accounts {
            validate_id(&account.provider_id, 64)?;
            validate_id(&account.record_id, 128)?;
            if !accounts.insert((&account.provider_id, &account.record_id)) {
                return Err(ProtocolError::Invalid("duplicate account"));
            }
            if account.days.len() > 366 {
                return Err(ProtocolError::Invalid("too many history days"));
            }
            for day in &account.days {
                validate_day(&day.day)?;
                validate_total(day.cost)?;
                validate_total(day.tokens)?;
                if !rows.insert((&account.provider_id, &account.record_id, &day.day)) {
                    return Err(ProtocolError::Invalid("duplicate account day"));
                }
                if day.models.len() > 512 || day.unpriced_models.len() > 512 {
                    return Err(ProtocolError::Invalid("too many models"));
                }
                let mut models = HashSet::new();
                for model in &day.models {
                    validate_text(&model.model, 256)?;
                    validate_total(model.cost)?;
                    validate_total(model.tokens)?;
                    if !models.insert(&model.model) {
                        return Err(ProtocolError::Invalid("duplicate model"));
                    }
                }
                for model in &day.unpriced_models {
                    validate_text(model, 256)?;
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountHistoryV1 {
    pub provider_id: String,
    pub record_id: String,
    pub days: Vec<DailyUsageV1>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DailyUsageV1 {
    pub day: String,
    pub cost: f64,
    pub tokens: f64,
    pub models: Vec<ModelUsageV1>,
    pub unpriced_models: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelUsageV1 {
    pub model: String,
    pub cost: f64,
    pub tokens: f64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedEnvelope {
    pub meta: EnvelopeMeta,
    #[serde(with = "base64_bytes")]
    pub nonce: Vec<u8>,
    #[serde(with = "base64_bytes")]
    pub ciphertext: Vec<u8>,
}

mod base64_bytes {
    use base64::engine::general_purpose::STANDARD;
    use base64::Engine;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&STANDARD.encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Vec<u8>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let encoded = String::deserialize(deserializer)?;
        STANDARD.decode(encoded).map_err(serde::de::Error::custom)
    }
}

fn validate_id(value: &str, max: usize) -> Result<(), ProtocolError> {
    if value.is_empty()
        || value.len() > max
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'@' | b'.'))
    {
        return Err(ProtocolError::Invalid("invalid identifier"));
    }
    Ok(())
}

fn validate_text(value: &str, max: usize) -> Result<(), ProtocolError> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(ProtocolError::Invalid("invalid model identifier"));
    }
    Ok(())
}

fn validate_day(value: &str) -> Result<(), ProtocolError> {
    let bytes = value.as_bytes();
    if bytes.len() != 10
        || bytes[4] != b'-'
        || bytes[7] != b'-'
        || bytes
            .iter()
            .enumerate()
            .any(|(index, byte)| !matches!(index, 4 | 7) && !byte.is_ascii_digit())
    {
        return Err(ProtocolError::Invalid("invalid calendar day"));
    }
    Ok(())
}

fn validate_total(value: f64) -> Result<(), ProtocolError> {
    if !value.is_finite() || value < 0.0 {
        return Err(ProtocolError::Invalid("invalid usage total"));
    }
    Ok(())
}
