[CmdletBinding()]
param(
    [string]$OutputRoot,
    [string]$Subject = 'CN=OpenMeter Private Deployment',
    [string]$PrebuiltRoot,
    [ValidateRange(0, 65535)][int]$PackageRevision = 2
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$env:WINAPP_CLI_TELEMETRY_OPTOUT = '1'

$repoRoot = Split-Path -Parent (Split-Path -Parent $PSScriptRoot)
if (-not $OutputRoot) {
    $OutputRoot = Join-Path $repoRoot 'artifacts\private-msix'
}
$OutputRoot = [System.IO.Path]::GetFullPath($OutputRoot)
if ($OutputRoot -like "$env:OneDrive*") {
    throw "Private deployment output cannot be placed in OneDrive: $OutputRoot"
}

Import-Module (Join-Path $PSScriptRoot 'PrivateMsix.psm1') -Force
if (-not (Get-Command winapp.exe -ErrorAction SilentlyContinue)) {
    throw 'Windows App Development CLI is required. Install Microsoft.WinAppCli with WinGet.'
}

$config = Get-Content -Raw -LiteralPath (Join-Path $repoRoot 'src-tauri\tauri.conf.json') | ConvertFrom-Json
$parts = @([string]$config.version -split '\.')
if ($parts.Count -ne 3 -or ($parts | Where-Object { $_ -notmatch '^\d+$' })) {
    throw "Tauri version must be numeric SemVer: $($config.version)"
}
$msixVersion = '{0}.{1}.{2}.{3}' -f $parts[0], $parts[1], $parts[2], $PackageRevision
$certificate = Get-OrCreateOpenMeterSigningCertificate -Subject $Subject

$binaries = @{}
if ($PrebuiltRoot) {
    $PrebuiltRoot = [System.IO.Path]::GetFullPath($PrebuiltRoot)
    foreach ($path in @(Assert-OpenMeterPrebuiltSet -Root $PrebuiltRoot)) {
        $binaries[(Split-Path -Leaf $path)] = $path
    }
}
else {
    Push-Location $repoRoot
    try {
        & npm.cmd run build
        if ($LASTEXITCODE -ne 0) { throw "Frontend build failed with exit code $LASTEXITCODE." }

        & cargo.exe build --release --manifest-path 'src-tauri\Cargo.toml' --bin openmeter-tray --bin openmeter
        if ($LASTEXITCODE -ne 0) { throw "OpenMeter release build failed with exit code $LASTEXITCODE." }

        & cargo.exe build --release --manifest-path 'sync-hub\Cargo.toml'
        if ($LASTEXITCODE -ne 0) { throw "Sync hub release build failed with exit code $LASTEXITCODE." }
    }
    finally {
        Pop-Location
    }
    $binaries['openmeter-tray.exe'] = Join-Path $repoRoot 'src-tauri\target\release\openmeter-tray.exe'
    $binaries['openmeter.exe'] = Join-Path $repoRoot 'src-tauri\target\release\openmeter.exe'
    $binaries['openmeter-sync-hub.exe'] = Join-Path $repoRoot 'sync-hub\target\release\openmeter-sync-hub.exe'
}

$scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("openmeter-private-msix-{0}" -f [guid]::NewGuid().ToString('N'))
$inputRoot = Join-Path $scratch 'input'
$layoutRoot = Join-Path $scratch 'layout'
try {
    New-Item -ItemType Directory -Path $inputRoot -Force | Out-Null
    Copy-Item -LiteralPath $binaries['openmeter-tray.exe'] -Destination (Join-Path $inputRoot 'openmeter-tray.exe')
    Copy-Item -LiteralPath $binaries['openmeter.exe'] -Destination (Join-Path $inputRoot 'openmeter.exe')
    Copy-Item -LiteralPath $binaries['openmeter-sync-hub.exe'] -Destination (Join-Path $inputRoot 'openmeter-sync-hub.exe')
    Copy-Item -LiteralPath (Join-Path $repoRoot 'src-tauri\icons\32x32.png') -Destination (Join-Path $inputRoot 'Square44x44Logo.png')
    Copy-Item -LiteralPath (Join-Path $repoRoot 'src-tauri\icons\128x128.png') -Destination (Join-Path $inputRoot 'Square150x150Logo.png')
    Copy-Item -LiteralPath (Join-Path $repoRoot 'src-tauri\icons\128x128.png') -Destination (Join-Path $inputRoot 'StoreLogo.png')

    New-OpenMeterMsixLayout -InputRoot $inputRoot -OutputRoot $layoutRoot -Version $msixVersion -Publisher $Subject | Out-Null
    $programRoot = Join-Path $layoutRoot 'VFS\ProgramFilesX64\OpenMeter'
    foreach ($name in 'openmeter-tray.exe', 'openmeter.exe', 'openmeter-sync-hub.exe') {
        $payload = Join-Path $programRoot $name
        & winapp.exe tool signtool sign /fd SHA256 /sha1 $certificate.Thumbprint /s My $payload
        if ($LASTEXITCODE -ne 0) { throw "SignTool failed for '$name' with exit code $LASTEXITCODE." }
        Assert-OpenMeterSignature -Path $payload -Thumbprint $certificate.Thumbprint | Out-Null
    }

    New-Item -ItemType Directory -Path $OutputRoot -Force | Out-Null
    $packagePath = Join-Path $OutputRoot ("OpenMeter_{0}_x64.msix" -f $msixVersion)
    if (Test-Path -LiteralPath $packagePath) { Remove-Item -LiteralPath $packagePath -Force }
    & winapp.exe tool makeappx pack /o /d $layoutRoot /p $packagePath
    if ($LASTEXITCODE -ne 0) { throw "MakeAppx failed with exit code $LASTEXITCODE." }

    & winapp.exe tool signtool sign /fd SHA256 /sha1 $certificate.Thumbprint /s My $packagePath
    if ($LASTEXITCODE -ne 0) { throw "SignTool failed for the MSIX with exit code $LASTEXITCODE." }
    Assert-OpenMeterSignature -Path $packagePath -Thumbprint $certificate.Thumbprint | Out-Null

    $certificatePath = Join-Path $OutputRoot 'OpenMeter-Private.cer'
    Export-Certificate -Cert $certificate -FilePath $certificatePath -Type CERT -Force | Out-Null

    $checksums = @($packagePath, $certificatePath) | ForEach-Object {
        $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $_
        '{0} *{1}' -f $hash.Hash.ToLowerInvariant(), (Split-Path -Leaf $_)
    }
    [System.IO.File]::WriteAllLines((Join-Path $OutputRoot 'SHA256SUMS.txt'), $checksums, (New-Object System.Text.UTF8Encoding($false)))

    [ordered]@{
        package = $packagePath
        certificate = $certificatePath
        thumbprint = $certificate.Thumbprint
        subject = $certificate.Subject
        version = $msixVersion
    } | ConvertTo-Json | Set-Content -LiteralPath (Join-Path $OutputRoot 'deployment.json') -Encoding UTF8

    Write-Host "Private MSIX: $packagePath"
    Write-Host "Public certificate: $certificatePath"
    Write-Host "Certificate thumbprint: $($certificate.Thumbprint)"
}
finally {
    if (Test-Path -LiteralPath $scratch) {
        Remove-Item -LiteralPath $scratch -Recurse -Force
    }
}
