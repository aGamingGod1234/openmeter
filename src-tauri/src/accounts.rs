use std::collections::HashSet;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const REGISTRY_VERSION: u32 = 2;

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AccountSourceKind {
    DefaultHome,
    Directory,
    Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountSource {
    pub id: String,
    pub kind: AccountSourceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub holds_default_source: bool,
}

impl AccountSource {
    pub fn default_home(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: AccountSourceKind::DefaultHome,
            path: None,
            holds_default_source: true,
        }
    }

    pub fn directory(id: impl Into<String>, path: PathBuf) -> Result<Self, String> {
        if path.as_os_str().is_empty() {
            return Err("account source directory cannot be empty".to_string());
        }
        Ok(Self {
            id: id.into(),
            kind: AccountSourceKind::Directory,
            path: Some(path),
            holds_default_source: false,
        })
    }

    pub fn manual(id: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind: AccountSourceKind::Manual,
            path: None,
            holds_default_source: false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountContext {
    pub provider_id: String,
    pub account_id: AccountId,
    pub card_id: String,
    pub display_name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub identity_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub sources: Vec<AccountSource>,
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
            identity_key: None,
            label: None,
            sources: vec![AccountSource::default_home(format!("{provider_id}-home"))],
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
            identity_key: None,
            label: Some(label.to_string()),
            sources: vec![AccountSource::manual(format!("{provider_id}-{account_id}"))],
            enabled: true,
        })
    }

    pub fn identified(
        provider_id: &str,
        identity_key: &str,
        label: Option<&str>,
        source: AccountSource,
    ) -> Result<Self, String> {
        let mut account = Self::default_for(provider_id)?;
        let identity_key = identity_key.trim();
        if identity_key.is_empty() {
            return Err("account identity key cannot be empty".to_string());
        }
        let label = label.map(str::trim).filter(|value| !value.is_empty());
        account.identity_key = Some(identity_key.to_string());
        account.label = label.map(str::to_string);
        account.display_name = label
            .map(|value| format!("{} — {value}", provider_display_name(provider_id)))
            .unwrap_or_else(|| provider_display_name(provider_id));
        account.sources = vec![source];
        Ok(account)
    }

    pub fn primary_source(&self) -> Option<&AccountSource> {
        self.sources
            .iter()
            .find(|source| source.holds_default_source)
            .or_else(|| self.sources.first())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AccountRegistry {
    version: u32,
    accounts: Vec<AccountContext>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionOneRegistry {
    version: u32,
    accounts: Vec<VersionOneAccount>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VersionOneAccount {
    provider_id: String,
    account_id: AccountId,
    card_id: String,
    display_name: String,
    source: VersionOneSource,
    enabled: bool,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum VersionOneSource {
    DefaultHome,
    Directory { path: PathBuf },
    Manual,
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

    pub fn upsert(&mut self, account: AccountContext) -> Result<(), String> {
        if let Some(existing) = self
            .accounts
            .iter_mut()
            .find(|existing| existing.card_id == account.card_id)
        {
            *existing = account;
        } else {
            self.accounts.push(account);
        }
        self.validate()
    }

    pub fn attach_source(
        &mut self,
        identity_key: &str,
        source: AccountSource,
    ) -> Result<(), String> {
        let account = self
            .accounts
            .iter_mut()
            .find(|account| account.identity_key.as_deref() == Some(identity_key))
            .ok_or_else(|| format!("unknown account identity '{identity_key}'"))?;
        if let Some(existing) = account
            .sources
            .iter_mut()
            .find(|existing| existing.id == source.id)
        {
            *existing = source;
        } else {
            account.sources.push(source);
        }
        self.validate()
    }

    pub fn remove(&mut self, card_id: &str) -> Result<bool, String> {
        let before = self.accounts.len();
        self.accounts.retain(|account| account.card_id != card_id);
        self.validate()?;
        Ok(before != self.accounts.len())
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
                let raw = raw.trim_start_matches('\u{feff}');
                let version = serde_json::from_str::<serde_json::Value>(raw)
                    .map_err(|error| format!("parse {}: {error}", path.display()))?
                    .get("version")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| format!("parse {}: missing registry version", path.display()))?;
                let registry = match version {
                    1 => migrate_version_one(
                        serde_json::from_str(raw)
                            .map_err(|error| format!("parse {}: {error}", path.display()))?,
                    ),
                    2 => serde_json::from_str(raw)
                        .map_err(|error| format!("parse {}: {error}", path.display()))?,
                    other => return Err(format!(
                        "unsupported account registry version {other}; expected {REGISTRY_VERSION}"
                    )),
                };
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
            if account.sources.is_empty() {
                return Err(format!(
                    "account '{}' must have at least one credential source",
                    account.card_id
                ));
            }
            let mut source_ids = HashSet::new();
            for source in &account.sources {
                if source.id.trim().is_empty() {
                    return Err(format!(
                        "account '{}' has an empty source id",
                        account.card_id
                    ));
                }
                if !source_ids.insert(source.id.as_str()) {
                    return Err(format!(
                        "account '{}' has duplicate source id '{}'",
                        account.card_id, source.id
                    ));
                }
                if source.kind == AccountSourceKind::Directory && source.path.is_none() {
                    return Err(format!("directory source '{}' has no path", source.id));
                }
            }
        }
        Ok(())
    }
}

fn migrate_version_one(registry: VersionOneRegistry) -> AccountRegistry {
    debug_assert_eq!(registry.version, 1);
    let accounts = registry
        .accounts
        .into_iter()
        .map(|account| {
            let label = account
                .display_name
                .split_once(" — ")
                .map(|(_, label)| label.to_string());
            let source = match account.source {
                VersionOneSource::DefaultHome => {
                    AccountSource::default_home(format!("{}-home", account.card_id))
                }
                VersionOneSource::Directory { path } => AccountSource {
                    id: format!("{}-directory", account.card_id),
                    kind: AccountSourceKind::Directory,
                    path: Some(path),
                    holds_default_source: false,
                },
                VersionOneSource::Manual => {
                    AccountSource::manual(format!("{}-manual", account.card_id))
                }
            };
            AccountContext {
                provider_id: account.provider_id,
                account_id: account.account_id,
                card_id: account.card_id,
                display_name: account.display_name,
                identity_key: None,
                label,
                sources: vec![source],
                enabled: account.enabled,
            }
        })
        .collect();
    AccountRegistry {
        version: REGISTRY_VERSION,
        accounts,
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
