param([string]$OutputPath = '.local/public-psd-260919')

# 只下载已登记的公开测试数据，不执行归档内容；源文件和派生产物留在 .local。
$ErrorActionPreference = 'Stop'
$repository = Split-Path $PSScriptRoot -Parent
$localRoot = [IO.Path]::GetFullPath((Join-Path $repository '.local')) + [IO.Path]::DirectorySeparatorChar
$output = [IO.Path]::GetFullPath((Join-Path $repository $OutputPath))
if (-not $output.StartsWith($localRoot, [StringComparison]::OrdinalIgnoreCase)) {
    throw 'OutputPath 必须位于仓库 .local 内。'
}
$catalog = Get-Content (Join-Path $PSScriptRoot 'psd-samples.json') -Raw -Encoding UTF8 | ConvertFrom-Json
New-Item -ItemType Directory -Force $output | Out-Null

function Resolve-Output([string]$RelativePath) {
    $path = [IO.Path]::GetFullPath((Join-Path $output $RelativePath))
    if (-not $path.StartsWith($output + [IO.Path]::DirectorySeparatorChar, [StringComparison]::OrdinalIgnoreCase)) {
        throw "样本路径越界：$RelativePath"
    }
    New-Item -ItemType Directory -Force (Split-Path $path -Parent) | Out-Null
    return $path
}

function Verify-File($Entry, [string]$Path) {
    $bytes = [IO.File]::ReadAllBytes($Path)
    if ($Entry.size -and $bytes.Length -ne $Entry.size) { throw "长度不匹配：$Path" }
    $sha256 = (Get-FileHash -LiteralPath $Path -Algorithm SHA256).Hash.ToLowerInvariant()
    if ($Entry.sha256 -and $sha256 -ne $Entry.sha256) { throw "SHA-256 不匹配：$Path" }
    if ($Entry.gitSha1) {
        $hash = [Security.Cryptography.SHA1]::Create()
        try {
            $prefix = [Text.Encoding]::UTF8.GetBytes("blob $($bytes.Length)`0")
            $null = $hash.TransformBlock($prefix, 0, $prefix.Length, $prefix, 0)
            $null = $hash.TransformFinalBlock($bytes, 0, $bytes.Length)
            $actual = [BitConverter]::ToString($hash.Hash).Replace('-', '').ToLowerInvariant()
            if ($actual -ne $Entry.gitSha1) { throw "Git blob 不匹配：$Path" }
        } finally { $hash.Dispose() }
    }
    return $sha256
}

$records = @()
foreach ($entry in $catalog.files) {
    $path = Resolve-Output $entry.path
    if (-not (Test-Path -LiteralPath $path)) {
        # 不覆盖已有数据；中断留下的 .download 必须重新下载并校验，不能视为成功。
        Invoke-WebRequest -UseBasicParsing -Uri $entry.url -OutFile ($path + '.download') -TimeoutSec 180
        $null = Verify-File $entry ($path + '.download')
        Move-Item -LiteralPath ($path + '.download') -Destination $path
    }
    $digest = Verify-File $entry $path
    $records += [ordered]@{ path = $entry.path; url = $entry.url; size = (Get-Item -LiteralPath $path).Length; sha256 = $digest; scenario = $entry.scenario }
    Write-Output "Verified $($entry.path)"
}

Add-Type -AssemblyName System.IO.Compression.FileSystem
foreach ($archive in $catalog.archives) {
    $zip = [IO.Compression.ZipFile]::OpenRead((Resolve-Output $archive.path))
    try {
        foreach ($item in $archive.extract) {
            $matches = @($zip.Entries | Where-Object { $_.Name -eq $item.name })
            if ($matches.Count -ne 1) { throw "归档条目不唯一：$($item.name)" }
            $entry = $matches[0]
            if ($entry.Length -gt 100MB) { throw '解压条目超过 100 MiB 上限。' }
            $path = Resolve-Output $item.path
            if (-not (Test-Path -LiteralPath $path)) {
                [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, ($path + '.download'), $true)
                $null = Verify-File $item ($path + '.download')
                Move-Item -LiteralPath ($path + '.download') -Destination $path
            }
            $digest = Verify-File $item $path
            $records += [ordered]@{ path = $item.path; archive = $archive.path; archiveEntry = $entry.FullName; size = $entry.Length; sha256 = $digest; scenario = $item.scenario }
        }
        # 许可原文单独保存，归档路径不会直接用作本机路径。
        $index = 0
        foreach ($entry in $zip.Entries | Where-Object { $_.Name -match '(?i)(license|readme|help).*\.(txt|pdf)$' -and $_.Length -lt 2MB }) {
            $path = Resolve-Output ("source-info/" + $archive.id + "-" + $index + "-" + $entry.Name)
            if (-not (Test-Path -LiteralPath $path)) { [IO.Compression.ZipFileExtensions]::ExtractToFile($entry, $path) }
            $index++
        }
    } finally { $zip.Dispose() }
}
[ordered]@{ verifiedAt = [DateTime]::UtcNow.ToString('o'); upstreamCommit = $catalog.upstreamCommit; files = $records } |
    ConvertTo-Json -Depth 8 | Set-Content -LiteralPath (Join-Path $output 'manifest.json') -Encoding UTF8
Write-Output "Verified $($records.Count) files in $output"
