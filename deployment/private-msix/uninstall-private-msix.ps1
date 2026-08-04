[CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'Medium')]
param(
    [switch]$RemoveCertificate,
    [string]$ExpectedThumbprint
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if ($RemoveCertificate -and -not $ExpectedThumbprint) {
    $metadataPath = Join-Path $repoRoot 'artifacts\private-msix\deployment.json'
    if (-not (Test-Path -LiteralPath $metadataPath -PathType Leaf)) {
        throw 'ExpectedThumbprint is required when deployment metadata is unavailable.'
    }
    $ExpectedThumbprint = [string](Get-Content -Raw -LiteralPath $metadataPath | ConvertFrom-Json).thumbprint
}

$packages = @(Get-AppxPackage -Name 'OpenMeter.Private' -ErrorAction SilentlyContinue)
foreach ($package in $packages) {
    if ($PSCmdlet.ShouldProcess($package.PackageFullName, 'Remove OpenMeter private MSIX')) {
        Get-Process -Name 'openmeter-tray' -ErrorAction SilentlyContinue | Stop-Process -Force
        Remove-AppxPackage -Package $package.PackageFullName
    }
}

if ($RemoveCertificate) {
    if ($ExpectedThumbprint -notmatch '^[0-9A-Fa-f]{40}$') { throw 'ExpectedThumbprint is invalid.' }
    $trustPath = "Cert:\LocalMachine\TrustedPeople\$($ExpectedThumbprint.ToUpperInvariant())"
    if ((Test-Path -LiteralPath $trustPath) -and $PSCmdlet.ShouldProcess($trustPath, 'Remove OpenMeter private trust certificate')) {
        $trustHelper = Join-Path $PSScriptRoot 'set-machine-trust.ps1'
        $process = Start-Process powershell.exe -Verb RunAs -WindowStyle Hidden -Wait -PassThru -ArgumentList @(
            '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File',
            ('"{0}"' -f $trustHelper),
            '-ExpectedThumbprint', $ExpectedThumbprint,
            '-Remove'
        )
        if ($process.ExitCode -ne 0) { throw 'Administrator certificate removal failed.' }
    }
}

Write-Host 'OpenMeter private package removal completed. User data was preserved.'
