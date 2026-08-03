param(
    [switch]$StaticOnly,
    [string]$HubUrl = 'http://100.90.87.7:6740',
    [string]$AdminBinary = 'C:\Program Files\OpenMeter Sync Hub\openmeter-sync-hub.exe',
    [string]$DatabasePath = 'C:\ProgramData\OpenMeterSync\hub.db',
    [string]$PepperPath = 'C:\ProgramData\OpenMeterSync\pepper.bin'
)

$ErrorActionPreference = 'Stop'
$installerPath = Join-Path $PSScriptRoot 'install-sync-hub.ps1'
$uninstallerPath = Join-Path $PSScriptRoot 'uninstall-sync-hub.ps1'
if (-not (Test-Path -LiteralPath $installerPath)) { throw 'Sync hub installer is missing' }
if (-not (Test-Path -LiteralPath $uninstallerPath)) { throw 'Sync hub uninstaller is missing' }

$installer = Get-Content -LiteralPath $installerPath -Raw
$liveTest = Get-Content -LiteralPath $PSCommandPath -Raw
if ($installer -notmatch '100\.90\.87\.7:6740') { throw 'Hub must bind its exact Tailscale address' }
if ($installer -match '0\.0\.0\.0|AnyAddress|OneDrive') { throw 'Unsafe hub binding or storage path' }
if ($installer -notmatch 'OpenMeterSyncHub') { throw 'Stable service identity missing' }
if ($installer -notmatch 'NT AUTHORITY\\LocalService') { throw 'Service must run as LocalService' }
if ($installer -notmatch 'C:\\ProgramData\\OpenMeterSync') { throw 'ProgramData storage path missing' }
if ($installer -notmatch '-RemoteAddress\s+[''\"]?100\.64\.0\.0/10') { throw 'Firewall must be tailnet-scoped' }
if ($installer -match 'RandomNumberGenerator\]::Fill' -or $liveTest -match 'RandomNumberGenerator\]::Fill') {
    throw 'Hub scripts must support inbox Windows PowerShell cryptography APIs'
}
if ($installer -notmatch 'sc\.exe create \$ServiceName ''binPath='' \$serviceCommand') {
    throw 'SCM options and values must be passed as separate PowerShell arguments'
}

if ($StaticOnly) { exit 0 }

$health = Invoke-RestMethod -Uri "$HubUrl/health" -Method Get -TimeoutSec 15
if ($null -ne $health -and $health.protocol -ne 1) { throw 'Unexpected hub protocol' }

foreach ($path in @($AdminBinary, $DatabasePath, $PepperPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing live hub file: $path" }
}

function New-TestDevice {
    $token = (& $AdminBinary enrollment create --database $DatabasePath --pepper-file $PepperPath | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($token)) {
        throw 'Could not create a live enrollment token.'
    }
    $body = @{ token = $token } | ConvertTo-Json -Compress
    Invoke-RestMethod -Uri "$HubUrl/v1/enroll" -Method Post -ContentType 'application/json' -Body $body -TimeoutSec 15
}

$first = New-TestDevice
$second = New-TestDevice
$generatedAt = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
$nonce = [Convert]::ToBase64String([byte[]](1..24))
$ciphertextBytes = [byte[]]::new(64)
$rng = [Security.Cryptography.RandomNumberGenerator]::Create()
try { $rng.GetBytes($ciphertextBytes) } finally { $rng.Dispose() }
$ciphertext = [Convert]::ToBase64String($ciphertextBytes)
[Array]::Clear($ciphertextBytes, 0, $ciphertextBytes.Length)
$envelope = @{
    meta = @{
        schema = 'openmeter.history.v1'
        device_id = $first.device_id
        revision = 1
        generated_at_ms = $generatedAt
    }
    nonce = $nonce
    ciphertext = $ciphertext
} | ConvertTo-Json -Depth 4 -Compress
$firstHeaders = @{ Authorization = "Bearer $($first.credential)" }
$secondHeaders = @{ Authorization = "Bearer $($second.credential)" }

$upload = Invoke-WebRequest -UseBasicParsing -Uri "$HubUrl/v1/devices/$($first.device_id)/envelope" `
    -Method Put -Headers $firstHeaders -ContentType 'application/json' -Body $envelope -TimeoutSec 15
if ($upload.StatusCode -ne 204) { throw 'Authenticated opaque upload failed.' }

Restart-Service -Name 'OpenMeterSyncHub' -Force
(Get-Service -Name 'OpenMeterSyncHub').WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
$afterRestart = @(Invoke-RestMethod -Uri "$HubUrl/v1/envelopes" -Method Get -Headers $secondHeaders -TimeoutSec 15)
if ($afterRestart.Count -ne 1 -or $afterRestart[0].ciphertext -ne $ciphertext) {
    throw 'Opaque envelope did not survive service restart.'
}

$revoked = Invoke-WebRequest -UseBasicParsing -Uri "$HubUrl/v1/devices/$($first.device_id)" `
    -Method Delete -Headers $firstHeaders -TimeoutSec 15
if ($revoked.StatusCode -ne 204) { throw 'Device revocation failed.' }
$afterRevoke = @(Invoke-RestMethod -Uri "$HubUrl/v1/envelopes" -Method Get -Headers $secondHeaders -TimeoutSec 15)
if ($afterRevoke.Count -ne 0) { throw 'Revoked envelope remains visible.' }

Invoke-WebRequest -UseBasicParsing -Uri "$HubUrl/v1/devices/$($second.device_id)" `
    -Method Delete -Headers $secondHeaders -TimeoutSec 15 | Out-Null
$first = $null
$second = $null
Write-Host 'OpenMeter Sync Hub health, enrollment, authentication, persistence, opaque transfer, and revocation checks passed.'
