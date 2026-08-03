param(
    [Parameter(Mandatory = $true)]
    [string]$BinaryPath
)

$ErrorActionPreference = 'Stop'
$ServiceName = 'OpenMeterSyncHub'
$InstallDirectory = 'C:\Program Files\OpenMeter Sync Hub'
$DataDirectory = 'C:\ProgramData\OpenMeterSync'
$InstalledBinary = Join-Path $InstallDirectory 'openmeter-sync-hub.exe'
$DatabasePath = Join-Path $DataDirectory 'hub.db'
$PepperPath = Join-Path $DataDirectory 'pepper.bin'
$FirewallRule = 'OpenMeter Sync Hub (Tailscale)'
$BindAddress = '100.90.87.7:6740'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this installer from an elevated PowerShell window.'
}

$source = (Resolve-Path -LiteralPath $BinaryPath).Path
New-Item -ItemType Directory -Force -Path $InstallDirectory, $DataDirectory | Out-Null
Copy-Item -LiteralPath $source -Destination $InstalledBinary -Force

if (-not (Test-Path -LiteralPath $PepperPath)) {
    $pepper = [byte[]]::new(32)
    $rng = [Security.Cryptography.RandomNumberGenerator]::Create()
    try { $rng.GetBytes($pepper) } finally { $rng.Dispose() }
    [IO.File]::WriteAllBytes($PepperPath, $pepper)
    [Array]::Clear($pepper, 0, $pepper.Length)
}

& icacls.exe $DataDirectory '/inheritance:r' '/grant:r' `
    'NT AUTHORITY\LocalService:(OI)(CI)M' `
    'BUILTIN\Administrators:(OI)(CI)F' `
    'NT AUTHORITY\SYSTEM:(OI)(CI)F' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not secure the sync data directory.' }
& icacls.exe $PepperPath '/inheritance:r' '/grant:r' `
    'NT AUTHORITY\LocalService:R' `
    'BUILTIN\Administrators:F' `
    'NT AUTHORITY\SYSTEM:F' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not secure the server pepper.' }

$existingController = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($existingController) {
    if ($existingController.Status -ne 'Stopped') {
        Stop-Service -Name $ServiceName -Force
        $existingController.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }
}

$serviceCommand = "`"$InstalledBinary`" service run --bind $BindAddress --database $DatabasePath --pepper-file $PepperPath"
$serviceClass = [wmiclass]'Win32_Service'
$existingService = Get-WmiObject -Class Win32_Service -Filter "Name='$ServiceName'"
if ($existingService) {
    $serviceResult = $existingService.Change(
        $null, $serviceCommand, $null, $null, 'Automatic', $null,
        'NT AUTHORITY\LocalService', $null, $null, $null, $null
    )
} else {
    $serviceResult = $serviceClass.Create(
        $ServiceName, 'OpenMeter Sync Hub', $serviceCommand, 16, 1, 'Automatic', $false,
        'NT AUTHORITY\LocalService', $null, $null, $null, $null
    )
}
if ($serviceResult.ReturnValue -ne 0) {
    throw "Could not configure the sync service (Win32 error $($serviceResult.ReturnValue))."
}
$serviceRegistry = "HKLM:\SYSTEM\CurrentControlSet\Services\$ServiceName"
Set-ItemProperty -LiteralPath $serviceRegistry -Name DelayedAutoStart -Value 1
Set-ItemProperty -LiteralPath $serviceRegistry -Name Description `
    -Value 'Stores end-to-end encrypted OpenMeter history envelopes on the private tailnet.'
& sc.exe failure $ServiceName 'reset=' '86400' `
    'actions=' 'restart/60000/restart/60000/none/0' | Out-Null
if ($LASTEXITCODE -ne 0) { throw 'Could not configure service recovery.' }

Remove-NetFirewallRule -DisplayName $FirewallRule -ErrorAction SilentlyContinue
# RemoteAddress 100.64.0.0/10 is deliberately tailnet-only.
New-NetFirewallRule -DisplayName $FirewallRule `
    -Direction Inbound -Action Allow -Protocol TCP -LocalPort 6740 `
    -LocalAddress '100.90.87.7' -RemoteAddress '100.64.0.0/10' `
    -InterfaceAlias 'Tailscale' -Profile Any | Out-Null

Start-Service -Name $ServiceName
(Get-Service -Name $ServiceName).WaitForStatus('Running', [TimeSpan]::FromSeconds(30))
Invoke-RestMethod -Uri 'http://100.90.87.7:6740/health' -TimeoutSec 15 | Out-Null
Write-Host 'OpenMeter Sync Hub is running at http://100.90.87.7:6740.'
