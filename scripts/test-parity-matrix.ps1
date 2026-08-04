$ErrorActionPreference = 'Stop'
$matrixPath = Join-Path $PSScriptRoot '..\docs\parity-matrix.md'
if (-not (Test-Path -LiteralPath $matrixPath -PathType Leaf)) {
    throw 'Parity matrix is missing.'
}
$text = Get-Content -LiteralPath $matrixPath -Raw
$required = @(
    'accounts', 'discovery', 'cache-stamp', 'cli-api', 'dashboard', 'tray',
    'pace', 'spend', 'pi', 'proxy', 'privacy', 'updates', 'cors', 'sync'
)
$missing = $required | Where-Object { $text -notmatch "\|\s*$([regex]::Escape($_))\s*\|" }
if ($missing) { throw "Missing parity rows: $($missing -join ', ')" }
foreach ($id in $required) {
    $row = ($text -split "`r?`n") | Where-Object { $_ -match "\|\s*$([regex]::Escape($id))\s*\|" } | Select-Object -First 1
    if ($row -notmatch '\|\s*Complete\s*\|\s*$') { throw "Parity row is not complete: $id" }
    if ($row -notmatch '`[^`]+`') { throw "Parity row has no concrete implementation/test reference: $id" }
}
$trackingRow = ($text -split "`r?`n") | Where-Object { $_ -match '\|\s*cross-device-tracking\s*\|' } | Select-Object -First 1
if (-not $trackingRow) { throw 'Missing parity row: cross-device-tracking' }
if ($trackingRow -notmatch '\|\s*(Pending live|Complete)\s*\|\s*$') {
    throw 'Cross-device tracking must remain pending until live acceptance or be complete.'
}
foreach ($evidence in @('Laptop', 'Desktop', '7/30-day', 'test-cross-device-sync.ps1', 'Pending live three-machine acceptance')) {
    if ($text -notmatch [regex]::Escape($evidence)) {
        throw "Cross-device parity evidence is missing: $evidence"
    }
}
Write-Host 'OpenUsage parity matrix is complete.'
