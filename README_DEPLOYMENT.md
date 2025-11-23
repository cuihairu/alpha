# 🚀 Alpha Finance 快速部署指南

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

- **Web 应用**: `http://your-server-ip`
- **API 文档**: `http://your-server-ip/api/docs`
- **数据库管理**: `http://your-server-ip:8123`

### 默认登录信息

- **ClickHouse 用户名**: `admin`
- **ClickHouse 密码**: `admin123`

---

## 📋 服务管理命令

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

## 🔧 故障排除

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

## 📈 更新项目

```bash
cd /opt/alpha
sudo -u alpha git pull origin main
sudo systemctl restart alpha-api-gateway alpha-data-engine alpha-real-time-feed
```

---

## 📚 详细文档

- [完整部署文档](docs/DEPLOYMENT.md)
- [API 文档](docs/API.md)
- [配置说明](docs/CONFIGURATION.md)
- [故障排除指南](docs/TROUBLESHOOTING.md)

---

## 🎯 生产环境建议

1. **安全配置**
   - 修改默认密码
   - 配置 SSL 证书
   - 设置防火墙规则

2. **性能优化**
   - 配置反向代理
   - 启用缓存
   - 监控系统资源

3. **备份策略**
   - 定期数据备份
   - 配置监控告警
   - 制定恢复计划

---

**🎉 恭喜！您的 Alpha Finance 金融数据分析平台已成功部署！**