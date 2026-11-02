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
$patterns = @($pattern)
$profileDirectories = 0
$configPath = Join-Path $ownedRoot 'config.json'
if (Test-Path -LiteralPath $configPath -PathType Leaf) {
    $config = [IO.File]::ReadAllText($configPath) | ConvertFrom-Json
    $profileKey = $config.PSObject.Properties['profileKey']
    if ($null -ne $profileKey -and $null -ne $profileKey.Value) {
        if ($profileKey.Value -cnotmatch '^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$') {
            throw '共享测试 profile 必须使用 UUID。'
        }
        $runDirectory = (Resolve-Path -LiteralPath $config.runRoot).Path
        $parent = Join-Path $runDirectory 'profiles'
        $profile = Join-Path $parent $profileKey.Value
        # 两个真实进程顺序复用的目录不在唯一报告根下，仍须检查其中的 WebView 子进程。
        # 只接受配置约束的 UUID 子目录，拒绝重解析目录，不从参数接受任意外部 profile。
        foreach ($candidate in @($parent, $profile)) {
            $item = Get-Item -LiteralPath $candidate
            if (-not $item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
                throw '共享 profile 不是普通测试目录。'
            }
        }
        $patterns += [regex]::Escape($profile.Replace('\', '/')) + '(?=[/"\s]|$)'
        $profileDirectories = 1
    }
}
function Find-OwnedProcesses {
    @(Get-CimInstance Win32_Process -Filter "Name = 'layerlens.exe' OR Name = 'msedgewebview2.exe' OR Name = 'msedgedriver.exe'" |
        Where-Object {
            $command = $_.CommandLine
            $command -and @($patterns | Where-Object { $command.Replace('\', '/') -match $_ }).Count -gt 0
        })
}
$clock = [Diagnostics.Stopwatch]::StartNew()
do {
    $remaining = @(Find-OwnedProcesses)
    if ($remaining.Count -eq 0 -or $clock.ElapsedMilliseconds -ge $WaitMs) { break }
    Start-Sleep -Milliseconds 100
} while ($true)
$result = [ordered]@{
    profileDirectories = $profileDirectories
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
