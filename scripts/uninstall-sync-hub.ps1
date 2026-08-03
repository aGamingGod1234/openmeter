param([switch]$RemoveData)

$ErrorActionPreference = 'Stop'
$ServiceName = 'OpenMeterSyncHub'
$InstallDirectory = 'C:\Program Files\OpenMeter Sync Hub'
$DataDirectory = 'C:\ProgramData\OpenMeterSync'
$FirewallRule = 'OpenMeter Sync Hub (Tailscale)'

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw 'Run this uninstaller from an elevated PowerShell window.'
}

$service = Get-Service -Name $ServiceName -ErrorAction SilentlyContinue
if ($service) {
    if ($service.Status -ne 'Stopped') {
        Stop-Service -Name $ServiceName -Force
        $service.WaitForStatus('Stopped', [TimeSpan]::FromSeconds(30))
    }
    & sc.exe delete $ServiceName | Out-Null
}
Remove-NetFirewallRule -DisplayName $FirewallRule -ErrorAction SilentlyContinue

if (Test-Path -LiteralPath $InstallDirectory) {
    $resolvedInstall = [IO.Path]::GetFullPath($InstallDirectory)
    if ($resolvedInstall -ne 'C:\Program Files\OpenMeter Sync Hub') {
        throw 'Refusing to remove an unexpected install directory.'
    }
    Remove-Item -LiteralPath $resolvedInstall -Recurse -Force
}
if ($RemoveData -and (Test-Path -LiteralPath $DataDirectory)) {
    $resolvedData = [IO.Path]::GetFullPath($DataDirectory)
    if ($resolvedData -ne 'C:\ProgramData\OpenMeterSync') {
        throw 'Refusing to remove an unexpected data directory.'
    }
    Remove-Item -LiteralPath $resolvedData -Recurse -Force
    Write-Host 'Service, firewall rule, and encrypted hub data were removed.'
} else {
    Write-Host 'Service and firewall rule removed; encrypted hub data was preserved.'
}
