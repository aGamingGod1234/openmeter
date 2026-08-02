# OpenMeter Full Windows Parity, Private Sync, and Trusted Signing Design

**Date:** 2026-08-03

**Status:** Approved in conversation; pending written-spec review

**Upstream baseline:** `robinebers/openusage` at `9d2bf09`

**Target:** Windows 11 on the owner's PC, with an always-on Mini PC at
`100.90.87.7` available through Tailscale

## Objective

Finish OpenMeter as a native Windows 11 implementation of OpenUsage with the
same user-visible capabilities, while preserving OpenMeter's additional
providers and Windows-specific features. The installed application must run on
the target PC under Smart App Control, remain independent of OneDrive, and use
the Mini PC as a private encrypted history-sync hub.

Parity means implementing each feature in the upstream baseline directly or
through the platform equivalent approved in this document. It does not mean
copying macOS implementation details that cannot operate on Windows.

## Chosen approach

Extend the existing Rust/Tauri implementation instead of rebuilding it. Keep
the current provider integrations and presentation shell, close the audited
upstream gaps at their existing runtime boundaries, add a small headless sync
service for the Mini PC, and Authenticode-sign Windows release artifacts using
Microsoft Trusted Signing.

Rejected alternatives:

- Encrypted file transfer over SSH/SFTP lacks robust conflict handling, device
  discovery, validation, and immediate synchronization.
- A new shared cross-platform core would discard substantial verified Windows
  work and delay a usable installation without improving near-term parity.

## Scope

The implementation will:

- Preserve all 18 providers currently supported by OpenMeter.
- Match upstream account-first behavior, including stable account identities,
  source attachment, live rename resolution, account-stamped caches, and plain
  family/card matching at CLI and HTTP boundaries.
- Match the dashboard, tray, customization, pace, notification, pricing,
  spend, sharing, proxy, privacy, update-channel, and stale-data behavior in the
  selected upstream baseline.
- Add normalized history synchronization through the Mini PC over Tailscale.
- Produce Authenticode-signed executables and an Authenticode-signed NSIS
  installer that Smart App Control accepts on the target PC.
- Keep application data under `%APPDATA%\OpenMeter` and installation data under
  `%LOCALAPPDATA%\OpenMeter`; OneDrive is neither used nor required.

The implementation will not expose a public synchronization endpoint, upload
credentials or raw logs, weaken Smart App Control, or silently install a local
root certificate.

The audited upstream gaps that must be closed include the standalone shell
environment snapshot, account/source lifecycle and custom Claude/Codex homes,
one live name resolver across every output surface, Pi history attribution,
peer-history identity remapping, stable/beta update selection, and the approved
Windows equivalents for capture privacy and iCloud sync. Existing Windows
features are re-verified rather than assumed complete.

## Application architecture

The Windows tray application remains one Tauri process with five internal
boundaries.

### Provider runtime

Provider modules discover credentials and local logs, fetch vendor quotas,
refresh tokens where required, calculate spend, and emit normalized account
snapshots. Refreshes execute independently with timeouts and bounded
concurrency. A provider failure retains its last-good snapshot and enters a
capped cooldown instead of blocking the refresh batch.

### Account registry

Every rendered card represents a stable account record. Provider-specific
locations are sources attached to that record, not identities themselves. A
source disappearing hides the card only when no other local source remains;
the record, layout, cache, pins, and history remain available for reattachment.

The registry owns user labels. The provider model contains only a derived
default name. Cards, Total Spend, share rendering, accessible tray text,
notifications, CLI output, and HTTP output resolve the current label at their
render or serialization boundaries. Renames are not baked into caches or
synchronized history.

### Presentation layer

One normalized snapshot model feeds the tray strip, dashboard, spend views,
pace warnings, customization, share cards, CLI, and loopback API. This prevents
surface-specific drift in names, stale state, peer history, or usage totals.

The tray supports text and compact-bar styles, at most two stars per provider,
customization order, real-data-only rendering, and privacy fallback. Dashboard
rows retain upstream click, context-menu, reorder, enablement, Always Visible,
On Demand, refresh, and reset behavior.

### Sync client

The client exports only normalized daily token/spend totals, model totals,
unknown-model identifiers, opaque account identity keys, provider enablement,
device identity, schema version, revision, and generation time. It explicitly
excludes credentials, tokens, emails, raw logs, quota responses, balances,
plans, provider errors, user labels, and local filesystem paths.

The payload is serialized deterministically, compressed, then encrypted with
XChaCha20-Poly1305 using a random 24-byte nonce. Schema version, device id, and
revision are authenticated as associated data. A random 32-byte history key is
generated by the first client and displayed once as recovery material for
enrolling another client; it is never sent to or generated by the hub. The
history key and device credential are stored in Windows Credential Manager.
The hub sees only an opaque envelope and routing metadata needed to authenticate
the device and select its latest revision.

### Windows integration

Windows-specific code owns per-user autostart, global shortcut, notifications,
capture exclusion, privacy-mode tray hiding, installer/PATH registration,
Credential Manager access, update integration, and signature validation.

## Mini PC sync hub

The repository will contain a small headless `openmeter-sync-hub` service. It
will run on the Mini PC, bind only to the Mini PC's Tailscale/LAN interface, and
will not configure public DNS, router port forwarding, or an internet-facing
listener.

The service provides:

- health and protocol-version discovery;
- authenticated device enrollment;
- upload of a device's next monotonically increasing encrypted revision;
- listing and download of current peer envelopes;
- device revocation and bounded record retention;
- SQLite persistence with transactional revision checks;
- strict request, payload-size, rate, and timeout limits.

An administrator creates a one-time enrollment token locally over the existing
SSH administration path. The client exchanges it once for a random device
credential; the hub stores only a keyed hash of that credential. Enrollment
tokens expire after 15 minutes and are invalidated on first use. A second
OpenMeter client imports the shared recovery material separately, so possession
of a hub credential alone cannot decrypt history.

Each device owns one stable random identity and one revision stream. Devices do
not overwrite one another. The hub rejects replayed or skipped-policy revisions,
invalid credentials, malformed envelopes, and oversized payloads before
persistence. It cannot decrypt valid history envelopes.

Default operational limits are a 2 MiB encrypted envelope, one accepted upload
per device per minute with burst tolerance for one retry, five-minute client
polling, a 15-second request timeout, and 30-day encrypted-record retention
after device revocation. These values are hub configuration with conservative
hard ceilings in the protocol implementation.

## Synchronization behavior

Local history is always authoritative for the local machine. Valid peer
history is additive and never replaces scanner output. Peer rows are matched by
opaque account identity rather than display name. An account seen only on a peer
contributes a separately identifiable Total Spend slice without creating a
local provider card.

The client uploads after a completed refresh batch, a manual refresh, or a
provider-enablement change. It polls while synchronization is enabled and also
syncs on startup and network recovery. When the hub is unavailable, the client
retains only its newest pending envelope and retries with capped exponential
backoff and jitter.

Disabling a provider immediately removes its peer contribution from all local
surfaces and omits it from the next local envelope. Revoking a device removes
its contribution immediately for clients receiving the new device list; its
encrypted hub record is deleted after the configured retention interval.

Corrupt, stale, unauthenticated, future-schema, or oversized peer envelopes are
ignored. The app retains last-good peer history and surfaces a redacted
diagnostic. Sync failure never prevents launch, provider refresh, CLI use, or
the local HTTP API.

## Platform equivalence

The following mappings define parity for macOS-only capabilities:

| OpenUsage/macOS | OpenMeter/Windows |
|---|---|
| iCloud history sync | End-to-end encrypted Mini PC sync over Tailscale |
| Keychain | Windows Credential Manager |
| Login item | Per-user Windows autostart |
| Menu-bar capture signal | `WDA_EXCLUDEFROMCAPTURE` plus explicit privacy shortcut/mode |
| Sparkle stable/beta feeds | Tauri signed updater with stable and beta channels |
| Apple signing/notarization | Microsoft Trusted Signing and Authenticode verification |

Windows does not expose a dependable system-wide signal equivalent to the
macOS capture indicator. OpenMeter therefore excludes its popover from
supported capture paths and gives the user an immediate shortcut/manual mode
that replaces live tray values with neutral branding. This is the approved
functional privacy equivalent.

## Local API and compatibility

The API remains bound to `127.0.0.1:6736`. Both `/v1/limits` and the deprecated
`/v1/usage` read the same rendered account snapshots used by the UI. Family IDs
return every matching account; direct card IDs return that card; unknown IDs
retain the upstream status/exit behavior.

The secure Windows default sends no CORS header, preventing arbitrary web pages
from reading local usage. An explicitly labeled compatibility setting may emit
the upstream permissive CORS header for users who deliberately need browser
widgets. Native clients are unaffected by the default.

## Updates and trusted signing

GitHub Actions produces the release artifacts from public source. The pipeline
first signs the tray executable, CLI executable, and any Windows hub executable
with Microsoft Trusted Signing, verifies them, packages those signed inner
binaries into NSIS, signs the completed installer, and verifies its
Authenticode chain and timestamp on a clean Windows runner before publication.

Application-level updater signatures remain mandatory and independent of
Authenticode. Stable and beta manifests are separate; enabling beta adds
prereleases without excluding later stable releases. Published releases include
SHA-256 sums and build provenance.

Signing-account creation, Microsoft identity verification, Azure billing, and
the final credential/service connection require owner participation. The code
and workflow will fail closed when signing configuration is absent rather than
publishing a release claimed to be trusted.

## Privacy and logging

Logs redact access tokens, refresh tokens, API keys, device secrets, recovery
material, account identity values, user paths, and encrypted payloads. Account
and device references in diagnostics use short process-local or one-way
fingerprints. Pasted provider keys and sync secrets never appear in frontend
state after persistence.

No OneDrive, analytics SDK, public listener, automatic router configuration, or
third-party synchronization service is introduced. Existing opt-in aggregate
telemetry remains separate from synchronization and disabled by default.

## Failure handling

- Provider timeouts and errors retain last-good data with an Outdated marker.
- Cache identity mismatch discards the cache entry rather than displaying it
  under another account.
- Hub unavailability queues the newest envelope and backs off.
- Invalid peer history is ignored while last-good peer data remains visible.
- A local API port conflict is diagnostic-only and does not block the tray.
- Update, signature, or manifest validation failure aborts installation.
- Credential Manager failure disables the affected secure feature and presents
  a remediation message; secrets are not downgraded to plaintext storage.
- Schema migration writes atomically and retains a recoverable prior copy.

## Verification strategy

### Contract and unit tests

Tests cover provider fixtures, account discovery and identity, rename
resolution, account-stamped cache behavior, family/card CLI and API matching,
pricing, spend aggregation, payload filtering, deterministic serialization,
encryption, revision handling, conflicts, retention, and settings migrations.

### Windows integration tests

Tests cover Tauri's selected main binary, NSIS contents, per-user paths, PATH
registration, autostart, shortcuts, notifications, capture exclusion, privacy
mode, updater manifests, stable/beta selection, Authenticode signatures, and
uninstall behavior.

### Mini PC integration tests

Tests exercise real Tailscale connectivity, enrollment, authenticated encrypted
upload/download, SQLite restart persistence, simultaneous device revisions,
revocation, retention, invalid-data rejection, and interrupted-network
recovery. Test data contains no production credentials or history.

### Live acceptance on the target PC

The final signed installer must:

1. pass Authenticode validation and launch under Smart App Control;
2. install per-user outside OneDrive and start the tray application;
3. register a working `openmeter` CLI on the user's PATH;
4. render detected providers and complete bounded refreshes;
5. exercise dashboard, customization, share, pace, notification, privacy,
   CLI, API, proxy, stable/beta update, and multi-account flows;
6. synchronize encrypted history through the Mini PC over Tailscale;
7. recover from hub and network interruption without losing local data;
8. update through a signed release and restart cleanly; and
9. uninstall integrations without deleting user data unless explicitly chosen.

No capability is complete solely because it compiles or passes CI. Completion
requires the signed release to run and pass the live acceptance flow on this
machine.

## External prerequisites and stopping conditions

The owner has approved Microsoft Trusted Signing and the Mini PC/Tailscale sync
architecture. Implementation can proceed without further product decisions.
It must stop and request owner action only when Microsoft requires identity or
billing confirmation, when Tailscale/Mini PC access needs a new credential or
administrative change, or when a destructive migration is the only remaining
option.
