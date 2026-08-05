[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ProvisionScript,
    [Parameter(Mandatory)][string]$ProtectedHistoryPath,
    [Parameter(Mandatory)][string]$ResultPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

try {
    $result = & $ProvisionScript -ProtectedHistoryPath $ProtectedHistoryPath
    [IO.File]::WriteAllText($ResultPath, ($result | Out-String).Trim(), (New-Object Text.UTF8Encoding($false)))
} catch {
    $failure = [pscustomobject]@{
        Error = $_.Exception.Message
        Succeeded = $false
    } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($ResultPath, $failure, (New-Object Text.UTF8Encoding($false)))
    exit 1
}
