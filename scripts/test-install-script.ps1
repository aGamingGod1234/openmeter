$ErrorActionPreference = 'Stop'
$installer = Get-Content -LiteralPath (Join-Path $PSScriptRoot '..\install.ps1') -Raw
foreach ($required in @(
    'Get-AuthenticodeSignature',
    'Status -ne',
    'SignerCertificate',
    'TimeStamperCertificate',
    'Authenticode publisher:'
)) {
    if ($installer -notmatch [regex]::Escape($required)) {
        throw "install.ps1 misses $required"
    }
}
Write-Host 'Quick installer Authenticode invariants passed.'
