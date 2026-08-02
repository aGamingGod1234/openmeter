# OpenMeter Upstream Behavior Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every audited behavior gap between OpenMeter and `robinebers/openusage` at `9d2bf09` while preserving all 18 providers on Windows.

**Architecture:** Extend the existing Rust provider/account/runtime boundaries and the vanilla TypeScript presentation layer. Account identity remains authoritative in Rust; all display surfaces resolve names at the boundary from one registry, and platform-only differences remain explicit settings rather than hidden divergences.

**Tech Stack:** Rust 2021, Tauri 2, Tokio, Serde, Reqwest, tiny_http, Windows APIs, TypeScript 5.6, Vitest 4, GitHub Actions Windows runners.

## Global Constraints

- Target Windows 11 only.
- Baseline OpenUsage commit is exactly `9d2bf09` until the parity release is complete.
- Preserve Claude, Codex, Cursor, Antigravity, Copilot, Devin, Grok, OpenCode, OpenRouter, Z.ai, MiniMax, DeepSeek, Moonshot, ElevenLabs, Ollama, Codebuff, Kilo, and AihubMix.
- Store state under `%APPDATA%\OpenMeter`; never use OneDrive.
- Keep the local API on `127.0.0.1:6736`.
- No credential, token, email, raw provider response, or absolute user path may enter logs or frontend state.
- Write a failing test before each implementation change.
- This PC's Smart App Control can block unsigned Cargo build scripts; when that occurs, push the focused commit and use the Windows GitHub Actions test job as the authoritative Rust result.

---

## File map

- `src-tauri/src/environment.rs`: immutable launch-time environment and home-directory snapshot.
- `src-tauri/src/accounts.rs`: account records, source attachment, registry migration, matching, and live name resolution.
- `src-tauri/src/discovery.rs`: Claude/Codex home discovery and identity validation.
- `src-tauri/src/providers/mod.rs`: account-scoped credential and refresh runtime.
- `src-tauri/src/providers/claude.rs`: Claude identity, auth, and config-directory behavior.
- `src-tauri/src/providers/codex.rs`: Codex identity, auth, and home behavior.
- `src-tauri/src/cache.rs`: cache identity stamp validation.
- `src-tauri/src/spend.rs`: account-scoped local history and Pi attribution.
- `src-tauri/src/contracts.rs`: normalized `/v1/limits` and CLI serialization with live names.
- `src-tauri/src/httpapi.rs`: family/card matching and optional compatibility CORS.
- `src-tauri/src/lib.rs`: configuration migration, updater channel selection, Tauri commands, and composition.
- `src/accounts-ui.ts`: frontend account records and rename behavior.
- `src/models.ts`: frontend settings and snapshot contracts.
- `src/main.ts`: settings UI and boundary rendering.

### Task 1: Stable launch environment snapshot

**Files:**
- Create: `src-tauri/src/environment.rs`
- Create: `src-tauri/tests/environment.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/providers/mod.rs`

**Interfaces:**
- Produces: `EnvironmentSnapshot::capture() -> EnvironmentSnapshot`
- Produces: `EnvironmentSnapshot::from_values(home: PathBuf, vars: BTreeMap<String, OsString>) -> Self`
- Produces: `EnvironmentSnapshot::var(&self, key: &str) -> Option<&OsStr>`
- Produces: `EnvironmentSnapshot::home_dir(&self) -> &Path`
- Consumed by: Task 3 discovery and every credential-path resolver.

- [ ] **Step 1: Write the failing snapshot tests**

```rust
use openmeter_lib::environment::EnvironmentSnapshot;
use std::{collections::BTreeMap, ffi::OsString, path::PathBuf};

#[test]
fn snapshot_is_immutable_after_construction() {
    let mut vars = BTreeMap::new();
    vars.insert("CLAUDE_CONFIG_DIR".into(), OsString::from(r"C:\Claude\work"));
    let snapshot = EnvironmentSnapshot::from_values(PathBuf::from(r"C:\Users\me"), vars);
    std::env::set_var("CLAUDE_CONFIG_DIR", r"C:\Claude\changed");
    assert_eq!(snapshot.var("CLAUDE_CONFIG_DIR").unwrap(), r"C:\Claude\work");
    assert_eq!(snapshot.home_dir(), PathBuf::from(r"C:\Users\me"));
}
```

- [ ] **Step 2: Run the focused test and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test environment`

Expected: compilation fails because `openmeter_lib::environment` does not exist.

- [ ] **Step 3: Implement the immutable snapshot**

```rust
#[derive(Debug, Clone)]
pub struct EnvironmentSnapshot {
    home: PathBuf,
    vars: BTreeMap<String, OsString>,
}

impl EnvironmentSnapshot {
    pub fn capture() -> Self {
        Self {
            home: dirs::home_dir().unwrap_or_default(),
            vars: std::env::vars_os().collect(),
        }
    }

    pub fn from_values(home: PathBuf, vars: BTreeMap<String, OsString>) -> Self {
        Self { home, vars }
    }

    pub fn var(&self, key: &str) -> Option<&OsStr> {
        self.vars.get(key).map(OsString::as_os_str)
    }

    pub fn home_dir(&self) -> &Path { &self.home }
}
```

Capture one `Arc<EnvironmentSnapshot>` during Tauri setup and pass it into discovery/runtime construction. Replace direct `std::env::var` home/config lookups in provider discovery with snapshot reads.

- [ ] **Step 4: Run environment and provider tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test environment --test provider_accounts`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/environment.rs src-tauri/src/lib.rs src-tauri/src/providers/mod.rs src-tauri/tests/environment.rs
git commit -m "feat: snapshot provider discovery environment"
```

### Task 2: Account-first registry and migration

**Files:**
- Modify: `src-tauri/src/accounts.rs`
- Modify: `src-tauri/src/cache.rs`
- Modify: `src-tauri/tests/accounts.rs`
- Modify: `src-tauri/tests/cache.rs`
- Create: `src-tauri/tests/fixtures/accounts-v1.json`

**Interfaces:**
- Produces: `AccountRecord { provider_id, record_id, identity_key, label, sources, enabled }`
- Produces: `AccountSource { id, kind, path, holds_default_source }`
- Produces: `AccountRegistry::attach_source(identity_key, source) -> Result<&AccountRecord, String>`
- Produces: `AccountRegistry::resolve_name(record_id) -> Option<String>`
- Produces: `AccountRegistry::match_token(token) -> Vec<&AccountRecord>`
- Consumed by: Tasks 3–6 and the sync plan.

- [ ] **Step 1: Add failing lifecycle and v1-migration tests**

```rust
#[test]
fn attaching_two_sources_for_one_identity_never_duplicates_a_card() {
    let mut registry = AccountRegistry::default();
    registry.attach_source("org_123", source("default", true)).unwrap();
    registry.attach_source("org_123", source("work-dir", false)).unwrap();
    assert_eq!(registry.accounts().len(), 1);
    assert_eq!(registry.accounts()[0].sources.len(), 2);
}

#[test]
fn migrated_v1_registry_preserves_existing_card_ids_and_labels() {
    let registry = AccountRegistry::load(fixture("accounts-v1.json")).unwrap();
    assert_eq!(registry.resolve_name("claude--work").as_deref(), Some("Claude — Company"));
    assert_eq!(registry.match_token("claude").len(), 2);
}
```

- [ ] **Step 2: Run tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test accounts --test cache`

Expected: compilation fails because records do not support multiple sources or identity keys.

- [ ] **Step 3: Implement registry v2 and atomic migration**

Use these concrete record shapes:

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountSource {
    pub id: String,
    pub kind: SourceKind,
    pub path: Option<PathBuf>,
    pub holds_default_source: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AccountRecord {
    pub provider_id: String,
    pub record_id: String,
    pub identity_key: Option<String>,
    pub label: Option<String>,
    pub sources: Vec<AccountSource>,
    pub enabled: bool,
}

impl AccountRecord {
    pub fn resolved_display_name(&self) -> String {
        self.label.as_deref().filter(|s| !s.trim().is_empty())
            .map(|label| format!("{} — {}", provider_display_name(&self.provider_id), label.trim()))
            .unwrap_or_else(|| provider_display_name(&self.provider_id))
    }
}
```

Parse `version: 1` through a private `RegistryV1`, convert singular sources into one-element vectors, preserve `card_id` as `record_id`, write `accounts-v1.json.bak`, then atomically write v2. New discovered identities use `provider@<first-eight-lowercase-hex-of-sha256>`; the first converted/default account retains the bare provider id.

Extend cache entries with `account_identity_stamp: Option<String>` and reject entries whose stamp differs from the active record identity.

- [ ] **Step 4: Run registry/cache tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test accounts --test cache --test contracts --test httpapi`

Expected: all pass, including preserved v1 IDs.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/accounts.rs src-tauri/src/cache.rs src-tauri/tests/accounts.rs src-tauri/tests/cache.rs src-tauri/tests/fixtures/accounts-v1.json
git commit -m "feat: make account registry identity-first"
```

### Task 3: Claude and Codex source discovery

**Files:**
- Create: `src-tauri/src/discovery.rs`
- Create: `src-tauri/tests/discovery.rs`
- Modify: `src-tauri/src/providers/claude.rs`
- Modify: `src-tauri/src/providers/codex.rs`
- Modify: `src-tauri/src/providers/mod.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `EnvironmentSnapshot` and `AccountRegistry` from Tasks 1–2.
- Produces: `discover_claude_sources(env) -> Vec<DiscoveredSource>`
- Produces: `discover_codex_sources(env) -> Vec<DiscoveredSource>`
- Produces: `DiscoveredSource { provider_id, identity_key, suggested_label, source }`

- [ ] **Step 1: Write failing discovery tests with temporary homes**

```rust
#[test]
fn claude_discovers_default_env_and_dot_directory_homes_once_per_identity() {
    let home = temp_home();
    credential(&home.join(".claude"), "org_same", "Personal");
    credential(&home.join(".claude-work"), "org_same", "Company");
    credential(&home.join(".config/claude-client"), "org_other", "Side");
    let found = discover_claude_sources(&snapshot(home));
    assert_eq!(found.iter().map(|v| &v.identity_key).collect::<HashSet<_>>().len(), 2);
    assert_eq!(found.len(), 3);
}

#[test]
fn codex_ignores_a_home_that_cannot_prove_an_account_identity() {
    let home = temp_home();
    write(home.join(".codex/auth.json"), r#"{"tokens":{"access_token":"secret"}}"#);
    assert!(discover_codex_sources(&snapshot(home)).is_empty());
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test discovery`

Expected: compilation fails because discovery functions do not exist.

- [ ] **Step 3: Implement bounded, identity-validating discovery**

Search only these roots:

```rust
pub enum CandidateRoot {
    Default(&'static str),
    HomePrefix(&'static str),
    ConfigChildren,
    Environment(&'static str),
}

pub const CLAUDE_CANDIDATES: &[CandidateRoot] = &[
    CandidateRoot::Default(".claude"),
    CandidateRoot::HomePrefix(".claude-"),
    CandidateRoot::ConfigChildren,
];

pub const CODEX_CANDIDATES: &[CandidateRoot] = &[
    CandidateRoot::Default(".codex"),
    CandidateRoot::Environment("CODEX_HOME"),
];
```

Canonicalize candidates, cap each directory walk at 128 entries, refuse symlink escapes outside the candidate root, and accept a candidate only after extracting a non-secret identity claim. Claude reads the organization/account claim already used by its API response/auth data. Codex accepts `tokens.account_id` or the ChatGPT account claim from the ID token; never log the token itself.

Attach all sources with the same identity to one record. Newly discovered records seed enabled, copy the provider family's default layout, and seed no stars.

- [ ] **Step 4: Run discovery, provider, and account tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test discovery --test provider_accounts --test accounts`

Expected: all pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/discovery.rs src-tauri/src/lib.rs src-tauri/src/providers src-tauri/tests/discovery.rs
git commit -m "feat: discover account-scoped Claude and Codex homes"
```

### Task 4: One live name resolver and account-scoped Pi history

**Files:**
- Modify: `src-tauri/src/accounts.rs`
- Modify: `src-tauri/src/contracts.rs`
- Modify: `src-tauri/src/httpapi.rs`
- Modify: `src-tauri/src/alerts.rs`
- Modify: `src-tauri/src/spend.rs`
- Modify: `src/accounts-ui.ts`
- Modify: `src/main.ts`
- Modify: `src-tauri/tests/accounts.rs`
- Modify: `src-tauri/tests/contracts.rs`
- Create: `src-tauri/tests/pi_history.rs`
- Modify: `tests/accounts-ui.test.ts`

**Interfaces:**
- Consumes: `AccountRegistry::resolve_name` from Task 2.
- Produces: `NameResolver::resolve(record_id, derived_default) -> String`.
- Produces: `collect_pi_events(root, default_owner) -> Vec<AccountUsageEvent>`.

- [ ] **Step 1: Write failing cross-surface rename and Pi attribution tests**

```rust
#[test]
fn rename_is_resolved_when_contract_is_serialized_not_when_snapshot_is_cached() {
    let (mut registry, snapshot) = seeded_snapshot("claude@abcd1234", "Claude — Old");
    registry.rename("claude@abcd1234", "Company").unwrap();
    let json = limits_envelope(&[snapshot], &registry, 1);
    assert_eq!(json["providers"][0]["name"], "Claude — Company");
}

#[test]
fn pi_event_is_owned_by_the_account_holding_the_default_source_at_event_time() {
    let events = collect_pi_events(fixture("pi-session.jsonl"), owner("claude@abcd1234"));
    assert!(events.iter().all(|event| event.record_id == "claude@abcd1234"));
}
```

- [ ] **Step 2: Run Rust and frontend tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test contracts --test pi_history`

Run: `npm test -- --run tests/accounts-ui.test.ts`

Expected: Rust lacks boundary resolution/Pi scanning; frontend rename currently rewrites the baked display name.

- [ ] **Step 3: Implement boundary-only name resolution and Pi scanning**

Keep `ProviderSnapshot.name` as the derived provider default. Resolve the registry label immediately before rendering or serialization in alerts, tray tooltips, Total Spend, share cards, CLI, and HTTP contracts. Frontend account records store `label` separately and compute display text through:

```ts
export function resolvedAccountName(account: AccountRecord): string {
  const base = providerDisplayName(account.provider_id);
  const label = account.label?.trim();
  return label ? `${base} — ${label}` : base;
}
```

Parse Pi JSONL through the same bounded line reader used by other spend scanners. Emit normalized events into the Claude or Codex record holding the default source; keep file mtime/size/catalog-generation cache semantics and do not double-count files already owned by another scanner.

- [ ] **Step 4: Run affected suites**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test accounts --test contracts --test httpapi --test pi_history`

Run: `npm test -- --run tests/accounts-ui.test.ts tests/layout.test.ts`

Expected: all pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/accounts.rs src-tauri/src/contracts.rs src-tauri/src/httpapi.rs src-tauri/src/alerts.rs src-tauri/src/spend.rs src/accounts-ui.ts src/main.ts src-tauri/tests tests/accounts-ui.test.ts
git commit -m "feat: resolve account names consistently across surfaces"
```

### Task 5: Stable/beta updates and explicit CORS compatibility

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/httpapi.rs`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src/models.ts`
- Modify: `src/main.ts`
- Modify: `src-tauri/tests/httpapi.rs`
- Create: `src-tauri/tests/update_channel.rs`
- Create: `tests/settings-compatibility.test.ts`
- Modify: `docs/local-http-api.md`

**Interfaces:**
- Produces: `UpdateChannel::{Stable, Beta}` and `update_endpoint(channel) -> Url`.
- Produces: `ApiPolicy { allow_browser_cors: bool }`.
- Adds config keys: `updateChannel: "stable" | "beta"`, `allowBrowserCors: boolean`.

- [ ] **Step 1: Write failing channel and CORS tests**

```rust
#[test]
fn beta_adds_prereleases_while_stable_uses_latest_manifest() {
    assert!(update_endpoint(UpdateChannel::Stable).as_str().ends_with("/latest/download/latest.json"));
    assert!(update_endpoint(UpdateChannel::Beta).as_str().ends_with("/releases/download/beta/latest.json"));
}

#[test]
fn cors_is_absent_by_default_and_permissive_only_when_enabled() {
    assert!(!response_headers(ApiPolicy::default()).contains_key("Access-Control-Allow-Origin"));
    assert_eq!(response_headers(ApiPolicy { allow_browser_cors: true })["Access-Control-Allow-Origin"], "*");
}
```

- [ ] **Step 2: Run focused tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test update_channel --test httpapi`

Run: `npm test -- --run tests/settings-compatibility.test.ts`

Expected: update channel and compatibility setting do not exist.

- [ ] **Step 3: Implement config migration, updater selection, and settings UI**

Default both keys securely:

```rust
obj.entry("updateChannel").or_insert(json!("stable"));
obj.entry("allowBrowserCors").or_insert(json!(false));
```

Build the updater with the selected endpoint. The beta manifest must include stable versions when their semantic version is newer. Add Settings → Updates radio controls and Settings → Advanced an explicitly warned “Allow browser pages to read local API” toggle. Rebuild API policy after config changes without rebinding a public interface.

- [ ] **Step 4: Run all contract/settings tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test update_channel --test httpapi --test cli --test contracts`

Run: `npm test -- --run`

Expected: all pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/lib.rs src-tauri/src/httpapi.rs src-tauri/tauri.conf.json src/models.ts src/main.ts src-tauri/tests tests/settings-compatibility.test.ts docs/local-http-api.md
git commit -m "feat: add beta updates and explicit API compatibility mode"
```

### Task 6: Full parity regression and documentation

**Files:**
- Create: `docs/parity-matrix.md`
- Modify: `docs/upstream-parity.md`
- Modify: `README.md`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes all prior tasks.
- Produces a CI-enforced parity matrix referencing tests for every upstream capability.

- [ ] **Step 1: Add a failing parity-matrix validation script to CI**

Create `scripts/test-parity-matrix.ps1` in this task with required capability IDs:

```powershell
$required = @('accounts','discovery','cache-stamp','cli-api','dashboard','tray','pace','spend','pi','proxy','privacy','updates','cors','sync')
$text = Get-Content "$PSScriptRoot\..\docs\parity-matrix.md" -Raw
$missing = $required | Where-Object { $text -notmatch "\| $_ \|" }
if ($missing) { throw "Missing parity rows: $($missing -join ', ')" }
```

- [ ] **Step 2: Run and verify RED**

Run: `powershell -NoProfile -File scripts/test-parity-matrix.ps1`

Expected: failure because `docs/parity-matrix.md` does not exist.

- [ ] **Step 3: Write the matrix and wire every verification command into CI**

Each matrix row contains capability ID, upstream source doc/test, Windows implementation files, Windows test, platform-equivalence note, and status. Mark a row complete only when its named test exists and passes. Add `rustfmt` coverage for all newly maintained Rust files.

- [ ] **Step 4: Run the complete local-capable suite and Windows CI**

Run: `npm test -- --run`

Run: `npm run build`

Run: `powershell -NoProfile -File scripts/test-parity-matrix.ps1`

Run in GitHub Actions: `cargo fmt --manifest-path src-tauri/Cargo.toml --all -- --check`, `cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings`, and `cargo test --manifest-path src-tauri/Cargo.toml --all-targets`.

Expected: every command and both CI jobs pass.

- [ ] **Step 5: Commit**

```powershell
git add .github/workflows/ci.yml scripts/test-parity-matrix.ps1 docs/parity-matrix.md docs/upstream-parity.md README.md
git commit -m "docs: verify OpenUsage parity baseline"
```
