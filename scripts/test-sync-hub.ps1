param(
    [switch]$StaticOnly,
    [string]$HubUrl = 'http://100.90.87.7:6740'
)

$ErrorActionPreference = 'Stop'
$installerPath = Join-Path $PSScriptRoot 'install-sync-hub.ps1'
$uninstallerPath = Join-Path $PSScriptRoot 'uninstall-sync-hub.ps1'
if (-not (Test-Path -LiteralPath $installerPath)) { throw 'Sync hub installer is missing' }
if (-not (Test-Path -LiteralPath $uninstallerPath)) { throw 'Sync hub uninstaller is missing' }

$installer = Get-Content -LiteralPath $installerPath -Raw
if ($installer -notmatch '100\.90\.87\.7:6740') { throw 'Hub must bind its exact Tailscale address' }
if ($installer -match '0\.0\.0\.0|AnyAddress|OneDrive') { throw 'Unsafe hub binding or storage path' }
if ($installer -notmatch 'OpenMeterSyncHub') { throw 'Stable service identity missing' }
if ($installer -notmatch 'NT AUTHORITY\\LocalService') { throw 'Service must run as LocalService' }
if ($installer -notmatch 'C:\\ProgramData\\OpenMeterSync') { throw 'ProgramData storage path missing' }
if ($installer -notmatch 'RemoteAddress\s+100\.64\.0\.0/10') { throw 'Firewall must be tailnet-scoped' }

if ($StaticOnly) { exit 0 }

$health = Invoke-RestMethod -Uri "$HubUrl/health" -Method Get -TimeoutSec 15
if ($null -ne $health -and $health.protocol -ne 1) { throw 'Unexpected hub protocol' }
Write-Host 'OpenMeter Sync Hub health check passed.'
