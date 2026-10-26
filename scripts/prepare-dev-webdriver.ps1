# 按已安装或显式指定的 WebView2 目录准备驱动；不安装或更新系统运行时。
# 检测依据：https://learn.microsoft.com/microsoft-edge/webview2/concepts/distribution
[CmdletBinding()]
param(
    [string]$OutputRoot = '.local/dev-mcp/webdriver',
    [string]$ReportPath = '.local/dev-mcp/driver-preparation.json',
    [string]$RuntimePath
)
$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest
$projectRoot = Split-Path $PSScriptRoot -Parent
function Resolve-ProjectPath([string]$Value) {
    if ([IO.Path]::IsPathRooted($Value)) { return [IO.Path]::GetFullPath($Value) }
    return [IO.Path]::GetFullPath((Join-Path $projectRoot $Value))
}
$output = Resolve-ProjectPath $OutputRoot
$reportFile = Resolve-ProjectPath $ReportPath
$report = [ordered]@{ formatVersion = 1; passed = $false; phase = 'runtime'; startedAt = [DateTime]::UtcNow.ToString('o') }
New-Item -ItemType Directory -Force -Path (Split-Path $reportFile -Parent) | Out-Null
try {
    if (-not [Environment]::Is64BitOperatingSystem -or $env:PROCESSOR_ARCHITECTURE -ne 'AMD64') { throw '当前驱动准备只支持 Windows x64。' }
    if ($PSBoundParameters.ContainsKey('RuntimePath')) {
        if ([string]::IsNullOrWhiteSpace($RuntimePath)) { throw '指定的 RuntimePath 不能为空。' }
        $folder = Resolve-ProjectPath $RuntimePath
        $binary = Join-Path $folder 'msedgewebview2.exe'
        if (-not (Test-Path -LiteralPath $binary -PathType Leaf)) { throw '指定运行时目录缺少 msedgewebview2.exe；不回退到系统运行时。' }
        $version = (Get-Item -LiteralPath $binary).VersionInfo.FileVersion
        if ($version -notmatch '^\d+\.\d+\.\d+\.\d+$') { throw '指定运行时文件缺少有效版本。' }
        $runtime = [pscustomobject]@{ Version = $version; Folder = $folder; Binary = $binary }
        $report.runtimeSelection = 'explicitDirectory'
    } else {
        $clientId = '{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'
        $machineBase = Join-Path ([Environment]::GetEnvironmentVariable('ProgramFiles(x86)')) 'Microsoft/EdgeWebView/Application'
        $candidates = @(
            @{ Key = "HKLM:\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\$clientId"; Base = $machineBase },
            @{ Key = "HKCU:\Software\Microsoft\EdgeUpdate\Clients\$clientId"; Base = "$env:LOCALAPPDATA\Microsoft\EdgeWebView\Application" }
        )
        $installed = @(foreach ($candidate in $candidates) {
            if (Test-Path -LiteralPath $candidate.Key) {
                $version = Get-ItemPropertyValue -LiteralPath $candidate.Key -Name pv
                if ($version -match '^\d+\.\d+\.\d+\.\d+$' -and [version]$version -gt [version]'0.0.0.0') {
                    $folder = Join-Path $candidate.Base $version
                    $binary = Join-Path $folder 'msedgewebview2.exe'
                    if (Test-Path -LiteralPath $binary -PathType Leaf) {
                        [pscustomobject]@{ Version = $version; Folder = $folder; Binary = $binary }
                    }
                }
            }
        })
        $runtime = $installed | Sort-Object { [version]$_.Version } -Descending | Select-Object -First 1
        if ($null -eq $runtime) { throw '未找到注册且可读取的 WebView2 Runtime，请先安装运行时。' }
        $report.runtimeSelection = 'installed'
    }
    if ((Get-Item -LiteralPath $runtime.Binary).VersionInfo.FileVersion -ne $runtime.Version) { throw '运行时注册版本与文件版本不一致。' }
    $runtimeSignature = Get-AuthenticodeSignature -LiteralPath $runtime.Binary
    if ($runtimeSignature.Status -ne 'Valid' -or $runtimeSignature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        throw '运行时必须具有有效的 Microsoft Authenticode 签名。'
    }
    $report.runtimeSigner = $runtimeSignature.SignerCertificate.Subject
    $report.runtimeVersion = $runtime.Version
    $report.runtimePath = $runtime.Folder
    $report.runtimeSha256 = (Get-FileHash -LiteralPath $runtime.Binary -Algorithm SHA256).Hash.ToLowerInvariant()
    $report.phase = 'driver'
    $directory = Join-Path $output $runtime.Version
    $driver = Join-Path $directory 'msedgedriver.exe'
    $url = "https://msedgedriver.microsoft.com/$($runtime.Version)/edgedriver_win64.zip"
    $report.sourceUrl = $url
    $report.cached = Test-Path -LiteralPath $driver -PathType Leaf
    if (-not $report.cached) {
        New-Item -ItemType Directory -Force -Path $output | Out-Null
        $download = Join-Path $output ('download-' + [guid]::NewGuid().ToString('N'))
        New-Item -ItemType Directory -Path $download | Out-Null
        $archive = Join-Path $download 'edgedriver_win64.zip'
        Invoke-WebRequest -Uri $url -OutFile $archive -UseBasicParsing -TimeoutSec 90
        $report.archiveSha256 = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
        # 只提取预期的根目录可执行文件，ZIP 内其他路径不写入文件系统。
        Add-Type -AssemblyName System.IO.Compression.FileSystem
        $zip = [IO.Compression.ZipFile]::OpenRead($archive)
        try {
            $entry = $zip.GetEntry('msedgedriver.exe')
            if ($null -eq $entry -or $entry.Length -gt 100MB) { throw '驱动压缩包缺少有效 msedgedriver.exe。' }
            $candidateDriver = Join-Path $download 'msedgedriver.exe'
            [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $candidateDriver, $false)
        } finally { $zip.Dispose() }
    } else { $candidateDriver = $driver }
    $signature = Get-AuthenticodeSignature -LiteralPath $candidateDriver
    if ($signature.Status -ne 'Valid' -or $signature.SignerCertificate.Subject -notmatch 'O=Microsoft Corporation') {
        throw '驱动必须具有有效的 Microsoft Authenticode 签名。'
    }
    $driverVersion = (Get-Item -LiteralPath $candidateDriver).VersionInfo.FileVersion
    if ($driverVersion -ne $runtime.Version) { throw "驱动版本 $driverVersion 与运行时 $($runtime.Version) 不一致。" }
    if (-not $report.cached) {
        New-Item -ItemType Directory -Force -Path $directory | Out-Null
        Copy-Item -LiteralPath $candidateDriver -Destination $driver -ErrorAction Stop
    }
    $report.driverPath = $driver
    $report.driverVersion = $driverVersion
    $report.driverSha256 = (Get-FileHash -LiteralPath $driver -Algorithm SHA256).Hash.ToLowerInvariant()
    $report.signer = $signature.SignerCertificate.Subject
    $report.passed = $true
    $report.phase = 'complete'
} catch {
    $report.error = $_.Exception.Message
    throw
} finally {
    # 元数据可以更新，驱动按版本复用；失败也保留阶段，不回退到任意旧版。
    [IO.File]::WriteAllText($reportFile, ($report | ConvertTo-Json -Depth 8), [Text.UTF8Encoding]::new($false))
    Write-Output "WebDriver preparation report: $reportFile"
}
