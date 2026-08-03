$ErrorActionPreference = 'Stop'
$workflowPath = Join-Path $PSScriptRoot '..\.github\workflows\release.yml'
$workflow = Get-Content -LiteralPath $workflowPath -Raw

$required = @(
    'azure/login@v3',
    'azure/artifact-signing-action@v2',
    'timestamp.acs.microsoft.com',
    'test-authenticode.ps1',
    'tauri.cmd bundle',
    'tauri.cmd signer sign',
    'id-token: write'
)
$required += @(
    'AZURE_CLIENT_ID',
    'AZURE_TENANT_ID',
    'AZURE_SUBSCRIPTION_ID',
    'ARTIFACT_SIGNING_ENDPOINT',
    'ARTIFACT_SIGNING_ACCOUNT',
    'ARTIFACT_SIGNING_PROFILE',
    'AUTHENTICODE_PUBLISHER'
)
$missing = $required | Where-Object { $workflow -notmatch [regex]::Escape($_) }
if ($missing) { throw "Release workflow misses: $($missing -join ', ')" }

$orderedSteps = @(
    'Sign inner executables',
    'Bundle NSIS',
    'Sign NSIS installer',
    'Sign final installer for Tauri updater',
    'Generate latest.json',
    'Attest installer provenance'
)
$previous = -1
foreach ($step in $orderedSteps) {
    $index = $workflow.IndexOf($step)
    if ($index -le $previous) { throw "Release step is missing or out of order: $step" }
    $previous = $index
}
if ($workflow -notmatch 'if:\s*startsWith\(github\.ref, ''refs/tags/''\)') {
    throw 'Release publication must remain tag-only.'
}
Write-Host 'Trusted release workflow invariants passed.'
