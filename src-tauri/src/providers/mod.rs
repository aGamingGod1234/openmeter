pub mod aihubmix;
pub mod antigravity;
pub mod claude;
pub mod codebuff;
pub mod codex;
pub mod copilot;
pub mod cursor;
pub mod deepseek;
pub mod devin;
pub mod elevenlabs;
pub mod grok;
pub mod hermes;
pub mod kilo;
pub mod minimax;
pub mod moonshot;
pub mod ollama;
pub mod opencode;
pub mod openrouter;
pub mod zai;

use serde::{Deserialize, Serialize};
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use crate::accounts::{AccountContext, AccountSource};
use crate::environment::EnvironmentSnapshot;

pub type ProviderFetchFuture = Pin<Box<dyn Future<Output = ProviderSnapshot> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderDescriptor {
    pub id: &'static str,
    pub display_name: &'static str,
}

pub fn provider_catalog() -> &'static [ProviderDescriptor] {
    &[
        ProviderDescriptor {
            id: "claude",
            display_name: "Claude",
        },
        ProviderDescriptor {
            id: "codex",
            display_name: "Codex",
        },
        ProviderDescriptor {
            id: "cursor",
            display_name: "Cursor",
        },
        ProviderDescriptor {
            id: "antigravity",
            display_name: "Antigravity",
        },
        ProviderDescriptor {
            id: "copilot",
            display_name: "Copilot",
        },
        ProviderDescriptor {
            id: "devin",
            display_name: "Devin",
        },
        ProviderDescriptor {
            id: "grok",
            display_name: "Grok",
        },
        ProviderDescriptor {
            id: "opencode",
            display_name: "OpenCode",
        },
        ProviderDescriptor {
            id: "openrouter",
            display_name: "OpenRouter",
        },
        ProviderDescriptor {
            id: "zai",
            display_name: "Z.ai",
        },
        ProviderDescriptor {
            id: "minimax",
            display_name: "MiniMax",
        },
        ProviderDescriptor {
            id: "deepseek",
            display_name: "DeepSeek",
        },
        ProviderDescriptor {
            id: "moonshot",
            display_name: "Moonshot",
        },
        ProviderDescriptor {
            id: "elevenlabs",
            display_name: "ElevenLabs",
        },
        ProviderDescriptor {
            id: "ollama",
            display_name: "Ollama",
        },
        ProviderDescriptor {
            id: "codebuff",
            display_name: "Codebuff",
        },
        ProviderDescriptor {
            id: "kilo",
            display_name: "Kilo",
        },
        ProviderDescriptor {
            id: "aihubmix",
            display_name: "AihubMix",
        },
    ]
}

#[derive(Clone)]
pub struct CredentialMaterial {
    source_id: String,
    secret: Vec<u8>,
}

impl CredentialMaterial {
    pub fn new(source_id: impl Into<String>, secret: Vec<u8>) -> Self {
        Self {
            source_id: source_id.into(),
            secret,
        }
    }

    pub fn source_id(&self) -> &str {
        &self.source_id
    }

    pub fn expose(&self) -> &[u8] {
        &self.secret
    }
}

impl std::fmt::Debug for CredentialMaterial {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("CredentialMaterial")
            .field("source_id", &self.source_id)
            .field("secret", &"[secret]")
            .finish()
    }
}

impl Drop for CredentialMaterial {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialProbe {
    pub source_id: String,
    pub credential_stamp: String,
}

type CredentialResolver =
    Arc<dyn Fn(&AccountContext) -> Result<CredentialMaterial, String> + Send + Sync>;
type RuntimeRefresher =
    Arc<dyn Fn(&AccountContext, &CredentialMaterial) -> ProviderFetchFuture + Send + Sync>;

#[derive(Clone)]
pub struct ProviderRuntime {
    id: &'static str,
    resolver: CredentialResolver,
    refresher: RuntimeRefresher,
}

impl ProviderRuntime {
    pub fn new<Resolve, Refresh, RefreshFuture>(
        id: &'static str,
        resolver: Resolve,
        refresher: Refresh,
    ) -> Self
    where
        Resolve: Fn(&AccountContext) -> Result<CredentialMaterial, String> + Send + Sync + 'static,
        Refresh: Fn(&AccountContext, &CredentialMaterial) -> RefreshFuture + Send + Sync + 'static,
        RefreshFuture: Future<Output = ProviderSnapshot> + Send + 'static,
    {
        Self {
            id,
            resolver: Arc::new(resolver),
            refresher: Arc::new(move |account, credential| {
                Box::pin(refresher(account, credential))
            }),
        }
    }

    pub fn probe(&self, account: &AccountContext) -> Result<CredentialProbe, String> {
        self.validate_account(account)?;
        let credential =
            (self.resolver)(account).map_err(|error| crate::redaction::redact_text(&error))?;
        Ok(CredentialProbe {
            source_id: credential.source_id().to_string(),
            credential_stamp: crate::redaction::credential_stamp(credential.expose()),
        })
    }

    pub async fn refresh(&self, account: &AccountContext) -> ProviderSnapshot {
        if let Err(error) = self.validate_account(account) {
            return self.error_snapshot(account, &error, "invalid-account");
        }
        let credential = match (self.resolver)(account) {
            Ok(credential) => credential,
            Err(error) => return self.error_snapshot(account, &error, "no-credential"),
        };
        let stamp = crate::redaction::credential_stamp(credential.expose());
        let raw = (self.refresher)(account, &credential).await;
        let fetched_at = chrono::Utc::now().timestamp_millis();
        let lifetime = (raw.expires_at - raw.fetched_at).clamp(0, 5 * 60 * 1_000);
        let mut snapshot =
            raw.with_cache_identity(account, &stamp, fetched_at, fetched_at + lifetime);
        crate::redaction::redact_snapshot(&mut snapshot);
        snapshot
    }

    fn validate_account(&self, account: &AccountContext) -> Result<(), String> {
        if account.provider_id == self.id {
            Ok(())
        } else {
            Err(format!(
                "runtime '{}' cannot refresh provider '{}'",
                self.id, account.provider_id
            ))
        }
    }

    fn error_snapshot(
        &self,
        account: &AccountContext,
        error: &str,
        stamp_seed: &str,
    ) -> ProviderSnapshot {
        let now = chrono::Utc::now().timestamp_millis();
        let mut snapshot = ProviderSnapshot::error(
            &account.card_id,
            &account.display_name,
            crate::redaction::redact_text(error),
        )
        .with_cache_identity(
            account,
            &crate::redaction::credential_stamp(stamp_seed.as_bytes()),
            now,
            now,
        );
        crate::redaction::redact_snapshot(&mut snapshot);
        snapshot
    }
}

/// Computes a one-way cache identity from the account's actual local
/// credential source. The bytes never leave this function.
pub fn account_credential_stamp(account: &AccountContext) -> String {
    let material = account_credential_material(account);
    crate::redaction::credential_stamp(material.expose())
}

pub fn account_credential_material(account: &AccountContext) -> CredentialMaterial {
    account_credential_material_with_environment(account, &EnvironmentSnapshot::capture())
}

fn account_credential_material_with_environment(
    account: &AccountContext,
    environment: &EnvironmentSnapshot,
) -> CredentialMaterial {
    let mut material = Vec::new();
    let mut sources = Vec::new();
    material.extend_from_slice(account.card_id.as_bytes());
    for path in credential_paths(account, environment) {
        if let Ok(bytes) = std::fs::read(&path) {
            sources.push(path.to_string_lossy().to_string());
            material.extend_from_slice(path.to_string_lossy().as_bytes());
            material.extend_from_slice(&bytes);
        }
    }
    for variable in credential_environment_variables(&account.provider_id) {
        if let Some(value) = environment.var(variable) {
            sources.push(format!("env:{variable}"));
            material.extend_from_slice(variable.as_bytes());
            material.extend_from_slice(value.to_string_lossy().as_bytes());
        }
    }
    for target in credential_manager_targets(&account.provider_id) {
        if let Some(bytes) = read_windows_credential(target) {
            sources.push(format!("wincred:{target}"));
            material.extend_from_slice(target.as_bytes());
            material.extend_from_slice(&bytes);
        }
    }
    if material.len() == account.card_id.len() {
        material.extend_from_slice(b":no-local-credential");
    }
    CredentialMaterial::new(
        if sources.is_empty() {
            "no-local-credential".to_string()
        } else {
            sources.join(";")
        },
        material,
    )
}

pub fn runtime_for(provider_id: &str) -> Option<ProviderRuntime> {
    runtime_for_with_environment(provider_id, EnvironmentSnapshot::capture())
}

pub fn runtime_for_with_environment(
    provider_id: &str,
    environment: EnvironmentSnapshot,
) -> Option<ProviderRuntime> {
    let descriptor = provider_catalog()
        .iter()
        .find(|descriptor| descriptor.id == provider_id)?;
    let id = descriptor.id;
    let environment = Arc::new(environment);
    Some(ProviderRuntime::new(
        id,
        move |account| {
            Ok(account_credential_material_with_environment(
                account,
                &environment,
            ))
        },
        move |account, _credential| account_snapshot(id, account.clone()),
    ))
}

fn account_snapshot(provider_id: &'static str, account: AccountContext) -> ProviderFetchFuture {
    match provider_id {
        "claude" => Box::pin(async move { claude::snapshot_for(&account).await }),
        "codex" => Box::pin(async move { codex::snapshot_for(&account).await }),
        _ if matches!(account.source, AccountSource::DefaultHome) => legacy_snapshot(provider_id),
        _ => Box::pin(async move {
            ProviderSnapshot::no_credentials(
                &account.card_id,
                &account.display_name,
                "This provider needs its default CLI profile on Windows.",
            )
        }),
    }
}

fn legacy_snapshot(provider_id: &'static str) -> ProviderFetchFuture {
    match provider_id {
        "claude" => Box::pin(claude::snapshot()),
        "codex" => Box::pin(codex::snapshot()),
        "cursor" => Box::pin(cursor::snapshot()),
        "antigravity" => Box::pin(antigravity::snapshot()),
        "copilot" => Box::pin(copilot::snapshot()),
        "devin" => Box::pin(devin::snapshot()),
        "grok" => Box::pin(grok::snapshot()),
        "opencode" => Box::pin(opencode::snapshot()),
        "openrouter" => Box::pin(openrouter::snapshot()),
        "zai" => Box::pin(zai::snapshot()),
        "minimax" => Box::pin(minimax::snapshot()),
        "deepseek" => Box::pin(deepseek::snapshot()),
        "moonshot" => Box::pin(moonshot::snapshot()),
        "elevenlabs" => Box::pin(elevenlabs::snapshot()),
        "ollama" => Box::pin(ollama::snapshot()),
        "codebuff" => Box::pin(codebuff::snapshot()),
        "kilo" => Box::pin(kilo::snapshot()),
        "aihubmix" => Box::pin(aihubmix::snapshot()),
        _ => Box::pin(async move {
            ProviderSnapshot::error(
                provider_id,
                provider_id,
                "unknown provider runtime".to_string(),
            )
        }),
    }
}

fn credential_paths(account: &AccountContext, environment: &EnvironmentSnapshot) -> Vec<PathBuf> {
    let relative: &[&str] = match account.provider_id.as_str() {
        "claude" => &[".claude/.credentials.json", ".credentials.json"],
        "codex" => &[".codex/auth.json", "auth.json"],
        "cursor" => &["Cursor/User/globalStorage/state.vscdb", "state.vscdb"],
        "copilot" => &[
            ".config/github-copilot/apps.json",
            ".config/github-copilot/hosts.json",
            "GitHub CLI/hosts.yml",
        ],
        "devin" => &[
            "devin/credentials.toml",
            ".local/share/devin/credentials.toml",
            "credentials.toml",
        ],
        "grok" => &[".grok/auth.json", "auth.json"],
        "opencode" => &[
            ".local/share/opencode/opencode.db",
            "opencode.db",
            ".local/share/opencode/auth.json",
        ],
        "openrouter" => &[".config/opencode/auth.json"],
        "zai" => &[".config/zai/key.json", "key.json"],
        "minimax" => &[".minimax/config.yaml"],
        "codebuff" => &[".config/manicode/credentials.json"],
        "kilo" => &[".local/share/kilo/auth.json"],
        _ => &[],
    };
    match &account.source {
        AccountSource::Directory { path } => relative.iter().map(|item| path.join(item)).collect(),
        AccountSource::Manual => vec![
            config_dir().join(format!("{}.json", account.card_id)),
            config_dir().join(format!("{}.json", account.provider_id)),
        ],
        AccountSource::DefaultHome => {
            let home = environment.home_dir().to_path_buf();
            let appdata = environment
                .var("APPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| home.clone());
            relative
                .iter()
                .flat_map(|item| [home.join(item), appdata.join(item)])
                .chain(std::iter::once(
                    config_dir().join(format!("{}.json", account.provider_id)),
                ))
                .collect()
        }
    }
}

fn credential_environment_variables(provider_id: &str) -> &'static [&'static str] {
    match provider_id {
        "openrouter" => &["OPENROUTER_API_KEY"],
        "zai" => &["ZAI_API_KEY", "GLM_API_KEY"],
        "minimax" => &["MINIMAX_API_KEY"],
        "deepseek" => &["DEEPSEEK_API_KEY"],
        "moonshot" => &["MOONSHOT_API_KEY", "KIMI_API_KEY"],
        "elevenlabs" => &["ELEVENLABS_API_KEY"],
        "codebuff" => &["CODEBUFF_API_KEY"],
        "kilo" => &["KILO_API_KEY"],
        "aihubmix" => &["AIHUBMIX_API_KEY"],
        _ => &[],
    }
}

fn credential_manager_targets(provider_id: &str) -> &'static [&'static str] {
    match provider_id {
        "antigravity" => &["gemini:antigravity"],
        "copilot" => &["gh:github.com", "gh:github.com:"],
        _ => &[],
    }
}

/// One row inside a provider card, e.g. "Session ▓▓▓░░ 43% left · Resets in 2h".
/// `resets_at` (epoch ms) + `period_ms` are the structured facts the pace
/// engine needs; the UI formats countdowns and projections from them.
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Metric {
    pub label: String,
    pub kind: String, // "progress" | "text"
    pub used_percent: Option<f64>,
    pub detail: Option<String>,
    pub value: Option<String>,
    pub resets_at: Option<i64>,
    pub period_ms: Option<i64>,
}

impl Metric {
    pub fn progress(label: &str, used_percent: f64, detail: Option<String>) -> Self {
        Self {
            label: label.into(),
            kind: "progress".into(),
            used_percent: Some(used_percent),
            detail,
            value: None,
            resets_at: None,
            period_ms: None,
        }
    }

    #[allow(dead_code)]
    pub fn text(label: &str, value: String) -> Self {
        Self {
            label: label.into(),
            kind: "text".into(),
            used_percent: None,
            detail: None,
            value: Some(value),
            resets_at: None,
            period_ms: None,
        }
    }

    pub fn with_reset(mut self, resets_at: Option<i64>, period_ms: Option<i64>) -> Self {
        self.resets_at = resets_at;
        self.period_ms = period_ms;
        self
    }
}

/// Everything one provider reports back after a refresh. `stale` marks a
/// snapshot that is actually the last good fetch, shown because the newest
/// attempt failed transiently (`warning` carries that error).
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
pub struct Snapshot {
    pub id: String,
    #[serde(default)]
    pub provider_id: String,
    #[serde(default = "default_account_id")]
    pub account_id: String,
    #[serde(default)]
    pub card_id: String,
    /// Opaque one-way credential identity used only to isolate local cache entries.
    #[serde(default)]
    pub credential_stamp: String,
    #[serde(default)]
    pub fetched_at: i64,
    #[serde(default)]
    pub expires_at: i64,
    pub name: String,
    pub plan: Option<String>,
    pub status: String, // "ok" | "no_credentials" | "error"
    pub error: Option<String>,
    pub metrics: Vec<Metric>,
    pub stale: bool,
    pub warning: Option<String>,
}

impl Snapshot {
    pub fn ok(id: &str, name: &str, plan: Option<String>, metrics: Vec<Metric>) -> Self {
        let fetched_at = chrono::Utc::now().timestamp_millis();
        Self {
            id: id.into(),
            provider_id: id.into(),
            account_id: default_account_id(),
            card_id: id.into(),
            credential_stamp: String::new(),
            fetched_at,
            expires_at: fetched_at + 5 * 60 * 1000,
            name: name.into(),
            plan,
            status: "ok".into(),
            error: None,
            metrics,
            stale: false,
            warning: None,
        }
    }

    pub fn no_credentials(id: &str, name: &str, hint: &str) -> Self {
        let fetched_at = chrono::Utc::now().timestamp_millis();
        Self {
            id: id.into(),
            provider_id: id.into(),
            account_id: default_account_id(),
            card_id: id.into(),
            credential_stamp: String::new(),
            fetched_at,
            expires_at: fetched_at,
            name: name.into(),
            plan: None,
            status: "no_credentials".into(),
            error: Some(hint.into()),
            metrics: vec![],
            stale: false,
            warning: None,
        }
    }

    pub fn error(id: &str, name: &str, message: String) -> Self {
        let fetched_at = chrono::Utc::now().timestamp_millis();
        Self {
            id: id.into(),
            provider_id: id.into(),
            account_id: default_account_id(),
            card_id: id.into(),
            credential_stamp: String::new(),
            fetched_at,
            expires_at: fetched_at,
            name: name.into(),
            plan: None,
            status: "error".into(),
            error: Some(message),
            metrics: vec![],
            stale: false,
            warning: None,
        }
    }

    pub fn with_cache_identity(
        mut self,
        account: &crate::accounts::AccountContext,
        credential_stamp: &str,
        fetched_at: i64,
        expires_at: i64,
    ) -> Self {
        self.id = account.card_id.clone();
        self.provider_id = account.provider_id.clone();
        self.account_id = account.account_id.as_str().to_string();
        self.card_id = account.card_id.clone();
        self.name = account.display_name.clone();
        self.credential_stamp = credential_stamp.to_string();
        self.fetched_at = fetched_at;
        self.expires_at = expires_at;
        self
    }
}

pub type ProviderSnapshot = Snapshot;

fn default_account_id() -> String {
    "default".to_string()
}

/// Optional outbound proxy from config.json `proxy: { enabled, url }`.
/// Loaded once per app run (Mac parity — a change needs a restart) and never
/// applied to loopback, so the local Antigravity/HTTP-API traffic stays direct.
fn proxy_url() -> Option<&'static str> {
    static PROXY: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    PROXY
        .get_or_init(|| {
            let cfg: serde_json::Value = std::fs::read_to_string(config_dir().join("config.json"))
                .ok()
                .and_then(|raw| serde_json::from_str(raw.trim_start_matches('\u{feff}')).ok())?;
            let proxy = cfg.get("proxy")?;
            if !proxy
                .get("enabled")
                .and_then(serde_json::Value::as_bool)
                .unwrap_or(false)
            {
                return None;
            }
            let url = proxy.get("url")?.as_str()?.trim().to_string();
            let valid = ["http://", "https://", "socks5://"]
                .iter()
                .any(|s| url.starts_with(s));
            if url.is_empty() || !valid {
                return None;
            }
            Some(url)
        })
        .as_deref()
}

pub fn http() -> reqwest::Client {
    let mut builder = reqwest::Client::builder()
        .user_agent("OpenMeter-Windows/0.4")
        .timeout(std::time::Duration::from_secs(20));
    if let Some(url) = proxy_url() {
        if let Ok(proxy) = reqwest::Proxy::all(url) {
            let proxy = proxy.no_proxy(reqwest::NoProxy::from_string("localhost,127.0.0.1,::1"));
            builder = builder.proxy(proxy);
        }
    }
    builder.build().expect("failed to build http client")
}

/// Where OpenMeter keeps settings, API keys, and local caches.
///
/// A first launch migrates an existing Pane or legacy OpenUsage directory.
pub fn config_dir() -> PathBuf {
    crate::platform::config_dir()
}

/// Reads a generic credential's blob from Windows Credential Manager.
pub fn read_windows_credential(target: &str) -> Option<Vec<u8>> {
    use windows::core::PCWSTR;
    use windows::Win32::Security::Credentials::{
        CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC,
    };
    let wide: Vec<u16> = target.encode_utf16().chain(std::iter::once(0)).collect();
    let mut pcred: *mut CREDENTIALW = std::ptr::null_mut();
    unsafe {
        if CredReadW(PCWSTR(wide.as_ptr()), CRED_TYPE_GENERIC, None, &mut pcred).is_err() {
            return None;
        }
        let cred = &*pcred;
        let blob =
            std::slice::from_raw_parts(cred.CredentialBlob, cred.CredentialBlobSize as usize)
                .to_vec();
        CredFree(pcred as *mut std::ffi::c_void);
        Some(blob)
    }
}

/// Credential blob → text: UTF-8 or UTF-16 LE, unwrapping go-keyring's
/// `go-keyring-base64:` prefix (used by Go CLIs like gh and Antigravity).
pub fn credential_string(target: &str) -> Option<String> {
    let blob = read_windows_credential(target)?;
    let text = String::from_utf8(blob.clone()).ok().or_else(|| {
        if blob.len() % 2 == 0 {
            let utf16: Vec<u16> = blob
                .chunks_exact(2)
                .map(|c| u16::from_le_bytes([c[0], c[1]]))
                .collect();
            String::from_utf16(&utf16).ok()
        } else {
            None
        }
    })?;
    let text = text.trim().trim_matches('\0').to_string();
    if let Some(b64) = text.strip_prefix("go-keyring-base64:") {
        use base64::Engine;
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(b64.trim())
            .ok()?;
        return String::from_utf8(decoded).ok();
    }
    Some(text)
}

/// Percent-used meter for pay-as-you-go balances. These APIs report only
/// what's left — never "of how much" — so OpenMeter remembers the highest
/// balance it has ever seen per provider (a top-up raises it automatically)
/// and meters usage against that high-water mark. Persisted so restarts
/// keep the story. As a progress row it also feeds the notification rules
/// ("Almost Out" fires under 10% remaining) like every other meter.
pub fn credit_meter(provider: &str, sign: &str, balance: f64) -> Option<Metric> {
    if !balance.is_finite() || balance < 0.0 {
        return None;
    }
    // Providers refresh concurrently and this is a read-modify-write on a
    // shared file — serialize it, or one card's just-raised high-water
    // mark can be overwritten by another's stale copy.
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _guard = LOCK.lock();
    let path = config_dir().join("credit_baselines.json");
    let mut doc: serde_json::Value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .filter(serde_json::Value::is_object)
        .unwrap_or_else(|| serde_json::json!({}));
    let high = doc
        .get(provider)
        .and_then(serde_json::Value::as_f64)
        .unwrap_or(0.0);
    if balance > high {
        doc[provider] = serde_json::Value::from(balance);
        let _ = std::fs::write(
            &path,
            serde_json::to_string_pretty(&doc).unwrap_or_default(),
        );
    }
    let high = high.max(balance);
    if high <= 0.0 {
        return None;
    }
    let used = ((1.0 - balance / high) * 100.0).clamp(0.0, 100.0);
    Some(Metric::progress(
        "Credits used",
        used,
        Some(format!("{sign}{balance:.2} of {sign}{high:.2} left")),
    ))
}

/// API key lookup: our saved config file first, then environment variables.
pub fn stored_api_key(provider: &str, env_vars: &[&str]) -> Option<String> {
    let path = config_dir().join(format!("{provider}.json"));
    if let Ok(raw) = std::fs::read_to_string(&path) {
        if let Ok(doc) = serde_json::from_str::<serde_json::Value>(&raw) {
            if let Some(key) = doc.get("apiKey").and_then(serde_json::Value::as_str) {
                let key = key.trim();
                if !key.is_empty() {
                    return Some(key.to_string());
                }
            }
        }
    }
    for var in env_vars {
        if let Ok(key) = std::env::var(var) {
            let key = key.trim().to_string();
            if !key.is_empty() {
                return Some(key);
            }
        }
    }
    None
}
