[CmdletBinding()]
param(
    [string]$SshExe = "$env:WINDIR\System32\OpenSSH\ssh.exe",
    [string]$IdentityFile = "$env:USERPROFILE\.ssh\codex_minipc_ed25519",
    [string]$RemoteHost = 'lucas@100.90.87.7',
    [string]$RemoteStagingScript = 'C:\Users\lucas\AppData\Local\Temp\OpenMeterDeploy-0.5.0.12\stage-interactive-provision.ps1'
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class OpenMeterCredentialReader {
    [StructLayout(LayoutKind.Sequential, CharSet = CharSet.Unicode)]
    private struct CREDENTIAL {
        public UInt32 Flags;
        public UInt32 Type;
        public IntPtr TargetName;
        public IntPtr Comment;
        public System.Runtime.InteropServices.ComTypes.FILETIME LastWritten;
        public UInt32 CredentialBlobSize;
        public IntPtr CredentialBlob;
        public UInt32 Persist;
        public UInt32 AttributeCount;
        public IntPtr Attributes;
        public IntPtr TargetAlias;
        public IntPtr UserName;
    }

    [DllImport("advapi32.dll", EntryPoint = "CredReadW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredRead(string target, UInt32 type, UInt32 flags, out IntPtr credential);

    [DllImport("advapi32.dll")]
    private static extern void CredFree(IntPtr credential);

    public static byte[] Read(string target) {
        IntPtr pointer;
        if (!CredRead(target, 1, 0, out pointer)) {
            throw new Win32Exception(Marshal.GetLastWin32Error(), "Windows Credential Manager read failed.");
        }
        try {
            CREDENTIAL credential = Marshal.PtrToStructure<CREDENTIAL>(pointer);
            byte[] bytes = new byte[credential.CredentialBlobSize];
            if (bytes.Length > 0) Marshal.Copy(credential.CredentialBlob, bytes, 0, bytes.Length);
            return bytes;
        } finally {
            CredFree(pointer);
        }
    }
}
'@

$common = @(
    '-i', $IdentityFile,
    '-o', 'BatchMode=yes',
    '-o', 'StrictHostKeyChecking=yes',
    $RemoteHost
)
$historyBytes = [OpenMeterCredentialReader]::Read('OpenMeter/sync/history-key')
if ($historyBytes.Length -ne 32) {
    [Array]::Clear($historyBytes, 0, $historyBytes.Length)
    throw 'Laptop history key is unavailable or invalid.'
}

$historyEncoded = $null
try {
    $historyEncoded = [Convert]::ToBase64String($historyBytes)
    $arguments = @($common) + @(
        'powershell', '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $RemoteStagingScript
    )
    $start = New-Object System.Diagnostics.ProcessStartInfo
    $start.FileName = $SshExe
    $start.Arguments = ($arguments | ForEach-Object {
        if ($_ -match '[\s"]') { '"' + ($_ -replace '"', '\"') + '"' } else { $_ }
    }) -join ' '
    $start.UseShellExecute = $false
    $start.CreateNoWindow = $true
    $start.RedirectStandardInput = $true
    $start.RedirectStandardOutput = $true
    $start.RedirectStandardError = $true
    if ($start.PSObject.Properties.Name -contains 'StandardInputEncoding') {
        $start.StandardInputEncoding = New-Object Text.UTF8Encoding($false)
    }
    $process = New-Object System.Diagnostics.Process
    $process.StartInfo = $start
    [void]$process.Start()
    $process.StandardInput.WriteLine($historyEncoded)
    $process.StandardInput.Close()
    $result = $process.StandardOutput.ReadToEnd()
    $errors = $process.StandardError.ReadToEnd()
    $process.WaitForExit()
    if ($process.ExitCode -ne 0) {
        if ($errors) { [Console]::Error.Write($errors) }
        throw 'Mini PC sync provisioning failed.'
    }
    $result.Trim()
} finally {
    [Array]::Clear($historyBytes, 0, $historyBytes.Length)
    $historyEncoded = $null
}
