Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

function Resolve-WindowsSdkTool {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [ValidateSet('MakeAppx.exe', 'SignTool.exe')]
        [string]$Name
    )

    $roots = @(
        (Join-Path ${env:ProgramFiles(x86)} 'Windows Kits\10\bin'),
        (Join-Path $env:ProgramFiles 'Windows Kits\10\bin')
    ) | Select-Object -Unique

    $matches = foreach ($root in $roots) {
        if (Test-Path -LiteralPath $root -PathType Container) {
            Get-ChildItem -LiteralPath $root -Directory -ErrorAction SilentlyContinue |
                Sort-Object Name -Descending |
                ForEach-Object {
                    $candidate = Join-Path $_.FullName "x64\$Name"
                    if (Test-Path -LiteralPath $candidate -PathType Leaf) { $candidate }
                }
        }
    }

    $match = $matches | Select-Object -First 1
    if (-not $match) {
        throw "$Name was not found in an installed Windows 10/11 SDK."
    }
    return $match
}

function New-OpenMeterMsixLayout {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$InputRoot,
        [Parameter(Mandatory)][string]$OutputRoot,
        [Parameter(Mandatory)][string]$Version,
        [Parameter(Mandatory)][string]$Publisher
    )

    if ($Version -notmatch '^\d+\.\d+\.\d+\.\d+$') {
        throw "MSIX version must contain four numeric components: $Version"
    }
    if (-not (Test-Path -LiteralPath $InputRoot -PathType Container)) {
        throw "MSIX input directory does not exist: $InputRoot"
    }
    if ([string]::IsNullOrWhiteSpace($Publisher)) {
        throw 'MSIX publisher cannot be empty.'
    }

    $payloadNames = @('openmeter-tray.exe', 'openmeter.exe', 'openmeter-sync-hub.exe')
    $assetNames = @('Square44x44Logo.png', 'Square150x150Logo.png', 'StoreLogo.png')
    foreach ($name in @($payloadNames) + @($assetNames)) {
        if (-not (Test-Path -LiteralPath (Join-Path $InputRoot $name) -PathType Leaf)) {
            throw "Required MSIX input is missing: $name"
        }
    }

    if (Test-Path -LiteralPath $OutputRoot) {
        Remove-Item -LiteralPath $OutputRoot -Recurse -Force
    }
    $programRoot = Join-Path $OutputRoot 'VFS\ProgramFilesX64\OpenMeter'
    $assetRoot = Join-Path $OutputRoot 'Assets'
    New-Item -ItemType Directory -Path $programRoot -Force | Out-Null
    New-Item -ItemType Directory -Path $assetRoot -Force | Out-Null

    foreach ($name in $payloadNames) {
        Copy-Item -LiteralPath (Join-Path $InputRoot $name) -Destination (Join-Path $programRoot $name)
    }
    foreach ($name in $assetNames) {
        Copy-Item -LiteralPath (Join-Path $InputRoot $name) -Destination (Join-Path $assetRoot $name)
    }

    $templatePath = Join-Path $PSScriptRoot 'AppxManifest.template.xml'
    [xml]$manifest = Get-Content -Raw -LiteralPath $templatePath
    $namespace = New-Object System.Xml.XmlNamespaceManager($manifest.NameTable)
    $namespace.AddNamespace('a', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
    $identity = $manifest.SelectSingleNode('/a:Package/a:Identity', $namespace)
    $identity.SetAttribute('Publisher', $Publisher)
    $identity.SetAttribute('Version', $Version)

    $settings = New-Object System.Xml.XmlWriterSettings
    $settings.Encoding = New-Object System.Text.UTF8Encoding($false)
    $settings.Indent = $true
    $manifestPath = Join-Path $OutputRoot 'AppxManifest.xml'
    $writer = [System.Xml.XmlWriter]::Create($manifestPath, $settings)
    try { $manifest.Save($writer) } finally { $writer.Dispose() }

    return $OutputRoot
}

Export-ModuleMember -Function Resolve-WindowsSdkTool, New-OpenMeterMsixLayout

