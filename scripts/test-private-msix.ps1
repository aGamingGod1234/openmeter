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

function Test-Signing {
    $suffix = [guid]::NewGuid().ToString('N')
    $subject = "CN=OpenMeter Private MSIX Test $suffix"
    $scratch = Join-Path ([System.IO.Path]::GetTempPath()) "openmeter-msix-signing-$suffix"
    $thumbprint = $null
    try {
        New-Item -ItemType Directory -Path $scratch | Out-Null
        $first = Get-OrCreateOpenMeterSigningCertificate -Subject $subject
        $thumbprint = $first.Thumbprint
        $second = Get-OrCreateOpenMeterSigningCertificate -Subject $subject

        Assert-Equal $first.Thumbprint $second.Thumbprint 'Signing certificate was not reused'
        Assert-Equal $first.Subject $subject 'Signing certificate subject mismatch'
        Assert-True $first.HasPrivateKey 'Signing certificate has no private key'
        Assert-True ($first.NotAfter -gt [DateTime]::UtcNow.AddDays(300)) 'Signing certificate lifetime is too short'

        $codeSigningEku = @($first.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' } | ForEach-Object {
            $_.EnhancedKeyUsages | ForEach-Object { $_.Value }
        })
        Assert-True ($codeSigningEku -contains '1.3.6.1.5.5.7.3.3') 'Certificate lacks Code Signing EKU'

        $rsa = [System.Security.Cryptography.X509Certificates.RSACertificateExtensions]::GetRSAPrivateKey($first)
        try {
            Assert-Equal $rsa.KeySize 3072 'Signing key is not RSA 3072'
            if ($rsa -is [System.Security.Cryptography.RSACng]) {
                $exportPolicy = $rsa.Key.ExportPolicy
                $exportable = ($exportPolicy -band [System.Security.Cryptography.CngExportPolicies]::AllowExport) -ne 0 -or
                    ($exportPolicy -band [System.Security.Cryptography.CngExportPolicies]::AllowPlaintextExport) -ne 0
                Assert-True (-not $exportable) 'Signing private key is exportable'
            }
            elseif ($rsa -is [System.Security.Cryptography.RSACryptoServiceProvider]) {
                Assert-True (-not $rsa.CspKeyContainerInfo.Exportable) 'Signing private key is exportable'
            }
        }
        finally {
            if ($rsa) { $rsa.Dispose() }
        }

        $unsigned = Join-Path $scratch 'unsigned.exe'
        [System.IO.File]::WriteAllText($unsigned, 'not a signed PE')
        $rejected = $false
        try { Assert-OpenMeterSignature -Path $unsigned -Thumbprint $thumbprint } catch { $rejected = $true }
        Assert-True $rejected 'Unsigned payload was accepted'

        $prebuilt = Join-Path $scratch 'prebuilt'
        New-Item -ItemType Directory -Path $prebuilt | Out-Null
        $checksumLines = foreach ($name in 'openmeter-tray.exe', 'openmeter.exe', 'openmeter-sync-hub.exe') {
            $path = Join-Path $prebuilt $name
            [System.IO.File]::WriteAllText($path, "verified-$name")
            $hash = Get-FileHash -Algorithm SHA256 -LiteralPath $path
            '{0}  {1}' -f $hash.Hash, $name
        }
        [System.IO.File]::WriteAllLines((Join-Path $prebuilt 'SHA256SUMS.txt'), $checksumLines)
        $verified = @(Assert-OpenMeterPrebuiltSet -Root $prebuilt)
        Assert-Equal $verified.Count 3 'Unexpected verified prebuilt count'

        [System.IO.File]::AppendAllText((Join-Path $prebuilt 'openmeter.exe'), 'tampered')
        $tamperRejected = $false
        try { Assert-OpenMeterPrebuiltSet -Root $prebuilt | Out-Null } catch { $tamperRejected = $true }
        Assert-True $tamperRejected 'Tampered prebuilt binary was accepted'
    }
    finally {
        if ($thumbprint -and (Test-Path -LiteralPath "Cert:\CurrentUser\My\$thumbprint")) {
            Remove-Item -LiteralPath "Cert:\CurrentUser\My\$thumbprint" -Force
        }
        if (Test-Path -LiteralPath $scratch) {
            Remove-Item -LiteralPath $scratch -Recurse -Force
        }
    }
}

if ($InstallOnly) {
    throw 'Install tests have not been implemented yet.'
}

if ($SigningOnly) {
    Test-Signing
    Write-Host 'Private MSIX signing tests passed.'
    exit 0
}

Test-Layout
if (-not $LayoutOnly) { Test-Signing }
Write-Host 'Private MSIX tests passed.'
