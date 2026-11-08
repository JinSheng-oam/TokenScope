[CmdletBinding()]
param(
    [ValidateSet('msi', 'nsis')]
    [string]$Bundle = 'msi',

    [switch]$SkipBuild,
    [switch]$ValidateOnly
)

<#
.SYNOPSIS
Builds and replaces the currently installed Windows TokenScope binaries.

.DESCRIPTION
Uses the registered installation directory. Stops only the desktop instance
and its own Agent, makes a SQLite online backup, and replaces the matching
desktop, library, Agent, and manifest files. Restarts a running application.
The MSI/NSIS registration and user configuration are left in place.

.PARAMETER Bundle
Installer bundle to build when SkipBuild is not specified. Defaults to MSI.

.PARAMETER SkipBuild
Uses the existing release binaries and staged Agent manifest.

.PARAMETER ValidateOnly
Checks the installed target, processes, and release artifacts without stopping
or changing the running application.
#>

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if ($PSVersionTable.PSVersion.Major -lt 7) {
    throw 'Run this script with PowerShell 7.'
}
if (-not $IsWindows) {
    throw 'This script supports Windows only.'
}

$repoRoot = Split-Path -Parent $PSScriptRoot
$desktopRoot = Join-Path $repoRoot 'apps\desktop'
$releaseRoot = Join-Path $desktopRoot 'src-tauri\target\release'
$resourceRoot = Join-Path $desktopRoot 'src-tauri\resources'

if (-not $SkipBuild -and -not $ValidateOnly) {
    & (Join-Path $PSScriptRoot 'build-installer.ps1') -Bundles $Bundle
    if ($LASTEXITCODE -ne 0) {
        throw "Installer build failed with exit code $LASTEXITCODE."
    }
}

$registrations = @(
    Get-ItemProperty -Path @(
        'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\*',
        'HKLM:\Software\WOW6432Node\Microsoft\Windows\CurrentVersion\Uninstall\*'
    ) -ErrorAction SilentlyContinue | Where-Object {
        $_.PSObject.Properties['DisplayName'] -and $_.DisplayName -eq 'TokenScope'
    }
)
if ($registrations.Count -ne 1 -or -not $registrations[0].PSObject.Properties['InstallLocation'] -or
    [string]::IsNullOrWhiteSpace($registrations[0].InstallLocation)) {
    throw '无法唯一确定 TokenScope 的已安装目录。'
}
$installDir = [IO.Path]::GetFullPath($registrations[0].InstallLocation).TrimEnd('\')
$installItem = Get-Item -LiteralPath $installDir
if (-not $installItem.PSIsContainer -or ($installItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw '安装目录无效或是重解析点。'
}
$installedResources = Join-Path $installDir 'resources'
$resourcesItem = Get-Item -LiteralPath $installedResources
if (-not $resourcesItem.PSIsContainer -or ($resourcesItem.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
    throw '安装资源目录无效或是重解析点。'
}

$manifestPath = Join-Path $resourceRoot 'agent-build.json'
$manifest = Get-Content -LiteralPath $manifestPath -Raw | ConvertFrom-Json
$appConfig = Get-Content -LiteralPath (Join-Path $desktopRoot 'src-tauri\tauri.conf.json') -Raw | ConvertFrom-Json
$arch = [Runtime.InteropServices.RuntimeInformation]::OSArchitecture.ToString().ToLowerInvariant()
if ($manifest.buildProfile -ne 'release' -or $manifest.targetPlatform -ne 'win32' -or
    $manifest.targetArch -ne $arch -or $manifest.binary -ne 'tokenscope-agent.exe' -or
    $manifest.appVersion -ne $appConfig.version) {
    throw '构建的 Agent 清单与当前桌面版本或系统平台不匹配。'
}
$files = @(
    @{ Source = (Join-Path $resourceRoot 'tokenscope-agent.exe'); Target = (Join-Path $installedResources 'tokenscope-agent.exe'); Backup = 'tokenscope-agent.exe' },
    @{ Source = $manifestPath; Target = (Join-Path $installedResources 'agent-build.json'); Backup = 'agent-build.json' },
    @{ Source = (Join-Path $releaseRoot 'tokenscope_desktop_lib.dll'); Target = (Join-Path $installDir 'tokenscope_desktop_lib.dll'); Backup = 'tokenscope_desktop_lib.dll' },
    @{ Source = (Join-Path $releaseRoot 'tokenscope-desktop.exe'); Target = (Join-Path $installDir 'tokenscope-desktop.exe'); Backup = 'tokenscope-desktop.exe' }
)
foreach ($file in $files) {
    foreach ($path in @($file.Source, $file.Target)) {
        $item = Get-Item -LiteralPath $path
        if ($item.PSIsContainer -or ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) {
            throw "程序文件无效或是重解析点: $path"
        }
    }
}
if ((Get-FileHash -LiteralPath $files[0].Source -Algorithm SHA256).Hash -ne $manifest.sha256) {
    throw '构建的 Agent 与其清单校验值不一致。'
}
$node = Get-Command node.exe -ErrorAction Stop
& $node.Source --no-warnings -e "const s=require('node:sqlite');if(typeof s.backup!=='function')process.exit(1)"
if ($LASTEXITCODE -ne 0) {
    throw '需要支持 node:sqlite backup 的 Node.js 22。'
}

$desktopPath = $files[3].Target
$agentPath = $files[0].Target
$dbPath = Join-Path $env:LOCALAPPDATA 'dev.tokenscope.desktop\tokenscope.db'
$shutdownPath = "$dbPath.shutdown"
$processes = @(Get-CimInstance Win32_Process | Where-Object { $_.Name -in @('tokenscope-desktop.exe', 'tokenscope-agent.exe') })
if (@($processes | Where-Object { -not $_.ExecutablePath }).Count) {
    throw '无法确认所有 TokenScope 进程的可执行文件路径。'
}
$desktopProcesses = @($processes | Where-Object { $_.ExecutablePath -ieq $desktopPath })
if ($desktopProcesses.Count -gt 1) {
    throw '安装目录有多个桌面进程，请先退出多余实例。'
}
$desktopPid = if ($desktopProcesses.Count) { [int]$desktopProcesses[0].ProcessId } else { 0 }
$installedAgents = @($processes | Where-Object { $_.ExecutablePath -ieq $agentPath })
foreach ($agent in $installedAgents) {
    if (-not $desktopPid -or $agent.ParentProcessId -ne $desktopPid -or -not $agent.CommandLine) {
        throw '检测到不是该桌面实例启动的 Agent，未对其发送停止请求。'
    }
    if ($agent.CommandLine -notmatch '(?i)--shutdown-file\s+(?:"([^"]+)"|(\S+))') {
        throw '无法确认 Agent 的停止标记路径。'
    }
    $agentMarker = if ($Matches.ContainsKey(1)) { $Matches[1] } else { $Matches[2] }
    if ([IO.Path]::GetFullPath($agentMarker) -ine $shutdownPath) {
        throw 'Agent 的停止标记路径与本机数据库不匹配。'
    }
}
if (@($processes | Where-Object {
    $_.Name -eq 'tokenscope-agent.exe' -and $_.ExecutablePath -ine $agentPath -and
    $_.CommandLine -and $_.CommandLine.IndexOf($dbPath, [StringComparison]::OrdinalIgnoreCase) -ge 0
}).Count) {
    throw '其他 Agent 正在使用本机数据库，请先停止该 Agent。'
}
if ($installedAgents.Count -gt 1 -or ($installedAgents.Count -and (Test-Path -LiteralPath $shutdownPath))) {
    throw 'Agent 数量异常或已有停止请求，未修改运行状态。'
}

if ($ValidateOnly) {
    Write-Host "已检查安装目录和构建文件: $installDir"
    return
}

Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class TokenScopeReinstallWindows {
    public delegate bool EnumCallback(IntPtr window, IntPtr parameter);
    [DllImport("user32.dll")] private static extern bool EnumWindows(EnumCallback callback, IntPtr parameter);
    [DllImport("user32.dll")] private static extern uint GetWindowThreadProcessId(IntPtr window, out uint processId);
    [DllImport("user32.dll", CharSet=CharSet.Unicode)] private static extern int GetClassName(IntPtr window, StringBuilder text, int count);
    [DllImport("user32.dll")] private static extern bool IsWindowVisible(IntPtr window);
    [DllImport("user32.dll", SetLastError=true)] private static extern bool PostThreadMessage(uint threadId, uint message, UIntPtr wparam, IntPtr lparam);
    public static uint FindEventThread(uint processId) {
        uint result = 0;
        EnumWindows((window, parameter) => {
            uint owner; uint thread = GetWindowThreadProcessId(window, out owner);
            var name = new StringBuilder(256); GetClassName(window, name, name.Capacity);
            if (owner == processId && name.ToString() == "Tao Thread Event Target") { result = thread; return false; }
            return true;
        }, IntPtr.Zero);
        return result;
    }
    public static bool HasVisibleWindow(uint processId) {
        bool visible = false;
        EnumWindows((window, parameter) => {
            uint owner; GetWindowThreadProcessId(window, out owner);
            var name = new StringBuilder(256); GetClassName(window, name, name.Capacity);
            if (owner == processId && name.ToString() == "Tauri Window" && IsWindowVisible(window)) { visible = true; return false; }
            return true;
        }, IntPtr.Zero);
        return visible;
    }
    public static bool RequestExit(uint threadId) { return PostThreadMessage(threadId, 0x0012, UIntPtr.Zero, IntPtr.Zero); }
}
'@

$eventThread = if ($desktopPid) { [TokenScopeReinstallWindows]::FindEventThread($desktopPid) } else { 0 }
if ($desktopPid -and -not $eventThread) {
    throw '无法找到已安装桌面程序的事件线程。'
}
$wasVisible = $desktopPid -and [TokenScopeReinstallWindows]::HasVisibleWindow($desktopPid)
$backupRoot = Join-Path $env:LOCALAPPDATA 'dev.tokenscope.desktop\reinstall-backups'
$operationId = [guid]::NewGuid().ToString('N')
$backupDir = Join-Path $backupRoot $operationId
New-Item -ItemType Directory -Path $backupDir | Out-Null
$stages = [Collections.Generic.List[string]]::new()
$replaced = [Collections.Generic.List[hashtable]]::new()
$replacementComplete = $false
$desktopStopped = $false

function Wait-ForProcessExit([int]$ProcessId, [int]$Seconds) {
    $process = Get-Process -Id $ProcessId -ErrorAction SilentlyContinue
    if ($process -and -not $process.WaitForExit($Seconds * 1000)) {
        throw "等待进程退出超时: $ProcessId"
    }
}

try {
    foreach ($file in $files) {
        $file.BackupPath = Join-Path $backupDir $file.Backup
        Copy-Item -LiteralPath $file.Target -Destination $file.BackupPath
        $file.OriginalHash = (Get-FileHash -LiteralPath $file.Target -Algorithm SHA256).Hash
        if ((Get-FileHash -LiteralPath $file.BackupPath -Algorithm SHA256).Hash -ne $file.OriginalHash) {
            throw '原程序备份校验失败。'
        }
        $file.Stage = Join-Path (Split-Path -Parent $file.Target) ".tokenscope-$operationId-$($file.Backup)"
        if (Test-Path -LiteralPath $file.Stage) { throw '暂存文件已存在。' }
        Copy-Item -LiteralPath $file.Source -Destination $file.Stage
        $stages.Add($file.Stage)
        $file.SourceHash = (Get-FileHash -LiteralPath $file.Source -Algorithm SHA256).Hash
        if ((Get-FileHash -LiteralPath $file.Stage -Algorithm SHA256).Hash -ne $file.SourceHash) {
            throw '构建文件暂存校验失败。'
        }
    }

    if ($desktopPid -and $wasVisible) {
        $desktopProcess = Get-Process -Id $desktopPid -ErrorAction SilentlyContinue
        if ($desktopProcess) { [void]$desktopProcess.CloseMainWindow() }
    }
    if ($installedAgents.Count) {
        $signal = [IO.File]::Open($shutdownPath, [IO.FileMode]::CreateNew, [IO.FileAccess]::Write, [IO.FileShare]::Read)
        $signal.Dispose()
        Wait-ForProcessExit ([int]$installedAgents[0].ProcessId) 120
    }
    if ($desktopPid -and (Get-Process -Id $desktopPid -ErrorAction SilentlyContinue)) {
        if (-not [TokenScopeReinstallWindows]::RequestExit($eventThread)) {
            throw '无法请求桌面程序正常退出。'
        }
        Wait-ForProcessExit $desktopPid 30
    }
    $desktopStopped = $desktopPid -ne 0
    $remaining = @(Get-CimInstance Win32_Process | Where-Object { $_.ExecutablePath -ieq $agentPath -or $_.ExecutablePath -ieq $desktopPath })
    if ($remaining.Count) { throw '安装目录仍有 TokenScope 进程运行。' }

    if (Test-Path -LiteralPath $dbPath -PathType Leaf) {
        $nodeScript = @'
const { DatabaseSync, backup } = require('node:sqlite');
(async () => {
  const db = new DatabaseSync(process.argv[1], { readOnly: true });
  try { await backup(db, process.argv[2]); } finally { db.close(); }
  const copy = new DatabaseSync(process.argv[2], { readOnly: true });
  try { if (copy.prepare('PRAGMA quick_check').get().quick_check !== 'ok') throw new Error('database backup check failed'); }
  finally { copy.close(); }
})().catch((error) => { console.error(error.message); process.exitCode = 1; });
'@
        & $node.Source --no-warnings -e $nodeScript $dbPath (Join-Path $backupDir 'tokenscope.db')
        if ($LASTEXITCODE -ne 0) { throw '数据库在线备份或完整性检查失败。' }
    }

    foreach ($file in $files) {
        if ((Get-FileHash -LiteralPath $file.Target -Algorithm SHA256).Hash -ne $file.OriginalHash) {
            throw '安装文件在更新期间发生变化。'
        }
        $file.ReplacedCopy = Join-Path (Split-Path -Parent $file.Target) ".tokenscope-$operationId-replaced-$($file.Backup)"
        if (Test-Path -LiteralPath $file.ReplacedCopy) { throw '替换备份文件已存在。' }
        $stages.Add($file.ReplacedCopy)
        [IO.File]::Replace($file.Stage, $file.Target, $file.ReplacedCopy)
        $replaced.Add($file)
        if ((Get-FileHash -LiteralPath $file.Target -Algorithm SHA256).Hash -ne $file.SourceHash) {
            throw '替换后的程序校验失败。'
        }
    }
    $replacementComplete = $true
}
catch {
    $failure = $_
    $rollbackErrors = [Collections.Generic.List[string]]::new()
    if (-not $replacementComplete) {
        for ($index = $replaced.Count - 1; $index -ge 0; $index--) {
            $file = $replaced[$index]
            try {
                $rollback = Join-Path (Split-Path -Parent $file.Target) ".tokenscope-$operationId-rollback-$($file.Backup)"
                $discard = Join-Path (Split-Path -Parent $file.Target) ".tokenscope-$operationId-discard-$($file.Backup)"
                if ((Test-Path -LiteralPath $rollback) -or (Test-Path -LiteralPath $discard)) {
                    throw '回滚暂存文件已存在。'
                }
                Copy-Item -LiteralPath $file.BackupPath -Destination $rollback
                $stages.Add($rollback)
                $stages.Add($discard)
                [IO.File]::Replace($rollback, $file.Target, $discard)
                if ((Get-FileHash -LiteralPath $file.Target -Algorithm SHA256).Hash -ne $file.OriginalHash) {
                    throw '回滚文件校验失败。'
                }
            }
            catch { $rollbackErrors.Add("$($file.Target): $_") }
        }
    }
    if ($rollbackErrors.Count) {
        throw "更新失败: $failure`n回滚未完成: $($rollbackErrors -join '; ')`n备份位于: $backupDir"
    }
    if ($desktopStopped -and -not $replacementComplete) {
        if ($wasVisible) { Start-Process -FilePath $desktopPath -WorkingDirectory $installDir | Out-Null }
        else { Start-Process -FilePath $desktopPath -ArgumentList '--background' -WorkingDirectory $installDir -WindowStyle Hidden | Out-Null }
    }
    throw $failure
}
finally {
    foreach ($stage in $stages) {
        if (Test-Path -LiteralPath $stage -PathType Leaf) { Remove-Item -LiteralPath $stage }
    }
}

Write-Host "已替换 TokenScope: $installDir"
Write-Host "原程序和数据库备份: $backupDir"
if ($desktopPid) {
    if ($wasVisible) { $started = Start-Process -FilePath $desktopPath -WorkingDirectory $installDir -PassThru }
    else { $started = Start-Process -FilePath $desktopPath -ArgumentList '--background' -WorkingDirectory $installDir -WindowStyle Hidden -PassThru }
    $deadline = [DateTime]::UtcNow.AddSeconds(30)
    do {
        Start-Sleep -Milliseconds 500
        if ($started.HasExited) { throw '新版桌面程序启动后退出，请检查日志。' }
        $child = Get-CimInstance Win32_Process -Filter "Name='tokenscope-agent.exe'" |
            Where-Object { $_.ParentProcessId -eq $started.Id -and $_.ExecutablePath -ieq $agentPath }
    } while (-not $child -and [DateTime]::UtcNow -lt $deadline)
    if (-not $child) { throw '新版桌面程序已启动，但未检测到其 Agent，请检查日志。' }
    Write-Host '新版桌面程序和 Agent 已启动。'
}
