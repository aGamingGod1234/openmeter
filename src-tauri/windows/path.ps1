param(
    [Parameter(Mandatory = $true)]
    [ValidateSet('Install', 'Remove')]
    [string] $Action,

    [Parameter(Mandatory = $true)]
    [string] $BinDir
)

$ErrorActionPreference = 'Stop'
$bin = [IO.Path]::GetFullPath($BinDir).TrimEnd('\')
$cli = Join-Path $bin 'openmeter.exe'

if ($Action -eq 'Install' -and -not (Test-Path -LiteralPath $cli -PathType Leaf)) {
    throw "Bundled CLI is missing: $cli"
}

$current = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($null -eq $current) { $current = '' }
$entries = @($current -split ';' | Where-Object { $_ -and $_.Trim() })
$kept = @($entries | Where-Object {
    -not [string]::Equals($_.Trim().TrimEnd('\'), $bin, [StringComparison]::OrdinalIgnoreCase)
})

if ($Action -eq 'Install') { $kept += $bin }
[Environment]::SetEnvironmentVariable('Path', ($kept -join ';'), 'User')

$appPath = 'HKCU:\Software\Microsoft\Windows\CurrentVersion\App Paths\openmeter.exe'
if ($Action -eq 'Install') {
    New-Item -Path $appPath -Force | Out-Null
    Set-Item -Path $appPath -Value $cli
    Set-ItemProperty -Path $appPath -Name Path -Value $bin
} else {
    Remove-Item -LiteralPath $appPath -Recurse -Force -ErrorAction SilentlyContinue
}
