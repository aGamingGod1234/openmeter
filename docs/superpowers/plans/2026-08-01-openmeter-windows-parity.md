# OpenMeter Windows Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship a Windows 11 x64 OpenMeter tray app, CLI, installer, and updater with current OpenUsage provider, multi-account, cache, and API behavior on top of Pane 0.4.28.

**Architecture:** Split Pane's Tauri-coupled Rust module into a reusable core that owns accounts, refresh, caching, serialization, and providers. The tray app, CLI, and loopback API consume that core; the TypeScript frontend renders account-stamped snapshots without reading credentials.

**Tech Stack:** Rust 2021, Tauri 2, Tokio, Serde, Reqwest, Tiny HTTP, Rusqlite, Windows APIs, TypeScript 5.6, Vite 6, Vitest, GitHub Actions, NSIS.

## Global Constraints

- Target Windows 11 x64 on the user's current machine only.
- Store settings and history locally under `%APPDATA%\OpenMeter`; no OneDrive, iCloud, or replacement cloud sync.
- Baselines are Pane commit `272a6ae` and OpenUsage commit `9d2bf09`.
- Preserve MIT attribution to Pane and OpenUsage while using independent OpenMeter operational identity.
- Never serialize or log credentials, cookies, authorization headers, secret query parameters, or raw user paths.
- Use test-first red/green cycles for every behavior change.
- Run Rust and installer checks on GitHub Actions because local Smart App Control blocks Cargo build scripts with `os error 4551`.

## File Structure

- `src-tauri/src/accounts.rs` — stable provider-account identities and registry persistence.
- `src-tauri/src/cache.rs` — account-stamped snapshot cache and freshness decisions.
- `src-tauri/src/contracts.rs` — legacy usage and stable limits wire serializers.
- `src-tauri/src/refresh.rs` — provider catalog, concurrent account refresh, cooldowns, and last-good behavior.
- `src-tauri/src/redaction.rs` — secret/path redaction for diagnostics.
- `src-tauri/src/platform.rs` — Windows config paths, protected storage, and capture exclusion.
- `src-tauri/src/httpapi.rs` — loopback routing over shared serialized state.
- `src-tauri/src/bin/openmeter.rs` — one-shot CLI argument parsing and output.
- `src-tauri/src/providers/*.rs` — provider credential and usage clients adapted to `AccountContext`.
- `src/models.ts` — frontend snapshot/account/config types.
- `src/layout.ts` — pure account-aware layout migration and mutation helpers.
- `src/main.ts` — rendering and Tauri event wiring only.
- `tests/*.test.ts` — Vitest frontend behavior tests.
- `.github/workflows/ci.yml` — pull-request and branch quality gate.
- `.github/workflows/release.yml` — tested OpenMeter installer/updater publication.

---

### Task 1: Independent OpenMeter Identity

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `src-tauri/src/main.rs`
- Modify: `src-tauri/src/providers/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `index.html`
- Modify: `src/main.ts`
- Modify: `README.md`
- Modify: `docs/privacy.md`
- Create: `src-tauri/tests/identity.rs`

**Interfaces:**
- Produces: crate `openmeter_lib`, executable/product `OpenMeter`, identifier `com.agaminggod.openmeter`, config root `%APPDATA%\OpenMeter`.

- [ ] **Step 1: Write the failing identity test**

```rust
#[test]
fn config_directory_uses_openmeter_identity() {
    let path = openmeter_lib::platform::config_dir_from(std::path::Path::new(r"C:\Users\test\AppData\Roaming"));
    assert_eq!(path, std::path::PathBuf::from(r"C:\Users\test\AppData\Roaming\OpenMeter"));
}
```

- [ ] **Step 2: Run `cargo test --manifest-path src-tauri/Cargo.toml --test identity` in GitHub CI and confirm RED because `platform::config_dir_from` does not exist.**
- [ ] **Step 3: Add `platform::config_dir_from`, rename package/crate/product/copy, remove Pane update URLs and public key, and change every operational `[pane]` prefix to `[openmeter]`. Keep credits rather than mechanically replacing attribution.**
- [ ] **Step 4: Run `npm run build` locally and the identity test in CI; expect both PASS.**
- [ ] **Step 5: Commit with `git commit -m "feat: establish independent OpenMeter identity"`.**

### Task 2: Frontend Test Harness and Pure Layout Module

**Files:**
- Modify: `package.json`
- Modify: `package-lock.json`
- Create: `vitest.config.ts`
- Create: `src/models.ts`
- Create: `src/layout.ts`
- Create: `tests/layout.test.ts`
- Modify: `src/main.ts`

**Interfaces:**
- Produces: `Snapshot`, `Metric`, `ProviderLayout`, `Layout`, `snapshotCardId(snapshot)`, and `migrateLayout(layout, snapshots)`.

- [ ] **Step 1: Add Vitest and write a failing test proving two Claude account cards receive distinct layout entries while the default card keeps ID `claude`.**

```ts
expect(migrateLayout(null, [defaultClaude, workClaude]).providerOrder)
  .toEqual(["claude", "claude--work"]);
```

- [ ] **Step 2: Run `npm test -- --run tests/layout.test.ts`; expect FAIL because `migrateLayout` is missing.**
- [ ] **Step 3: Extract frontend types and layout migration from `main.ts`; make account card ID the only UI/layout key.**
- [ ] **Step 4: Run the focused test and `npm run build`; expect PASS.**
- [ ] **Step 5: Commit with `git commit -m "test: add account-aware frontend layout boundary"`.**

### Task 3: Stable Account Registry

**Files:**
- Create: `src-tauri/src/accounts.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/providers/mod.rs`
- Create: `src-tauri/tests/accounts.rs`

**Interfaces:**
- Produces: `AccountId(String)`, `AccountContext { provider_id, account_id, card_id, display_name, source }`, `AccountRegistry::load(path)`, `AccountRegistry::save(path)`, and `AccountRegistry::match_token(token)`.

- [ ] **Step 1: Write failing tests for default-card compatibility, deterministic qualified IDs, duplicate rejection, and family/exact matching.**

```rust
assert_eq!(AccountContext::default_for("claude").card_id, "claude");
assert_eq!(AccountContext::named("claude", "work", "Work").unwrap().card_id, "claude--work");
assert_eq!(registry.match_token("claude").len(), 2);
assert_eq!(registry.match_token("claude--work").len(), 1);
```

- [ ] **Step 2: Run `cargo test --test accounts` in CI; expect RED because the account types do not exist.**
- [ ] **Step 3: Implement the registry with schema version `1`, atomic temp-file replacement, and no credential fields.**
- [ ] **Step 4: Run account tests in CI; expect PASS.**
- [ ] **Step 5: Commit with `git commit -m "feat: add stable provider account registry"`.**

### Task 4: Account-Stamped Snapshot and Cache

**Files:**
- Create: `src-tauri/src/cache.rs`
- Modify: `src-tauri/src/providers/mod.rs`
- Create: `src-tauri/tests/cache.rs`

**Interfaces:**
- Produces: `ProviderSnapshot { provider_id, account_id, card_id, credential_stamp, fetched_at, expires_at, ... }`, `SnapshotCache::read`, `write`, `fresh`, and `last_good`.

- [ ] **Step 1: Write failing tests showing a cached personal Claude snapshot cannot be returned for Work, a changed credential stamp invalidates freshness, and expired data remains available only through `last_good`.**
- [ ] **Step 2: Run `cargo test --test cache` in CI; expect RED because `SnapshotCache` is missing.**
- [ ] **Step 3: Implement versioned JSON persistence with atomic writes and cache keys `(card_id, credential_stamp, schema_version)`.**
- [ ] **Step 4: Run cache tests in CI; expect PASS.**
- [ ] **Step 5: Commit with `git commit -m "feat: isolate cached snapshots by provider account"`.**

### Task 5: Shared Limits and Usage Contracts

**Files:**
- Create: `src-tauri/src/contracts.rs`
- Modify: `src-tauri/src/providers/mod.rs`
- Modify: `src-tauri/src/httpapi.rs`
- Create: `src-tauri/tests/contracts.rs`
- Create: `src-tauri/tests/fixtures/limits-codex.json`
- Modify: `docs/local-http-api.md`

**Interfaces:**
- Produces: `LimitResourceDescriptor`, `serialize_usage(&[ProviderSnapshot])`, and `serialize_limits(&[ProviderSnapshot], generated_at)` returning `serde_json::Value`.

- [ ] **Step 1: Write a failing fixture test requiring schema `openusage.limits.v1`, provider/account keys, scalar resource units, reset timestamps, stale state, expiry, and per-card errors.**

```rust
assert_eq!(wire["schema"], "openusage.limits.v1");
assert_eq!(wire["providers"]["codex"]["resources"]["weekly"]["unit"], "percent");
assert_eq!(wire["providers"]["codex"]["resources"]["weekly"]["used"], 42.0);
```

- [ ] **Step 2: Run `cargo test --test contracts` in CI; expect RED because the serializer is missing.**
- [ ] **Step 3: Implement descriptor-driven serialization; UI-only text metrics without descriptors must never appear in limits resources.**
- [ ] **Step 4: Run contract tests in CI; expect PASS and byte-stable fixture output.**
- [ ] **Step 5: Commit with `git commit -m "feat: add OpenUsage-compatible limits contract"`.**

### Task 6: Loopback API Routing and Concurrency

**Files:**
- Modify: `src-tauri/src/httpapi.rs`
- Create: `src-tauri/tests/httpapi.rs`

**Interfaces:**
- Consumes: account matching and shared contract serializers.
- Produces: `GET /v1/limits`, `/v1/limits/:id`, `/v1/usage`, `/v1/usage/:id`, `OPTIONS`, 404/405/503 behavior.

- [ ] **Step 1: Write table-driven failing route tests for collection, exact card, family, unknown ID, OPTIONS, wrong method, and the invariant that ID routes always return collection-shaped payloads.**
- [ ] **Step 2: Run `cargo test --test httpapi` in CI; expect RED on `/v1/limits` and current single-object usage routing.**
- [ ] **Step 3: Move routing behind a testable `ApiState`; add a 16-request concurrency semaphore and no CORS response header.**
- [ ] **Step 4: Run HTTP and contract tests in CI; expect PASS.**
- [ ] **Step 5: Commit with `git commit -m "feat: complete local usage and limits API"`.**

### Task 7: Reusable Refresh Coordinator and CLI

**Files:**
- Create: `src-tauri/src/refresh.rs`
- Create: `src-tauri/src/bin/openmeter.rs`
- Modify: `src-tauri/Cargo.toml`
- Modify: `src-tauri/src/lib.rs`
- Create: `src-tauri/tests/cli.rs`
- Create: `src-tauri/tests/refresh.rs`

**Interfaces:**
- Produces: `RefreshCoordinator::cached`, `refresh(force, filter)`, CLI `openmeter [--force] [provider-or-family]`.

- [ ] **Step 1: Write failing refresh tests proving concurrent accounts are retained, one failure uses only that account's last-good data, and force bypasses freshness but not a server-provided cooldown.**
- [ ] **Step 2: Write failing CLI argument tests for no args, `--force`, family filter, exact account filter, invalid duplicate filters, and `--help`.**
- [ ] **Step 3: Run `cargo test --test refresh --test cli` in CI; expect RED.**
- [ ] **Step 4: Extract Pane's `fetch_usage` and `guarded` logic into the coordinator; make Tauri invocation a thin adapter and emit limits JSON from the CLI.**
- [ ] **Step 5: Run refresh, CLI, API, and cache tests in CI; expect PASS.**
- [ ] **Step 6: Commit with `git commit -m "feat: share refresh and cache across app CLI and API"`.**

### Task 8: Account-Aware Providers and Credential Protection

**Files:**
- Modify: `src-tauri/src/providers/mod.rs`
- Modify: every file under `src-tauri/src/providers/`
- Create: `src-tauri/src/redaction.rs`
- Create: `src-tauri/tests/provider_accounts.rs`
- Create: `src-tauri/tests/redaction.rs`
- Modify: `docs/providers.md`

**Interfaces:**
- Consumes: `AccountContext`.
- Produces: `ProviderRuntime::probe(account)` and `ProviderRuntime::refresh(account)` for every provider.

- [ ] **Step 1: Add failing shared contract tests requiring probe and refresh to use the same credential source and requiring all errors to pass redaction.**
- [ ] **Step 2: Add provider fixtures for the ten OpenUsage parity providers and tests for Windows credential/file locations, mapping, reset windows, and no-credential states.**
- [ ] **Step 3: Run provider/redaction tests in CI; expect RED on account parameters and redaction.**
- [ ] **Step 4: Adapt providers in canonical order—Claude, Codex, Cursor, Antigravity, Copilot, Devin, Grok, OpenCode, OpenRouter, Z.ai—then adapt Pane-only providers without changing their observable single-account behavior.**
- [ ] **Step 5: Run all Rust tests in CI; expect PASS with no secret-like fixture values in snapshots.**
- [ ] **Step 6: Commit with `git commit -m "feat: make provider runtimes account aware"`.**

### Task 9: Account-Aware Dashboard, Customize, and Settings

**Files:**
- Modify: `index.html`
- Modify: `src/main.ts`
- Modify: `src/styles.css`
- Modify: `src/models.ts`
- Modify: `src/layout.ts`
- Create: `tests/accounts-ui.test.ts`
- Create: `tests/customize.test.ts`
- Create: `tests/settings.test.ts`

**Interfaces:**
- Consumes: account-stamped snapshots and registry commands.
- Produces: named sibling cards, account CRUD, account-safe layout, stars, tray pins, and provider-family grouping.

- [ ] **Step 1: Write failing DOM tests for two Claude cards, account-specific Outdated state, add/rename/remove account, layout retention, two-star-per-card enforcement, and Ctrl+Z.**
- [ ] **Step 2: Run `npm test -- --run`; expect RED.**
- [ ] **Step 3: Implement account management in Settings and render `displayName` while keeping `cardId` as the DOM/layout key.**
- [ ] **Step 4: Run Vitest and `npm run build`; expect PASS with no warnings.**
- [ ] **Step 5: Commit with `git commit -m "feat: add multi-account dashboard and settings"`.**

### Task 10: Windows Privacy Mode and Diagnostics

**Files:**
- Create: `src-tauri/src/platform.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `index.html`
- Modify: `src/main.ts`
- Create: `src-tauri/tests/platform.rs`
- Create: `tests/privacy.test.ts`
- Modify: `docs/privacy.md`

**Interfaces:**
- Produces: `set_capture_exclusion(hwnd, enabled)`, `set_privacy_mode(enabled)`, privacy shortcut, and redacted diagnostics export.

- [ ] **Step 1: Write failing tests around the platform adapter proving capture exclusion requests `WDA_EXCLUDEFROMCAPTURE` and privacy mode removes tray values without disabling providers.**
- [ ] **Step 2: Run Rust platform tests in CI and frontend privacy tests locally; expect RED.**
- [ ] **Step 3: Implement the Windows call, startup application, tray restoration, shortcut, and diagnostics UI.**
- [ ] **Step 4: Run focused tests and full frontend build; expect PASS.**
- [ ] **Step 5: Commit with `git commit -m "feat: add Windows capture exclusion and privacy mode"`.**

### Task 11: CI, Public Fork, Updater, and Installer

**Files:**
- Create: `.github/workflows/ci.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `install.ps1`
- Modify: `src-tauri/tauri.conf.json`
- Modify: `README.md`
- Create: `docs/upstream-parity.md`
- Create: `scripts/smoke-install.ps1`

**Interfaces:**
- Produces: public `aGamingGod1234/openmeter`, branch CI, x64 NSIS artifact, SHA-256, updater signature, provenance, and silent install/upgrade/uninstall smoke checks.

- [ ] **Step 1: Add CI jobs that run `npm ci`, `npm test -- --run`, `npm run build`, `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, and `cargo test --all-targets`.**
- [ ] **Step 2: Create the authorized public repository, set `origin` to it and `pane-upstream` to `ItsJazii/pane`, then push only after local diff review.**
- [ ] **Step 3: Generate a new Tauri updater key pair; store only the private key/password as repository secrets and commit only the public key.**
- [ ] **Step 4: Run workflow dispatch; require green CI and download the unsigned dry-run installer artifact.**
- [ ] **Step 5: Run `scripts/smoke-install.ps1` on this machine to install per-user, verify process/tray/API/CLI/config path, upgrade over the same install, and uninstall while preserving user data.**
- [ ] **Step 6: Tag the approved version, verify release assets and updater manifest hashes, install the release, and run live provider smoke checks.**
- [ ] **Step 7: Commit with `git commit -m "ci: publish verified OpenMeter Windows releases"`.**

## Plan Self-Review

- Every design requirement maps to Tasks 1–11.
- Account identity is defined once in Task 3 and consumed unchanged by cache, contracts, refresh, providers, and UI.
- Machine-readable output is serialized once in Task 5 and reused by HTTP and CLI.
- Windows-only functionality is isolated from provider logic.
- Each behavior task starts with a failing test and records the expected failure reason.
- No implementation task depends on OneDrive, iCloud, Authenticode, or an unavailable local Rust compiler.
