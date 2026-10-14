# 只检查命令行中属于本次唯一运行目录的应用／驱动，不按进程名批量终止。
[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)][string]$RunRoot,
    [switch]$Terminate,
    [ValidateRange(0, 10000)][int]$WaitMs = 5000
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$ownedRoot = (Resolve-Path -LiteralPath $RunRoot).Path.TrimEnd('\', '/')
if ((Split-Path $ownedRoot -Leaf) -notmatch '^(desktop|webview)-[a-zA-Z0-9]+$') { throw '进程检查需要唯一的桌面测试目录。' }
$pattern = [regex]::Escape($ownedRoot.Replace('\', '/')) + '(?=[/"\s]|$)'
function Find-OwnedProcesses {
    @(Get-CimInstance Win32_Process -Filter "Name = 'layerlens.exe' OR Name = 'msedgewebview2.exe' OR Name = 'msedgedriver.exe'" |
        Where-Object { $_.CommandLine -and $_.CommandLine.Replace('\', '/') -match $pattern })
}
$clock = [Diagnostics.Stopwatch]::StartNew()
do {
    $remaining = @(Find-OwnedProcesses)
    if ($remaining.Count -eq 0 -or $clock.ElapsedMilliseconds -ge $WaitMs) { break }
    Start-Sleep -Milliseconds 100
} while ($true)
$result = [ordered]@{
    remaining = @($remaining | Select-Object ProcessId, Name)
    terminated = @()
}
if ($Terminate) {
    foreach ($candidate in $remaining) {
        $current = Get-CimInstance Win32_Process -Filter "ProcessId = $($candidate.ProcessId)"
        if ($null -ne $current -and $current.CreationDate -eq $candidate.CreationDate -and $current.CommandLine -eq $candidate.CommandLine) {
            Stop-Process -Id $current.ProcessId -Force -ErrorAction Stop
            $result.terminated += $current.ProcessId
        }
    }
}
$result | ConvertTo-Json -Depth 5 -Compress
