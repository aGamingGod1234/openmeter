[CmdletBinding()]
param(
    [string]$ConfigPath = "$env:APPDATA\OpenMeter\config.json"
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$bytes = [IO.File]::ReadAllBytes($ConfigPath)
$hadBom = $bytes.Length -ge 3 -and $bytes[0] -eq 0xEF -and $bytes[1] -eq 0xBB -and $bytes[2] -eq 0xBF
$config = [Text.Encoding]::UTF8.GetString($bytes).TrimStart([char]0xFEFF) | ConvertFrom-Json
$json = $config | ConvertTo-Json -Depth 20
$temporary = "$ConfigPath.new"
[IO.File]::WriteAllText($temporary, $json, (New-Object Text.UTF8Encoding($false)))
Move-Item -LiteralPath $temporary -Destination $ConfigPath -Force

$written = [IO.File]::ReadAllBytes($ConfigPath)
$hasBom = $written.Length -ge 3 -and $written[0] -eq 0xEF -and $written[1] -eq 0xBB -and $written[2] -eq 0xBF
[pscustomobject]@{
    Path = [IO.Path]::GetFullPath($ConfigPath)
    RemovedBom = $hadBom
    HasBom = $hasBom
    SyncEnabled = [bool]$config.syncEnabled
    DeviceId = [string]$config.syncDeviceId
    DeviceLabel = [string]$config.syncDeviceLabel
} | ConvertTo-Json -Compress
