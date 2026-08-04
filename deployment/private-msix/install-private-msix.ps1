[CmdletBinding()]
param(
    [string]$PackagePath,
    [string]$CertificatePath,
    [string]$ExpectedThumbprint,
    [int]$HealthTimeoutSeconds = 45
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
$metadataPath = Join-Path $repoRoot 'artifacts\private-msix\deployment.json'
if (-not $PackagePath -or -not $CertificatePath -or -not $ExpectedThumbprint) {
    if (-not (Test-Path -LiteralPath $metadataPath -PathType Leaf)) {
        throw "Private deployment metadata is missing: $metadataPath"
    }
    $metadata = Get-Content -Raw -LiteralPath $metadataPath | ConvertFrom-Json
    if (-not $PackagePath) { $PackagePath = [string]$metadata.package }
    if (-not $CertificatePath) { $CertificatePath = [string]$metadata.certificate }
    if (-not $ExpectedThumbprint) { $ExpectedThumbprint = [string]$metadata.thumbprint }
}

Import-Module (Join-Path $PSScriptRoot 'PrivateMsix.psm1') -Force
$legacyPath = Join-Path $env:LOCALAPPDATA 'OpenMeter\openmeter-tray.exe'
Remove-OpenMeterLegacyStartup -ExpectedPath $legacyPath | Out-Null
Get-Process -Name 'openmeter-tray' -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -and $_.Path.Equals($legacyPath, [StringComparison]::OrdinalIgnoreCase) } |
    Stop-Process -Force

$trustPath = "Cert:\LocalMachine\TrustedPeople\$($ExpectedThumbprint.ToUpperInvariant())"
$addedMachineTrust = -not (Test-Path -LiteralPath $trustPath -PathType Leaf)
$trustHelper = Join-Path $PSScriptRoot 'set-machine-trust.ps1'
if ($addedMachineTrust) {
    $trustProcess = Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
        ('"{0}"' -f $trustHelper),
        '-CertificatePath', ('"{0}"' -f $CertificatePath),
        '-ExpectedThumbprint', $ExpectedThumbprint
    )
    if ($trustProcess.ExitCode -ne 0 -or -not (Test-Path -LiteralPath $trustPath -PathType Leaf)) {
        throw 'Administrator approval did not establish OpenMeter machine trust.'
    }
}

try {
    $result = Install-OpenMeterPrivateMsix `
        -PackagePath $PackagePath `
        -CertificatePath $CertificatePath `
        -ExpectedThumbprint $ExpectedThumbprint `
        -HealthTimeoutSeconds $HealthTimeoutSeconds
}
catch {
    if ($addedMachineTrust -and (Test-Path -LiteralPath $trustPath -PathType Leaf)) {
        Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -ArgumentList @(
            '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
            ('"{0}"' -f $trustHelper),
            '-ExpectedThumbprint', $ExpectedThumbprint,
            '-Remove'
        )
    }
    throw
}

$acceptancePath = Join-Path (Split-Path -Parent $PackagePath) 'acceptance.json'
[ordered]@{
    verifiedAtUtc = [DateTime]::UtcNow.ToString('o')
    packageFullName = $result.PackageFullName
    packageFamilyName = $result.PackageFamilyName
    installLocation = $result.InstallLocation
    certificateThumbprint = $result.Thumbprint
    certificateStore = 'LocalMachine\TrustedPeople'
    localApiStatus = $result.ApiStatus
    dataRoot = $result.DataRoot
} | ConvertTo-Json | Set-Content -LiteralPath $acceptancePath -Encoding UTF8

Write-Host "OpenMeter private MSIX installed: $($result.PackageFullName)"
Write-Host "Local API: HTTP $($result.ApiStatus)"
Write-Host "Acceptance evidence: $acceptancePath"
