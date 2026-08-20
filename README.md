# BTBoy 自动追番 Telegram 机器人

订阅蜜柑计划（Mikan）RSS，智能解析集数 / 版本(v2) / 简繁体，按规则查重过滤，
定时把**固定格式的磁力链接**推送到你绑定的 Telegram 频道，交给下游（如 光鸭云盘 + 转发机器人）离线下载。

## 功能

- **订阅管理**：`/sub <RSS>` 添加，自动识别番名，交互式设置起始集 / 简繁偏好；`/list` `/show` `/edit` `/del` 全量管理
- **智能解析**：识别 `第07话` `- 07` `S01E07` `07v2` `07.5`，简中 / 繁中 / 简繁双语，统一成 `01 02 03…` 格式
- **冲突询问**：同一集出现多版本（07 与 07v2）或多语言（简中/繁中）时，机器人发按钮**问你**；选一次后记住该订阅偏好，后续自动套用
- **查重去重**：磁力 hash 级去重 + 同集同版本同语言去重
- **定时推送**：默认每 5 分钟轮询（`/interval` 可改），固定格式发到频道，集数带标签方便检索
- **关键词过滤**：每个订阅可配包含词 / 排除词（如排除"生肉"、只要 1080P）
- **备用 RSS**：`/backup <id> <url>`，主源无更新时自动用备用源兜底
- **自动停用**：`/autodisable on` + `/total <id> <n>`（或 `/bgm <id> <bgmid>` 自动从 Bangumi 取总集数），全部集数推完自动停用
- **遗漏检测**：`/gap on`，检测范围内缺集并通知
- **摸鱼检测**：`/slack <天数>`，超过 N 天无更新提醒
- **跳过 .5 集**：`/skiphalf on`，跳过 07.5 / 13.5 这类特殊集
- **日志**：stdout + 滚动文件双日志，`/logs` 直接查看

## 快速开始（服务器 / VPS）

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
  你的DockerHub账号/btboy:latest

# 方式 B：docker compose
cp .env.example .env   # 编辑填入 Token / ADMIN_ID
docker compose up -d
```

### 3. 首次配置

1. 私聊机器人发 `/admin`，成为管理员
2. `/bind`，然后**转发一条**来自目标频道的消息给它 → 频道绑定完成
3. `/sub https://mikanani.me/RSS/xxxxxx` → 确认番名 → 输入起始集 → 选简繁偏好
4. 之后机器人自动定时推送磁力到频道

### 4. 常用命令速查

| 命令 | 说明 |
|---|---|
| `/sub <rss>` | 添加订阅 |
| `/list` `/show <id>` `/edit <id>` `/del <id>` | 订阅管理 |
| `/bind` | 绑定推送频道（转发一条频道消息） |
| `/rss on\|off` | RSS 轮询总开关 |
| `/interval <分钟>` | 轮询间隔 |
| `/skiphalf on\|off` | 跳过 .5 特殊集 |
| `/gap on\|off` | 遗漏检测 |
| `/slack <天数>\|off` | 摸鱼检测 |
| `/autodisable on\|off` | 全部推完自动停用 |
| `/total <id> <n>` | 手动设总集数 |
| `/bgm <id> <bgmid>` | 绑定 Bangumi 自动取总集数 |
| `/backup <id> <rss>` | 设置备用 RSS |
| `/rmbackup <id>` | 移除备用 RSS |
| `/push <id>` | 立即拉取推送一次 |
| `/test` | 发测试消息到频道 |
| `/status` `/logs [n]` | 状态 / 日志 |

## Docker Hub 发布（把镜像推上去）

```bash
# 1. 登录（token 在 Docker Hub → Account Settings → Security 创建）
docker login

# 2. 本地构建
docker build -t 你的DockerHub账号/btboy:latest .

# 3. 推送
docker push 你的DockerHub账号/btboy:latest

# 4.（推荐）多架构镜像：amd64 + arm64，VPS/软路由都能用
docker buildx build --platform linux/amd64,linux/arm64 \
  -t 你的DockerHub账号/btboy:latest --push .
```

> 提示：Dockerfile 为多阶段构建，运行镜像只有几十 MB。
> 依赖锁定在 `Cargo.lock`（含 `takecell` 兼容版本），在任何有网环境 `docker build` 即可复现。

## 本地开发

```bash
cargo build --release
TELEGRAM_BOT_TOKEN=xxx ADMIN_ID=xxx cargo run
cargo test     # 解析器单元测试
cargo clippy
```

## 配置环境变量

| 变量 | 必填 | 说明 |
|---|---|---|
| `TELEGRAM_BOT_TOKEN` | ✅ | BotFather 令牌 |
| `ADMIN_ID` | 建议 | 管理员数字 ID（不填则第一个发 `/admin` 的人） |
| `CHANNEL_ID` | 选 | 全局推送频道 ID（不填用 `/bind`） |
| `FETCH_INTERVAL_MIN` | 选 | 轮询间隔分钟，默认 5 |
| `RUST_LOG` | 选 | 日志级别，默认 info |
| `DB_PATH` | 选 | 数据库路径，默认 `data/btboy.db` |

数据持久化在 `data/` 卷（SQLite + 日志）。

## 说明

- 只供**单管理员**使用，安全性更高
- 目前不支持"检测本地下载文件夹"，因为纯推磁力场景不适用（如需可与下载器打通再做）
