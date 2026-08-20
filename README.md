# BTBoy 自动追番 Telegram 机器人

**通用 RSS 订阅解析器**：订阅任意 RSS（蜜柑 Mikan / bangumi.moe / 任意字幕组），按 RSS 原样解析条目，
智能识别集数 / 版本(v2) / 简繁体 / 片源（ABEMA/CR/Baha），按规则查重过滤，
定时把**固定格式的磁力链接**推送到你绑定的 Telegram 频道，交给下游（如 光鸭云盘 + 转发机器人）离线下载。

磁力获取：RSS 里自带 magnet 直接用；若 RSS 只给 `.torrent` 文件链接，则按原链接下载、解析 info 字典算 infohash 生成 magnet（不依赖任何站点特判）。

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

## 调试：手动验证磁力解析

服务器（有外网）上直接验证某条 RSS 是否能解析出磁力：

```bash
btboy resolve "https://bangumi.moe/rss/tags/xxx"
```

会逐条打印 集数·片源 / 标题 / 链接 / 磁力，磁力能出来就说明链路正常。

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

## GitHub 上传（开源）

仓库已初始化并提交，只需推到 GitHub：

```bash
# 1. 在 GitHub 网页新建空仓库（不要勾选 README / .gitignore，避免冲突）
# 2. 添加远程并推送
git remote add origin https://github.com/WuCaiCaiCai/BTBoy.git
git branch -M main
git push -u origin main
```

> ⚠️ **隐私提醒**：当前 git 提交作者是真实姓名 + QQ 邮箱，推到公开仓库会永久暴露。
> 推送前建议改成 GitHub 匿名邮箱（在 GitHub → Settings → Emails 查看你的
> `xxxx+username@users.noreply.github.com`）：
>
> ```bash
> # 一次性重写全部历史提交的作者
> git filter-branch -f --env-filter '
> export GIT_AUTHOR_NAME="你的GitHub用户名"
> export GIT_AUTHOR_EMAIL="你的noreply邮箱"
> export GIT_COMMITTER_NAME="你的GitHub用户名"
> export GIT_COMMITTER_EMAIL="你的noreply邮箱"
> ' -- --all
>
> # 以后的新提交也用匿名邮箱
> git config user.name "你的GitHub用户名"
> git config user.email "你的noreply邮箱"
> ```
> 仓库还没推到远端，重写历史是安全的；推出去之后就不能改了。

## 部署到自己的机器

### 方式一：Docker（推荐，服务器 / VPS / 软路由）

见上方「快速开始」。数据都在 `./data` 目录（SQLite + 日志），`docker compose up -d` 即可，重启容器数据不丢。

### 方式二：直接跑二进制（有 Rust 环境的开发机）

```bash
cargo build --release
# 前台跑（Ctrl+C 退出）
TELEGRAM_BOT_TOKEN=你的Token ADMIN_ID=你的ID ./target/release/btboy
# 或后台跑
nohup env TELEGRAM_BOT_TOKEN=你的Token ADMIN_ID=你的ID \
  ./target/release/btboy >/dev/null 2>&1 &
```

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
