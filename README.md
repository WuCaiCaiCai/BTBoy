<div align="center">

# 🧲 BTBoy

**自动追番 · RSS 磁力推送 Telegram 机器人**

通用 RSS 订阅解析器，订阅任意 RSS（蜜柑 Mikan / bangumi.moe / 任意字幕组），智能识别集数 / 版本 / 简繁体 / 片源，
定时把**固定格式的磁力链接**推送到你绑定的 Telegram 频道，喂给光鸭 / qBittorrent 等下载器离线跑。

[![GitHub stars](https://img.shields.io/github/stars/WuCaiCaiCai/BTBoy?style=for-the-badge&logo=github&logoColor=white&label=GitHub%20Stars&color=181717)](https://github.com/WuCaiCaiCai/BTBoy)
[![Docker pulls](https://img.shields.io/docker/pulls/wucaicai/btboy?style=for-the-badge&logo=docker&logoColor=white&label=Pulls&color=2496ED)](https://hub.docker.com/r/wucaicai/btboy)
[![Docker stars](https://img.shields.io/docker/stars/wucaicai/btboy?style=for-the-badge&logo=docker&logoColor=white&label=Docker%20Stars&color=2496ED)](https://hub.docker.com/r/wucaicai/btboy)
[![Docker image size](https://img.shields.io/docker/image-size/wucaicai/btboy/latest?style=for-the-badge&logo=docker&logoColor=white&label=镜像大小&color=2496ED)](https://hub.docker.com/r/wucaicai/btboy)
[![Rust](https://img.shields.io/badge/Rust-1.92%2B-000000?style=for-the-badge&logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![License](https://img.shields.io/github/license/WuCaiCaiCai/BTBoy?style=for-the-badge&color=00C853)](https://github.com/WuCaiCaiCai/BTBoy/blob/main/LICENSE)

---

### ⭐ Star 曲线

![star-history](https://api.star-history.com/svg?repos=WuCaiCaiCai/BTBoy&type=Date)

</div>

---

## ✨ 功能

| 能力 | 说明 |
|---|---|
| 🔗 **任意 RSS** | 蜜柑 / bangumi.moe / 任意字幕组，RSS 给什么解析什么，无站点特判 |
| 🧲 **磁力获取** | RSS 自带 magnet 直接用；只有 `.torrent` 链接则下载解析 info 字典算 infohash，并带上完整 tracker |
| 🧠 **智能解析** | 识别 `第07话` `- 07` `S01E07` `07v2` `07.5`，简中 / 繁中 / 简繁双语，统一成 `01 02 03…` |
| 🎬 **片源识别** | ABEMA / CR / Baha 等多来源，同集多来源自动发按钮让你选 |
| 🔔 **冲突询问** | 07 与 07v2、简中与繁中并存时弹按钮问你，选一次记住偏好，后续自动套用 |
| 🧯 **查重去重** | 磁力 hash 级去重 + 同集同版本同语言去重，绝不重复推 |
| ⏱️ **定时推送** | 默认每 5 分钟轮询（`/interval` 可调），固定格式发到频道 |
| 📦 **备用 RSS** | 主源无更新自动用备用源兜底 |
| 🏁 **自动停用** | 绑定 Bangumi 自动取总集数，全部推完自动停用订阅 |
| 🔍 **遗漏检测** | 检测范围内缺集并通知 |
| 🥱 **摸鱼检测** | 超过 N 天无更新提醒，及时发现有源断更 |
| 📜 **日志** | Docker stdout + 滚动文件双日志，`/logs` 直接看 |

---

## 🚀 快速开始

### 1. 准备

- `@BotFather` 创建机器人，拿到 Token
- 建一个频道，把机器人加为**频道管理员**
- `@userinfobot` 查你的数字 ID

### 2. Docker 运行

```bash
# 方式 A：直接 docker run
docker run -d --name btboy --restart unless-stopped \
  -e TELEGRAM_BOT_TOKEN=你的Token \
  -e ADMIN_ID=你的数字ID \
  -v $PWD/data:/data \
  wucaicai/btboy:latest

# 方式 B：docker compose
cp .env.example .env   # 编辑填入 Token / ADMIN_ID
docker compose up -d
```

### 3. 首次配置

1. 私聊机器人发 `/admin` → 成为管理员
2. `/bind` → 转发一条频道消息（或直接输入频道ID）→ 绑定完成
3. `/sub <rss>` → 确认番名 → 起始集 → 简繁偏好
4. 之后机器人自动定时推送磁力到频道

---

## 🎛️ 命令一览

### 订阅管理

| 命令 | 说明 |
|---|---|
| `/sub <rss>` | 添加订阅（交互引导） |
| `/list` | 列出所有订阅 |
| `/show` `/edit` `/del` | 详情 / 编辑 / 删除（无参时弹订阅选择） |
| `/push` | 立即拉取（对话框：全部拉取 / 逐个订阅） |

### 全局设置

| 命令 | 说明 |
|---|---|
| `/bind` | 绑定推送频道 |
| `/rss on\|off` | 轮询总开关 |
| `/interval <分钟>` | 轮询间隔 |
| `/skiphalf on\|off` | 跳过 07.5 这类特殊集 |
| `/gap on\|off` | 遗漏检测 |
| `/slack <天数>\|off` | 摸鱼检测 |
| `/autodisable on\|off` | 全部推完自动停用 |

### 订阅级

| 命令 | 说明 |
|---|---|
| `/total <id> <n>` | 手动设总集数 |
| `/bgm <id> <bgmid>` | 绑定 Bangumi 自动取总集数 |
| `/backup <id> <rss>` / `/rmbackup <id>` | 设置 / 移除备用 RSS |

### 其他

| 命令 | 说明 |
|---|---|
| `/test` | 发测试消息到频道 |
| `/status` `/logs [n]` | 状态 / 日志 |
| `/cancel` | 取消当前对话 |
| `/admin [id]` | 首个使用者成为管理员 |

> 💡 所有需要参数的命令都支持交互引导：直接发命令名即可，机器人会一步步提示。输入 `/` 有命令补全。

---

## 🛠️ 调试：验证磁力解析

服务器（有外网）上直接验证某条 RSS 能否解析出磁力：

```bash
btboy resolve "https://bangumi.moe/rss/tags/xxx"
```

会逐条打印 集数·片源 / 标题 / 链接 / 磁力。

---

## 📦 Docker Hub 发布

```bash
# 1. 登录（token 在 Docker Hub → Account Settings → Security 创建）
docker login -u wucaicai

# 2. 构建 + 推送（单架构，快）
docker build -t wucaicai/btboy:latest .
docker push wucaicai/btboy:latest

# 3.（可选）多架构 amd64 + arm64
docker run --privileged --rm tonistiigi/binfmt --install all
docker buildx create --name multi --driver docker-container --use
docker buildx build --platform linux/amd64,linux/arm64 -t wucaicai/btboy:latest --push .
```

> Dockerfile 多阶段构建，运行镜像只有几十 MB；依赖锁定在 `Cargo.lock`，任何有网环境可复现。

---

## 🌐 GitHub 上传

```bash
git remote add origin https://github.com/WuCaiCaiCai/BTBoy.git
git branch -M main
git push -u origin main
```

---

## 💻 部署到自己的机器

### 方式一：Docker（推荐）

见上方「快速开始」。数据都在 `./data` 目录（SQLite + 日志），重启不丢。

### 方式二：直接跑二进制（有 Rust 环境）

```bash
cargo build --release
nohup env TELEGRAM_BOT_TOKEN=你的Token ADMIN_ID=你的ID \
  ./target/release/btboy >/dev/null 2>&1 &
```

---

## ⚙️ 环境变量

| 变量 | 必填 | 说明 |
|---|---|---|
| `TELEGRAM_BOT_TOKEN` | ✅ | BotFather 令牌 |
| `ADMIN_ID` | 建议 | 管理员数字 ID（不填则第一个发 `/admin` 的人） |
| `CHANNEL_ID` | 选 | 全局推送频道 ID（不填用 `/bind`） |
| `FETCH_INTERVAL_MIN` | 选 | 轮询间隔分钟，默认 5 |
| `RUST_LOG` | 选 | 日志级别，默认 info |
| `DB_PATH` | 选 | 数据库路径，默认 `data/btboy.db` |

---

## 🧪 本地开发

```bash
cargo build --release
cargo test       # 基于真实 RSS 数据的管线测试
cargo clippy
```

---

## 📄 License

[MIT](LICENSE)

<div align="center">

**如果 BTBoy 帮到了你，欢迎 ⭐ Star 支持一下！**

[![GitHub stars](https://img.shields.io/github/stars/WuCaiCaiCai/BTBoy?style=for-the-badge&logo=github&logoColor=white&color=ffd700)](https://github.com/WuCaiCaiCai/BTBoy)

</div>
