[CmdletBinding()]
param(
    [switch]$LayoutOnly,
    [switch]$SigningOnly,
    [switch]$InstallOnly
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Assert-True {
    param([bool]$Condition, [string]$Message)
    if (-not $Condition) { throw $Message }
}

function Assert-Equal {
    param($Actual, $Expected, [string]$Message)
    if ($Actual -ne $Expected) {
        throw "$Message (expected '$Expected', got '$Actual')"
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$modulePath = Join-Path $repoRoot 'deployment\private-msix\PrivateMsix.psm1'
if (-not (Test-Path -LiteralPath $modulePath -PathType Leaf)) {
    throw "Private MSIX module is missing: $modulePath"
}
Import-Module $modulePath -Force

function Test-Layout {
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) ("openmeter-msix-layout-{0}" -f [guid]::NewGuid().ToString('N'))
    $inputRoot = Join-Path $scratch 'input'
    $outputRoot = Join-Path $scratch 'layout'
    try {
        New-Item -ItemType Directory -Path $inputRoot | Out-Null
        foreach ($name in 'openmeter-tray.exe', 'openmeter.exe', 'openmeter-sync-hub.exe') {
            [System.IO.File]::WriteAllText((Join-Path $inputRoot $name), "fixture-$name")
        }
        foreach ($name in 'Square44x44Logo.png', 'Square150x150Logo.png', 'StoreLogo.png') {
            [System.IO.File]::WriteAllBytes((Join-Path $inputRoot $name), [byte[]](1, 2, 3, 4))
        }

        New-OpenMeterMsixLayout `
            -InputRoot $inputRoot `
            -OutputRoot $outputRoot `
            -Version '0.5.0.0' `
            -Publisher 'CN=OpenMeter Private Deployment'

        $programRoot = Join-Path $outputRoot 'VFS\ProgramFilesX64\OpenMeter'
        foreach ($name in 'openmeter-tray.exe', 'openmeter.exe', 'openmeter-sync-hub.exe') {
            Assert-True (Test-Path -LiteralPath (Join-Path $programRoot $name) -PathType Leaf) "Missing staged payload: $name"
        }

        [xml]$manifest = Get-Content -Raw -LiteralPath (Join-Path $outputRoot 'AppxManifest.xml')
        $ns = New-Object System.Xml.XmlNamespaceManager($manifest.NameTable)
        $ns.AddNamespace('a', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
        $ns.AddNamespace('uap', 'http://schemas.microsoft.com/appx/manifest/uap/windows10')
        $ns.AddNamespace('desktop', 'http://schemas.microsoft.com/appx/manifest/desktop/windows10')
        $identity = $manifest.SelectSingleNode('/a:Package/a:Identity', $ns)
        $application = $manifest.SelectSingleNode('/a:Package/a:Applications/a:Application', $ns)

        Assert-Equal $identity.Name 'OpenMeter.Private' 'Unexpected package identity'
        Assert-Equal $identity.ProcessorArchitecture 'x64' 'Unexpected package architecture'
        Assert-Equal $identity.Version '0.5.0.0' 'Unexpected package version'
        Assert-Equal $identity.Publisher 'CN=OpenMeter Private Deployment' 'Unexpected package publisher'
        Assert-Equal $application.Executable 'VFS\ProgramFilesX64\OpenMeter\openmeter-tray.exe' 'Unexpected app executable'
        Assert-Equal $application.EntryPoint 'Windows.FullTrustApplication' 'Unexpected entry point'

        $manifestText = Get-Content -Raw -LiteralPath (Join-Path $outputRoot 'AppxManifest.xml')
        Assert-True (-not $manifestText.Contains($env:USERNAME)) 'Manifest leaked the Windows username'
        Assert-True (-not $manifestText.Contains($repoRoot)) 'Manifest leaked the checkout path'
    }
    finally {
        if (Test-Path -LiteralPath $scratch) {
            Remove-Item -LiteralPath $scratch -Recurse -Force
        }
    }
}

if ($SigningOnly) {
    throw 'Signing tests have not been implemented yet.'
}
if ($InstallOnly) {
    throw 'Install tests have not been implemented yet.'
}

Test-Layout
Write-Host 'Private MSIX layout tests passed.'

