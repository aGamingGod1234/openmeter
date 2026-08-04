# OpenMeter private sync hub

The hub is a Windows service on the Mini PC at `100.90.87.7:6740`. It stores authenticated, encrypted envelopes in `C:\ProgramData\OpenMeterSync\hub.db`; it never receives the history recovery key, provider credentials, account labels, raw logs, errors, or filesystem paths.

## Install on the Mini PC

From an elevated PowerShell window, build or download `openmeter-sync-hub.exe`, then run:

```powershell
.\scripts\install-sync-hub.ps1 -BinaryPath .\openmeter-sync-hub.exe
```

The installer creates `OpenMeterSyncHub` as an automatic delayed-start service under `NT AUTHORITY\LocalService`, locks the data directory and 32-byte server pepper to LocalService/Administrators/System, and permits TCP 6740 only on the Tailscale address and tailnet source range.

Create a 15-minute single-use enrollment token locally on the Mini PC:

```powershell
& 'C:\Program Files\OpenMeter Sync Hub\openmeter-sync-hub.exe' enrollment create --database C:\ProgramData\OpenMeterSync\hub.db --pepper-file C:\ProgramData\OpenMeterSync\pepper.bin
```

Paste that token into OpenMeter Settings -> Sync. Save the display-once recovery key somewhere secure. Import the same recovery key before enrolling another PC.

For the intended three-device layout, set the local labels to `Laptop` and `Desktop`. Create a fresh 15-minute, single-use enrollment token for each device. Import the saved recovery key only through each device's OpenMeter Settings -> Sync screen; never place it in PowerShell, SSH, logs, or the hub. The hub receives encrypted version-one history and version-two tracking envelopes independently.

The tracking dashboard provides All Devices, Laptop, and Desktop filters; 7/30-day Tokens/Spend graphs; Device/Provider grouping; freshest quota observations; and Current/Delayed/Offline/Needs upgrade device health. A client keeps its encrypted last-good peer cache during hub downtime and publishes the newest pending envelope after connectivity returns.

## Recoverable upgrade and rollback

An in-place install stops the service before copying the database, writes a timestamped backup to `C:\ProgramData\OpenMeterSync\backups\hub-<UTC timestamp>.db`, verifies its SHA-256 against the stopped database, and opens it through the new hub binary's read-only integrity check. The existing `pepper.bin` hash must remain unchanged. The previous executable is retained as `C:\Program Files\OpenMeter Sync Hub\openmeter-sync-hub.previous.exe`.

If live acceptance fails, stop `OpenMeterSyncHub`, copy the previous executable back over `openmeter-sync-hub.exe`, and start the service. Never delete or replace `hub.db`, `pepper.bin`, or the backups directory during rollback. Verify `/health`, `/v1/envelopes`, and `/v2/envelopes` after restart.

Build the disposable Rust fixture generator and run dual-device acceptance on the Mini PC without using production identities or recovery material:

```powershell
cargo build --manifest-path crates/openmeter-sync-protocol/Cargo.toml --example tracking_fixture --release
.\scripts\test-cross-device-sync.ps1 -FixtureGeneratorPath .\crates\openmeter-sync-protocol\target\release\examples\tracking_fixture.exe
```

The acceptance script enrolls two temporary identities, uploads independent v1/v2 records with one duplicate and one unique normalized event, restarts the service, verifies both encrypted v2 records, and revokes the temporary devices.

## Verify and remove

```powershell
.\scripts\test-sync-hub.ps1
.\scripts\uninstall-sync-hub.ps1
```

Uninstall preserves encrypted data by default. Add `-RemoveData` only when the database and pepper should be permanently removed.
