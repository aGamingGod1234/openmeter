param(
    [switch]$StaticOnly,
    [string]$HubUrl = 'http://100.90.87.7:6740',
    [string]$AdminBinary = 'C:\Program Files\OpenMeter Sync Hub\openmeter-sync-hub.exe',
    [string]$FixtureGeneratorPath = '.\tracking_fixture.exe',
    [string]$DatabasePath = 'C:\ProgramData\OpenMeterSync\hub.db',
    [string]$PepperPath = 'C:\ProgramData\OpenMeterSync\pepper.bin'
)

$ErrorActionPreference = 'Stop'
if ($StaticOnly) { exit 0 }

foreach ($path in @($AdminBinary, $FixtureGeneratorPath, $DatabasePath, $PepperPath)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing acceptance input: $path" }
}

function New-SyntheticDevice {
    $token = (& $AdminBinary enrollment create --database $DatabasePath --pepper-file $PepperPath | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($token)) {
        throw 'Could not create a temporary enrollment token.'
    }
    Invoke-RestMethod -Uri "$HubUrl/v1/enroll" -Method Post -ContentType 'application/json' `
        -Body (@{ token = $token } | ConvertTo-Json -Compress) -TimeoutSec 15
}

function Invoke-OpaqueUpload {
    param([string]$Path, [string]$Credential, [object]$Envelope)
    $response = Invoke-WebRequest -UseBasicParsing -Uri "$HubUrl$Path" -Method Put `
        -Headers @{ Authorization = "Bearer $Credential" } -ContentType 'application/json' `
        -Body ($Envelope | ConvertTo-Json -Depth 8 -Compress) -TimeoutSec 15
    if ($response.StatusCode -ne 204) { throw "Opaque upload failed for $Path" }
}

$first = $null
$second = $null
try {
    $first = New-SyntheticDevice
    $second = New-SyntheticDevice
    $fixtureText = (& $FixtureGeneratorPath $first.device_id $second.device_id | Out-String).Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($fixtureText)) {
        throw 'Rust tracking fixture generator failed.'
    }
    $fixtures = $fixtureText | ConvertFrom-Json

    # Version one and version two use independent revisions and storage.
    $v1 = @{
        meta = @{
            schema = 'openmeter.history.v1'
            device_id = $first.device_id
            revision = 1
            generated_at_ms = [DateTimeOffset]::UtcNow.ToUnixTimeMilliseconds()
        }
        nonce = [Convert]::ToBase64String([byte[]](1..24))
        ciphertext = [Convert]::ToBase64String([byte[]](1..32))
    }
    Invoke-OpaqueUpload -Path "/v1/devices/$($first.device_id)/envelope" `
        -Credential $first.credential -Envelope $v1
    Invoke-OpaqueUpload -Path "/v2/devices/$($first.device_id)/envelope" `
        -Credential $first.credential -Envelope $fixtures.envelope_a
    Invoke-OpaqueUpload -Path "/v2/devices/$($second.device_id)/envelope" `
        -Credential $second.credential -Envelope $fixtures.envelope_b

    $secondHeaders = @{ Authorization = "Bearer $($second.credential)" }
    $v1Records = @(Invoke-RestMethod -Uri "$HubUrl/v1/envelopes" -Headers $secondHeaders -TimeoutSec 15)
    if ($v1Records.Count -ne 1) { throw 'Version-one envelope was not independently visible.' }

    Restart-Service -Name 'OpenMeterSyncHub' -Force
    (Get-Service -Name 'OpenMeterSyncHub').WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
    $firstHeaders = @{ Authorization = "Bearer $($first.credential)" }
    $fromFirst = @(Invoke-RestMethod -Uri "$HubUrl/v2/envelopes" -Headers $firstHeaders -TimeoutSec 15)
    $fromSecond = @(Invoke-RestMethod -Uri "$HubUrl/v2/envelopes" -Headers $secondHeaders -TimeoutSec 15)
    if ($fromFirst.Count -ne 1 -or $fromSecond.Count -ne 1) {
        throw 'Both synthetic version-two envelopes did not survive restart.'
    }
    if ($fixtures.expected_unique_events -ne 2) {
        throw 'Fixture does not encode one duplicate plus one unique event.'
    }
    Write-Host 'Synthetic dual-protocol, two-device, restart, duplicate, and unique-event acceptance passed.'
} finally {
    foreach ($device in @($first, $second)) {
        if ($null -ne $device -and -not [string]::IsNullOrWhiteSpace($device.device_id)) {
            & $AdminBinary device revoke --database $DatabasePath --id $device.device_id | Out-Null
            if ($LASTEXITCODE -ne 0) { Write-Warning "Could not revoke synthetic device $($device.device_id)." }
        }
    }
    $first = $null
    $second = $null
}
