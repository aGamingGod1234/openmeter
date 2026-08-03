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

Paste that token into OpenMeter Settings → Sync. Save the display-once recovery key somewhere secure. Import the same recovery key before enrolling another PC.

## Verify and remove

```powershell
.\scripts\test-sync-hub.ps1
.\scripts\uninstall-sync-hub.ps1
```

Uninstall preserves encrypted data by default. Add `-RemoveData` only when the database and pepper should be permanently removed.
