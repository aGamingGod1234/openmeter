param(
    [string] $RepositoryRoot = (Split-Path -Parent $PSScriptRoot)
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($RepositoryRoot)
$sources = [ordered]@{
    'openmeter.exe' = Join-Path $root 'src-tauri\target\release\openmeter.exe'
    'path.ps1' = Join-Path $root 'src-tauri\windows\path.ps1'
    'hooks.nsh' = Join-Path $root 'src-tauri\windows\hooks.nsh'
}

foreach ($source in $sources.Values) {
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Bundle input does not exist: $source"
    }
}

# tauri-build resolves resources from src-tauri while tauri-bundler resolves
# the same map from the repository root. Stage identical inputs under both
# bases so a single relative resource map is valid in both phases.
foreach ($base in @($root, (Join-Path $root 'src-tauri'))) {
    $stage = Join-Path $base 'bundle-inputs'
    New-Item -ItemType Directory -Path $stage -Force | Out-Null
    foreach ($entry in $sources.GetEnumerator()) {
        Copy-Item -LiteralPath $entry.Value -Destination (Join-Path $stage $entry.Key) -Force
    }
}

Write-Host 'Staged bundle inputs for Cargo and NSIS.'
