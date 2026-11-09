# TokenScope

TokenScope 是一款本地 AI 用量监控桌面软件，支持 Codex、Claude Code、Antigravity、DeepSeek Harness 和 Pi Agent 等来源。桌面端使用 Tauri 2 和 Svelte 5，Rust Agent 在后台采集，用量保存在本机 SQLite 中。

- 按日期、设备、工具、模型、项目和会话查看 Token 用量。模型调用量仅统计本机，配额与 Token 用量分开显示。
- 查看估算成本和模型定价，支持手动定价、关键词匹配开关及美元 / 人民币显示。估算金额不等于实际账单。
- 通过 WebDAV 或自建 Hub 同步设备用量。Agent 配置还支持维护统一指令，单独通过 WebDAV 在设备之间同步。

## 开发环境

本地工作区如包含 docs/，可从 docs/README.md 进入开发、架构和运维文档。该目录按项目约定不随 GitHub 仓库发布。

使用 Node.js 22 或更高版本、Rust 1.90.0（由 `rust-toolchain.toml` 固定），并安装对应平台的 Tauri 构建依赖。Windows 默认使用 PowerShell 7。

在 Windows 可运行 `start-desktop.bat`，也可以从仓库根目录运行：

```text
npm --prefix apps/desktop ci
npm --prefix apps/desktop run tauri:dev
```

开发与打包命令会构建并暂存同版本 Agent。构建桌面安装包使用 `npm --prefix apps/desktop run tauri:build`。升级时配套使用桌面端和 Agent，避免旧程序读取新版本数据库。

## 命令行

在仓库根目录运行：

```text
cargo run -p tokenscope-agent -- --scan-all --db ./tokenscope.db
cargo run -p tokenscope-agent -- --sync-status --db ./tokenscope.db
cargo run -p tokenscope-agent -- --help
```

部署自建同步服务请参阅 [Hub 部署说明](deploy/README.md)。

## 验证

```text
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
npm --prefix apps/desktop run check
npm --prefix apps/desktop test
npm --prefix apps/desktop run build
```

本项目采用 [MIT 许可证](LICENSE)。
