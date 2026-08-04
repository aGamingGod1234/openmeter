# OpenMeter Cross-Device Tracking Design

**Date:** 2026-08-04

**Status:** Approved in conversation; pending written-spec review

**Repository branch:** `feature/full-windows-parity`

**Target topology:** This Windows 11 PC and the Windows 11 desktop as tracking
clients, with the existing Mini PC at `100.90.87.7` as the private Tailscale
sync hub

## Objective

Extend OpenMeter's verified version-one encrypted history sync into a complete
cross-device tracking experience. The laptop and desktop must continue to work
independently while offline, synchronize automatically when Tailscale is
available, avoid double-counting shared usage records, and expose combined and
per-device tracking in the existing dashboard.

The Mini PC remains a storage and routing service. It must not learn provider
credentials, prompts, raw logs, account labels, model history in plaintext, or
the shared recovery key.

## Chosen approach

Add a version-two event ledger and quota-snapshot protocol alongside the
existing version-one daily aggregates. Each client publishes deterministic,
privacy-minimized usage facts and its latest normalized quota observations in
an encrypted envelope. New dashboard projections derive totals and graphs from
the union of unique event IDs rather than adding pre-aggregated peer totals.

Version one remains available during the rollout. The upgraded Mini PC hub
stores version-one and version-two envelopes independently, so an upgraded
client does not make an older client unusable and rollback does not require a
database restore.

Rejected alternatives:

- Adding daily totals from each machine cannot distinguish separate activity
  from the same activity discovered twice and therefore can double-count.
- Copying raw provider logs would simplify deduplication but would expose more
  private data than the dashboard needs.
- Making the Mini PC decrypt and aggregate history would centralize the
  recovery key and weaken the current end-to-end encryption boundary.

## Scope

The implementation will:

- track normalized token and estimated-cost events from both Windows clients;
- deduplicate the same provider event discovered on more than one client;
- synchronize normalized quota/reset observations without raw vendor payloads;
- render combined, per-device, and per-provider history views;
- show friendly encrypted device labels and synchronization freshness;
- retain offline local tracking and bounded retry behavior;
- migrate the hub and both clients without deleting version-one history;
- preserve storage outside OneDrive; and
- deploy through the existing Tailscale/LAN relay topology.

The implementation will not expose the hub publicly, synchronize credentials
or prompts, infer a device label from the Windows hostname, weaken Windows
security policy, or require OneDrive.

The feature covers the two approved tracking clients. The Mini PC runs only the
hub and is not counted as a tracking client unless OpenMeter is separately
installed there in a later request.

## Architecture

### Normalized event extraction

Provider log scanners will emit event-level normalized facts before reducing
them into the existing `DailySpend` projections. A version-two usage event
contains:

- opaque event ID;
- provider family and opaque account record ID;
- UTC occurrence time;
- model identifier;
- input, output, cached, reasoning, and total token counts when available;
- locally calculated cost and pricing version; and
- a bounded source kind needed to interpret incomplete fields.

No event contains prompt text, response text, provider access tokens, emails,
filesystem paths, raw JSON, or user-assigned account labels.

When a provider exposes a stable request, message, or session-event identifier,
the event ID is an HMAC of that identifier plus provider and account identity
using a key derived from the shared history key. When no native identifier is
available, the scanner builds a canonical fingerprint from the normalized
timestamp, provider, opaque account, model, token fields, cost fields, and the
source record's stable ordinal. The HMAC makes IDs comparable between enrolled
clients without revealing source identifiers to the hub.

Events from genuinely independent local records remain distinct. Identical
events discovered on both clients collapse to one event ID. A collision with
different normalized content is rejected and surfaced as a redacted sync
diagnostic rather than silently merged.

Providers that expose only a current aggregate and no event history do not
fabricate usage events. Their limits are represented only as quota snapshots.

### Quota snapshots

A quota snapshot contains provider family, opaque account record ID, metric
identity, normalized used/remaining/limit values, reset time, observation time,
and source device ID. It excludes the raw response, plan metadata not displayed
by OpenMeter, balances unrelated to the shown metric, and provider errors.

Quotas are state, not additive usage. The All Devices view groups snapshots by
provider, account, and metric and selects the freshest non-stale observation.
It displays the observing device and warns when fresh devices disagree beyond
the metric's configured tolerance. A device-filtered view displays only that
device's observation. Quota percentages are never summed across devices.

### Version-two encrypted payload

`TrackingPayloadV2` contains an encrypted device descriptor, a bounded rolling
event window, quota snapshots, and tombstones needed to remove events invalidated
by a corrected local scan. The descriptor contains the random device ID,
user-selected label, payload generation time, client version, and event cursor.

The default device labels for this rollout are `Laptop` and `Desktop`. They are
explicit configuration values, not automatically collected hostnames. Labels
are encrypted with the rest of the payload and are not routing metadata.

The payload is deterministically serialized, compressed with zstd, and
encrypted using XChaCha20-Poly1305. A separate subkey is derived from the
existing recovery key for version two. Envelope schema, device ID, revision,
and generation time remain authenticated associated data. Device credentials
and recovery material remain in Windows Credential Manager.

The rolling event window is 90 days, while dashboard defaults remain 30 days.
The hard decrypted-payload limit remains 8 MiB and the encrypted-envelope limit
remains 2 MiB. If the bounded payload would exceed the limit, the client drops
the oldest complete days first and reports the retained date range.

### Hub compatibility and storage

The existing version-one endpoints and SQLite rows remain unchanged. The hub
adds version-two envelope routes and a `tracking_envelopes` table keyed by
device ID. It validates routing metadata, authentication, exact revision
increments, body limits, and supported schema without decrypting payloads.

The database migration is additive and transactional. Before deployment, the
installer creates a timestamped copy of `C:\ProgramData\OpenMeterSync\hub.db`
in the same ACL-protected directory. A failed migration leaves the existing
service and version-one table usable.

One 15-minute, single-use enrollment token is created for the desktop. The
desktop imports the existing recovery key locally before enrollment. The key
must never be placed in a command, log, repository, clipboard capture, or chat.

### Client synchronization and merge

Each client owns an independent monotonically increasing version-two revision.
After a completed local refresh, it writes its newest encrypted envelope to the
local pending slot and attempts upload. Only the newest pending revision is
retained. Startup, manual Sync Now, network recovery, provider enablement
changes, and the existing periodic schedule trigger synchronization.

After authentication, a client downloads active peer envelopes, decrypts and
validates them, and replaces that peer's last-good version-two state atomically.
Invalid or future-schema peers are ignored without discarding prior valid
state. Revocation removes the peer projection immediately.

The merged tracking projection is the set union of local and active-peer events
by event ID. Tombstones take precedence over matching events. Account identity
controls provider/account grouping; device identity controls stacked series and
filters. Display labels never participate in identity or deduplication.

Version-one synchronized daily totals remain visible in a clearly labeled
`Legacy synced history` slice only until both tracking clients have published a
valid version-two envelope covering the corresponding date. Version-two events
then replace the overlapping legacy slice so totals are not counted twice.

## Dashboard experience

The existing provider cards and Total Spend card remain the primary dashboard.
Cross-device controls are added without creating a separate application.

### Global device filter

A compact filter above the tracking surfaces offers `All Devices`, `Laptop`,
and `Desktop`. The selection applies to Total Spend, Usage Trend, model totals,
and the new comparison chart. Provider cards remain locally actionable; when a
remote device is selected, quota cards are read-only and show their source and
freshness.

### Tracking graph

The Usage Trend surface gains:

- 7-day and 30-day ranges;
- Tokens and Spend metrics;
- stacked-by-device and stacked-by-provider grouping; and
- hover/focus details containing date, device, provider, model total, tokens,
  and estimated cost.

The graph uses the same deduplicated projection as Total Spend. Accessible text
summarizes the selected range, total, peak day, and series values. Empty and
offline states distinguish `no local records`, `device has not synced`, and
`hub unavailable`.

### Device status

Settings -> Sync lists the friendly label, opaque short device reference,
last-generated time, last-received time, client version, and state for each
device. States are `Current`, `Delayed`, `Offline`, `Needs upgrade`, and
`Revoked`. The UI does not reveal full device credentials or recovery material.

### Total Spend and quota display

Total Spend retains Today, Yesterday, and 30 Days and adds a Device/Provider
breakdown toggle. Synced event history contributes to all three ranges. Copy as
image includes the active device filter and a `Synced through Mini PC` caption
without exposing network addresses.

The All Devices quota display chooses the freshest observation per stable
account metric. Device-specific views expose that machine's observation and
last-seen time. A provider unavailable on one device does not erase a fresher
observation from the other.

## Privacy and security

- The Mini PC sees random device IDs, schema/revision timestamps, ciphertext
  size, and authenticated request timing only.
- Friendly device labels, event IDs, provider/model facts, quota values, and
  tracking history are encrypted.
- Event identifiers are keyed HMAC values and cannot be correlated outside the
  recovery-key group.
- The hub binds only to `100.90.87.7:6740` and retains the existing tailnet-only
  firewall rule.
- Raw logs and provider credentials never leave the originating client.
- Sync diagnostics redact device credentials, the recovery key, event IDs,
  account identity stamps, user paths, and envelope contents.
- A client with only an enrollment credential cannot decrypt any history.
- Revoking a device prevents future reads and writes; active clients remove its
  contribution on their next successful device-list refresh.

## Migration and rollback

Deployment order is hub, laptop, then desktop.

1. Verify and copy the hub database and pepper inside the protected data
   directory.
2. Stop the hub service, install the compatible binary, perform the additive
   migration, restart, and verify version-one plus version-two health.
3. Install the upgraded client on the laptop and confirm its existing local and
   version-one history is unchanged.
4. Publish the laptop's first version-two envelope and verify the hub still
   serves version one.
5. Install OpenMeter on the desktop outside OneDrive.
6. Import the recovery key through the desktop UI, create a fresh enrollment
   token on the Mini PC, enroll, and publish the desktop's first envelope.
7. Verify overlap replacement, unique-event union, per-device views, and quota
   freshness from both clients.

Client rollback reinstalls the previous client and continues version-one sync;
version-two local state remains encrypted and ignored. Hub rollback restores
the previous executable without deleting the additive table. The database copy
is used only if the additive migration itself is corrupt. No rollback step
deletes local logs, local caches, Credential Manager entries, or hub history.

## Error handling

- Hub or Tailscale loss leaves local tracking fully operational and queues only
  the newest encrypted revision.
- A duplicate event ID with different content quarantines that peer revision
  and retains its last-good state.
- A bad quota snapshot is dropped independently from valid usage events.
- Clock skew uses server receipt time for freshness classification but never
  rewrites event occurrence time.
- Partial provider scans publish only completed provider results and retain the
  prior event cursor for failed providers.
- A version mismatch displays `Needs upgrade` and preserves version-one
  compatibility.
- Credential Manager failure disables synchronization without writing secrets
  to application configuration.
- Database migration or service-start failure restores the prior hub binary and
  leaves the version-one endpoint available.

## Verification strategy

### Protocol and merge tests

Tests cover deterministic cross-device event IDs, distinct-event preservation,
same-event deduplication, collision quarantine, tombstones, payload limits,
versioned subkey derivation, authenticated metadata, corrupt ciphertext,
legacy overlap replacement, quota freshness, disagreement handling, and clock
skew.

### Hub tests

Tests cover additive SQLite migration, simultaneous version-one and version-two
storage, independent revisions, enrollment, authentication, replay rejection,
revocation, restart persistence, size/rate limits, and rollback to the prior
binary while version-one data remains readable.

### Dashboard tests

Tests cover All Devices and individual filters, token/spend selection, 7/30-day
ranges, device/provider stacking, accessible graph summaries, empty/offline
states, freshest-quota selection, disagreement warnings, legacy labeling, copy
metadata, and device status states.

### Live three-machine acceptance

Acceptance is complete only when:

1. the Mini PC service is running at the exact Tailscale address after restart;
2. the laptop and desktop each have a unique active device identity;
3. both clients retain local functionality with the hub stopped;
4. a synthetic duplicate fixture contributes once across both clients;
5. unique local activity contributes to the correct device series and combined
   total;
6. quota observations select the freshest device without summing;
7. this PC and the desktop render the same combined totals after synchronization;
8. a revoked test device disappears from projections;
9. neither client stores OpenMeter data under OneDrive;
10. hub database inspection exposes routing metadata and ciphertext but no
    provider, model, device-label, username, or path plaintext; and
11. restoring the previous client and hub binaries leaves version-one sync and
    local tracking operational.

Production history is never altered to manufacture acceptance data. Synthetic
fixtures use separate test device identities and are revoked after validation.

## Deployment authority and stopping conditions

The owner authorized implementation and deployment to this PC, the Mini PC,
and the desktop. The implementation may build, install, upgrade, restart the
OpenMeter clients and `OpenMeterSyncHub`, create a desktop enrollment token,
and migrate the hub database as described above. It may not disable Windows
security controls, expose the service publicly, delete history, merge to the
default branch, publish a release, or transmit the recovery key.

Stop for owner input if the saved recovery key is unavailable at desktop
enrollment, the verified machine identity differs from the documented desktop,
the database backup cannot be verified, a destructive migration becomes
necessary, or authentication requires interactive credential entry.
