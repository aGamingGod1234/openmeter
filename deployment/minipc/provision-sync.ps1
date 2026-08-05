[CmdletBinding()]
param(
    [string]$HubUrl = 'http://100.90.87.7:6740',
    [string]$DeviceLabel = 'Mini PC',
    [string]$HubExe = 'C:\Program Files\OpenMeter Sync Hub\openmeter-sync-hub.exe',
    [string]$HubDatabase = 'C:\ProgramData\OpenMeterSync\hub.db',
    [string]$HubPepper = 'C:\ProgramData\OpenMeterSync\pepper.bin',
    [string]$ProtectedHistoryPath
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($DeviceLabel.Length -lt 1 -or $DeviceLabel.Length -gt 32 -or $DeviceLabel -match '[\x00-\x1F]') {
    throw 'Device label must be between 1 and 32 printable characters.'
}
$hub = [Uri]$HubUrl
if ($hub.Scheme -notin @('http', 'https') -or $hub.Host -ne '100.90.87.7' -or $hub.AbsolutePath -ne '/') {
    throw 'Hub URL is outside the approved Mini PC endpoint.'
}

Add-Type -TypeDefinition @'
using System;
using System.ComponentModel;
using System.Runtime.InteropServices;

public static class OpenMeterCredentialWriter {
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

    [DllImport("advapi32.dll", EntryPoint = "CredWriteW", CharSet = CharSet.Unicode, SetLastError = true)]
    private static extern bool CredWrite(ref CREDENTIAL credential, UInt32 flags);

    [DllImport("kernel32.dll", EntryPoint = "RtlZeroMemory")]
    private static extern void ZeroMemory(IntPtr destination, UIntPtr length);

    public static void Write(string target, byte[] secret) {
        if (String.IsNullOrWhiteSpace(target) || !target.StartsWith("OpenMeter/", StringComparison.Ordinal) || secret == null || secret.Length == 0 || secret.Length > 2560) {
            throw new ArgumentException("Credential input is invalid.");
        }
        IntPtr targetPointer = IntPtr.Zero;
        IntPtr userPointer = IntPtr.Zero;
        IntPtr blobPointer = IntPtr.Zero;
        try {
            targetPointer = Marshal.StringToCoTaskMemUni(target);
            userPointer = Marshal.StringToCoTaskMemUni("OpenMeter");
            blobPointer = Marshal.AllocHGlobal(secret.Length);
            Marshal.Copy(secret, 0, blobPointer, secret.Length);
            CREDENTIAL credential = new CREDENTIAL {
                Type = 1,
                TargetName = targetPointer,
                CredentialBlobSize = (UInt32)secret.Length,
                CredentialBlob = blobPointer,
                Persist = 2,
                UserName = userPointer
            };
            if (!CredWrite(ref credential, 0)) {
                throw new Win32Exception(Marshal.GetLastWin32Error(), "Windows Credential Manager write failed.");
            }
        } finally {
            if (blobPointer != IntPtr.Zero) {
                ZeroMemory(blobPointer, (UIntPtr)secret.Length);
                Marshal.FreeHGlobal(blobPointer);
            }
            if (targetPointer != IntPtr.Zero) Marshal.ZeroFreeCoTaskMemUnicode(targetPointer);
            if (userPointer != IntPtr.Zero) Marshal.ZeroFreeCoTaskMemUnicode(userPointer);
        }
    }
}
'@

$historyEncoded = $null
$token = $null
if ($ProtectedHistoryPath) {
    Add-Type -AssemblyName System.Security
    $protectedBytes = [IO.File]::ReadAllBytes($ProtectedHistoryPath)
    try {
        $payloadBytes = [Security.Cryptography.ProtectedData]::Unprotect(
            $protectedBytes,
            $null,
            [Security.Cryptography.DataProtectionScope]::CurrentUser
        )
        try {
            $payload = [Text.Encoding]::UTF8.GetString($payloadBytes) | ConvertFrom-Json
            $token = [string]$payload.token
            $historyBytes = [Convert]::FromBase64String([string]$payload.history)
            $payload = $null
        } finally {
            [Array]::Clear($payloadBytes, 0, $payloadBytes.Length)
        }
    } finally {
        [Array]::Clear($protectedBytes, 0, $protectedBytes.Length)
        Remove-Item -LiteralPath $ProtectedHistoryPath -Force -ErrorAction SilentlyContinue
    }
} else {
    $historyEncoded = ([Console]::In.ReadLine()).Trim().TrimStart([char]0xFEFF)
    $oemUtf8Marker = ([string][char]8745) + ([char]9559) + ([char]9488)
    if ($historyEncoded.StartsWith($oemUtf8Marker, [StringComparison]::Ordinal)) {
        $historyEncoded = $historyEncoded.Substring($oemUtf8Marker.Length)
    }
    $invalidCodePoints = @($historyEncoded.ToCharArray() |
        Where-Object { $_ -notmatch '^[A-Za-z0-9+/=]$' } |
        ForEach-Object { [int]$_ })
    if ($invalidCodePoints.Count -gt 0) {
        throw "History key transport encoding is invalid (length $($historyEncoded.Length); unexpected code points $($invalidCodePoints -join ','))."
    }
    $historyBytes = [Convert]::FromBase64String($historyEncoded)
}
if ($historyBytes.Length -ne 32) {
    [Array]::Clear($historyBytes, 0, $historyBytes.Length)
    throw 'History key input is invalid.'
}

$credentialBytes = $null
try {
    if (-not $token) {
        $token = (& $HubExe enrollment create --database $HubDatabase --pepper-file $HubPepper | Out-String).Trim()
        if ($LASTEXITCODE -ne 0) {
            throw 'The hub did not issue a valid one-use enrollment token.'
        }
    }
    if ($token -notmatch '^[A-Za-z0-9_-]{40,64}$') {
        throw 'The hub did not issue a valid one-use enrollment token.'
    }
    $body = @{ token = $token } | ConvertTo-Json -Compress
    $enrollment = Invoke-RestMethod -Method Post -Uri ([Uri]::new($hub, 'v1/enroll')) -ContentType 'application/json' -Body $body -TimeoutSec 15
    if ($enrollment.device_id -notmatch '^device-[0-9a-f]{32}$' -or [string]::IsNullOrWhiteSpace($enrollment.credential)) {
        throw 'Hub returned an invalid enrollment response.'
    }

    $credentialBytes = [Text.Encoding]::UTF8.GetBytes([string]$enrollment.credential)
    [OpenMeterCredentialWriter]::Write('OpenMeter/sync/device-credential', $credentialBytes)
    [OpenMeterCredentialWriter]::Write('OpenMeter/sync/history-key', $historyBytes)

    $configRoot = Join-Path $env:APPDATA 'OpenMeter'
    $oneDriveRoot = [IO.Path]::GetFullPath((Join-Path $env:USERPROFILE 'OneDrive'))
    $resolvedConfigRoot = [IO.Path]::GetFullPath($configRoot)
    if ($resolvedConfigRoot.StartsWith($oneDriveRoot, [StringComparison]::OrdinalIgnoreCase)) {
        throw 'OpenMeter config root must remain outside OneDrive.'
    }
    New-Item -ItemType Directory -Path $configRoot -Force | Out-Null
    $configPath = Join-Path $configRoot 'config.json'
    if (Test-Path -LiteralPath $configPath -PathType Leaf) {
        $config = Get-Content -Raw -LiteralPath $configPath | ConvertFrom-Json
    } else {
        $config = [pscustomobject]@{}
    }
    $patch = [ordered]@{
        syncEnabled = $true
        syncHubUrl = $HubUrl
        syncDeviceId = [string]$enrollment.device_id
        syncRevision = 0
        syncLastSuccess = 0
        syncTrackingRevision = 0
        syncTrackingLastSuccess = 0
        syncDeviceLabel = $DeviceLabel
    }
    foreach ($entry in $patch.GetEnumerator()) {
        if ($config.PSObject.Properties.Name -contains $entry.Key) {
            $config.($entry.Key) = $entry.Value
        } else {
            $config | Add-Member -NotePropertyName $entry.Key -NotePropertyValue $entry.Value
        }
    }
    $temporary = "$configPath.new"
    $configJson = $config | ConvertTo-Json -Depth 20
    [IO.File]::WriteAllText($temporary, $configJson, (New-Object Text.UTF8Encoding($false)))
    Move-Item -LiteralPath $temporary -Destination $configPath -Force

    [pscustomobject]@{
        DeviceId = [string]$enrollment.device_id
        DeviceLabel = $DeviceLabel
        ConfigRoot = $resolvedConfigRoot
        CredentialsStored = $true
    } | ConvertTo-Json -Compress
} finally {
    $token = $null
    $historyEncoded = $null
    if ($credentialBytes) { [Array]::Clear($credentialBytes, 0, $credentialBytes.Length) }
    if ($historyBytes) { [Array]::Clear($historyBytes, 0, $historyBytes.Length) }
}
