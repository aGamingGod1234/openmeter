# OpenMeter Private Mini PC Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Synchronize normalized OpenMeter history through the Windows 11 Mini PC at `100.90.87.7` while keeping the hub unable to decrypt user history.

**Architecture:** A shared Rust protocol crate owns payload validation and XChaCha20-Poly1305 envelopes. A separate Windows service stores authenticated opaque device revisions in SQLite; the Tauri client keeps keys in Windows Credential Manager and merges valid peer history additively at the normalized spend boundary.

**Tech Stack:** Rust 2021, Serde, XChaCha20-Poly1305, zstd, SHA-256/HMAC, Axum, Tokio, rusqlite, windows-service, Windows Credential Manager, Reqwest rustls, Tailscale.

## Global Constraints

- Hub host is Windows 11 Pro `MiniPC`, Tailscale IPv4 `100.90.87.7`.
- Bind only to `100.90.87.7`; never `0.0.0.0`, `::`, a public address, or OneDrive-backed storage.
- The hub never receives a history encryption key, credentials, tokens, emails, raw logs, provider responses, labels, or filesystem paths.
- XChaCha20-Poly1305 uses a random 24-byte nonce and authenticates schema version, device id, and revision as associated data.
- Maximum encrypted envelope is 2 MiB; request timeout is 15 seconds; poll interval is five minutes.
- Accept at most one upload per device per minute with one retry burst.
- Enrollment tokens expire after 15 minutes and are single-use.
- Revoked encrypted records are retained for 30 days, then deleted.
- Local history is authoritative; peer history is additive.
- Write failing tests before implementation and never use production history or secrets in tests.

---

## File map

- `crates/openmeter-sync-protocol`: shared, platform-neutral history schema, validation, compression, and encryption.
- `sync-hub`: independent headless Axum/SQLite Windows service and administration CLI.
- `src-tauri/src/credential_store.rs`: Windows Credential Manager wrapper.
- `src-tauri/src/sync_client.rs`: enrollment, encrypted upload/download, backoff, and peer cache.
- `src-tauri/src/sync_history.rs`: conversion between local spend rows and synchronized normalized history.
- `src-tauri/src/lib.rs`: commands, lifecycle scheduling, and configuration.
- `src/main.ts`: sync settings, device status, enrollment, recovery-key import/export, and diagnostics.
- `scripts/install-sync-hub.ps1`: explicit Mini PC service installation and firewall scoping.
- `scripts/test-sync-hub.ps1`: live tailnet health and opaque round-trip acceptance.

### Task 1: Shared history protocol and encryption

**Files:**
- Create: `crates/openmeter-sync-protocol/Cargo.toml`
- Create: `crates/openmeter-sync-protocol/src/lib.rs`
- Create: `crates/openmeter-sync-protocol/src/model.rs`
- Create: `crates/openmeter-sync-protocol/src/crypto.rs`
- Create: `crates/openmeter-sync-protocol/tests/envelope.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Produces: `HistoryPayloadV1`, `AccountHistoryV1`, `DailyUsageV1`, `ModelUsageV1`.
- Produces: `EnvelopeMeta { schema, device_id, revision, generated_at_ms }`.
- Produces: `seal(key: &HistoryKey, meta: EnvelopeMeta, payload: &HistoryPayloadV1) -> Result<EncryptedEnvelope, ProtocolError>`.
- Produces: `open(key: &HistoryKey, envelope: &EncryptedEnvelope) -> Result<HistoryPayloadV1, ProtocolError>`.

- [ ] **Step 1: Write failing round-trip and tamper tests**

```rust
#[test]
fn round_trip_preserves_normalized_history_only() {
    let key = HistoryKey::from_bytes([7; 32]);
    let meta = EnvelopeMeta::new("device-a", 4, 1_800_000_000_000).unwrap();
    let envelope = seal(&key, meta.clone(), &fixture_payload()).unwrap();
    assert_ne!(envelope.ciphertext, serde_json::to_vec(&fixture_payload()).unwrap());
    assert_eq!(open(&key, &envelope).unwrap(), fixture_payload());
    assert_eq!(envelope.meta, meta);
}

#[test]
fn changing_authenticated_revision_rejects_the_envelope() {
    let key = HistoryKey::from_bytes([7; 32]);
    let mut envelope = seal(&key, meta(4), &fixture_payload()).unwrap();
    envelope.meta.revision = 5;
    assert_eq!(open(&key, &envelope), Err(ProtocolError::Authentication));
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test --manifest-path crates/openmeter-sync-protocol/Cargo.toml`

Expected: failure because the crate does not exist.

- [ ] **Step 3: Implement strict models and authenticated encryption**

Use these dependencies exactly:

```toml
[dependencies]
base64 = "0.22"
chacha20poly1305 = "0.10"
rand = "0.8"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
zeroize = { version = "1", features = ["derive"] }
zstd = "0.13"
```

Serialize payloads with `serde_json::to_vec`, compress at zstd level 3, reject uncompressed payloads above 8 MiB, and encrypt with:

```rust
let cipher = XChaCha20Poly1305::new(Key::from_slice(key.expose()));
let ciphertext = cipher.encrypt(
    XNonce::from_slice(&nonce),
    Payload { msg: &compressed, aad: &serde_json::to_vec(&meta)? },
)?;
```

All schema structs use `#[serde(deny_unknown_fields)]`. Validate provider/record/model string lengths, finite nonnegative totals, unique `(provider_id, record_id, day)` rows, revision `>= 1`, and ciphertext `<= 2 MiB`.

- [ ] **Step 4: Run protocol tests and Clippy**

Run: `cargo test --manifest-path crates/openmeter-sync-protocol/Cargo.toml`

Run: `cargo clippy --manifest-path crates/openmeter-sync-protocol/Cargo.toml --all-targets -- -D warnings`

Expected: all pass.

- [ ] **Step 5: Commit**

```powershell
git add crates/openmeter-sync-protocol src-tauri/Cargo.toml
git commit -m "feat: define encrypted history sync protocol"
```

### Task 2: Transactional hub storage and revision policy

**Files:**
- Create: `sync-hub/Cargo.toml`
- Create: `sync-hub/src/lib.rs`
- Create: `sync-hub/src/store.rs`
- Create: `sync-hub/tests/store.rs`

**Interfaces:**
- Consumes: `EncryptedEnvelope` from Task 1.
- Produces: `Store::open(path) -> Result<Store, HubError>`.
- Produces: `Store::enroll_device(device_id, credential_hash, now_ms) -> Result<(), HubError>`.
- Produces: `Store::put(device_id, envelope, now_ms) -> Result<(), PutError>`.
- Produces: `Store::list_active(excluding_device, now_ms) -> Result<Vec<EncryptedEnvelope>, HubError>`.
- Produces: `Store::revoke(device_id, now_ms) -> Result<(), HubError>` and `Store::purge(now_ms)`.

- [ ] **Step 1: Write failing storage tests**

```rust
#[test]
fn revision_must_increase_exactly_and_devices_cannot_overwrite_peers() {
    let store = Store::open(temp_db()).unwrap();
    store.enroll_device("a", [1; 32], 900).unwrap();
    store.enroll_device("b", [2; 32], 900).unwrap();
    store.put("a", envelope("a", 1), 1_000).unwrap();
    assert_eq!(store.put("a", envelope("a", 1), 2_000), Err(PutError::Replay));
    assert_eq!(store.put("b", envelope("a", 2), 3_000), Err(PutError::DeviceMismatch));
    store.put("a", envelope("a", 2), 4_000).unwrap();
}

#[test]
fn revoked_record_is_hidden_immediately_and_purged_after_thirty_days() {
    let store = Store::open(temp_db()).unwrap();
    store.enroll_device("a", [1; 32], 900).unwrap();
    store.put("a", envelope("a", 1), 1_000).unwrap();
    store.revoke("a", 2_000).unwrap();
    assert!(store.list_active("b", 2_001).unwrap().is_empty());
    store.purge(2_000 + 30 * 24 * 60 * 60 * 1_000 + 1).unwrap();
    assert_eq!(store.envelope_count().unwrap(), 0);
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test --manifest-path sync-hub/Cargo.toml --test store`

Expected: compilation fails because `Store` does not exist.

- [ ] **Step 3: Implement SQLite schema and transactions**

Create tables:

```sql
CREATE TABLE devices (
  device_id TEXT PRIMARY KEY,
  credential_hash BLOB NOT NULL,
  created_at_ms INTEGER NOT NULL,
  revoked_at_ms INTEGER
);
CREATE TABLE envelopes (
  device_id TEXT PRIMARY KEY REFERENCES devices(device_id),
  revision INTEGER NOT NULL,
  generated_at_ms INTEGER NOT NULL,
  received_at_ms INTEGER NOT NULL,
  envelope_json BLOB NOT NULL
);
CREATE TABLE enrollment_tokens (
  token_hash BLOB PRIMARY KEY,
  expires_at_ms INTEGER NOT NULL,
  consumed_at_ms INTEGER
);
```

Use `BEGIN IMMEDIATE` for enrollment, upload, revocation, and purge. Reject an upload unless its envelope device matches authentication and its revision is exactly the stored revision plus one; allow revision 1 when absent.

- [ ] **Step 4: Run storage tests across restart**

Run: `cargo test --manifest-path sync-hub/Cargo.toml --test store`

Expected: all pass, including reopen of the same SQLite file.

- [ ] **Step 5: Commit**

```powershell
git add sync-hub
git commit -m "feat: persist sync device revisions transactionally"
```

### Task 3: Authenticated hub HTTP API and administration CLI

**Files:**
- Create: `sync-hub/src/auth.rs`
- Create: `sync-hub/src/api.rs`
- Create: `sync-hub/src/main.rs`
- Create: `sync-hub/tests/api.rs`
- Modify: `sync-hub/Cargo.toml`

**Interfaces:**
- Consumes: `Store` and protocol envelopes.
- Produces endpoints: `GET /health`, `POST /v1/enroll`, `PUT /v1/devices/:id/envelope`, `GET /v1/envelopes`, `DELETE /v1/devices/:id`.
- Produces CLI: `openmeter-sync-hub serve`, `enrollment create`, `device revoke`, `device list`.

- [ ] **Step 1: Write failing API policy tests**

```rust
#[tokio::test]
async fn enrollment_is_single_use_and_upload_requires_device_bearer() {
    let app = test_app().await;
    let token = app.create_enrollment(1_000).await;
    let enrolled = post_enroll(&app, &token).await;
    assert_eq!(post_enroll(&app, &token).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(put_envelope(&app, None, envelope(&enrolled.id, 1)).await.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(put_envelope(&app, Some(&enrolled.credential), envelope(&enrolled.id, 1)).await.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn oversized_body_is_rejected_before_json_parsing() {
    assert_eq!(post_bytes(test_app().await, vec![0; 2 * 1024 * 1024 + 1]).await.status(), StatusCode::PAYLOAD_TOO_LARGE);
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test --manifest-path sync-hub/Cargo.toml --test api`

Expected: API module and routes do not exist.

- [ ] **Step 3: Implement API, credential hashing, limits, and CLI**

Use `axum = "0.8"`, `tower-http = { version = "0.6", features = ["limit", "timeout", "trace"] }`, `hmac = "0.12"`, and `sha2 = "0.10"`. Generate 32-byte enrollment/device secrets. Store `HMAC-SHA256(server_pepper, secret)` only. Compare MACs through the `Mac::verify_slice` constant-time path.

Read the 32-byte server pepper from `--pepper-file`; reject a missing file,
wrong length, or an ACL that grants ordinary Users write access. The pepper is
hub-local authentication state, never the client history key.

Bind configuration rejects unspecified, loopback, multicast, or non-Tailscale/LAN addresses; production command is:

```powershell
openmeter-sync-hub.exe serve --bind 100.90.87.7:6740 --database C:\ProgramData\OpenMeterSync\hub.db
```

Return JSON problem details without secrets. Disable CORS. Limit body size to 2 MiB, headers to 32 KiB, request duration to 15 seconds, and uploads per credential to the approved token bucket.

- [ ] **Step 4: Run all hub tests and Clippy**

Run: `cargo test --manifest-path sync-hub/Cargo.toml --all-targets`

Run: `cargo clippy --manifest-path sync-hub/Cargo.toml --all-targets -- -D warnings`

Expected: all pass.

- [ ] **Step 5: Commit**

```powershell
git add sync-hub
git commit -m "feat: serve authenticated opaque sync envelopes"
```

### Task 4: Windows Credential Manager and resilient client

**Files:**
- Create: `src-tauri/src/credential_store.rs`
- Create: `src-tauri/src/sync_client.rs`
- Create: `src-tauri/tests/credential_store.rs`
- Create: `src-tauri/tests/sync_client.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src-tauri/Cargo.toml`

**Interfaces:**
- Consumes: protocol crate and hub API from Tasks 1–3.
- Produces: `CredentialStore::{read, write, delete}(target) -> Result<SecretVec, CredentialError>`.
- Produces: `SyncClient::enroll`, `SyncClient::push`, `SyncClient::pull`, `SyncClient::revoke`.
- Produces: `RetryState::next(now_ms, outcome) -> Option<i64>`.

- [ ] **Step 1: Write failing secret/redaction and retry tests**

```rust
#[test]
fn credential_debug_never_exposes_secret() {
    let secret = SecretVec::new(vec![1, 2, 3, 4]);
    assert_eq!(format!("{secret:?}"), "[secret]");
}

#[test]
fn newest_pending_snapshot_replaces_older_and_backoff_is_capped() {
    let mut queue = PendingEnvelope::default();
    queue.replace(envelope(4));
    queue.replace(envelope(5));
    assert_eq!(queue.current().unwrap().meta.revision, 5);
    assert!(RetryState::after_failures(20).delay() <= Duration::from_secs(30 * 60));
}
```

- [ ] **Step 2: Run and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test credential_store --test sync_client`

Expected: modules do not exist.

- [ ] **Step 3: Implement Credential Manager storage and client state machine**

Use `CredWriteW`, `CredReadW`, `CredDeleteW`, and `CredFree` with generic credentials scoped to `OpenMeter/sync/history-key` and `OpenMeter/sync/device-credential`. Zero secret buffers on drop. A Credential Manager failure disables sync and returns a remediation error; never write plaintext fallback files.

Use a `reqwest::Client` with rustls, 15-second timeout, no redirects, no proxy inheritance for the Tailscale hub, and an exact host allowlist from config. Keep only the newest pending envelope in `%APPDATA%\OpenMeter\sync-pending.json`; the file is already ciphertext. Apply exponential backoff with full jitter from 5 seconds to 30 minutes and reset after a successful health/push/pull cycle.

- [ ] **Step 4: Run client tests and redaction suite**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test credential_store --test sync_client --test redaction`

Expected: all pass; Windows integration case round-trips a disposable credential and deletes it.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/credential_store.rs src-tauri/src/sync_client.rs src-tauri/src/lib.rs src-tauri/Cargo.toml src-tauri/tests/credential_store.rs src-tauri/tests/sync_client.rs
git commit -m "feat: add secure resilient Windows sync client"
```

### Task 5: History projection, peer merge, and settings UI

**Files:**
- Create: `src-tauri/src/sync_history.rs`
- Create: `src-tauri/tests/sync_history.rs`
- Modify: `src-tauri/src/spend.rs`
- Modify: `src-tauri/src/contracts.rs`
- Modify: `src-tauri/src/httpapi.rs`
- Modify: `src-tauri/src/lib.rs`
- Modify: `src/models.ts`
- Modify: `src/main.ts`
- Create: `tests/sync-settings.test.ts`

**Interfaces:**
- Consumes: account registry from parity Task 2 and sync client from Task 4.
- Produces: `export_history(local, registry, enabled) -> HistoryPayloadV1`.
- Produces: `merge_peer_history(local, peers, registry, enabled) -> CombinedHistory`.
- Adds Tauri commands: `sync_status`, `sync_enroll`, `sync_import_recovery`, `sync_now`, `sync_revoke_device`, `sync_disable`.

- [ ] **Step 1: Write failing payload-filter and merge tests**

```rust
#[test]
fn export_contains_totals_but_no_labels_credentials_errors_or_paths() {
    let payload = export_history(&fixture_local(), &registry(), &enabled());
    let json = serde_json::to_string(&payload).unwrap();
    for forbidden in ["Company", "access_token", "C:\\Users", "HTTP 429"] {
        assert!(!json.contains(forbidden));
    }
}

#[test]
fn peer_history_adds_to_totals_without_creating_a_local_card() {
    let combined = merge_peer_history(local(), vec![remote_only_account()], &registry(), &enabled());
    assert_eq!(combined.cards.len(), local().cards.len());
    assert!(combined.total_spend.iter().any(|row| row.record_id == "claude@abcd1234"));
}
```

- [ ] **Step 2: Run Rust/frontend tests and verify RED**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test sync_history`

Run: `npm test -- --run tests/sync-settings.test.ts`

Expected: projection, peer merge, and settings do not exist.

- [ ] **Step 3: Implement one combined history boundary and UI**

Export only calendar-day tokens/cost, per-model totals, unpriced model identifiers, opaque account identity, provider id, and enabled providers. Deduplicate peer envelopes by device and highest valid revision. Ignore future schemas and data outside the scanner's rolling window. Disabled providers contribute nothing.

Feed `CombinedHistory` into dashboard spend, Total Spend, trend, model breakdown, share cards, `/v1/usage`, and `/v1/limits`; quotas and errors remain local.

Settings includes an off-by-default Sync section with hub URL, status, last success, device list, enrollment token entry, recovery key import/display-once, Sync Now, revoke, and Disable Sync. Never return stored recovery material from a normal `sync_status` call.

- [ ] **Step 4: Run affected suites**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --test sync_history --test contracts --test httpapi`

Run: `npm test -- --run`

Expected: all pass.

- [ ] **Step 5: Commit**

```powershell
git add src-tauri/src/sync_history.rs src-tauri/src/spend.rs src-tauri/src/contracts.rs src-tauri/src/httpapi.rs src-tauri/src/lib.rs src/models.ts src/main.ts src-tauri/tests/sync_history.rs tests/sync-settings.test.ts
git commit -m "feat: merge encrypted peer history across app surfaces"
```

### Task 6: Install and verify the Mini PC Windows service

**Files:**
- Create: `sync-hub/src/service.rs`
- Modify: `sync-hub/src/main.rs`
- Modify: `sync-hub/Cargo.toml`
- Create: `scripts/install-sync-hub.ps1`
- Create: `scripts/uninstall-sync-hub.ps1`
- Create: `scripts/test-sync-hub.ps1`
- Create: `docs/sync-hub.md`
- Modify: `.github/workflows/ci.yml`

**Interfaces:**
- Consumes: completed hub executable.
- Produces: Windows service name `OpenMeterSyncHub` running as `NT AUTHORITY\LocalService`.
- Produces: inbound firewall rule limited to TCP 6740 on the Tailscale interface/profile.

- [ ] **Step 1: Write failing PowerShell path/firewall invariants**

```powershell
$script = Get-Content "$PSScriptRoot\install-sync-hub.ps1" -Raw
if ($script -notmatch '100\.90\.87\.7:6740') { throw 'Hub must bind its exact Tailscale address' }
if ($script -match '0\.0\.0\.0|AnyAddress|OneDrive') { throw 'Unsafe hub binding or storage path' }
if ($script -notmatch 'OpenMeterSyncHub') { throw 'Stable service identity missing' }
```

- [ ] **Step 2: Run and verify RED**

Run: `powershell -NoProfile -File scripts/test-sync-hub.ps1 -StaticOnly`

Expected: failure because installer/service files do not exist.

- [ ] **Step 3: Implement Windows service commands and idempotent installer**

Use `windows-service = "0.8"`. `openmeter-sync-hub service run` registers the SCM dispatcher; `service install` creates an automatic delayed-start service with recovery restart after 60 seconds. The installer copies the signed binary to `C:\Program Files\OpenMeter Sync Hub`, creates `C:\ProgramData\OpenMeterSync` restricted to LocalService and Administrators, generates `pepper.bin` with 32 bytes from `RandomNumberGenerator.Fill`, applies the same ACL, and passes `--pepper-file C:\ProgramData\OpenMeterSync\pepper.bin` to the service. It creates only the scoped TCP 6740 rule.

The uninstall script stops and removes the service and firewall rule but preserves the database unless `-RemoveData` is explicitly supplied.

- [ ] **Step 4: Build, deploy through SSH, and run live opaque round-trip**

Run in CI: `cargo build --manifest-path sync-hub/Cargo.toml --release --target x86_64-pc-windows-msvc`.

Copy the verified artifact to the Mini PC over the existing SSH channel, run `install-sync-hub.ps1` in an elevated session, then run:

```powershell
Invoke-RestMethod http://100.90.87.7:6740/health
powershell -NoProfile -File scripts/test-sync-hub.ps1 -HubUrl http://100.90.87.7:6740
```

Expected: service is Running, health reports protocol 1, enrollment is single-use, ciphertext round-trips, and the hub database contains no plaintext fixture marker.

- [ ] **Step 5: Commit**

```powershell
git add sync-hub scripts/install-sync-hub.ps1 scripts/uninstall-sync-hub.ps1 scripts/test-sync-hub.ps1 docs/sync-hub.md .github/workflows/ci.yml
git commit -m "feat: deploy private sync hub as a Windows service"
```
