[CmdletBinding()]
param(
    [string]$ProvisionScript = "$env:LOCALAPPDATA\Temp\OpenMeterDeploy-0.5.0.12\provision-sync.ps1",
    [string]$InteractiveRunner = "$env:LOCALAPPDATA\Temp\OpenMeterDeploy-0.5.0.12\interactive-provision.ps1",
    [string]$TaskName = 'OpenMeter Sync Provisioning',
    [string]$InteractiveUser = $env:USERNAME,
    [string]$HubExe = 'C:\Program Files\OpenMeter Sync Hub\openmeter-sync-hub.exe',
    [string]$HubDatabase = 'C:\ProgramData\OpenMeterSync\hub.db',
    [string]$HubPepper = 'C:\ProgramData\OpenMeterSync\pepper.bin'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Security

$historyEncoded = ([Console]::In.ReadLine()).Trim().TrimStart([char]0xFEFF)
$oemUtf8Marker = ([string][char]8745) + ([char]9559) + ([char]9488)
if ($historyEncoded.StartsWith($oemUtf8Marker, [StringComparison]::Ordinal)) {
    $historyEncoded = $historyEncoded.Substring($oemUtf8Marker.Length)
}
$historyBytes = [Convert]::FromBase64String($historyEncoded)
if ($historyBytes.Length -ne 32) {
    [Array]::Clear($historyBytes, 0, $historyBytes.Length)
    throw 'History key input is invalid.'
}

$stageRoot = Join-Path $env:LOCALAPPDATA ('Temp\OpenMeterProvision-' + [Guid]::NewGuid().ToString('N'))
$protectedPath = Join-Path $stageRoot 'history.dpapi'
$resultPath = Join-Path $stageRoot 'result.json'
New-Item -ItemType Directory -Path $stageRoot -Force | Out-Null
$token = (& $HubExe enrollment create --database $HubDatabase --pepper-file $HubPepper | Out-String).Trim()
if ($LASTEXITCODE -ne 0 -or $token -notmatch '^[A-Za-z0-9_-]{40,64}$') {
    [Array]::Clear($historyBytes, 0, $historyBytes.Length)
    throw 'The hub did not issue a valid one-use enrollment token.'
}
try {
    $payload = @{ token = $token; history = [Convert]::ToBase64String($historyBytes) } | ConvertTo-Json -Compress
    $payloadBytes = [Text.Encoding]::UTF8.GetBytes($payload)
    $protectedBytes = [Security.Cryptography.ProtectedData]::Protect(
        $payloadBytes,
        $null,
        [Security.Cryptography.DataProtectionScope]::CurrentUser
    )
    try {
        [IO.File]::WriteAllBytes($protectedPath, $protectedBytes)
    } finally {
        [Array]::Clear($protectedBytes, 0, $protectedBytes.Length)
        [Array]::Clear($payloadBytes, 0, $payloadBytes.Length)
        $payload = $null
    }
} finally {
    [Array]::Clear($historyBytes, 0, $historyBytes.Length)
    $historyEncoded = $null
    $token = $null
}

$powerShellExe = "$env:WINDIR\System32\WindowsPowerShell\v1.0\powershell.exe"
$actionArguments = "-NoProfile -ExecutionPolicy Bypass -File `"$InteractiveRunner`" -ProvisionScript `"$ProvisionScript`" -ProtectedHistoryPath `"$protectedPath`" -ResultPath `"$resultPath`""
$action = New-ScheduledTaskAction -Execute $powerShellExe -Argument $actionArguments
$principal = New-ScheduledTaskPrincipal -UserId $InteractiveUser -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -ExecutionTimeLimit (New-TimeSpan -Minutes 2) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName $TaskName -Action $action -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $TaskName

$deadline = (Get-Date).AddSeconds(90)
do {
    Start-Sleep -Milliseconds 500
} until ((Test-Path -LiteralPath $resultPath -PathType Leaf) -or (Get-Date) -ge $deadline)

try {
    if (-not (Test-Path -LiteralPath $resultPath -PathType Leaf)) {
        throw 'Interactive sync provisioning timed out.'
    }
    Get-Content -Raw -LiteralPath $resultPath
} finally {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $protectedPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $resultPath -Force -ErrorAction SilentlyContinue
    Remove-Item -LiteralPath $stageRoot -Force -ErrorAction SilentlyContinue
}
