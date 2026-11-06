# TokenScope Hub 自托管部署（M1-06）

此部署模板使用 Go Hub + SQLite + Caddy 自动 HTTPS。适合个人/小团队多电脑同步；当前是单 Hub 单信任空间，不适合互不信任的多租户共享。

## 1. 前置条件

- Docker Engine 24+、Docker Compose v2。
- 一个指向服务器公网 IP 的 DNS 名称，例如 `tokens.example.com`。
- 服务器 TCP 80/443 可从公网访问；如需 HTTP/3，开放 UDP 443。
- 确认 `172.30.0.0/24` 没有与现有 Docker/VPN 网段冲突；冲突时同步修改 `compose.yaml` 的 network subnet 与 `TOKENSCOPE_TRUSTED_PROXY_CIDRS`。

## 2. 配置并启动

```bash
cd deploy
cp .env.example .env
# 编辑 TOKENSCOPE_DOMAIN、ACME_EMAIL
# 生成管理 Token：openssl rand -hex 32
# 将结果填入 .env 的 TOKENSCOPE_ADMIN_TOKEN
chmod 600 .env
docker compose config
docker compose up -d --build
docker compose ps
docker compose logs -f caddy hub
```

Caddy 自动申请并续签证书。客户端 Hub URL 填 `https://你的域名`。不要把 Hub 8787 端口映射到宿主机公网；它只在 Docker 私有网络内开放。

## 3. 生成一次性配对码

```bash
docker compose run --rm hub pair-code --db /data/tokenscope-hub.db
```

配对码只显示一次，有效期 5 分钟。不要把配对码写进工单、shell 历史或聊天记录。

## 4. 健康检查与日志

```bash
curl -fsS https://你的域名/healthz
docker compose ps
docker compose logs --since=30m hub
```

不要在生产日志中打印 Authorization Header、配对码、设备 Token 或请求体。

## 5. 备份与恢复

Hub 使用 SQLite WAL。为确保一致性，推荐短暂停机后备份 Docker 数据卷：

```bash
cd deploy
docker compose stop hub
mkdir -p backups
docker run --rm \
  -v tokenscope_hub-data:/data:ro \
  -v "$PWD/backups:/backup" \
  alpine:3.21 sh -c 'tar czf /backup/tokenscope-hub-$(date -u +%Y%m%dT%H%M%SZ).tgz -C /data .'
docker compose start hub
```

恢复前先停止 Hub，并将备份解压回同一数据卷。不要在 Hub 运行时只复制主 `.db` 文件而忽略 `-wal`；也不要执行 `docker compose down -v`，后者会删除数据卷。

建议至少每日备份、异机保存，并定期执行恢复演练。Caddy 的 `caddy-data` 卷保存证书状态，建议一并备份。

## 6. 安全边界

- Hub 容器非 root、只读根文件系统、丢弃 Linux capabilities、启用 `no-new-privileges`。首次启动由一次性 `hub-data-init` 容器将命名卷目录设置为 UID/GID `10001:10001` 和 `0700`，否则只读权限检查会阻止 Hub 启动。
- Hub 数据只写入 `/data` 持久卷，目录由程序限制为 `0700`，数据库文件为 `0600`（非 Windows）。
- Caddy 是唯一公网入口；Hub 仅信任 `172.30.0.0/24` 内反代传来的 HTTPS 标记。
- 管理 Token 仅用于管理员 API，不用于设备同步。
- 当前配对到同一个 Hub 的设备属于同一信任空间，彼此可读取已同步的用量数据。不要向不同客户共用此实例。
- 该模板没有提供账号/租户隔离、公开注册或公网管理 Web UI。

## 7. 更新

```bash
docker compose pull caddy
docker compose build --pull hub
docker compose up -d
```

更新前备份数据卷；升级后检查 `/healthz`、设备列表和同步游标。SQLite migration 是前向迁移，回滚旧镜像前应先验证 schema 兼容，不要直接降级覆盖。
