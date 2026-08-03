use std::collections::HashSet;
use std::path::{Path, PathBuf};

use base64::Engine;
use serde_json::Value;

use crate::accounts::AccountSource;
use crate::environment::EnvironmentSnapshot;

const MAX_DIRECTORY_ENTRIES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredSource {
    pub provider_id: String,
    pub identity_key: String,
    pub suggested_label: Option<String>,
    pub source: AccountSource,
}

pub fn discover_claude_sources(environment: &EnvironmentSnapshot) -> Vec<DiscoveredSource> {
    let active = environment
        .var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| environment.home_dir().join(".claude"));
    let mut candidates = vec![active.clone()];
    candidates.extend(prefixed_children(environment.home_dir(), ".claude-"));
    candidates.extend(directory_children(&environment.home_dir().join(".config")));
    discover_candidates("claude", candidates, &active, claude_identity)
}

pub fn discover_codex_sources(environment: &EnvironmentSnapshot) -> Vec<DiscoveredSource> {
    let active = environment
        .var("CODEX_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| environment.home_dir().join(".codex"));
    let candidates = vec![active.clone(), environment.home_dir().join(".codex")];
    discover_candidates("codex", candidates, &active, codex_identity)
}

fn discover_candidates(
    provider_id: &str,
    candidates: Vec<PathBuf>,
    active: &Path,
    identity_reader: fn(&Path) -> Option<(String, Option<String>)>,
) -> Vec<DiscoveredSource> {
    let active = canonical_directory(active);
    let mut seen = HashSet::new();
    candidates
        .into_iter()
        .take(MAX_DIRECTORY_ENTRIES)
        .filter_map(|candidate| {
            let canonical = canonical_directory(&candidate)?;
            if !seen.insert(canonical.clone()) {
                return None;
            }
            let (identity_key, suggested_label) = identity_reader(&canonical)?;
            let source_id = source_id(provider_id, &canonical);
            let source = if active.as_ref() == Some(&canonical) {
                AccountSource::default_home(source_id)
            } else {
                AccountSource::directory(source_id, canonical).ok()?
            };
            Some(DiscoveredSource {
                provider_id: provider_id.to_string(),
                identity_key,
                suggested_label,
                source,
            })
        })
        .collect()
}

fn canonical_directory(path: &Path) -> Option<PathBuf> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return None;
    }
    std::fs::canonicalize(path).ok()
}

fn prefixed_children(root: &Path, prefix: &str) -> Vec<PathBuf> {
    bounded_children(root)
        .into_iter()
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with(prefix))
        })
        .collect()
}

fn directory_children(root: &Path) -> Vec<PathBuf> {
    bounded_children(root)
}

fn bounded_children(root: &Path) -> Vec<PathBuf> {
    let Some(canonical_root) = canonical_directory(root) else {
        return Vec::new();
    };
    let Ok(entries) = std::fs::read_dir(&canonical_root) else {
        return Vec::new();
    };
    entries
        .take(MAX_DIRECTORY_ENTRIES)
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let file_type = entry.file_type().ok()?;
            if file_type.is_symlink() || !file_type.is_dir() {
                return None;
            }
            let path = entry.path().canonicalize().ok()?;
            path.starts_with(&canonical_root).then_some(path)
        })
        .collect()
}

fn claude_identity(root: &Path) -> Option<(String, Option<String>)> {
    if !root.join(".credentials.json").is_file() {
        return None;
    }
    let document: Value =
        serde_json::from_slice(&std::fs::read(root.join(".claude.json")).ok()?).ok()?;
    let account = document.get("oauthAccount")?;
    let identity = account
        .get("accountUuid")
        .or_else(|| account.get("organizationUuid"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())?;
    let label = account
        .get("organizationName")
        .or_else(|| account.get("emailAddress"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    Some((identity.to_string(), label))
}

fn codex_identity(root: &Path) -> Option<(String, Option<String>)> {
    let document: Value =
        serde_json::from_slice(&std::fs::read(root.join("auth.json")).ok()?).ok()?;
    let tokens = document.get("tokens")?;
    let identity = tokens
        .get("account_id")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
        .or_else(|| {
            tokens
                .get("id_token")
                .and_then(Value::as_str)
                .and_then(codex_id_token_account)
        })?;
    Some((identity, None))
}

fn codex_id_token_account(token: &str) -> Option<String> {
    let payload = token.split('.').nth(1)?;
    let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(payload)
        .ok()?;
    let claims: Value = serde_json::from_slice(&bytes).ok()?;
    claims
        .pointer("/https:~1~1api.openai.com~1auth/chatgpt_account_id")
        .or_else(|| claims.get("chatgpt_account_id"))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn source_id(provider_id: &str, path: &Path) -> String {
    let stamp = crate::redaction::credential_stamp(path.as_os_str().to_string_lossy().as_bytes());
    format!("{provider_id}-{}", &stamp[..16])
}
