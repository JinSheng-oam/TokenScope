# TokenScope

> 轻量级、多设备 AI Token 监控桌面软件与自建同步 Hub。
> 支持 Codex、Claude Code、Antigravity 等开发工具，支持本机统计、自建 Hub、多设备拓扑同步。

---

## 核心特性

- **现代桌面端（Linear Craft 设计语言）**
  - 基于 Rust + Tauri 2 + Svelte 5 构建，开箱即用，资源极简（资源包 gzip < 31 kB）。
  - 支持 **Dark（深灰黑曜石）** / **Light（浅色微冷灰）** / **System（跟随系统）** 模式一键无缝切换。
  - 严谨的排版与视觉层级：1px 微光细线边框、等宽数据数字（Tabular Figures）、原生轻量级 SVG 7日波形趋势图。
- **严密的数据精度分层**
  - 严格区分真实精确 Token、估算 Token 与模型配额水位，坚决不使用虚假数据冒充实际消耗。
  - 支持 Prompt Cache（缓存命中 Token）、输入 Token、输出 Token 三态明细统计。
- **多维度监控下钻**
  - **Dashboard（总览）**：30天总量、今日消耗、预估成本、活跃工具环比趋势、近期高额调用事件。
  - **Devices（多设备）**：全网设备卡片、在线状态与同步时间、未分配归属（`unassigned`）显式分离。
  - **Tools / Models / Projects**：按工具类型（Codex、Claude Code、Antigravity）、模型排行及项目路径聚合分析。
  - **Antigravity 额度水位**：监控各模型分组配额比例、重置倒计时（严格遵循 Quota `0 ≠ null` 语义）。
- **多端同步服务（去中心化 WebDAV / 自建 Hub 双模支持）**
  - **去中心化 WebDAV 云盘同步**：支持坚果云（Nutstore）、Nextcloud、Alist、InfiniCLOUD 等标准 WebDAV 网盘。每台设备独占目录写入，无并发写冲突；密码经由系统原生安全密码库（Keyring）加密保存，磁盘零明文残留。
  - **自建 Hub 跨端同步**：轻量级 Go + SQLite 自建 Hub 服务，单 Docker Compose 一键启动（内置 Caddy 自动 HTTPS），支持 256-bit Base32 一次性安全配对码与增量 Change Feed。
- **低内存后台常驻架构**
  - 登录自启动支持 `--background` 模式，不预创建 WebView 进程，仅常驻系统托盘与 Agent 数据采集器。

---

## 架构概览

```
[ 本机开发工具 (Codex / Claude Code / Antigravity) ]
                       │
                       ▼
            [ 本机 TokenScope Agent ] ──> [ 本机 SQLite (tokenscope.db v8) ]
                       │                               ▲
        ┌──────────────┴──────────────┐                │ (短命 IPC 查询)
        ▼                             ▼                │
[ WebDAV 网盘 (坚果云/Nextcloud) ]  [ 自建 Hub ] ─── [ TokenScope 桌面客户端 ]
   (去中心化批次同步)              (Go + SQLite)        (Tauri 2 + Svelte 5)
```

---

## 快速上手

### 环境要求
- Node.js 18+ (推荐 Node.js 22+)
- Rust 1.80+ (推荐最新稳定版，使用 `rustup`)
- PowerShell 7 (Windows 环境推荐)

### 1. 启动桌面端

```bash
# 方式 A：直接运行启动脚本（Windows）
start-desktop.bat

# 方式 B：手动通过 npm 启动开发环境
cd apps/desktop
npm install
npm run tauri:dev
```

### 2. 命令行 Agent 使用

```bash
# 本机单次全量扫描
cargo run -p tokenscope-agent -- --scan-all --db ./tokenscope.db

# 启动后台常驻采集 Daemon (含 WebDAV 自动同步)
cargo run -p tokenscope-agent -- --scan-all --daemon --interval-seconds 30 --sync-webdav-config ./sync-webdav.json --db ./tokenscope.db

# 执行一次性 WebDAV 同步
cargo run -p tokenscope-agent -- --sync-webdav-once --sync-webdav-config ./sync-webdav.json --db ./tokenscope.db

# 查看本地队列状态
cargo run -p tokenscope-agent -- --sync-status --db ./tokenscope.db
```

### 3. 部署自建同步 Hub

```bash
cd deploy
docker compose up -d
```

---

## 常用命令

| 命令 | 描述 |
| :--- | :--- |
| `npm --prefix apps/desktop run check` | Svelte 5 前端类型与模板检查 |
| `npm --prefix apps/desktop test` | 前端组件与工具函数单测 (Vitest) |
| `cargo clippy --workspace --all-targets` | Rust 工作区代码质量检查 |
| `cargo test --workspace` | 全量后端单元与集成测试 |

---

## 开源协议

本项目采用 [MIT 许可证](LICENSE)。
