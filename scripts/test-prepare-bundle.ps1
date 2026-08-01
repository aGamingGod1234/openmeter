$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $projectRoot 'src-tauri\tauri.conf.json'
$overlayPath = Join-Path $projectRoot 'src-tauri\tauri.bundle.conf.json'
$workflowPath = Join-Path $projectRoot '.github\workflows\release.yml'
$builder = Join-Path $PSScriptRoot 'prepare-bundle.ps1'

$config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
if ($config.bundle.resources) {
    throw 'Release-only resources must not break ordinary Cargo builds.'
}
$overlay = Get-Content -LiteralPath $overlayPath -Raw | ConvertFrom-Json
$resources = $overlay.bundle.resources
if (-not $resources) { throw 'Bundle resources must live in the release overlay.' }
if ($resources.'bundle-inputs/openmeter.exe' -ne 'resources/openmeter.exe') {
    throw 'The CLI must install under the resources directory expected by the PATH hook.'
}
if ($resources.'bundle-inputs/path.ps1' -ne 'resources/path.ps1') {
    throw 'The PATH helper must install under the resources directory.'
}
if ($overlay.bundle.windows.nsis.installerHooks -ne 'bundle-inputs/hooks.nsh') {
    throw 'The NSIS hook must use the shared staging-relative path.'
}
if ($config.build.beforeBuildCommand -ne 'npm run build') {
    throw 'Tauri already runs the frontend build from the frontend root.'
}
if ($config.build.beforeDevCommand -ne 'npm run dev') {
    throw 'Tauri already runs the frontend dev server from the frontend root.'
}

$workflow = Get-Content -LiteralPath $workflowPath -Raw
if ($workflow -notmatch '(?ms)- name: Build \(signed\).*?working-directory: src-tauri') {
    throw 'The signed build must run from src-tauri so all resource consumers share one base directory.'
}
if ($workflow -notmatch [regex]::Escape('..\node_modules\.bin\tauri.cmd build --bundles nsis --config tauri.bundle.conf.json')) {
    throw 'The signed build must invoke the repository-local Tauri CLI.'
}
if ($workflow -notmatch [regex]::Escape('.\scripts\prepare-bundle.ps1')) {
    throw 'The workflow must stage inputs for both Tauri path bases.'
}

$fixture = Join-Path ([IO.Path]::GetTempPath()) ("openmeter-bundle-test-" + [guid]::NewGuid())
try {
    $cli = Join-Path $fixture 'src-tauri\target\release\openmeter.exe'
    $pathHelper = Join-Path $fixture 'src-tauri\windows\path.ps1'
    $hooks = Join-Path $fixture 'src-tauri\windows\hooks.nsh'
    New-Item -ItemType Directory -Path (Split-Path $cli), (Split-Path $pathHelper) -Force | Out-Null
    Set-Content -LiteralPath $cli -Value 'fake-cli' -Encoding Ascii
    Set-Content -LiteralPath $pathHelper -Value '# path helper' -Encoding Ascii
    Set-Content -LiteralPath $hooks -Value '; hooks' -Encoding Ascii

    & $builder -RepositoryRoot $fixture

    foreach ($base in @($fixture, (Join-Path $fixture 'src-tauri'))) {
        foreach ($name in @('openmeter.exe', 'path.ps1', 'hooks.nsh')) {
            $staged = Join-Path $base "bundle-inputs\$name"
            if (-not (Test-Path -LiteralPath $staged -PathType Leaf)) {
                throw "Missing staged input: $staged"
            }
        }
    }
} finally {
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
}

Write-Host 'dual-base Tauri bundle staging regression test passed.'
