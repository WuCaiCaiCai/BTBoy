<div align="center">

# 🧲 BTBoy

**自动追番 · RSS 磁力推送 Telegram 机器人**

订阅任意 RSS（蜜柑 / bangumi.moe / 任意字幕组），自动解析集数、版本、简繁体、片源，
定时把磁力链接推送到你的 Telegram 频道，喂给光鸭 / qBittorrent 离线下载。

[![GitHub stars](https://img.shields.io/github/stars/WuCaiCaiCai/BTBoy?style=flat-square&logo=github&label=Stars&color=ffd700)](https://github.com/WuCaiCaiCai/BTBoy)
[![Docker pulls](https://img.shields.io/docker/pulls/wucaicai/btboy?style=flat-square&logo=docker&label=Pulls&color=2496ED)](https://hub.docker.com/r/wucaicai/btboy)
[![License](https://img.shields.io/github/license/WuCaiCaiCai/BTBoy?style=flat-square&color=00C853)](https://github.com/WuCaiCaiCai/BTBoy/blob/main/LICENSE)

</div>

---

## 特性

- **任意 RSS**：蜜柑 / bangumi.moe / 任意字幕组，RSS 给什么解析什么，无站点特判
- **磁力获取**：RSS 自带 magnet 直接用；只有 `.torrent` 就下载解析 infohash，并带上完整 tracker
- **智能解析**：识别 `第07话` `- 07` `S01E07` `07v2` `07.5`，简中/繁中/简繁，统一成 `01 02 03…`
- **冲突询问**：同集多来源（ABEMA/CR/Baha）或多版本（07 与 07v2）时弹按钮问你，选一次记住偏好
- **查重去重**：磁力 hash 级去重，绝不重复推
- **定时推送**：默认每 5 分钟轮询，固定格式发到频道
- **备用 RSS / 自动停用 / 遗漏检测 / 摸鱼检测**：追更省心

## 快速开始

```bash
# 1. 运行（替换 Token 和你的数字 ID）
docker run -d --name btboy --restart unless-stopped \
  -e TELEGRAM_BOT_TOKEN=你的Token \
  -e ADMIN_ID=你的数字ID \
  -v $PWD/data:/data \
  wucaicai/btboy:latest

# 2. 私聊机器人配置
#    /admin  → 成为管理员
#    /bind   → 绑定推送频道（转发一条频道消息或输入频道ID）
#    /sub <RSS链接> → 添加订阅（按提示一步步来）
```

> 📖 完整使用教程见 **[docs/USAGE.md](docs/USAGE.md)**，常见问题见 **[docs/FAQ.md](docs/FAQ.md)**

## 命令一览

| 命令 | 说明 |
|---|---|
| `/sub <rss>` | 添加订阅（交互引导） |
| `/list` | 列出所有订阅 |
| `/show` `/edit` `/del` | 详情 / 编辑 / 删除（无参弹订阅选择） |
| `/push` | 立即拉取（全部 / 逐个） |
| `/bind` | 绑定推送频道 |
| `/rss on\|off` | 轮询总开关 |
| `/interval <分钟>` | 轮询间隔 |
| `/skiphalf on\|off` | 跳过 07.5 特殊集 |
| `/gap on\|off` | 遗漏检测 |
| `/slack <天数>\|off` | 摸鱼检测 |
| `/autodisable on\|off` | 全部推完自动停用 |
| `/total <id> <n>` | 手动设总集数 |
| `/bgm <id> <bgmid>` | 绑定 Bangumi 自动取总集数 |
| `/backup <id> <rss>` | 设置备用 RSS |
| `/rmbackup <id>` | 移除备用 RSS |
| `/test` `/status` `/logs` | 测试 / 状态 / 日志 |
| `/cancel` | 取消当前操作 |

> 输入 `/` 有命令补全；所有带参命令都支持交互引导，直接发命令名即可。

## 磁力是怎么来的

1. RSS 里自带 `magnet:` → 直接用
2. RSS 只有 `.torrent` 链接 → 按原链接下载，解析 `info` 字典算 infohash，生成 `magnet:?xt=urn:btih:...`，并带上种子的全部 tracker

可用 `btboy resolve <rss-url>` 在服务器上逐条验证解析结果。

## 环境变量

| 变量 | 必填 | 说明 |
|---|---|---|
| `TELEGRAM_BOT_TOKEN` | ✅ | BotFather 令牌 |
| `ADMIN_ID` | 建议 | 管理员数字 ID（不填则第一个发 `/admin` 的人） |
| `CHANNEL_ID` | 选 | 推送频道 ID（不填用 `/bind`） |
| `FETCH_INTERVAL_MIN` | 选 | 轮询间隔分钟，默认 5 |
| `RUST_LOG` | 选 | 日志级别，默认 info |
| `DB_PATH` | 选 | 数据库路径，默认 `data/btboy.db` |

## 本地开发

```bash
cargo build --release
cargo test       # 基于真实 RSS 数据的管线测试
cargo clippy
```

维护者指南（构建 / Docker Hub 发布 / GitHub 上传）见 **[docs/DEVELOPMENT.md](docs/DEVELOPMENT.md)**。

---

<div align="center">

### ⭐ Star 曲线

![star-history](https://api.star-history.com/svg?repos=WuCaiCaiCai/BTBoy&type=Date)

**如果 BTBoy 帮到了你，欢迎点个 Star 支持！**

[![GitHub stars](https://img.shields.io/github/stars/WuCaiCaiCai/BTBoy?style=for-the-badge&logo=github&label=Star%20me&color=ffd700)](https://github.com/WuCaiCaiCai/BTBoy)

</div>
