[CmdletBinding()]
param(
    [Parameter(Mandatory)][string]$ResultPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class OpenMeterDiagnosticCredentialReader {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct CREDENTIAL {
        public UInt32 Flags; public UInt32 Type; public IntPtr TargetName; public IntPtr Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public UInt32 CredentialBlobSize; public IntPtr CredentialBlob; public UInt32 Persist;
        public UInt32 AttributeCount; public IntPtr Attributes; public IntPtr TargetAlias; public IntPtr UserName;
    }
    [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredRead(string target, UInt32 type, UInt32 flags, out IntPtr credential);
    [DllImport("advapi32.dll")] private static extern void CredFree(IntPtr credential);
    public static byte[] Read(string target) {
        IntPtr pointer;
        if (!CredRead(target, 1, 0, out pointer)) throw new Win32Exception(Marshal.GetLastWin32Error());
        try {
            CREDENTIAL credential = Marshal.PtrToStructure<CREDENTIAL>(pointer);
            byte[] bytes = new byte[credential.CredentialBlobSize];
            if (bytes.Length > 0) Marshal.Copy(credential.CredentialBlob, bytes, 0, bytes.Length);
            return bytes;
        } finally { CredFree(pointer); }
    }
}
'@

$credentialBytes = $null
$historyBytes = $null
try {
    $configPath = Join-Path $env:APPDATA 'OpenMeter\config.json'
    $config = Get-Content -Raw -LiteralPath $configPath | ConvertFrom-Json
    $credentialBytes = [OpenMeterDiagnosticCredentialReader]::Read('OpenMeter/sync/device-credential')
    $historyBytes = [OpenMeterDiagnosticCredentialReader]::Read('OpenMeter/sync/history-key')
    $credential = [Text.Encoding]::UTF8.GetString($credentialBytes)
    $credentialMatchesDevice = $credential.StartsWith(([string]$config.syncDeviceId + '.'), [StringComparison]::Ordinal)
    $hubStatus = try {
        (Invoke-WebRequest -UseBasicParsing 'http://100.90.87.7:6740/v1/envelopes' -Headers @{ Authorization = "Bearer $credential" } -TimeoutSec 15).StatusCode
    } catch {
        [int]$_.Exception.Response.StatusCode
    }
    $process = Get-Process -Name 'openmeter-tray' -ErrorAction SilentlyContinue | Select-Object -First 1
    $result = [pscustomobject]@{
        DeviceId = [string]$config.syncDeviceId
        SyncEnabled = [bool]$config.syncEnabled
        CredentialPresent = $credentialBytes.Length -gt 0
        CredentialMatchesDevice = $credentialMatchesDevice
        HistoryKeyLength = $historyBytes.Length
        HubAuthenticatedStatus = $hubStatus
        ProcessSessionId = $process.SessionId
        CurrentSessionId = [Diagnostics.Process]::GetCurrentProcess().SessionId
    } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($ResultPath, $result, (New-Object Text.UTF8Encoding($false)))
} catch {
    $failure = [pscustomobject]@{ Error = $_.Exception.Message; Succeeded = $false } | ConvertTo-Json -Compress
    [IO.File]::WriteAllText($ResultPath, $failure, (New-Object Text.UTF8Encoding($false)))
    exit 1
} finally {
    if ($credentialBytes) { [Array]::Clear($credentialBytes, 0, $credentialBytes.Length) }
    if ($historyBytes) { [Array]::Clear($historyBytes, 0, $historyBytes.Length) }
    $credential = $null
}
