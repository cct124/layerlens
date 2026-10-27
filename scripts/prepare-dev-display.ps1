param(
    [ValidateSet('prepare', 'restore')][string]$Mode = 'prepare',
    [string]$ReportRoot = '.local/dev-mcp',
    [switch]$AllowLocalDisplayChange
)
# 仅准备隔离测试桌面；真实功能仍经 MCP。默认拒绝在普通开发桌面修改显示模式。
# ChangeDisplaySettings 使用动态标志 0，不写注册表；退出阶段复原原始模式。
$ErrorActionPreference = 'Stop'
if ($env:GITHUB_ACTIONS -ne 'true' -and !$AllowLocalDisplayChange) { throw 'Display changes require CI or explicit -AllowLocalDisplayChange.' }
if ((Get-Process -Id $PID).SessionId -le 0) { throw 'An interactive test desktop is required.' }
[void](New-Item -ItemType Directory -Path $ReportRoot -Force)
$displayRoot = (Resolve-Path -LiteralPath $ReportRoot).Path
$originalFile = Join-Path $displayRoot 'display-original.xml'
$reportFile = Join-Path $displayRoot ('display-' + $Mode + '.json')
$report = @{ Mode=$Mode; Passed=$false; RegistryUpdated=$false }
try {
    Add-Type -TypeDefinition @'
using System;
using System.Runtime.InteropServices;
// DEVMODEW display union, explicit initialized strings and exact buffer size.
[StructLayout(LayoutKind.Sequential, CharSet=CharSet.Unicode)]
public struct TestDisplayMode {
 [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string dmDeviceName;
 public ushort dmSpecVersion,dmDriverVersion,dmSize,dmDriverExtra;
 public uint dmFields; public int x,y; public uint dmDisplayOrientation,dmDisplayFixedOutput;
 public short dmColor,dmDuplex,dmYResolution,dmTTOption,dmCollate;
 [MarshalAs(UnmanagedType.ByValTStr, SizeConst=32)] public string dmFormName;
 public ushort dmLogPixels; public uint dmBitsPerPel,dmPelsWidth,dmPelsHeight,dmDisplayFlags,dmDisplayFrequency;
 public uint dmICMMethod,dmICMIntent,dmMediaType,dmDitherType,dmReserved1,dmReserved2,dmPanningWidth,dmPanningHeight;
 public static TestDisplayMode Create() {var m=new TestDisplayMode();m.dmDeviceName="";m.dmFormName="";m.dmSize=(ushort)Marshal.SizeOf(typeof(TestDisplayMode));if(m.dmSize!=220)throw new Exception("Unexpected DEVMODE size");return m;}
}
public static class TestDisplay {
 [DllImport("user32.dll",CharSet=CharSet.Unicode,EntryPoint="EnumDisplaySettingsW")] private static extern bool Enum(string device,uint index,ref TestDisplayMode mode);
 [DllImport("user32.dll",CharSet=CharSet.Unicode,EntryPoint="ChangeDisplaySettingsW")] public static extern int Change(ref TestDisplayMode mode,uint flags);
 public static bool Current(ref TestDisplayMode mode) {return Enum(null,UInt32.MaxValue,ref mode);}
 public static bool At(uint index,ref TestDisplayMode mode) {return Enum(null,index,ref mode);}
}
'@
    $current = [TestDisplayMode]::Create()
    if (![TestDisplay]::Current([ref]$current)) { throw 'Cannot query current display mode.' }
    $report.Before = @{ Width=$current.dmPelsWidth; Height=$current.dmPelsHeight }
    if ($Mode -eq 'prepare') {
        if (Test-Path -LiteralPath $originalFile) { throw 'Original display state already exists; restore or use a fresh report directory.' }
        $current | Export-Clixml -LiteralPath $originalFile
        $target = $current
        if ($current.dmPelsWidth -lt 1920 -or $current.dmPelsHeight -lt 1080) {
            $candidates = @()
            for ([uint32]$index=0; $index -lt 1024; $index++) {
                $candidate=[TestDisplayMode]::Create()
                if (![TestDisplay]::At($index,[ref]$candidate)) { break }
                if ($candidate.dmBitsPerPel -eq 32 -and $candidate.dmPelsWidth -ge 1920 -and $candidate.dmPelsHeight -ge 1080) { $candidates += ,$candidate }
            }
            if (!$candidates.Count) { throw 'No supported display mode can fit the declared test windows.' }
            $target=@($candidates | Sort-Object @{Expression={[long]$_.dmPelsWidth*$_.dmPelsHeight}},dmDisplayFrequency)[0]
        }
    } else {
        if (!(Test-Path -LiteralPath $originalFile)) { throw 'Original display state is missing; no restore was attempted.' }
        $saved=Import-Clixml -LiteralPath $originalFile
        $target=[TestDisplayMode]::Create()
        foreach ($property in $saved.PSObject.Properties) { $target.($property.Name)=$property.Value }
    }
    $report.Changed=$current.dmPelsWidth -ne $target.dmPelsWidth -or $current.dmPelsHeight -ne $target.dmPelsHeight
    if ($report.Changed) {
        $target.dmFields=0x180000
        $result=[TestDisplay]::Change([ref]$target,2)
        if ($result -ne 0) { throw "Display mode validation failed: $result" }
        $result=[TestDisplay]::Change([ref]$target,0)
        if ($result -ne 0) { throw "Dynamic display change failed: $result" }
    }
    $after=[TestDisplayMode]::Create()
    if (![TestDisplay]::Current([ref]$after)) { throw 'Cannot verify the actual display mode.' }
    $report.After=@{ Width=$after.dmPelsWidth; Height=$after.dmPelsHeight }
    if ($after.dmPelsWidth -ne $target.dmPelsWidth -or $after.dmPelsHeight -ne $target.dmPelsHeight) { throw 'Actual display mode differs from the requested mode.' }
    $report.Passed=$true
} catch {
    $report.Error=$_.Exception.Message
    throw
} finally {
    $report | ConvertTo-Json -Depth 4 | Out-File -LiteralPath $reportFile -Encoding utf8
}
