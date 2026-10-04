# Alpha Finance 快速部署指南

## 一键部署到 Ubuntu 24.04

### 前提条件
- Ubuntu 24.04 LTS 服务器
- sudo 权限
- 稳定的网络连接

### 快速部署

1. **克隆项目**
```bash
git clone https://github.com/cuihairu/alpha.git
cd alpha
```

2. **执行一键部署脚本**
```bash
sudo ./scripts/deploy-ubuntu.sh
```

### 部署完成后的访问地址

- **Web 应用**: `http://your-server-ip`（需 nginx；脚本仅在已安装 nginx 时配置站点，未装时请先 `apt install -y nginx` 后重跑）
- **API 网关**: `http://your-server-ip:9080/health`（网关 `/api/v1/*` 反代 data-engine；接口清单见 [docs/market-data-api.md](docs/market-data-api.md)）
- **ClickHouse**: `http://localhost:8123`（防火墙默认不放行 8123，仅本机可访问；如需远程访问请自行放行并改密）

### 默认登录信息

- **ClickHouse 用户名**: `admin`
- **ClickHouse 密码**: `admin123`

---

## 服务管理命令

### 查看服务状态
```bash
sudo systemctl status alpha-api-gateway alpha-data-engine alpha-real-time-feed
```

### 重启所有服务
```bash
sudo systemctl restart alpha-api-gateway alpha-data-engine alpha-real-time-feed
```

### 查看服务日志
```bash
sudo journalctl -u alpha-* -f
```

### 查看 Docker 容器
```bash
docker ps
```

### 查看 ClickHouse 状态
```bash
curl http://localhost:8123/ping
```

---

## 故障排除

### 服务无法启动
```bash
# 检查端口占用
sudo netstat -tlnp | grep -E "9080|9081|9082|8123"

# 检查日志
sudo journalctl -u alpha-api-gateway -n 50
```

### ClickHouse 连接失败
```bash
# 检查 ClickHouse 容器
sudo docker ps | grep clickhouse

# 查看 ClickHouse 日志
sudo docker logs clickhouse

# 重启 ClickHouse
sudo docker-compose restart clickhouse
```

### 前端无法访问
```bash
# 检查 Nginx 状态
sudo systemctl status nginx

# 重新加载 Nginx
sudo nginx -t && sudo systemctl reload nginx
```

---

## 更新项目

```bash
cd /opt/alpha
git pull origin main          # 以部署用户执行（脚本以 $SUDO_USER 为项目属主）
sudo systemctl restart alpha-api-gateway alpha-data-engine alpha-real-time-feed
```

---

## 详细文档

- [完整部署文档](docs/deployment-runbook.md)
- [市场数据 API](docs/market-data-api.md)
- [告警与故障诊断](docs/alerting-and-diagnosis.md)

---

## 生产环境建议

- 改掉 ClickHouse 默认口令 `admin/admin123`：口令写死在两处——
  `docker-compose.yml` `CLICKHOUSE_PASSWORD` 与
  `config/clickhouse-users.xml`，改完 `docker compose up -d --force-recreate
  clickhouse` 重建，再同步 `scripts/deploy-ubuntu.sh:169` 的 init 口令；
- TLS 终止放 nginx：证书配进 `/etc/nginx/sites-available/alpha`（脚本已
  站点化），8123/9081 等非网关端口继续不对外放行；
- 鉴权默认全关（见 `docs/auth.md`），对外暴露前至少开网关 JWT 与
  data-engine `X-Api-Key`；
- 备份对象按数据分级：ClickHouse 数据卷（行情归档）定期
  `clickhouse-backup` 或卷快照；Postgres/Timescale 未默认启用。

---

部署验证：`curl -fsS http://localhost:9080/health` 返回 200、
`docker ps` 里 clickhouse 为 Up、`systemctl status alpha-*` 三个服务
均 active（running），才算部署完成。
