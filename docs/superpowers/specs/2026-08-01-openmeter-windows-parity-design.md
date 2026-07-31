# OpenMeter Windows Parity Design

## Objective

OpenMeter is a Windows 11 x64 tray application derived from Pane 0.4.28 and designed to match the user-visible behavior and machine-readable contracts of OpenUsage at commit `9d2bf09`. It keeps Pane's working Windows integrations and additional providers while closing current OpenUsage parity gaps. The distributable uses its own name, package identity, settings directory, updater key, and public GitHub release channel.

## Constraints

- Support Windows 11 x64 on the user's current machine only.
- Keep settings and usage history local. Do not add OneDrive, iCloud, or another cloud synchronization service.
- Use Windows-native equivalents for macOS-only integrations.
- Preserve clear MIT attribution to Pane, OpenUsage, and their contributors.
- Never expose credentials in logs, diagnostics, caches, CLI output, or local HTTP responses.
- Make future OpenUsage changes classifiable and portable without coupling the Windows shell to provider internals.
- Verify provider behavior with fixtures and live credentials already present on the machine, without printing secrets.

## Source Baselines

- Windows base: `ItsJazii/pane` commit `272a6ae` (Pane 0.4.28).
- parity baseline: `robinebers/openusage` commit `9d2bf09`.
- reference port: CrossUsage 1.3.3 for independently implemented Windows and Linux provider behavior.

Pane is the execution base because it already supplies the Windows tray shell, Tauri updater, autostart, global shortcut, notification, spend, share-card, and provider integrations. OpenUsage remains the behavioral source of truth for its documented providers, cache semantics, CLI/API contracts, and current account-first work.

## System Boundaries

### Account registry

An `AccountRegistry` owns stable provider-account identities. Each account has an ID, provider ID, display name, source descriptor, and enabled state. It never contains raw credentials. The default account retains the plain provider ID for compatibility; additional accounts use a deterministic qualified ID. Registry persistence is separate from layout persistence so reordering or hiding a card cannot alter credential identity.

### Credential resolution

Each provider receives an `AccountContext` and asks its credential resolver for the selected source. Resolution order is existing CLI/editor state, Windows Credential Manager or DPAPI-protected app storage, environment variables, then a user-supplied key when supported. Credential values remain inside the provider request boundary. Account probing uses the same resolver as refresh so first-run detection cannot disagree with live behavior.

### Provider runtime

Every provider produces a normalized `ProviderSnapshot` containing account identity, provider metadata, typed metrics, warnings, fetch time, and cache status. Provider clients do not render UI and do not serialize API responses. Refreshes execute concurrently per account, use provider-specific cooldowns, and preserve the last good account-stamped snapshot after transient failure.

OpenMeter guarantees support for all providers in the parity baseline: Antigravity, Claude, Codex, Copilot, Cursor, Devin, Grok, OpenCode, OpenRouter, and Z.ai. Pane-only providers remain available and use the same runtime boundary.

### Cache and refresh coordinator

The coordinator is the single refresh path for the GUI, CLI `--force`, tray strip, and local API. Cache keys include provider ID, account ID, credential-source stamp, and schema version. Cached snapshots are shown immediately and revalidated on the existing five-minute schedule. Force refresh bypasses freshness but still respects safety cooldowns imposed by vendor rate limits.

### Presentation and persistence

The Tauri frontend consumes normalized snapshots only. It retains Pane's dashboard, Total Spend ring, usage trends, pace indicators, reset toggles, quick links, share cards, and drag behavior. Customize persists provider/account order, metric order, visibility, Always Visible/On Demand membership, and up to two starred metrics per card. Settings persist under `%APPDATA%\OpenMeter`; secrets use Windows-protected storage rather than the settings JSON.

### Machine-readable interfaces

The Rust serializer is shared by:

- `GET /v1/usage` and `GET /v1/usage/:id`, retaining the legacy UI contract.
- `GET /v1/limits`, providing the stable OpenUsage limits contract.
- `openmeter` CLI output, including cached default operation and `--force` refresh.

The HTTP server binds only to `127.0.0.1:6736`, emits no credential material, and does not opt into browser-readable CORS. A port conflict is logged and surfaced in diagnostics without preventing the tray app from starting.

## Windows Platform Behavior

- A single process owns the tray icon and popover.
- The tray click and configurable global shortcut toggle the same focused popover.
- Popover placement respects the taskbar edge and active monitor.
- Startup uses the current-user Startup Apps registration.
- Alerts use Windows toast notifications and retain once-per-reset-window suppression.
- App-managed secrets use user-scoped Windows protection.
- Share cards copy PNG bytes directly to the Windows clipboard.
- `SetWindowDisplayAffinity(WDA_EXCLUDEFROMCAPTURE)` excludes the popover from supported capture paths. An explicit privacy-mode shortcut hides live tray numbers because Windows exposes no reliable cross-application "screen sharing started" signal.
- Updates are downloaded from the OpenMeter GitHub release channel and verified with an OpenMeter-specific Tauri updater public key.

## Branding and Distribution

The application name is OpenMeter. Its Rust crate, executable, Tauri product name, application identifier, autostart identity, settings directory, asset names, installer name, update URLs, updater key, README, privacy document, and UI copy must not reuse Pane's operational identity. Attribution remains visible in README, About, LICENSE notices, and release documentation.

GitHub Actions builds the Windows x64 NSIS installer from a tag, runs the complete test suite first, emits SHA-256 digests and Tauri updater signatures, and publishes build provenance. Authenticode signing is optional until the repository has an appropriate certificate; updater verification is mandatory from the first release.

## Failure Handling and Privacy

External failures are classified as missing credentials, expired authentication, vendor throttling, vendor outage, network/proxy failure, malformed response, or local-source failure. The UI shows a friendly account-specific action while retaining last-good data with an Outdated marker. Unexpected internal failures are logged loudly and never converted into fabricated usage data.

Redaction tests cover authorization headers, tokens, cookies, API keys, user paths, query secrets, and provider response fields. Telemetry is disabled by default for this personal build. If retained later, it requires an explicit opt-in and a separately approved privacy contract.

## Verification Strategy

Rust unit and fixture tests cover account identity, credential-source stamps, cache isolation, refresh coordination, provider mapping, pricing, log scanning, cooldowns, redaction, and serializers. TypeScript tests cover layout migration, account cards, Customize operations, Settings, share cards, and error states. Contract fixtures verify `/v1/usage`, `/v1/limits`, and CLI JSON against the documented OpenUsage shapes.

Windows integration checks cover single instance, tray toggle, shortcut registration, Startup Apps, notifications, protected secrets, local API routing, capture exclusion, clean install, settings-preserving upgrade, and uninstall. Live smoke checks run only for locally authenticated providers and report provider/account success without credential values.

The current machine's Smart App Control policy blocks Cargo-generated unsigned build scripts (`os error 4551`). Local frontend builds remain valid; Rust, installer, and integration verification therefore run on the public repository's GitHub-hosted Windows runner, followed by installation of the produced artifact on this machine.

## Delivery Slices

1. Identity and contract foundation: OpenMeter branding, independent persistence, account model, shared serializers, `/v1/limits`, and CLI skeleton.
2. Account-aware refresh: registry, cache stamps, provider-runtime boundary, default-account migration, and concurrent per-account refresh.
3. Provider parity: port baseline behavior and fixtures for every OpenUsage provider, then adapt Pane-only providers.
4. UI parity: account cards, Customize, Settings, share behavior, tray pins, privacy mode, and diagnostics.
5. Distribution: public fork, CI quality gates, updater identity, installer/update smoke tests, and machine installation.

Each slice must leave a buildable, testable application and may not defer a requirement to an unspecified future task.
