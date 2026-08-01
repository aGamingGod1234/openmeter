$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $projectRoot 'src-tauri\tauri.conf.json'
$overlayPath = Join-Path $projectRoot 'src-tauri\tauri.bundle.conf.json'
$workflowPath = Join-Path $projectRoot '.github\workflows\release.yml'

$config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
if ($config.bundle.resources) {
    throw 'Release-only resources must not break ordinary Cargo builds.'
}
$overlay = Get-Content -LiteralPath $overlayPath -Raw | ConvertFrom-Json
$resources = $overlay.bundle.resources
if (-not $resources) { throw 'Bundle resources must live in the release overlay.' }
if ($resources.'target/release/openmeter.exe' -ne 'openmeter.exe') {
    throw 'The CLI resource must resolve relative to src-tauri.'
}
if ($resources.'windows/path.ps1' -ne 'path.ps1') {
    throw 'The PATH helper must resolve relative to src-tauri.'
}
if ($overlay.bundle.windows.nsis.installerHooks -ne 'windows/hooks.nsh') {
    throw 'The NSIS hook must resolve relative to src-tauri.'
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

Write-Host 'single-directory Tauri bundle regression test passed.'
