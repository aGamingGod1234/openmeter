# OpenMeter Cross-Device Tracking Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Synchronize deduplicated usage events and normalized quota snapshots between the laptop and desktop through the existing encrypted Mini PC hub, then render combined and per-device tracking in OpenMeter.

**Architecture:** Keep the version-one daily-history path operational while adding a separate version-two event envelope, additive hub table, and client peer cache. Rust performs privacy filtering, keyed event identity, deduplication, quota freshness selection, and daily projection; TypeScript renders the resulting device/provider series without receiving secrets or raw logs.

**Tech Stack:** Rust 2021, Tauri 2, TypeScript 5.6, Vitest 4, Axum 0.8, SQLite/rusqlite, XChaCha20-Poly1305, HKDF/HMAC-SHA256, zstd, Windows Credential Manager, PowerShell deployment, Tailscale.

## Global Constraints

- Target only Windows 11 clients; the Mini PC remains a headless hub.
- Keep application state under `%APPDATA%\OpenMeter` and installation state under `%LOCALAPPDATA%\OpenMeter`; never use OneDrive.
- Keep the hub bound to `100.90.87.7:6740` with the existing Tailscale-only firewall scope.
- Never synchronize prompts, responses, credentials, emails, account labels, raw logs, errors, hostnames, or filesystem paths.
- Never place the recovery key in source, commands, logs, tool output, chat, or deployment artifacts.
- Preserve version-one sync until laptop and desktop both pass version-two live acceptance.
- Do not disable Windows Application Control or other Windows security controls.
- Do not merge to `main`, publish a release, or delete history as part of this plan.
- Every schema and migration change must be additive, test-first, and reversible.

---

### Task 1: Version-Two Protocol and Keyed Event Identity

**Files:**
- Create: `crates/openmeter-sync-protocol/src/tracking.rs`
- Create: `crates/openmeter-sync-protocol/src/tracking_crypto.rs`
- Create: `crates/openmeter-sync-protocol/tests/tracking_envelope.rs`
- Modify: `crates/openmeter-sync-protocol/src/lib.rs`
- Modify: `crates/openmeter-sync-protocol/src/model.rs`
- Modify: `crates/openmeter-sync-protocol/Cargo.toml`

**Interfaces:**
- Consumes: existing 32-byte recovery material and `EncryptedEnvelope` wire representation.
- Produces: `TrackingPayloadV2`, `UsageEventV2`, `QuotaSnapshotV2`, `DeviceDescriptorV2`, `EnvelopeMeta::tracking_v2`, `derive_tracking_key`, `tracking_event_id`, `seal_tracking`, and `open_tracking`.

- [ ] **Step 1: Write failing validation and encryption tests**

Add tests that construct this minimal payload and assert round-trip equality, authenticated revision failure, unknown-field rejection, size rejection, and deterministic event IDs:

```rust
let payload = TrackingPayloadV2 {
    device: DeviceDescriptorV2::new("device-a", "Laptop", 1_800_000_000_000, "0.6.0")?,
    events: vec![UsageEventV2::new(
        "evt_0123456789abcdef0123456789abcdef",
        "codex",
        "account.opaque",
        1_800_000_000_000,
        "gpt-5",
        TokenUsageV2 { input: 10.0, output: 5.0, cached: 2.0, reasoning: 1.0, total: 18.0 },
        0.25,
        "catalog-2026-08-04",
        "codex-log",
    )?],
    quotas: vec![],
    tombstones: vec![],
    retained_from_day: "2026-05-07".into(),
};
```

- [ ] **Step 2: Run the protocol test and verify the red state**

Run: `cargo test --manifest-path crates/openmeter-sync-protocol/Cargo.toml --test tracking_envelope`

Expected: compilation fails because the version-two types and functions do not exist.

- [ ] **Step 3: Add strict version-two models**

Implement `#[serde(deny_unknown_fields)]` models with bounded counts and lengths. Keep `EnvelopeMeta::new` as the version-one constructor and add `EnvelopeMeta::tracking_v2(device_id, revision, generated_at_ms)` for schema `openmeter.tracking.v2`. Use a 90-day payload window, 512 accounts/providers, 100,000 events, 4,096 quotas, and 100,000 tombstones as validation ceilings. Reject non-finite or negative token/cost/quota values, control characters, duplicate event IDs, duplicate tombstones, and event/tombstone overlap.

- [ ] **Step 4: Add subkey derivation and keyed identifiers**

Add `hkdf = "0.12"`, `hmac = "0.12"`, and `sha2 = "0.10"`. Implement:

```rust
pub fn derive_tracking_key(recovery: &[u8; 32]) -> Result<TrackingKey, ProtocolError>;
pub fn tracking_event_id(
    key: &TrackingKey,
    provider_id: &str,
    account_id: &str,
    source_record: &[u8],
) -> Result<String, ProtocolError>;
```

Use HKDF-SHA256 with info `openmeter/tracking/v2/encryption` for encryption and `openmeter/tracking/v2/event-id` for HMAC identity. Encode the first 32 HMAC bytes as lowercase hex with an `evt_` prefix. Keep key types redacted under `Debug` and zeroized on drop.

- [ ] **Step 5: Implement version-two sealing and opening**

Mirror the existing bounded zstd/XChaCha20-Poly1305 path, but accept only `openmeter.tracking.v2` metadata and deserialize only `TrackingPayloadV2`. Authenticate schema, device ID, revision, and generation time as associated data.

- [ ] **Step 6: Run protocol tests and commit**

Run:

```powershell
cargo test --manifest-path crates/openmeter-sync-protocol/Cargo.toml --all-targets
cargo clippy --manifest-path crates/openmeter-sync-protocol/Cargo.toml --all-targets -- -D warnings
```

Commit: `feat(sync): add encrypted tracking v2 protocol`

### Task 2: Event-Level Local Tracking Extraction

**Files:**
- Create: `src-tauri/src/tracking_events.rs`
- Create: `src-tauri/tests/tracking_events.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/spend.rs`
- Modify: `src-tauri/src/providers/devin.rs`
- Modify: `src-tauri/src/providers/hermes.rs`
- Modify: `src-tauri/src/providers/minimax.rs`
- Modify: `src-tauri/src/providers/opencode.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: provider/account-scoped local log records, `TrackingKey`, `AccountRegistry`, and the existing pricing functions.
- Produces: `LocalTrackingEvent`, `TrackingScan`, and `collect_tracking_for_accounts(cursor_csv, registry, environment, tracking_key, now)`.

- [ ] **Step 1: Write failing extraction and privacy tests**

Create fixtures containing the same Codex event at two different paths and assert identical keyed event IDs. Assert a second event with different normalized usage remains distinct. Serialize exported events and assert the fixture prompt, Windows username, path, access token, and raw provider event ID are absent.

- [ ] **Step 2: Run the targeted test and verify the red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test tracking_events`

Expected: compilation fails because `tracking_events` and `collect_tracking_for_accounts` do not exist.

- [ ] **Step 3: Add an event collector without changing current spend output**

Define:

```rust
pub struct LocalTrackingEvent {
    pub event: UsageEventV2,
    pub day: chrono::NaiveDate,
}

pub struct TrackingScan {
    pub events: Vec<LocalTrackingEvent>,
    pub legacy_spend: Vec<ProviderSpend>,
    pub completed_providers: HashSet<String>,
}
```

Keep `spend::collect_for_accounts` behavior unchanged. Add an optional `TrackingEventCollector` at the existing parsing boundary so one scan can produce both `ProviderSpend` and event facts.

- [ ] **Step 4: Give every historical source a stable source record**

Use native event/session identifiers where present. Otherwise feed the collector canonical source bytes as follows:

- Claude, Codex, Grok, Kimi/Moonshot: the exact JSONL record bytes plus provider and opaque account identity;
- Pi: `AccountUsageEvent.event_id`, falling back to its canonical numeric/model fields;
- Cursor: normalized CSV columns in header order;
- Devin, Hermes, MiniMax, OpenCode, and provider event collectors: native ID when exposed, otherwise timestamp/model/token/cost fields plus stable result ordinal;
- pricing-only synthetic events: the same source record fingerprint used by their originating parser.

The HMAC result is retained; the source bytes and native identifier are discarded immediately.

- [ ] **Step 5: Preserve partial-scan correctness**

Only add a provider to `completed_providers` after its parser finishes. Do not emit tombstones for failed providers. Keep at most 90 complete UTC days, sorting by `(occurred_at_ms, event_id)` for deterministic serialization.

- [ ] **Step 6: Run spend, Pi, extraction, and full Rust tests**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test tracking_events
cargo test --manifest-path src-tauri/Cargo.toml --test pi_history
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
```

If Windows Application Control blocks Cargo-generated build helpers locally, capture the Code Integrity event and require the corresponding GitHub Actions job to pass before deployment; do not weaken the policy.

Commit: `feat(tracking): extract privacy-safe usage events`

### Task 3: Deduplicated Projection, Legacy Replacement, and Quota Selection

**Files:**
- Create: `src-tauri/src/tracking_projection.rs`
- Create: `src-tauri/tests/tracking_projection.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/src/sync_history.rs`

**Interfaces:**
- Consumes: local `TrackingPayloadV2`, decrypted peer payloads with receipt times, version-one peer history, `AccountRegistry`, and current date.
- Produces: `TrackingDashboard { generated_at_ms, devices, days, quotas, legacy_present }` returned by the Tauri command `fetch_tracking`.

- [ ] **Step 1: Write failing merge tests**

Cover these exact cases:

```rust
assert_eq!(merge(events_from_laptop_and_desktop_with_same_id).unique_events, 1);
assert_eq!(merge(two_distinct_ids).unique_events, 2);
assert!(merge(same_id_with_different_content).quarantined_devices.contains("device-b"));
assert_eq!(select_quota(two_devices).source_device_id, "newest-device");
assert!(select_quota(disagreeing_fresh_devices).disagreement);
```

Also assert that version-one rows remain labeled legacy until valid version-two coverage includes the same account/day, then disappear for that overlap.

- [ ] **Step 2: Run the projection test and verify the red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test tracking_projection`

- [ ] **Step 3: Implement collision-safe set union**

Index events by event ID. Identical content collapses. A content mismatch quarantines the later peer payload and retains that peer's prior last-good projection. Apply tombstones before daily aggregation.

- [ ] **Step 4: Produce bounded dashboard rows**

Aggregate unique events into:

```rust
pub struct TrackingDay {
    pub day: String,
    pub device_id: String,
    pub device_label: String,
    pub provider_id: String,
    pub model: String,
    pub tokens: f64,
    pub cost: f64,
}
```

Return only 30 dashboard days while retaining 90 days in encrypted peer state. Sort devices by label and rows by day/device/provider/model.

- [ ] **Step 5: Implement quota freshness and disagreement**

Group by provider/account/metric. Prefer the newest observation no more than two refresh intervals stale. Mark disagreement when two fresh observations differ by more than 5 percentage points or reset times differ by more than five minutes. Never add percentages or limits.

- [ ] **Step 6: Run targeted and full tests, then commit**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test tracking_projection
cargo test --manifest-path src-tauri/Cargo.toml --test sync_history
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
```

Commit: `feat(tracking): merge deduplicated device projections`

### Task 4: Additive Hub Version-Two Storage and API

**Files:**
- Create: `sync-hub/tests/tracking_api.rs`
- Modify: `sync-hub/src/store.rs`
- Modify: `sync-hub/src/api.rs`
- Modify: `sync-hub/tests/store.rs`
- Modify: `sync-hub/tests/api.rs`

**Interfaces:**
- Consumes: authenticated version-two `EncryptedEnvelope` uploads.
- Produces: `Store::put_tracking`, `Store::list_active_tracking`, `TrackingEnvelopeRecord`, `PUT /v2/devices/{device_id}/envelope`, and `GET /v2/envelopes`.

- [ ] **Step 1: Write failing additive-migration tests**

Create a database with one version-one envelope, reopen it with the upgraded store, and assert:

```rust
assert_eq!(store.list_active("other", now)?.len(), 1);
assert_eq!(store.list_active_tracking("other", now)?.len(), 0);
store.put_tracking("device-a", &v2_envelope, now)?;
assert_eq!(store.list_active("other", now)?.len(), 1);
assert_eq!(store.list_active_tracking("other", now)?.len(), 1);
```

- [ ] **Step 2: Run hub tests and verify the red state**

Run: `cargo test --manifest-path sync-hub/Cargo.toml --test tracking_api --test store`

- [ ] **Step 3: Add the independent table transactionally**

Create:

```sql
CREATE TABLE IF NOT EXISTS tracking_envelopes (
  device_id TEXT PRIMARY KEY REFERENCES devices(device_id),
  revision INTEGER NOT NULL,
  generated_at_ms INTEGER NOT NULL,
  received_at_ms INTEGER NOT NULL,
  envelope_json BLOB NOT NULL
);
```

Reuse authentication, body, nonce, ciphertext, replay, exact-revision, revocation, and retention checks. Do not modify or rewrite `envelopes`.

- [ ] **Step 4: Add version-two routes and receipt metadata**

Return `Vec<TrackingEnvelopeRecord>` where each record contains only `received_at_ms` and `envelope`. Use a separate upload-rate key `(device_id, "v2")` so dual version-one/version-two publication does not self-rate-limit.

- [ ] **Step 5: Verify revocation and purge both tables**

Extend revocation tests to assert both projections disappear immediately and both encrypted rows purge after 30 days.

- [ ] **Step 6: Run hub tests and commit**

Run:

```powershell
cargo test --manifest-path sync-hub/Cargo.toml --all-targets
cargo clippy --manifest-path sync-hub/Cargo.toml --all-targets -- -D warnings
```

Commit: `feat(hub): store tracking v2 envelopes alongside v1`

### Task 5: Client Transport, Persistent Peer Cache, and Dual Publication

**Files:**
- Create: `src-tauri/src/sync_tracking.rs`
- Create: `src-tauri/tests/sync_tracking.rs`
- Modify: `src-tauri/src/sync_client.rs`
- Modify: `src-tauri/src/sync_runtime.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/tests/sync_client.rs`

**Interfaces:**
- Consumes: version-two scan, current normalized provider snapshots, device label, hub credential, and recovery key.
- Produces: `SyncClient::push_tracking`, `SyncClient::pull_tracking`, encrypted `sync-pending-v2.json`, encrypted-envelope `sync-peers-v2.json`, and expanded `SyncStatus`.

- [ ] **Step 1: Write failing transport and offline-cache tests**

Assert exact `/v2` paths, response-size limits, rejection of mismatched device IDs, newest-pending replacement, atomic peer-cache replacement, and successful reopening of cached envelopes after a simulated restart.

- [ ] **Step 2: Run targeted tests and verify the red state**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test sync_client --test sync_tracking`

- [ ] **Step 3: Add version-two transport methods**

Implement:

```rust
pub async fn push_tracking(
    &self,
    device_id: &str,
    credential: &str,
    envelope: &EncryptedEnvelope,
) -> Result<(), SyncError>;

pub async fn pull_tracking(
    &self,
    credential: &str,
) -> Result<Vec<TrackingEnvelopeRecord>, SyncError>;
```

Keep the exact-host, no-proxy, no-redirect, 15-second timeout behavior.

- [ ] **Step 4: Persist last-good peer envelopes without plaintext**

Store only received encrypted records in `%APPDATA%\OpenMeter\sync-peers-v2.json`. Validate wire sizes before atomic write. Decrypt into memory only after loading and discard invalid records independently.

- [ ] **Step 5: Capture normalized quota snapshots from refresh**

After `fetch_usage` completes, pass successful progress metrics to `sync_tracking::replace_local_quotas(&snapshots, &registry)`. Map card IDs through the registry identity stamp; omit display labels, plans, text-only balances, warnings, and errors.

- [ ] **Step 6: Publish v1 and v2 independently**

Keep the existing version-one revision and pending file. Add `syncTrackingRevision`, `syncTrackingLastSuccess`, and `syncDeviceLabel` configuration fields. A version-two failure must not roll back a successful version-one upload, and either failure must retain only its corresponding newest pending envelope.

- [ ] **Step 7: Expand redacted status**

Return friendly device summaries, protocol version, last generated/received time, and state without returning event IDs, full account stamps, credentials, recovery material, or ciphertext.

Register `fetch_tracking` and `sync_set_device_label` in the Tauri invoke handler. Extend configuration defaults and the persisted-key allowlist with `syncTrackingRevision`, `syncTrackingLastSuccess`, and `syncDeviceLabel`; migration defaults must not overwrite existing sync fields.

- [ ] **Step 8: Run sync tests and commit**

Run:

```powershell
cargo test --manifest-path src-tauri/Cargo.toml --test sync_client --test sync_tracking
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
```

Commit: `feat(sync): publish and cache tracking v2`

### Task 6: Dashboard Tracking Model and Controls

**Files:**
- Create: `src/tracking.ts`
- Create: `tests/tracking.test.ts`
- Modify: `src/main.ts`
- Modify: `src/styles.css`
- Modify: `src/sync-settings.ts`
- Modify: `tests/sync-settings.test.ts`

**Interfaces:**
- Consumes: `invoke<TrackingDashboard>("fetch_tracking")` and expanded `SyncStatus`.
- Produces: pure `filterTracking`, `buildTrackingSeries`, `selectQuotaRows`, `trackingSummary`, and dashboard render/event handlers.

- [ ] **Step 1: Write failing pure-model tests**

Use fixtures for Laptop and Desktop and assert:

```ts
expect(filterTracking(data, "all").days).toHaveLength(4);
expect(filterTracking(data, "device-laptop").days.every(row => row.device_id === "device-laptop")).toBe(true);
expect(buildTrackingSeries(data, { range: 7, metric: "tokens", groupBy: "device" }).series.map(s => s.label)).toEqual(["Desktop", "Laptop"]);
expect(selectQuotaRows(data, "all")[0].source_device_label).toBe("Desktop");
```

Also test 7/30-day ranges, spend/tokens, provider grouping, zero states, delayed/offline devices, disagreement copy, and HTML escaping.

- [ ] **Step 2: Run Vitest and verify the red state**

Run: `npm test -- --run tests/tracking.test.ts`

- [ ] **Step 3: Implement the pure tracking module**

Define discriminated selections:

```ts
export type DeviceFilter = "all" | string;
export type TrackingMetric = "tokens" | "spend";
export type TrackingRange = 7 | 30;
export type TrackingGroup = "device" | "provider";
```

All chart values derive from the backend's already-deduplicated day rows. Sort series and legends deterministically.

- [ ] **Step 4: Add the global filter and graph controls**

Render `All Devices`, `Laptop`, and `Desktop` chips above Total Spend. Add 7/30 Days, Tokens/Spend, and Device/Provider controls to Usage Trend. Use stacked SVG bars with keyboard-focusable segments and an accessible text summary containing total, peak day, and series values.

- [ ] **Step 5: Apply the filter to existing spend surfaces**

For `All Devices`, use the version-two projection plus uncovered legacy rows. For a device, use only that device's daily rows. Keep local provider cards actionable; render remote quota cards read-only with source label, last observation, and disagreement indicator.

- [ ] **Step 6: Add device health to Settings -> Sync**

Render label, short opaque reference, client version, last generated, last received, and one of `Current`, `Delayed`, `Offline`, `Needs upgrade`, or `Revoked`. Add an editable local label constrained to 1-32 printable characters and save it through `sync_set_device_label`.

- [ ] **Step 7: Style narrow and expanded layouts**

Add responsive rules so filters wrap within the tray popover, legends remain readable, stacked segments retain a 2-pixel minimum hit target, and compact mode hides secondary labels without hiding accessible summaries. Reuse existing CSS variables and motion-reduction behavior.

- [ ] **Step 8: Run frontend tests/build and commit**

Run:

```powershell
npm test -- --run
npm run build
```

Commit: `feat(ui): add cross-device tracking dashboard`

### Task 7: Hub Upgrade Backup, Deployment Scripts, and Documentation

**Files:**
- Modify: `scripts/install-sync-hub.ps1`
- Modify: `scripts/test-sync-hub.ps1`
- Create: `scripts/test-cross-device-sync.ps1`
- Modify: `scripts/smoke-install.ps1`
- Modify: `docs/sync-hub.md`
- Modify: `docs/parity-matrix.md`
- Modify: `scripts/test-parity-matrix.ps1`

**Interfaces:**
- Consumes: built client installer, hub binary, protected Mini PC database, and synthetic test identities.
- Produces: verified hub backup/upgrade, dual-protocol acceptance script, and documented enrollment/rollback procedure.

- [ ] **Step 1: Write failing script invariants**

Extend static tests to require a timestamped `hub.db` backup before binary replacement, backup hash verification, preservation of `pepper.bin`, dual `/v1` and `/v2` probes, and rollback instructions that never remove data.

- [ ] **Step 2: Run script tests and verify the red state**

Run:

```powershell
.\scripts\test-sync-hub.ps1 -StaticOnly
.\scripts\test-parity-matrix.ps1
```

- [ ] **Step 3: Make hub upgrade recoverable**

Before stopping the service, copy `C:\ProgramData\OpenMeterSync\hub.db` to `C:\ProgramData\OpenMeterSync\backups\hub-<UTC timestamp>.db`, compute SHA-256 for both, and abort if the backup cannot be opened read-only. Preserve the prior executable as `openmeter-sync-hub.previous.exe` until live acceptance passes.

- [ ] **Step 4: Add synthetic dual-device acceptance**

`test-cross-device-sync.ps1` must create two temporary enrollment identities, upload one duplicate and one unique encrypted fixture through version two, verify both envelopes survive restart, revoke the temporary devices, and avoid production credentials/history. The Rust test fixture generator supplies ciphertext; PowerShell never receives a production recovery key.

- [ ] **Step 5: Update documentation and parity enforcement**

Document Laptop/Desktop labels, UI-only recovery-key import, one token per device, expected filters/graphs, offline behavior, backup location, rollback binary, and evidence checklist. Mark cross-device tracking complete only when live three-machine acceptance is recorded.

- [ ] **Step 6: Run script checks and commit**

Run:

```powershell
.\scripts\test-sync-hub.ps1 -StaticOnly
.\scripts\test-parity-matrix.ps1
.\scripts\test-release-workflow.ps1
.\scripts\test-install-script.ps1
```

Commit: `chore(sync): add recoverable cross-device deployment`

### Task 8: Full Verification and GitHub CI

**Files:**
- Modify: `.github/workflows/ci.yml`
- Modify: `CHANGELOG.md`

**Interfaces:**
- Consumes: all implementation tasks.
- Produces: one reproducible branch validation result and a CI run covering protocol, hub, client, frontend, and deployment invariants.

- [ ] **Step 1: Add maintained-file formatting and new test gates**

Include the new Rust modules in `rustfmt --check`, run the cross-device static script, and keep all existing Clippy, Rust, frontend, parity, installer, and release-policy gates.

- [ ] **Step 2: Run the complete local suite**

Run:

```powershell
$formatFiles = @(
  (Get-ChildItem crates/openmeter-sync-protocol/src -Filter *.rs).FullName
  'src-tauri/src/tracking_events.rs'
  'src-tauri/src/tracking_projection.rs'
  'src-tauri/src/sync_tracking.rs'
  (Get-ChildItem sync-hub/src -Filter *.rs).FullName
)
rustfmt --edition 2021 --check --config skip_children=true $formatFiles
cargo clippy --manifest-path crates/openmeter-sync-protocol/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path crates/openmeter-sync-protocol/Cargo.toml --all-targets
cargo clippy --manifest-path sync-hub/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path sync-hub/Cargo.toml --all-targets
cargo clippy --manifest-path src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path src-tauri/Cargo.toml --all-targets
npm test -- --run
npm run build
.\scripts\test-sync-hub.ps1 -StaticOnly
.\scripts\test-parity-matrix.ps1
.\scripts\test-release-workflow.ps1
.\scripts\test-install-script.ps1
```

- [ ] **Step 3: Record environmental blocks honestly**

If Application Control blocks a generated Cargo executable, save the exact Code Integrity event ID/policy ID in the verification notes, do not change policy, and rely on a green Windows GitHub Actions run for the blocked Rust gate. All unblocked local gates must still pass.

- [ ] **Step 4: Commit, push the feature branch, and wait for CI**

Commit: `test(sync): verify cross-device tracking`

Push only `feature/full-windows-parity`. Do not create or merge a pull request unless separately authorized. Require every CI job on the resulting commit to conclude `success` before deployment.

### Task 9: Build and Upgrade the Mini PC and This PC

**Files:**
- Runtime artifacts only; no source edits expected.

**Interfaces:**
- Consumes: CI-verified hub binary and Windows installer from the feature branch build.
- Produces: upgraded Mini PC hub and upgraded Laptop client with version-one and version-two health.

- [ ] **Step 1: Build release artifacts without claiming trusted signing**

Build the hub, tray, CLI, and NSIS bundle. Record SHA-256 hashes. Because Microsoft Trusted Signing remains unconfigured, label these machine-local artifacts unsigned and do not publish them as a trusted release.

- [ ] **Step 2: Verify exact targets before mutation**

Confirm the local hostname/client identity and confirm Mini PC hostname, Tailscale IP `100.90.87.7`, service name `OpenMeterSyncHub`, binary path, database path, pepper path, service account, and firewall rule. Stop if any identity differs.

- [ ] **Step 3: Upgrade the Mini PC with backup and rollback binary**

Transfer by SSH/SCP, verify the remote hash, run the upgraded installer, and verify service state, exact bind, firewall scope, `/health`, version-one API, version-two API, database backup hash, and restart persistence.

- [ ] **Step 4: Upgrade this PC in place**

Run the silent installer, preserve `%APPDATA%\OpenMeter`, start the tray, verify CLI/PATH/API, confirm no OpenMeter path contains `OneDrive`, set the encrypted label to `Laptop`, and run Sync Now.

- [ ] **Step 5: Verify the first version-two envelope**

Confirm the hub has one active production version-two record, the laptop status is `Current`, version-one sync still succeeds, and database byte inspection contains neither `Laptop` nor provider/model/user/path plaintext.

### Task 10: Install the Desktop and Complete Three-Machine Acceptance

**Files:**
- Runtime artifacts only; update `docs/parity-matrix.md` evidence after acceptance.

**Interfaces:**
- Consumes: saved recovery key entered by the owner in the desktop UI, one single-use enrollment token, and verified artifacts from Task 9.
- Produces: Desktop enrollment and matching combined tracking on both Windows clients.

- [ ] **Step 1: Wake and verify the desktop**

Use `C:\Users\aGamingGod\Scripts\Wake-Desktop.ps1`. If direct Tailscale SSH is unavailable, use the documented Mini PC `ProxyCommand` route to `192.168.1.1`. Require hostname `aGamingGod` before transfer or installation.

- [ ] **Step 2: Transfer and install the verified client**

Copy the installer, compare SHA-256 locally and remotely, install per-user outside OneDrive, and verify tray process, CLI, PATH, loopback API, and provider discovery. Set the encrypted label to `Desktop`.

If Windows Application Control refuses the unsigned machine-local build, stop and report the signing prerequisite. Do not bypass the policy or install a local trust root.

- [ ] **Step 3: Import the recovery key locally**

Pause for the owner to type or paste the saved recovery key directly into OpenMeter Settings -> Sync on the desktop. Do not automate, capture, print, or transmit the key.

- [ ] **Step 4: Create and consume one enrollment token**

Create a 15-minute token on the Mini PC, enter it in the desktop OpenMeter UI, confirm it is single-use, and verify a second unique active production device identity appears.

- [ ] **Step 5: Synchronize and verify dashboard parity**

Run Sync Now on Desktop and Laptop. Verify All Devices, Laptop, and Desktop filters; 7/30-day token/spend graphs; device/provider stacking; freshest quota selection; device status/freshness; and equal combined totals on both clients.

- [ ] **Step 6: Verify deduplication and offline recovery safely**

Use the synthetic test identities from `test-cross-device-sync.ps1` to prove duplicate collapse and unique-event addition. Stop the hub service briefly, verify both clients retain local dashboards, restart it, and verify automatic recovery. Do not alter production logs to create fixtures.

- [ ] **Step 7: Verify privacy, revocation, restart, and rollback readiness**

Inspect hub storage for plaintext leakage, revoke synthetic identities, restart hub and both clients, recheck totals, confirm neither client uses OneDrive, and confirm the previous hub/client binaries plus database backup remain available.

- [ ] **Step 8: Record live acceptance and final state**

Update the parity evidence with commit, CI run, artifact hashes, device IDs shortened/redacted, service state, sync timestamps, graph/filter checks, privacy inspection, and rollback locations. Commit: `docs: verify cross-device tracking rollout`.

## Final Completion Gate

Do not call the feature complete until all ten tasks are checked, the feature-branch CI run is green, the Mini PC survives a service restart, Laptop and Desktop display identical combined totals, device filters differ where expected, synthetic duplicate facts count once, local tracking survives hub downtime, and no production secret appears in source, logs, commands, or hub plaintext.
