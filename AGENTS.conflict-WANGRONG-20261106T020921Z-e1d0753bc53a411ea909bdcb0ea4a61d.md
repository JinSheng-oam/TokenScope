# AGENTS.md

> TokenScope: 轻量级、多设备 AI Token 监控桌面端与跨端同步系统（Codex / Claude Code / Antigravity）。

## 1. 运行与验证

- **环境要求**：PowerShell 7 (`pwsh.exe`)、Node.js 22+、Rust 1.80+。
- **启动桌面端**：`cd apps/desktop && npm run tauri:dev`（或双击运行 `start-desktop.bat`）。
- **前端检查与测试**：`npm --prefix apps/desktop run check` 与 `npm --prefix apps/desktop test`。
- **后端全量测试**：`cargo test --workspace`（必须在 PowerShell 7 环境下执行）。

## 2. 技术栈

- **桌面端**：Tauri 2 + Svelte 5 (Runes 响应式) + TypeScript + Vite。
- **存储与核心**：Rust (Cargo Workspace) + 本机 SQLite (`tokenscope.db`，当前 Schema v9)。
- **同步网络**：去中心化 WebDAV 云盘同步（Keyring 加密安全存储）/ 自建 Hub 同步（Go + SQLite）。

## 3. 目录与架构约定

- `apps/desktop/`：Tauri 2 桌面端外壳（`src-tauri`）与 Svelte 5 数据分析仪表盘（`src`）。
- `crates/core-domain/`：规范模型事实定义、Token 分级精算与不可篡改记录哈希计算。
- `crates/local-store/`：本地 SQLite 持久化与增量迁移（v9: 仪表盘分类更新版本；v8: WebDAV 同步游标）。
- `crates/sync-client/`：跨端同步客户端库（WebDAV 去中心化协议与 Hub 配对协议）。
- `crates/tokenscope-agent/`：单机日志采集 Agent CLI 与常驻后台守护进程。
- **安全红线**：WebDAV 密码与 Hub Token 必须走 OS 原生 Keyring 凭据管理，严禁明文入库或落盘。

## 4. 平台与编码规范

- **Shell 约束**：所有终端命令强制使用 `C:\Program Files\PowerShell\7\pwsh.exe`。
- **编码格式**：`.bat` 脚本必须为 **ANSI (GBK) + CRLF**；`.ps1` 脚本必须为 **UTF-8 with BOM + CRLF**。
- **文件读取**：默认采用 UTF-8 编码读取。

## 5. 当前状态与下一步

- **当前状态**：Schema v9 迁移就绪；WebDAV 多端同步已全链路打通并完成集成测试与 UI 体验优化。
- **下一步规划**：多平台原生安装包打包流水线测试；扩展更多第三方模型与工具适配器。
