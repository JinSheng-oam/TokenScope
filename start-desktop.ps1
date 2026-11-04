# TokenScope 桌面端启动脚本
Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

# 切换到 apps/desktop 目录
Set-Location (Join-Path $PSScriptRoot "apps\desktop")

# 检查 Cargo 环境变量，若当前会话未识别则自动补全
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    if (Test-Path "D:\Rust\cargo\bin\cargo.exe") {
        $env:PATH = "D:\Rust\cargo\bin;$env:PATH"
    } elseif (Test-Path "$env:USERPROFILE\.cargo\bin\cargo.exe") {
        $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
    }
}

# 释放占用 1420 端口的进程
$connections = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
if ($connections) {
    $pidsToKill = $connections.OwningProcess | Select-Object -Unique
    foreach ($p in $pidsToKill) {
        Stop-Process -Id $p -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "[TokenScope] 正在启动桌面端开发服务..." -ForegroundColor Cyan
npm run tauri:dev
