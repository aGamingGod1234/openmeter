use std::collections::HashMap;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex, OnceLock};

use crate::accounts::AccountContext;
use crate::cache::SnapshotCache;
use crate::providers::{self, ProviderSnapshot};

const FRESH_MS: i64 = 5 * 60 * 1_000;
const ERROR_COOLDOWN_MS: i64 = 60 * 1_000;
const RATE_LIMIT_COOLDOWN_MS: i64 = 5 * 60 * 1_000;
const MAX_RETRY_AFTER_MS: i64 = 60 * 60 * 1_000;

pub type FetchFuture = Pin<Box<dyn Future<Output = ProviderSnapshot> + Send + 'static>>;
type Fetcher = Arc<dyn Fn() -> FetchFuture + Send + Sync>;
type Clock = Arc<dyn Fn() -> i64 + Send + Sync>;

#[derive(Clone)]
pub struct RefreshTarget {
    pub account: AccountContext,
    pub credential_stamp: String,
    fetcher: Fetcher,
}

impl RefreshTarget {
    pub fn new<F>(account: AccountContext, credential_stamp: impl Into<String>, fetcher: F) -> Self
    where
        F: Fn() -> FetchFuture + Send + Sync + 'static,
    {
        Self {
            account,
            credential_stamp: credential_stamp.into(),
            fetcher: Arc::new(fetcher),
        }
    }
}

#[derive(Clone)]
struct Cooldown {
    until_ms: i64,
    note: String,
}

pub struct RefreshCoordinator {
    cache: Mutex<SnapshotCache>,
    cooldowns: Mutex<HashMap<String, Cooldown>>,
    cache_path: Option<PathBuf>,
    clock: Clock,
}

impl RefreshCoordinator {
    pub fn in_memory(cache: SnapshotCache, clock: Clock) -> Self {
        Self {
            cache: Mutex::new(cache),
            cooldowns: Mutex::new(HashMap::new()),
            cache_path: None,
            clock,
        }
    }

    pub fn persistent(cache_path: PathBuf) -> Self {
        let cache = SnapshotCache::read(&cache_path).unwrap_or_default();
        Self {
            cache: Mutex::new(cache),
            cooldowns: Mutex::new(HashMap::new()),
            cache_path: Some(cache_path),
            clock: Arc::new(|| chrono::Utc::now().timestamp_millis()),
        }
    }

    pub fn cached(&self, filter: Option<&str>, targets: &[RefreshTarget]) -> Vec<ProviderSnapshot> {
        let Ok(cache) = self.cache.lock() else {
            return Vec::new();
        };
        targets
            .iter()
            .filter(|target| matches_filter(&target.account, filter))
            .filter_map(|target| {
                cache
                    .last_good(&target.account.card_id, &target.credential_stamp)
                    .cloned()
            })
            .collect()
    }

    pub async fn refresh(
        &self,
        force: bool,
        filter: Option<&str>,
        targets: &[RefreshTarget],
    ) -> Vec<ProviderSnapshot> {
        let now = (self.clock)();
        let selected: Vec<RefreshTarget> = targets
            .iter()
            .filter(|target| matches_filter(&target.account, filter))
            .cloned()
            .collect();
        let mut results: Vec<Option<ProviderSnapshot>> = vec![None; selected.len()];
        let mut pending = Vec::new();

        for (index, target) in selected.iter().enumerate() {
            if let Some(cooldown) = self.active_cooldown(target, now) {
                results[index] = Some(self.fallback_or_error(target, &cooldown.note, now));
                continue;
            }
            if !force {
                let fresh = self.cache.lock().ok().and_then(|cache| {
                    cache
                        .fresh(&target.account.card_id, &target.credential_stamp, now)
                        .cloned()
                });
                if let Some(snapshot) = fresh {
                    results[index] = Some(snapshot);
                    continue;
                }
            }
            let fetcher = Arc::clone(&target.fetcher);
            pending.push((
                index,
                target.clone(),
                tauri::async_runtime::spawn(async move { fetcher().await }),
            ));
        }

        let mut cache_changed = false;
        for (index, target, handle) in pending {
            let raw = match handle.await {
                Ok(snapshot) => snapshot,
                Err(error) => ProviderSnapshot::error(
                    &target.account.card_id,
                    &target.account.display_name,
                    format!("provider task failed: {error}"),
                ),
            };
            let expires_at = if raw.expires_at > raw.fetched_at && raw.fetched_at > 0 {
                now + (raw.expires_at - raw.fetched_at).min(FRESH_MS)
            } else {
                now + FRESH_MS
            };
            let mut snapshot =
                raw.with_cache_identity(&target.account, &target.credential_stamp, now, expires_at);
            crate::redaction::redact_snapshot(&mut snapshot);
            if snapshot.status == "ok" {
                if let Ok(mut cache) = self.cache.lock() {
                    cache.insert(snapshot.clone());
                    cache_changed = true;
                }
                self.clear_cooldown(&target);
                results[index] = Some(snapshot);
            } else if snapshot.status == "error" {
                let note = snapshot
                    .error
                    .clone()
                    .unwrap_or_else(|| "refresh failed".to_string());
                self.set_cooldown(&target, &note, now);
                results[index] = Some(self.fallback_or_error(&target, &note, now));
            } else {
                self.clear_cooldown(&target);
                results[index] = Some(snapshot);
            }
        }

        if cache_changed {
            if let (Some(path), Ok(cache)) = (&self.cache_path, self.cache.lock()) {
                let _ = cache.write(path);
            }
        }
        results.into_iter().flatten().collect()
    }

    fn active_cooldown(&self, target: &RefreshTarget, now: i64) -> Option<Cooldown> {
        self.cooldowns
            .lock()
            .ok()
            .and_then(|cooldowns| cooldowns.get(&target_key(target)).cloned())
            .filter(|cooldown| now < cooldown.until_ms)
    }

    fn set_cooldown(&self, target: &RefreshTarget, error: &str, now: i64) {
        let retry_after_ms = parse_retry_after(error);
        let duration = retry_after_ms.unwrap_or_else(|| {
            if error.contains("429") {
                RATE_LIMIT_COOLDOWN_MS
            } else {
                ERROR_COOLDOWN_MS
            }
        });
        let note = if retry_after_ms.is_some() {
            format!(
                "rate limited - the vendor asked to wait ~{}m",
                (duration / 60_000).max(1)
            )
        } else if error.contains("429") {
            format!("rate limited - cooling down for a few minutes ({error})")
        } else {
            error.to_string()
        };
        if let Ok(mut cooldowns) = self.cooldowns.lock() {
            cooldowns.insert(
                target_key(target),
                Cooldown {
                    until_ms: now + duration,
                    note,
                },
            );
        }
    }

    fn clear_cooldown(&self, target: &RefreshTarget) {
        if let Ok(mut cooldowns) = self.cooldowns.lock() {
            cooldowns.remove(&target_key(target));
        }
    }

    fn fallback_or_error(
        &self,
        target: &RefreshTarget,
        warning: &str,
        now: i64,
    ) -> ProviderSnapshot {
        let previous = self.cache.lock().ok().and_then(|cache| {
            cache
                .last_good(&target.account.card_id, &target.credential_stamp)
                .cloned()
        });
        if let Some(mut snapshot) = previous {
            snapshot.stale = true;
            snapshot.warning = Some(warning.to_string());
            return snapshot;
        }
        ProviderSnapshot::error(
            &target.account.card_id,
            &target.account.display_name,
            warning.to_string(),
        )
        .with_cache_identity(&target.account, &target.credential_stamp, now, now)
    }
}

fn target_key(target: &RefreshTarget) -> String {
    format!("{}\0{}", target.account.card_id, target.credential_stamp)
}

fn matches_filter(account: &AccountContext, filter: Option<&str>) -> bool {
    filter.is_none_or(|token| token == account.card_id || token == account.provider_id)
}

fn parse_retry_after(error: &str) -> Option<i64> {
    error
        .split("retry_after_s=")
        .nth(1)
        .and_then(|rest| {
            rest.chars()
                .take_while(|character| character.is_ascii_digit())
                .collect::<String>()
                .parse::<i64>()
                .ok()
        })
        .map(|seconds| (seconds * 1_000).min(MAX_RETRY_AFTER_MS))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CliOptions {
    pub force: bool,
    pub filter: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CliAction {
    Run(CliOptions),
    Help,
}

impl CliOptions {
    pub fn parse<I, S>(args: I) -> Result<CliAction, String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut options = Self::default();
        for argument in args {
            let argument = argument.as_ref();
            match argument {
                "-h" | "--help" => return Ok(CliAction::Help),
                "--force" if !options.force => options.force = true,
                "--force" => return Err("--force may be supplied only once".to_string()),
                value if value.starts_with('-') => return Err(format!("unknown option: {value}")),
                value if options.filter.is_none() => options.filter = Some(value.to_string()),
                _ => return Err("supply at most one provider or account filter".to_string()),
            }
        }
        Ok(CliAction::Run(options))
    }
}

pub fn default_targets(disabled: &[String]) -> Vec<RefreshTarget> {
    macro_rules! target {
        ($id:literal, $snapshot:path) => {{
            let account = AccountContext::default_for($id).expect("static provider id");
            let stamp = providers::account_credential_stamp(&account);
            RefreshTarget::new(account, stamp, || Box::pin($snapshot()))
        }};
    }
    let targets = vec![
        target!("claude", providers::claude::snapshot),
        target!("codex", providers::codex::snapshot),
        target!("cursor", providers::cursor::snapshot),
        target!("opencode", providers::opencode::snapshot),
        target!("copilot", providers::copilot::snapshot),
        target!("grok", providers::grok::snapshot),
        target!("devin", providers::devin::snapshot),
        target!("minimax", providers::minimax::snapshot),
        target!("openrouter", providers::openrouter::snapshot),
        target!("zai", providers::zai::snapshot),
        target!("antigravity", providers::antigravity::snapshot),
        target!("deepseek", providers::deepseek::snapshot),
        target!("moonshot", providers::moonshot::snapshot),
        target!("elevenlabs", providers::elevenlabs::snapshot),
        target!("ollama", providers::ollama::snapshot),
        target!("codebuff", providers::codebuff::snapshot),
        target!("kilo", providers::kilo::snapshot),
        target!("aihubmix", providers::aihubmix::snapshot),
    ];
    targets
        .into_iter()
        .filter(|target| !disabled.iter().any(|id| id == &target.account.provider_id))
        .collect()
}

pub async fn refresh_default(
    force: bool,
    filter: Option<&str>,
    disabled: &[String],
) -> Vec<ProviderSnapshot> {
    static COORDINATOR: OnceLock<RefreshCoordinator> = OnceLock::new();
    let coordinator = COORDINATOR.get_or_init(|| {
        RefreshCoordinator::persistent(providers::config_dir().join("snapshots-v1.json"))
    });
    let targets = default_targets(disabled);
    coordinator.refresh(force, filter, &targets).await
}

pub const CLI_HELP: &str = "OpenMeter usage limits\n\nUSAGE:\n    openmeter [--force] [provider-or-account]\n\nOPTIONS:\n    --force    Ignore fresh cache entries (vendor cooldowns still apply)\n    -h, --help Show this help\n";
