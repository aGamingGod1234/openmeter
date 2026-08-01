$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$configPath = Join-Path $projectRoot 'src-tauri\tauri.conf.json'
$workflowPath = Join-Path $projectRoot '.github\workflows\release.yml'

$config = Get-Content -LiteralPath $configPath -Raw | ConvertFrom-Json
$resources = $config.bundle.resources
if (-not $resources) { throw 'Bundle resources must live in the primary Tauri config.' }
if ($resources.'target/release/openmeter.exe' -ne 'openmeter.exe') {
    throw 'The CLI resource must resolve relative to src-tauri.'
}
if ($resources.'windows/path.ps1' -ne 'path.ps1') {
    throw 'The PATH helper must resolve relative to src-tauri.'
}
if ($config.bundle.windows.nsis.installerHooks -ne 'windows/hooks.nsh') {
    throw 'The NSIS hook must resolve relative to src-tauri.'
}
if ($config.build.beforeBuildCommand -ne 'npm --prefix .. run build') {
    throw 'The frontend build must work while Tauri runs from src-tauri.'
}
if ($config.build.beforeDevCommand -ne 'npm --prefix .. run dev') {
    throw 'The frontend dev server must work while Tauri runs from src-tauri.'
}

$workflow = Get-Content -LiteralPath $workflowPath -Raw
if ($workflow -notmatch '(?ms)- name: Build \(signed\).*?working-directory: src-tauri') {
    throw 'The signed build must run from src-tauri so all resource consumers share one base directory.'
}
if ($workflow -notmatch [regex]::Escape('..\node_modules\.bin\tauri.cmd build --bundles nsis')) {
    throw 'The signed build must invoke the repository-local Tauri CLI.'
}
if ($workflow -match 'tauri\.bundle\.generated\.conf\.json') {
    throw 'The generated overlay reintroduces ambiguous resource path bases.'
}

Write-Host 'single-directory Tauri bundle regression test passed.'
