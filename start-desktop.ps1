# TokenScope 桌面端启动脚本
Set-Location "$PSScriptRoot\apps\desktop"

# 检查 1420 端口
$connections = Get-NetTCPConnection -LocalPort 1420 -State Listen -ErrorAction SilentlyContinue
if ($connections) {
    foreach ($c in $connections) {
        Stop-Process -Id $c.OwningProcess -Force -ErrorAction SilentlyContinue
    }
}

Write-Host "[TokenScope] 正在启动桌面端..." -ForegroundColor Cyan
npm run tauri:dev
