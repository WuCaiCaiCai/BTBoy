# 🔧 开发与发布指南（维护者）

> 面向维护者的构建、测试、发布流程。普通用户看 [USAGE.md](USAGE.md) 即可。

## 本地构建与测试

```bash
# 编译
cargo build --release

# 测试（含基于真实 RSS 数据的管线测试）
cargo test

# 静态检查
cargo clippy

# 直接运行（开发机）
TELEGRAM_BOT_TOKEN=xxx ADMIN_ID=xxx cargo run
```

### 验证磁力解析

```bash
./target/release/btboy resolve "<rss-url>"
```

逐条打印 集数·片源 / 标题 / 链接 / 磁力，用于确认 RSS → 磁力链路正常。

## GitHub 上传

```bash
git remote add origin https://github.com/WuCaiCaiCai/BTBoy.git
git branch -M main
git push -u origin main
```

> 注意：仓库公开后所有提交历史可见。用隐私邮箱（GitHub noreply）提交，避免暴露个人邮箱。

## Docker Hub 发布

```bash
# 1. 登录（token 在 Docker Hub → Account Settings → Security 创建）
docker login -u wucaicai

# 2. 单架构构建 + 推送（快）
docker build -t wucaicai/btboy:latest .
docker push wucaicai/btboy:latest

# 3. 多架构 amd64 + arm64（VPS / 软路由都能用）
#    需先注册多架构模拟器，否则 arm64 报 exec format error
docker run --privileged --rm tonistiigi/binfmt --install all
docker buildx create --name multi --driver docker-container --use
docker buildx build --platform linux/amd64,linux/arm64 -t wucaicai/btboy:latest --push .
```

> 多架构 = QEMU 模拟编译，arm64 较慢（20-40 分钟）属正常；只需 amd64 用单架构命令即可。

## 镜像结构

Dockerfile 多阶段构建：

- **构建阶段**：`rust:1.92-slim` 编译 release 二进制（依赖锁定在 `Cargo.lock`，含 `takecell` 兼容版本）
- **运行阶段**：`debian:bookworm-slim` 只带二进制 + ca-certificates，镜像约 38MB

## 数据

- 运行数据全在 `/data`（`btboy.db` SQLite + 日志），通过 volume 挂载持久化
- `.env` / `data/` / `logs/` 均被 `.gitignore` 排除，不会入库
