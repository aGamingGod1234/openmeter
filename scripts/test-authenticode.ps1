param(
    [Parameter(Mandatory = $true)]
    [string]$Path,
    [Parameter(Mandatory = $true)]
    [string]$ExpectedPublisher
)

$ErrorActionPreference = 'Stop'
$resolved = (Resolve-Path -LiteralPath $Path).Path
$signature = Get-AuthenticodeSignature -LiteralPath $resolved
if ($signature.Status -ne 'Valid') {
    throw "Invalid Authenticode signature: $($signature.StatusMessage)"
}
if (-not $signature.SignerCertificate -or
    $signature.SignerCertificate.Subject -notlike "*$ExpectedPublisher*") {
    throw 'Unexpected Authenticode publisher subject.'
}
if (-not $signature.TimeStamperCertificate) {
    throw 'Authenticode signature has no RFC3161 timestamp.'
}
$hash = (Get-FileHash -LiteralPath $resolved -Algorithm SHA256).Hash
if ($hash -notmatch '^[0-9A-Fa-f]{64}$') { throw 'SHA-256 unavailable.' }
Write-Host "Valid timestamped Authenticode signature: $resolved"
