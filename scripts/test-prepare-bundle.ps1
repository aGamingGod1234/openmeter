$ErrorActionPreference = 'Stop'
$projectRoot = Split-Path -Parent $PSScriptRoot
$builder = Join-Path $PSScriptRoot 'prepare-bundle.ps1'
$fixture = Join-Path ([IO.Path]::GetTempPath()) ("openmeter-bundle-test-" + [guid]::NewGuid())
$output = Join-Path $fixture 'bundle.generated.json'

try {
    $cli = Join-Path $fixture 'src-tauri\target\release\openmeter.exe'
    $pathHelper = Join-Path $fixture 'src-tauri\windows\path.ps1'
    $hooks = Join-Path $fixture 'src-tauri\windows\hooks.nsh'
    New-Item -ItemType Directory -Path (Split-Path $cli), (Split-Path $pathHelper) -Force | Out-Null
    Set-Content -LiteralPath $cli -Value 'fake-cli' -Encoding Ascii
    Set-Content -LiteralPath $pathHelper -Value '# path helper' -Encoding Ascii
    Set-Content -LiteralPath $hooks -Value '; hooks' -Encoding Ascii

    & $builder -RepositoryRoot $fixture -OutputPath $output

    $config = Get-Content -LiteralPath $output -Raw | ConvertFrom-Json
    $sources = @($config.bundle.resources.PSObject.Properties.Name)
    if ($sources.Count -ne 2) { throw "Expected two resources, got $($sources.Count)." }
    foreach ($source in $sources) {
        if (-not [IO.Path]::IsPathRooted($source)) { throw "Resource is not absolute: $source" }
        if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Resource is missing: $source" }
    }
    if ($config.bundle.resources.$cli -ne 'openmeter.exe') { throw 'CLI destination is incorrect.' }
    if ($config.bundle.resources.$pathHelper -ne 'path.ps1') { throw 'PATH helper destination is incorrect.' }
    if ($config.bundle.windows.nsis.installerHooks -ne $hooks) { throw 'Installer hook path is incorrect.' }

    Write-Host 'prepare-bundle absolute-path regression test passed.'
} finally {
    if (Test-Path -LiteralPath $fixture) {
        Remove-Item -LiteralPath $fixture -Recurse -Force
    }
}
