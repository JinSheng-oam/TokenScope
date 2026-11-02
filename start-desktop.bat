@echo off
chcp 65001 >nul
cd /d "%~dp0apps\desktop"

rem 清理可能占用 1420 端口的残留开发进程
for /f "tokens=5" %%a in ('netstat -aon ^| findstr :1420 ^| findstr LISTENING') do (
    taskkill /f /pid %%a >nul 2>nul
)

echo [TokenScope] 正在启动桌面端...
call npm run tauri:dev
pause
