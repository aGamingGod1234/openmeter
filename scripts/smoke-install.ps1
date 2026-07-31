param(
    [Parameter(Mandatory = $true)]
    [string] $InstallerPath,

    [switch] $KeepInstalled
)

$ErrorActionPreference = 'Stop'
$installer = (Resolve-Path -LiteralPath $InstallerPath).Path
$installRoot = Join-Path $env:LOCALAPPDATA 'OpenMeter'
$trayExe = Join-Path $installRoot 'openmeter-tray.exe'
$cliExe = Join-Path $installRoot 'resources\openmeter.exe'
$uninstaller = Join-Path $installRoot 'uninstall.exe'
$dataRoot = Join-Path $env:APPDATA 'OpenMeter'
$sentinel = Join-Path $dataRoot 'smoke-preserve.txt'
$createdSentinel = -not (Test-Path -LiteralPath $sentinel)

New-Item -ItemType Directory -Path $dataRoot -Force | Out-Null
if ($createdSentinel) { Set-Content -LiteralPath $sentinel -Value 'preserve-user-data' -Encoding Ascii }

function Install-OpenMeter {
    Get-Process -Name openmeter-tray, openmeter -ErrorAction SilentlyContinue |
        Stop-Process -Force -ErrorAction SilentlyContinue
    $process = Start-Process -FilePath $installer -ArgumentList '/S' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Installer exited with code $($process.ExitCode)" }
    foreach ($path in @($trayExe, $cliExe, $uninstaller)) {
        if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw "Missing installed file: $path" }
    }
}

try {
    Install-OpenMeter
    Install-OpenMeter

    $help = & $cliExe --help 2>&1 | Out-String
    if ($LASTEXITCODE -ne 0 -or $help -notmatch 'OpenMeter usage limits') {
        throw 'Installed CLI help smoke check failed.'
    }

    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if ($userPath -notlike "*$installRoot\resources*") { throw 'CLI directory was not added to the user PATH.' }

    Start-Process -FilePath $trayExe -WindowStyle Hidden
    $deadline = (Get-Date).AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 500
        $running = Get-Process -Name openmeter-tray -ErrorAction SilentlyContinue
    } until ($running -or (Get-Date) -ge $deadline)
    if (-not $running) { throw 'Tray process did not start.' }

    $apiReady = $false
    $deadline = (Get-Date).AddSeconds(30)
    do {
        try {
            $response = Invoke-WebRequest 'http://127.0.0.1:6736/v1/limits' -UseBasicParsing -TimeoutSec 2
            $apiReady = $response.StatusCode -eq 200
        } catch { Start-Sleep -Milliseconds 500 }
    } until ($apiReady -or (Get-Date) -ge $deadline)
    if (-not $apiReady) { throw 'Loopback API did not become ready.' }

    if ($KeepInstalled) {
        Write-Host 'OpenMeter install, upgrade, CLI, tray, PATH, and API smoke checks passed; installation retained.'
        return
    }

    Get-Process -Name openmeter-tray -ErrorAction SilentlyContinue | Stop-Process -Force
    $process = Start-Process -FilePath $uninstaller -ArgumentList '/S' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "Uninstaller exited with code $($process.ExitCode)" }
    if (-not (Test-Path -LiteralPath $sentinel)) { throw 'Uninstall removed user data.' }
    if (Test-Path -LiteralPath $trayExe) { throw 'Uninstall left the tray executable behind.' }
    Write-Host 'OpenMeter silent install, upgrade, CLI, tray, API, and uninstall smoke checks passed.'
} finally {
    if ($createdSentinel -and (Test-Path -LiteralPath $sentinel)) {
        Remove-Item -LiteralPath $sentinel -Force
    }
}
