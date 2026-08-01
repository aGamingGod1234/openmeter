param(
    [string] $RepositoryRoot = (Split-Path -Parent $PSScriptRoot),
    [string] $OutputPath
)

$ErrorActionPreference = 'Stop'
$root = [IO.Path]::GetFullPath($RepositoryRoot)
if (-not $OutputPath) {
    $OutputPath = Join-Path $root 'src-tauri\tauri.bundle.generated.conf.json'
}
$output = [IO.Path]::GetFullPath($OutputPath)
$cli = Join-Path $root 'src-tauri\target\release\openmeter.exe'
$pathHelper = Join-Path $root 'src-tauri\windows\path.ps1'
$hooks = Join-Path $root 'src-tauri\windows\hooks.nsh'

foreach ($source in @($cli, $pathHelper, $hooks)) {
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) {
        throw "Bundle input does not exist: $source"
    }
}

$resources = [ordered]@{}
$resources[$cli] = 'openmeter.exe'
$resources[$pathHelper] = 'path.ps1'
$config = [ordered]@{
    '$schema' = 'https://schema.tauri.app/config/2'
    bundle = [ordered]@{
        resources = $resources
        windows = [ordered]@{
            nsis = [ordered]@{
                installerHooks = $hooks
            }
        }
    }
}

$parent = Split-Path -Parent $output
New-Item -ItemType Directory -Path $parent -Force | Out-Null
$json = $config | ConvertTo-Json -Depth 6
[IO.File]::WriteAllText($output, $json, (New-Object System.Text.UTF8Encoding($false)))
Write-Host "Generated bundle configuration: $output"
