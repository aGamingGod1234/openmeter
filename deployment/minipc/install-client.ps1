[CmdletBinding()]
param(
    [string]$Source = "$env:LOCALAPPDATA\Temp\OpenMeterDeploy-0.5.0.12\openmeter-tray.exe",
    [string]$InstallRoot = "$env:ProgramFiles\OpenMeter Client",
    [string]$TaskName = 'OpenMeter Client',
    [string]$InteractiveUser = $env:USERNAME,
    [Parameter(Mandatory)]
    [ValidatePattern('^[0-9A-Fa-f]{40}$')]
    [string]$ExpectedThumbprint
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$installExe = Join-Path $InstallRoot 'openmeter-tray.exe'
$expected = $ExpectedThumbprint.ToUpperInvariant()
$sourceSignature = Get-AuthenticodeSignature -LiteralPath $Source
if ($sourceSignature.Status -ne 'Valid' -or $sourceSignature.SignerCertificate.Thumbprint -ne $expected) {
    throw 'Transferred client signature is invalid.'
}

Get-Process -Name 'openmeter-tray' -ErrorAction SilentlyContinue |
    Where-Object { $_.Path -eq $installExe } |
    Stop-Process -Force

if (-not (Test-Path -LiteralPath $InstallRoot -PathType Container)) {
    New-Item -ItemType Directory -Path $InstallRoot -Force | Out-Null
}
if (Test-Path -LiteralPath $installExe -PathType Leaf) {
    $stamp = (Get-Date).ToUniversalTime().ToString('yyyyMMddTHHmmssZ')
    Move-Item -LiteralPath $installExe -Destination (Join-Path $InstallRoot "openmeter-tray.$stamp.previous.exe")
}
Copy-Item -LiteralPath $Source -Destination $installExe

$installedSignature = Get-AuthenticodeSignature -LiteralPath $installExe
if ($installedSignature.Status -ne 'Valid' -or $installedSignature.SignerCertificate.Thumbprint -ne $expected) {
    throw 'Installed client signature is invalid.'
}

$action = New-ScheduledTaskAction -Execute $installExe -WorkingDirectory $InstallRoot
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $InteractiveUser
$principal = New-ScheduledTaskPrincipal -UserId $InteractiveUser -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -MultipleInstances IgnoreNew
Register-ScheduledTask `
    -TaskName $TaskName `
    -Action $action `
    -Trigger $trigger `
    -Principal $principal `
    -Settings $settings `
    -Force | Out-Null
Start-ScheduledTask -TaskName $TaskName

$deadline = (Get-Date).AddSeconds(30)
do {
    Start-Sleep -Milliseconds 500
    $process = Get-CimInstance Win32_Process -Filter "Name='openmeter-tray.exe'" |
        Where-Object { $_.ExecutablePath -eq $installExe } |
        Select-Object -First 1
} until ($process -or (Get-Date) -ge $deadline)
if (-not $process) {
    throw 'Mini PC client did not start in the active console session.'
}

$apiStatus = $null
$deadline = (Get-Date).AddSeconds(30)
do {
    try {
        $apiStatus = (Invoke-WebRequest -UseBasicParsing 'http://127.0.0.1:6736/v1/limits' -TimeoutSec 3).StatusCode
    } catch {
        $apiStatus = $null
    }
    if ($apiStatus -ne 200) {
        Start-Sleep -Milliseconds 500
    }
} until ($apiStatus -eq 200 -or (Get-Date) -ge $deadline)
if ($apiStatus -ne 200) {
    throw 'Mini PC client local API did not reach HTTP 200.'
}

[pscustomobject]@{
    Runtime = $process.ExecutablePath
    Signature = $installedSignature.Status.ToString()
    Thumbprint = $installedSignature.SignerCertificate.Thumbprint
    Api = $apiStatus
    TaskState = (Get-ScheduledTask -TaskName $TaskName).State.ToString()
} | ConvertTo-Json -Compress
