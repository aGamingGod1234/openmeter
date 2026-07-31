use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const REGISTRY_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AccountId(String);

impl AccountId {
    pub fn new(value: impl Into<String>) -> Result<Self, String> {
        let value = value.into();
        validate_segment("account", &value)?;
        Ok(Self(value))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[serde(deny_unknown_fields)]
pub enum AccountSource {
    DefaultHome,
    Directory { path: PathBuf },
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountContext {
    pub provider_id: String,
    pub account_id: AccountId,
    pub card_id: String,
    pub display_name: String,
    pub source: AccountSource,
    pub enabled: bool,
}

impl AccountContext {
    pub fn default_for(provider_id: &str) -> Result<Self, String> {
        validate_segment("provider", provider_id)?;
        Ok(Self {
            provider_id: provider_id.to_string(),
            account_id: AccountId("default".to_string()),
            card_id: provider_id.to_string(),
            display_name: provider_display_name(provider_id),
            source: AccountSource::DefaultHome,
            enabled: true,
        })
    }

    pub fn named(provider_id: &str, account_id: &str, label: &str) -> Result<Self, String> {
        validate_segment("provider", provider_id)?;
        validate_segment("account", account_id)?;
        if account_id == "default" {
            return Err("named accounts cannot use the reserved id 'default'".to_string());
        }
        let label = label.trim();
        if label.is_empty() {
            return Err("account label cannot be empty".to_string());
        }
        Ok(Self {
            provider_id: provider_id.to_string(),
            account_id: AccountId(account_id.to_string()),
            card_id: format!("{provider_id}--{account_id}"),
            display_name: format!("{} — {label}", provider_display_name(provider_id)),
            source: AccountSource::Manual,
            enabled: true,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRegistry {
    version: u32,
    accounts: Vec<AccountContext>,
}

impl Default for AccountRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            accounts: Vec::new(),
        }
    }
}

impl AccountRegistry {
    pub fn from_accounts(accounts: Vec<AccountContext>) -> Result<Self, String> {
        let registry = Self {
            version: REGISTRY_VERSION,
            accounts,
        };
        registry.validate()?;
        Ok(registry)
    }

    pub fn accounts(&self) -> &[AccountContext] {
        &self.accounts
    }

    pub fn match_token(&self, token: &str) -> Vec<&AccountContext> {
        self.accounts
            .iter()
            .filter(|account| account.card_id == token || account.provider_id == token)
            .collect()
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        match std::fs::read_to_string(path) {
            Ok(raw) => {
                let registry: Self = serde_json::from_str(raw.trim_start_matches('\u{feff}'))
                    .map_err(|error| format!("parse {}: {error}", path.display()))?;
                registry.validate()?;
                Ok(registry)
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(error) => Err(format!("read {}: {error}", path.display())),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        self.validate()?;
        let bytes = serde_json::to_vec_pretty(self)
            .map_err(|error| format!("serialize account registry: {error}"))?;
        crate::platform::atomic_write(path, &bytes)
            .map_err(|error| format!("save {}: {error}", path.display()))
    }

    fn validate(&self) -> Result<(), String> {
        if self.version != REGISTRY_VERSION {
            return Err(format!(
                "unsupported account registry version {}; expected {REGISTRY_VERSION}",
                self.version
            ));
        }
        let mut cards = HashSet::new();
        for account in &self.accounts {
            validate_segment("provider", &account.provider_id)?;
            validate_segment("account", account.account_id.as_str())?;
            let expected = if account.account_id.as_str() == "default" {
                account.provider_id.clone()
            } else {
                format!("{}--{}", account.provider_id, account.account_id.as_str())
            };
            if account.card_id != expected {
                return Err(format!(
                    "card id '{}' does not match provider/account identity",
                    account.card_id
                ));
            }
            if !cards.insert(account.card_id.clone()) {
                return Err(format!(
                    "duplicate account card id '{}': registry rejected",
                    account.card_id
                ));
            }
        }
        Ok(())
    }
}

fn validate_segment(kind: &str, value: &str) -> Result<(), String> {
    if value.is_empty() || value.len() > 64 {
        return Err(format!("{kind} id must contain 1 to 64 characters"));
    }
    if !value.bytes().all(|byte| {
        byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'_'
    }) {
        return Err(format!(
            "{kind} id may contain only lowercase ASCII letters, digits, '-' and '_'"
        ));
    }
    if value.starts_with('-') || value.ends_with('-') || value.contains("--") {
        return Err(format!("{kind} id contains a reserved separator"));
    }
    Ok(())
}

fn provider_display_name(provider_id: &str) -> String {
    match provider_id {
        "aihubmix" => "AihubMix".to_string(),
        "codex" => "Codex".to_string(),
        "copilot" => "Copilot".to_string(),
        "deepseek" => "DeepSeek".to_string(),
        "elevenlabs" => "ElevenLabs".to_string(),
        "minimax" => "MiniMax".to_string(),
        "moonshot" => "Moonshot".to_string(),
        "opencode" => "OpenCode".to_string(),
        "openrouter" => "OpenRouter".to_string(),
        "zai" => "Z.ai".to_string(),
        other => {
            let mut chars = other.chars();
            chars
                .next()
                .map(|first| first.to_uppercase().collect::<String>() + chars.as_str())
                .unwrap_or_default()
        }
    }
}
