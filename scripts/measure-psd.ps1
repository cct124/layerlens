# Windows-only process measurement. Keep this file ASCII for Windows PowerShell 5.1.
# The child pauses at checkpoints so OS peak counters cannot be lost to a fast exit.
param(
    [Parameter(Mandatory = $true)][string]$InputPath,
    [Parameter(Mandatory = $true)][string]$OutputPath,
    [ValidateSet('metadata', 'preview', 'all')][string]$Mode = 'preview',
    [ValidateRange(1, 100)][int]$Runs = 6,
    [ValidateRange(1, 65536)][long]$MaxFileMiB = 64,
    [ValidateRange(1, [long]::MaxValue)][long]$MaxTotalPixels = 16777216,
    [ValidateRange(1, 65536)][long]$MaxDecodedMiB = 128,
    [ValidateRange(1, 4294967295)][long]$MaxLayers = 4096,
    [ValidateRange(1, 3600)][int]$TimeoutSeconds = 180,
    [ValidateRange(1, 65536)][long]$MaxWorkingSetMiB = 1024,
    [ValidateRange(1, 65536)][long]$MaxPrivateMiB = 1536
)

$ErrorActionPreference = 'Stop'
Set-StrictMode -Version Latest

function Quote-NativeArgument([string]$Value) {
    # Windows CRT quoting, including embedded quotes and trailing backslashes.
    $escaped = [regex]::Replace($Value, '(\\*)"', '$1$1\"')
    $escaped = [regex]::Replace($escaped, '(\\+)$', '$1$1')
    return '"' + $escaped + '"'
}

function Write-NewJson([string]$Path, $Value) {
    $bytes = [Text.Encoding]::UTF8.GetBytes(($Value | ConvertTo-Json -Depth 40))
    $stream = [IO.File]::Open($Path, [IO.FileMode]::CreateNew)
    try { $stream.Write($bytes, 0, $bytes.Length) } finally { $stream.Dispose() }
}

function Get-Sha256([string]$Path) {
    $hash = [Security.Cryptography.SHA256]::Create()
    $stream = [IO.File]::OpenRead($Path)
    try { return [BitConverter]::ToString($hash.ComputeHash($stream)).Replace('-', '').ToLowerInvariant() }
    finally { $stream.Dispose(); $hash.Dispose() }
}

function Read-Memory($Process) {
    $Process.Refresh()
    return [ordered]@{
        workingSetBytes = $Process.WorkingSet64
        privateBytes = $Process.PrivateMemorySize64
        processPeakWorkingSetBytes = $Process.PeakWorkingSet64
        processCpuMs = $Process.TotalProcessorTime.TotalMilliseconds
    }
}

function Assert-ResourceGuard([long]$WorkingSet, [long]$PrivateBytes) {
    if ($WorkingSet -gt $MaxWorkingSetMiB * 1MB) { throw 'Working-set guard exceeded.' }
    if ($PrivateBytes -gt $MaxPrivateMiB * 1MB) { throw 'Private-memory guard exceeded.' }
}

function Get-Statistics($Values) {
    $sorted = @($Values | Sort-Object)
    if ($sorted.Count -eq 0) { return $null }
    $middle = [int][Math]::Floor($sorted.Count / 2)
    $median = $sorted[$middle]
    if ($sorted.Count % 2 -eq 0) { $median = ($sorted[$middle - 1] + $sorted[$middle]) / 2 }
    return [ordered]@{ count = $sorted.Count; min = $sorted[0]; median = $median; max = $sorted[-1] }
}

if ($env:OS -ne 'Windows_NT') { throw 'Process measurements are currently supported on Windows only.' }
$repoRoot = [IO.Path]::GetFullPath((Join-Path $PSScriptRoot '..'))
$samplePath = (Resolve-Path -LiteralPath $InputPath).Path
$resultPath = [IO.Path]::GetFullPath($OutputPath)
if (Test-Path -LiteralPath $resultPath) { throw 'OutputPath must not already exist.' }
if (-not (Test-Path -LiteralPath (Split-Path -Parent $resultPath) -PathType Container)) {
    throw 'The output parent directory must exist.'
}

Push-Location $repoRoot
$previousConsoleEncoding = [Console]::OutputEncoding
try {
    # Cargo emits UTF-8 JSON even when launched through a noninteractive Node parent.
    [Console]::OutputEncoding = New-Object Text.UTF8Encoding $false
    # Build first; compilation and environment queries are outside the measured child lifetime.
    & cargo build --locked --release -p layerlens-core --example inspect_psd
    if ($LASTEXITCODE -ne 0) { throw 'Release build failed.' }
    $metadataJson = & cargo metadata --locked --no-deps --format-version 1
    if ($LASTEXITCODE -ne 0) { throw 'Cargo metadata failed.' }
    $targetDirectory = ($metadataJson | ConvertFrom-Json).target_directory
    $executable = Join-Path $targetDirectory 'release/examples/inspect_psd.exe'
    $os = Get-CimInstance Win32_OperatingSystem
    $machine = Get-CimInstance Win32_ComputerSystem
    $cpu = @(Get-CimInstance Win32_Processor | ForEach-Object { $_.Name })
    $rustVersion = & rustc -Vv
    if ($LASTEXITCODE -ne 0) { throw 'rustc version query failed.' }
    $revision = & git rev-parse HEAD
    if ($LASTEXITCODE -ne 0) { throw 'Git revision query failed.' }
    $dirty = @(& git status --porcelain).Count -gt 0
    if ($LASTEXITCODE -ne 0) { throw 'Git status query failed.' }
    $environment = [ordered]@{
        os = $os.Caption; osVersion = $os.Version; architecture = $os.OSArchitecture
        cpu = $cpu; logicalProcessors = $machine.NumberOfLogicalProcessors
        physicalMemoryBytes = $machine.TotalPhysicalMemory
        powerShellVersion = $PSVersionTable.PSVersion.ToString()
        rustc = $rustVersion; revision = $revision; dirtyWorktree = $dirty
        cargoLockSha256 = Get-Sha256 (Join-Path $repoRoot 'Cargo.lock')
        executableSha256 = Get-Sha256 $executable
        build = 'cargo build --locked --release -p layerlens-core --example inspect_psd'
        rustflags = $env:RUSTFLAGS; encodedRustflags = $env:CARGO_ENCODED_RUSTFLAGS
    }

    $arguments = @($samplePath, $resultPath, '--benchmark-runs', "$Runs",
        '--max-file-mib', "$MaxFileMiB", '--max-total-pixels', "$MaxTotalPixels",
        '--max-decoded-mib', "$MaxDecodedMiB", '--max-layers', "$MaxLayers")
    if ($Mode -eq 'metadata') { $arguments += '--metadata-only' }
    if ($Mode -eq 'preview') { $arguments += '--preview-only' }
    $startInfo = New-Object Diagnostics.ProcessStartInfo
    $startInfo.FileName = $executable
    $startInfo.Arguments = ($arguments | ForEach-Object { Quote-NativeArgument $_ }) -join ' '
    $startInfo.WorkingDirectory = $repoRoot
    $startInfo.UseShellExecute = $false
    $startInfo.CreateNoWindow = $true
    $startInfo.RedirectStandardInput = $true
    $startInfo.RedirectStandardOutput = $true
    $startInfo.RedirectStandardError = $true
    $startInfo.StandardOutputEncoding = [Text.Encoding]::UTF8
    $startInfo.StandardErrorEncoding = [Text.Encoding]::UTF8
    $process = New-Object Diagnostics.Process
    $process.StartInfo = $startInfo
    $checkpoints = New-Object 'System.Collections.Generic.List[object]'
    $peakWorkingSet = 0L
    $observedPrivatePeak = 0L
    $failure = $null
    $exitCode = $null
    $stderr = ''
    $stderrTask = $null
    $ownsOutput = $false
    $clock = [Diagnostics.Stopwatch]::StartNew()
    $started = $false
    try {
        $started = $process.Start()
        if (-not $started) { throw 'Could not start the measurement child.' }
        $stderrTask = $process.StandardError.ReadToEndAsync()
        $lineTask = $process.StandardOutput.ReadLineAsync()
        while ($true) {
            $lineReady = $lineTask.Wait(25)
            if (-not $process.HasExited) {
                $memory = Read-Memory $process
                $peakWorkingSet = [Math]::Max($peakWorkingSet, $memory.processPeakWorkingSetBytes)
                $observedPrivatePeak = [Math]::Max($observedPrivatePeak, $memory.privateBytes)
            }
            if ($clock.Elapsed.TotalSeconds -gt $TimeoutSeconds) { throw 'Measurement session timeout.' }
            if ($lineReady) {
                $line = $lineTask.Result
                if ($null -eq $line) { break }
                $checkpoint = $line | ConvertFrom-Json
                $position = $checkpoints.Count
                $expectedRun = if ($position -eq 0) { 0 } else { [int][Math]::Floor(($position - 1) / 3) + 1 }
                $expectedPhase = if ($position -eq 0) { 'ready' } else { @('opened', 'exported', 'released')[($position - 1) % 3] }
                if ($checkpoint.run -ne $expectedRun -or $checkpoint.phase -ne $expectedPhase -or $expectedRun -gt $Runs) {
                    throw 'Unexpected measurement checkpoint order.'
                }
                # Ready is emitted only after the child atomically creates its output directory.
                if ($position -eq 0) { $ownsOutput = $true }
                $snapshot = Read-Memory $process
                $snapshot.run = $checkpoint.run
                $snapshot.phase = $checkpoint.phase
                $checkpoints.Add($snapshot)
                $peakWorkingSet = [Math]::Max($peakWorkingSet, $snapshot.processPeakWorkingSetBytes)
                $observedPrivatePeak = [Math]::Max($observedPrivatePeak, $snapshot.privateBytes)
                Assert-ResourceGuard $peakWorkingSet $observedPrivatePeak
                $process.StandardInput.WriteLine('continue')
                $process.StandardInput.Flush()
                $lineTask = $process.StandardOutput.ReadLineAsync()
            }
            Assert-ResourceGuard $peakWorkingSet $observedPrivatePeak
        }
        if (-not $process.WaitForExit(5000)) { throw 'Child did not exit after closing stdout.' }
        $exitCode = $process.ExitCode
        $stderr = $stderrTask.Result
        if ($exitCode -ne 0) { throw "Measurement child failed (exit $exitCode)." }
        if ($checkpoints.Count -ne 1 + 3 * $Runs) { throw 'Incomplete checkpoint sequence.' }
    } catch {
        $failure = $_.Exception.Message
    } finally {
        if ($started) {
            if (-not $process.HasExited) {
                $process.Kill()
                if (-not $process.WaitForExit(5000)) { $failure = 'Child did not exit after termination.' }
            }
            if ($process.HasExited) { $exitCode = $process.ExitCode }
            if ($null -ne $stderrTask -and $stderrTask.Wait(5000)) { $stderr = $stderrTask.Result }
        }
        $clock.Stop()
        $process.Dispose()
    }

    # Child reports are compact summaries. Full PSD metadata is never copied into baseline.json.
    $reports = @()
    if ($ownsOutput) {
        for ($index = 1; $index -le $Runs; $index++) {
            $reportPath = Join-Path $resultPath ('run-{0:D3}/report.json' -f $index)
            if (Test-Path -LiteralPath $reportPath) {
                $reports += Get-Content -LiteralPath $reportPath -Raw -Encoding UTF8 | ConvertFrom-Json
            }
        }
    }
    $repeatStatistics = [ordered]@{}
    if ($null -eq $failure -and $reports.Count -eq $Runs) {
        $repeats = @($reports | Select-Object -Skip 1)
        foreach ($field in @('sourceReadMs', 'preflightMs', 'candidateParseMs', 'normalizationMs', 'sourceHashMs', 'sourceReleaseMs', 'totalMs')) {
            $repeatStatistics[$field] = Get-Statistics @($repeats | ForEach-Object { $_.openTimings.$field })
        }
        $repeatStatistics.documentReleaseMs = Get-Statistics @($repeats | ForEach-Object { $_.documentReleaseMs })
    } elseif ($null -eq $failure) {
        $failure = 'Missing child reports.'
    }
    $baseline = [ordered]@{
        schemaVersion = 1; measuredAt = [DateTimeOffset]::Now.ToString('o')
        environment = $environment; mode = $Mode; requestedRuns = $Runs
        cacheCondition = 'First run: OS cache uncontrolled. Subsequent runs: recently read same file; no cache flush, no decoded image cache.'
        measurement = 'OS peak working set observed every 25ms and at acknowledged checkpoints; private peak is sampled, not an OS high-water counter. Process peak is cumulative across runs.'
        limitations = @('Stage times exclude checkpoint waits and report serialization.',
            'PNG writes close files but do not request physical-device synchronization.',
            'Working set and private bytes include allocator/runtime retention; release is not proof of OS memory reclamation.',
            'CLI work is measured; no UI display, application cache, cancellation, or concurrent documents are tested.')
        guard = @{ timeoutSeconds = $TimeoutSeconds; maxWorkingSetMiB = $MaxWorkingSetMiB; maxPrivateMiB = $MaxPrivateMiB }
        sessionElapsedMs = $clock.Elapsed.TotalMilliseconds
        peakWorkingSetBytes = $peakWorkingSet; observedPrivatePeakBytes = $observedPrivatePeak
        exitCode = $exitCode; failure = $failure; stderr = $stderr
        checkpoints = @($checkpoints.ToArray()); runs = $reports; repeatOpenStatisticsMs = $repeatStatistics
    }
    if ($ownsOutput) {
        Write-NewJson (Join-Path $resultPath 'baseline.json') $baseline
    }
    if ($null -ne $failure) { throw $failure }
    Write-Output "Measured $Runs runs ($Mode). Report: $(Join-Path $resultPath 'baseline.json')"
} finally {
    [Console]::OutputEncoding = $previousConsoleEncoding
    Pop-Location
}
