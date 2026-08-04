[CmdletBinding()]
param(
    [string]$CertificatePath,
    [Parameter(Mandatory)][ValidatePattern('^[0-9A-Fa-f]{40}$')][string]$ExpectedThumbprint,
    [switch]$Remove
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

$principal = New-Object Security.Principal.WindowsPrincipal([Security.Principal.WindowsIdentity]::GetCurrent())
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Machine certificate trust changes require an elevated administrator process.'
}

$thumbprint = $ExpectedThumbprint.ToUpperInvariant()
$trustPath = "Cert:\LocalMachine\TrustedPeople\$thumbprint"
if ($Remove) {
    if (Test-Path -LiteralPath $trustPath -PathType Leaf) {
        Remove-Item -LiteralPath $trustPath -Force
    }
    exit 0
}

if (-not (Test-Path -LiteralPath $CertificatePath -PathType Leaf)) {
    throw "Public certificate does not exist: $CertificatePath"
}
$certificate = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($CertificatePath)
if ($certificate.Thumbprint -ne $thumbprint) { throw 'Public certificate thumbprint mismatch.' }
if ($certificate.Subject -ne 'CN=OpenMeter Private Deployment') { throw "Unexpected certificate subject: $($certificate.Subject)" }
$codeSigningEku = @($certificate.Extensions | Where-Object { $_.Oid.Value -eq '2.5.29.37' } | ForEach-Object {
    $_.EnhancedKeyUsages | ForEach-Object { $_.Value }
})
if ($codeSigningEku -notcontains '1.3.6.1.5.5.7.3.3') { throw 'Certificate lacks the Code Signing EKU.' }

if (-not (Test-Path -LiteralPath $trustPath -PathType Leaf)) {
    Import-Certificate -FilePath $CertificatePath -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople' | Out-Null
}
if (-not (Test-Path -LiteralPath $trustPath -PathType Leaf)) {
    throw 'OpenMeter certificate was not added to LocalMachine\TrustedPeople.'
}

