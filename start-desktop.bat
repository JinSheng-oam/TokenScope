@echo off
cd /d "%~dp0apps\desktop"

rem 检查并补全 Cargo / Rust 环境变量
where cargo >nul 2>nul
if %errorlevel% neq 0 (
    if exist "D:\Rust\cargo\bin\cargo.exe" set "PATH=D:\Rust\cargo\bin;%PATH%"
    if exist "%USERPROFILE%\.cargo\bin\cargo.exe" set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
)

rem 释放 1420 端口占用的旧进程
for /f "tokens=5" %%a in ('netstat -aon ^| findstr :1420 ^| findstr LISTENING') do (
    taskkill /f /pid %%a >nul 2>nul
)

echo [TokenScope] 正在启动桌面端开发服务...
call npm run tauri:dev
pause
