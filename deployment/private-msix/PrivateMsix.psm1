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

function Get-OrCreateOpenMeterSigningCertificate {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)]
        [ValidatePattern('^CN=[^,]+$')]
        [string]$Subject
    )

    $now = Get-Date
    $matches = @(Get-ChildItem -LiteralPath 'Cert:\CurrentUser\My' | Where-Object {
        $_.Subject -eq $Subject -and
        $_.HasPrivateKey -and
        $_.NotBefore -le $now -and
        $_.NotAfter -gt $now.AddDays(30)
    })
    if ($matches.Count -gt 1) {
        throw "More than one usable signing certificate has subject '$Subject'."
    }
    if ($matches.Count -eq 1) {
        return $matches[0]
    }

    return New-SelfSignedCertificate `
        -Type CodeSigningCert `
        -Subject $Subject `
        -KeyAlgorithm RSA `
        -KeyLength 3072 `
        -KeyExportPolicy NonExportable `
        -HashAlgorithm SHA256 `
        -CertStoreLocation 'Cert:\CurrentUser\My' `
        -NotAfter $now.AddYears(3)
}

function Assert-OpenMeterSignature {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$Path,
        [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$Thumbprint,
        [switch]$RequireTrusted
    )

    if (-not (Test-Path -LiteralPath $Path -PathType Leaf)) {
        throw "Signed file does not exist: $Path"
    }
    $signature = Get-AuthenticodeSignature -LiteralPath $Path
    if (-not $signature.SignerCertificate) {
        throw "File has no Authenticode signer: $Path"
    }
    if ($signature.SignerCertificate.Thumbprint -ne $Thumbprint) {
        throw "Unexpected Authenticode signer for '$Path'."
    }
    if ($signature.Status -in @('NotSigned', 'HashMismatch')) {
        throw "Invalid Authenticode signature for '$Path': $($signature.StatusMessage)"
    }
    if ($RequireTrusted -and $signature.Status -ne 'Valid') {
        throw "Authenticode signature is not trusted for '$Path': $($signature.StatusMessage)"
    }
    return $signature
}

function Assert-OpenMeterPrebuiltSet {
    [CmdletBinding()]
    param([Parameter(Mandatory)][string]$Root)

    if (-not (Test-Path -LiteralPath $Root -PathType Container)) {
        throw "Prebuilt directory does not exist: $Root"
    }
    $checksumPath = Join-Path $Root 'SHA256SUMS.txt'
    if (-not (Test-Path -LiteralPath $checksumPath -PathType Leaf)) {
        throw "Prebuilt checksum file is missing: $checksumPath"
    }

    $expected = @{}
    foreach ($line in Get-Content -LiteralPath $checksumPath) {
        if ($line -match '^([0-9A-Fa-f]{64})\s+\*?([^\\/]+)$') {
            $expected[$matches[2]] = $matches[1].ToUpperInvariant()
        }
    }

    $verified = foreach ($name in 'openmeter-tray.exe', 'openmeter.exe', 'openmeter-sync-hub.exe') {
        if (-not $expected.ContainsKey($name)) {
            throw "No recorded SHA-256 checksum exists for '$name'."
        }
        $path = Join-Path $Root $name
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
            throw "Required prebuilt binary is missing: $path"
        }
        $actual = (Get-FileHash -Algorithm SHA256 -LiteralPath $path).Hash
        if ($actual -ne $expected[$name]) {
            throw "SHA-256 mismatch for prebuilt binary '$name'."
        }
        $path
    }
    return $verified
}

function Test-OpenMeterPrivatePackage {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][string]$PackagePath,
        [Parameter(Mandatory)][string]$CertificatePath,
        [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$ExpectedThumbprint
    )

    if (-not (Test-Path -LiteralPath $CertificatePath -PathType Leaf)) {
        throw "Public certificate does not exist: $CertificatePath"
    }
    $certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CertificatePath)
    $normalizedThumbprint = $ExpectedThumbprint.ToUpperInvariant()
    if ($certificate.Thumbprint -ne $normalizedThumbprint) {
        throw 'Public certificate does not match the expected thumbprint.'
    }
    if (-not (Test-Path -LiteralPath $PackagePath -PathType Leaf)) {
        throw "MSIX package does not exist: $PackagePath"
    }

    $signature = Assert-OpenMeterSignature -Path $PackagePath -Thumbprint $normalizedThumbprint

    Add-Type -AssemblyName System.IO.Compression.FileSystem
    $archive = [System.IO.Compression.ZipFile]::OpenRead($PackagePath)
    try {
        $entry = $archive.GetEntry('AppxManifest.xml')
        if (-not $entry) { throw 'MSIX has no AppxManifest.xml.' }
        $reader = New-Object System.IO.StreamReader($entry.Open())
        try { [xml]$manifest = $reader.ReadToEnd() } finally { $reader.Dispose() }
    }
    finally {
        $archive.Dispose()
    }

    $namespace = New-Object System.Xml.XmlNamespaceManager($manifest.NameTable)
    $namespace.AddNamespace('a', 'http://schemas.microsoft.com/appx/manifest/foundation/windows10')
    $identity = $manifest.SelectSingleNode('/a:Package/a:Identity', $namespace)
    if (-not $identity) { throw 'MSIX manifest has no package identity.' }
    if ($identity.Name -ne 'OpenMeter.Private') {
        throw "Unexpected MSIX identity: $($identity.Name)"
    }
    if ($identity.Publisher -ne $certificate.Subject) {
        throw 'MSIX publisher does not match the public certificate subject.'
    }
    if ($identity.ProcessorArchitecture -ne 'x64') {
        throw "Unexpected MSIX architecture: $($identity.ProcessorArchitecture)"
    }

    return [pscustomobject]@{
        PackagePath = [System.IO.Path]::GetFullPath($PackagePath)
        CertificatePath = [System.IO.Path]::GetFullPath($CertificatePath)
        Thumbprint = $certificate.Thumbprint
        Publisher = $identity.Publisher
        Identity = $identity.Name
        Version = $identity.Version
        SignatureStatus = $signature.Status.ToString()
    }
}

function Assert-OpenMeterMsixTrust {
    [CmdletBinding()]
    param(
        [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$Thumbprint,
        [string]$StoreLocation = 'Cert:\LocalMachine\TrustedPeople'
    )

    if ($StoreLocation -notmatch '^Cert:\\(CurrentUser|LocalMachine)\\(TrustedPeople|Root)$') {
        throw "Unsupported certificate store location: $StoreLocation"
    }
    $trustPath = Join-Path $StoreLocation $Thumbprint.ToUpperInvariant()
    if (-not (Test-Path -LiteralPath $trustPath -PathType Leaf)) {
        throw "OpenMeter publisher certificate is not trusted for the current user: $Thumbprint"
    }
    return Get-Item -LiteralPath $trustPath
}

function Install-OpenMeterPrivateMsix {
    [CmdletBinding(SupportsShouldProcess, ConfirmImpact = 'Medium')]
    param(
        [Parameter(Mandatory)][string]$PackagePath,
        [Parameter(Mandatory)][string]$CertificatePath,
        [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$ExpectedThumbprint,
        [int]$HealthTimeoutSeconds = 45
    )

    $validated = Test-OpenMeterPrivatePackage `
        -PackagePath $PackagePath `
        -CertificatePath $CertificatePath `
        -ExpectedThumbprint $ExpectedThumbprint
    if (-not $PSCmdlet.ShouldProcess('CurrentUser OpenMeter package', 'Install private MSIX')) {
        return $validated
    }

    $thumbprint = $validated.Thumbprint
    $installed = $false
    try {
        Assert-OpenMeterMsixTrust -Thumbprint $thumbprint | Out-Null

        Get-Process -Name 'openmeter-tray' -ErrorAction SilentlyContinue | Stop-Process -Force
        Add-AppxPackage -Path $PackagePath -ForceApplicationShutdown -ForceUpdateFromAnyVersion
        $installed = $true

        $package = Get-AppxPackage -Name 'OpenMeter.Private' | Sort-Object Version -Descending | Select-Object -First 1
        if (-not $package) { throw 'OpenMeter.Private was not registered after Add-AppxPackage.' }
        Start-Process explorer.exe -ArgumentList "shell:AppsFolder\$($package.PackageFamilyName)!OpenMeter"

        $deadline = [DateTime]::UtcNow.AddSeconds($HealthTimeoutSeconds)
        $statusCode = 0
        do {
            try {
                $response = Invoke-WebRequest -UseBasicParsing -Uri 'http://127.0.0.1:6736/v1/limits' -TimeoutSec 3
                $statusCode = [int]$response.StatusCode
            }
            catch {
                Start-Sleep -Milliseconds 500
            }
        } while ($statusCode -ne 200 -and [DateTime]::UtcNow -lt $deadline)
        if ($statusCode -ne 200) { throw 'OpenMeter local API did not return HTTP 200 after installation.' }

        $dataRoot = [System.IO.Path]::GetFullPath((Join-Path $env:APPDATA 'OpenMeter'))
        if ($env:OneDrive -and $dataRoot.StartsWith([System.IO.Path]::GetFullPath($env:OneDrive), [StringComparison]::OrdinalIgnoreCase)) {
            throw "OpenMeter data root is inside OneDrive: $dataRoot"
        }

        return [pscustomobject]@{
            PackageFullName = $package.PackageFullName
            PackageFamilyName = $package.PackageFamilyName
            InstallLocation = $package.InstallLocation
            Thumbprint = $thumbprint
            ApiStatus = $statusCode
            DataRoot = $dataRoot
        }
    }
    catch {
        if ($installed) {
            Get-AppxPackage -Name 'OpenMeter.Private' -ErrorAction SilentlyContinue | Remove-AppxPackage -ErrorAction SilentlyContinue
        }
        throw
    }
}

Export-ModuleMember -Function @(
    'Resolve-WindowsSdkTool',
    'New-OpenMeterMsixLayout',
    'Get-OrCreateOpenMeterSigningCertificate',
    'Assert-OpenMeterSignature',
    'Assert-OpenMeterPrebuiltSet',
    'Test-OpenMeterPrivatePackage',
    'Assert-OpenMeterMsixTrust',
    'Install-OpenMeterPrivateMsix'
)
