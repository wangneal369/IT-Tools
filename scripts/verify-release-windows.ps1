#Requires -Version 7.0
<#
.SYNOPSIS
  Windows Release + thin LTO 本机验证：serde_json_path 验活、构建、体积红线与冒烟。

.DESCRIPTION
  复现 CI Release 门禁的 Windows 一侧：
    1. 定位 Windows 10/11 SDK 的 fxc.exe 并加入 PATH（GPUI Direct3D 11 着色器编译依赖）。
    2. cargo test --release -p devtools-tools --test release_lto_jsonpath
       —— 在真实 Release LTO 下验证 serde_json_path 5 个 inventory 内置函数。
    3. cargo build --release -p devtools-app。
    4. 测量 target\release\devtools.exe 未压缩字节数（<= 45 MiB）与便携 zip（<= 22 MiB）。
    5. 冒烟：--help、--silent 常驻后 --quit-running 有序退出。

.PARAMETER SkipSmokeTest
  跳过 GUI 冒烟（无交互桌面会话或已有实例在运行时使用）。

.PARAMETER SdkRoots
  额外的 Windows Kits 根目录，例如 D:\Windows Kits\10。

.EXAMPLE
  pwsh -File scripts/verify-release-windows.ps1
#>
[CmdletBinding()]
param(
    [switch]$SkipSmokeTest,
    [string[]]$SdkRoots = @()
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$PSNativeCommandUseErrorActionPreference = $true

$MaxBinaryBytes = 45 * 1MB
$MaxArchiveBytes = 22 * 1MB

$repoRoot = Split-Path -Parent $PSScriptRoot
Push-Location $repoRoot
try {
    function Write-Step([string]$Message) {
        Write-Host ""
        Write-Host "==> $Message" -ForegroundColor Cyan
    }

    function Find-FxcExe {
        $roots = [System.Collections.Generic.List[string]]::new()
        foreach ($candidate in @(
                $env:WindowsSdkDir,
                'C:\Program Files (x86)\Windows Kits\10',
                'C:\Program Files\Windows Kits\10',
                'D:\Windows Kits\10',
                'E:\Windows Kits\10'
            )) {
            if ($candidate -and (Test-Path -LiteralPath $candidate)) {
                $roots.Add($candidate)
            }
        }
        foreach ($root in $SdkRoots) {
            if (Test-Path -LiteralPath $root) {
                $roots.Add($root)
            }
        }
        foreach ($drive in Get-PSDrive -PSProvider FileSystem -ErrorAction SilentlyContinue) {
            $guess = Join-Path $drive.Root 'Windows Kits\10'
            if (Test-Path -LiteralPath $guess) {
                $roots.Add($guess)
            }
        }

        $found = foreach ($root in ($roots | Select-Object -Unique)) {
            $binRoot = Join-Path $root 'bin'
            if (-not (Test-Path -LiteralPath $binRoot)) { continue }
            foreach ($versionDir in Get-ChildItem -LiteralPath $binRoot -Directory) {
                $fxc = Join-Path $versionDir.FullName 'x64\fxc.exe'
                if (Test-Path -LiteralPath $fxc) {
                    [pscustomobject]@{
                        Path    = $fxc
                        Version = $versionDir.Name
                    }
                }
            }
        }

        $best = $found |
            Sort-Object -Property @{ Expression = { try { [version]$_.Version } catch { [version]'0.0.0.0' } } } -Descending |
            Select-Object -First 1
        if (-not $best) {
            throw "未找到 Windows SDK 的 fxc.exe。请安装 Windows 10/11 SDK，或用 -SdkRoots 指定 Kits 根目录。"
        }
        return $best.Path
    }

    function Assert-Budget([string]$Label, [long]$Actual, [long]$Limit) {
        $actualMiB = [math]::Round($Actual / 1MB, 2)
        $limitMiB = [math]::Round($Limit / 1MB, 2)
        Write-Host ("  {0}: {1} bytes ({2} MiB)，上限 {3} MiB" -f $Label, $Actual, $actualMiB, $limitMiB)
        if ($Actual -gt $Limit) {
            throw "$Label 超出体积红线：$Actual > $Limit bytes"
        }
    }

    Write-Step "定位 Windows SDK fxc.exe"
    $fxc = Find-FxcExe
    $fxcDir = Split-Path -Parent $fxc
    $env:PATH = "$fxcDir;$env:PATH"
    Write-Host "  fxc.exe: $fxc"

    Write-Step "Release LTO JSONPath 验活（serde_json_path inventory）"
    cargo test --release --locked -p devtools-tools --test release_lto_jsonpath

    Write-Step "构建 devtools-app (release)"
    cargo build --release --locked -p devtools-app

    $exe = Join-Path $repoRoot 'target\release\devtools.exe'
    if (-not (Test-Path -LiteralPath $exe)) {
        throw "未找到 Release 二进制：$exe"
    }

    Write-Step "校验 Windows GUI 子系统与嵌入式应用图标"
    $peBytes = [System.IO.File]::ReadAllBytes($exe)
    $peOffset = [BitConverter]::ToInt32($peBytes, 0x3C)
    $subsystem = [BitConverter]::ToUInt16($peBytes, $peOffset + 24 + 68)
    if ($subsystem -ne 2) {
        throw "devtools.exe PE Subsystem 应为 2 (IMAGE_SUBSYSTEM_WINDOWS_GUI)，实际为 $subsystem"
    }
    Write-Host "  PE Subsystem = 2 (IMAGE_SUBSYSTEM_WINDOWS_GUI) OK"

    if (-not ('DevToolsIconCheck' -as [type])) {
        Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
public static class DevToolsIconCheck {
    [DllImport("kernel32.dll", CharSet = CharSet.Unicode, SetLastError = true)]
    public static extern IntPtr LoadLibraryExW(string lpLibFileName, IntPtr hFile, uint dwFlags);
    [DllImport("kernel32.dll", SetLastError = true)]
    public static extern bool FreeLibrary(IntPtr hLibModule);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern IntPtr LoadImageW(IntPtr hInst, IntPtr name, uint type, int cx, int cy, uint fuLoad);
    [DllImport("user32.dll", SetLastError = true)]
    public static extern bool DestroyIcon(IntPtr hIcon);
}
"@
    }
    $hMod = [DevToolsIconCheck]::LoadLibraryExW($exe, [IntPtr]::Zero, 0x00000002)
    if ($hMod -eq [IntPtr]::Zero) {
        throw "无法以数据文件方式加载 $exe"
    }
    try {
        # 验证资源 ID = 1 的 RT_GROUP_ICON（与 gpui-pre-windows LoadImageW 一致）
        $hIcon = [DevToolsIconCheck]::LoadImageW($hMod, [IntPtr]1, 1, 0, 0, 0x00000040)
        if ($hIcon -eq [IntPtr]::Zero) {
            throw "devtools.exe 缺少嵌入式应用图标资源 (RT_GROUP_ICON ID = 1)"
        }
        [void][DevToolsIconCheck]::DestroyIcon($hIcon)
        Write-Host "  Embedded Icon (RT_GROUP_ICON #1) OK"
    }
    finally {
        [void][DevToolsIconCheck]::FreeLibrary($hMod)
    }

    $exeBytes = (Get-Item -LiteralPath $exe).Length

    Write-Step "测量未压缩二进制"
    Assert-Budget 'devtools.exe' $exeBytes $MaxBinaryBytes

    Write-Step "测量便携 zip"
    $zip = Join-Path $repoRoot 'target\release\DevTools-portable.zip'
    if (Test-Path -LiteralPath $zip) {
        Remove-Item -LiteralPath $zip -Force
    }
    Compress-Archive -LiteralPath $exe -DestinationPath $zip -Force
    $zipBytes = (Get-Item -LiteralPath $zip).Length
    Assert-Budget 'DevTools-portable.zip' $zipBytes $MaxArchiveBytes

    if (-not $SkipSmokeTest) {
        Write-Step "冒烟：--help"
        $helpOut = [System.IO.Path]::GetTempFileName()
        try {
            $helpProc = Start-Process -FilePath $exe -ArgumentList '--help' -RedirectStandardOutput $helpOut -Wait -PassThru
            if ($helpProc.ExitCode -ne 0) {
                throw "--help 退出码 $($helpProc.ExitCode)"
            }
            $helpText = Get-Content -LiteralPath $helpOut -Raw -Encoding utf8
            if ($helpText -notmatch 'DevTools') {
                throw "--help 输出未包含 DevTools 用法说明"
            }
            Write-Host "  --help OK"
        }
        finally {
            Remove-Item -LiteralPath $helpOut -Force -ErrorAction SilentlyContinue
        }

        Write-Step "冒烟：--silent 常驻 -> --quit-running 有序退出"
        $proc = Start-Process -FilePath $exe -ArgumentList '--silent' -PassThru
        try {
            # 等待单实例 IPC 与 Headless 初始化；若已有实例，本次进程会静默退出。
            Start-Sleep -Seconds 5
            $proc.Refresh()
            if ($proc.HasExited) {
                throw "--silent 进程提前退出：请确认没有已运行的 DevTools 实例（本脚本需要独占验证）。"
            }
            $quitProc = Start-Process -FilePath $exe -ArgumentList '--quit-running' -Wait -PassThru
            if ($quitProc.ExitCode -ne 0) {
                throw "--quit-running 退出码 $($quitProc.ExitCode)"
            }
            if (-not $proc.WaitForExit(30000)) {
                throw "--silent 实例未在 30s 内响应 --quit-running"
            }
            Write-Host ("  --silent -> --quit-running OK（退出码 {0}）" -f $proc.ExitCode)
        }
        finally {
            $proc.Refresh()
            if (-not $proc.HasExited) {
                Stop-Process -Id $proc.Id -Force -ErrorAction SilentlyContinue
            }
        }
    }
    else {
        Write-Host ""
        Write-Host "已跳过 GUI 冒烟（-SkipSmokeTest）。" -ForegroundColor Yellow
    }

    Write-Step "PASS"
    Write-Host ("  devtools.exe           = {0} bytes" -f $exeBytes)
    Write-Host ("  DevTools-portable.zip  = {0} bytes" -f $zipBytes)
    Write-Host ("  红线                     45 MiB / 22 MiB")
}
finally {
    Pop-Location
}
