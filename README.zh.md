# Ferrss
> 原名 **opencli-rs**，后更名为 **AutoCLI**，自 v0.3.11 起更名为 **Ferrss**。

**[English](README.md) | [中文](README.zh.md) | [日本語](README.ja.md)**

<p align="center">
  <img src="assets/title_screen.jpg" alt="ferrss" width="800" />
</p>

<p align="center">
  <a href="https://github.com/oouxx/Ferrss"><b>github.com/oouxx/Ferrss</b></a> — 本地优先，无需云端账号
</p>

---

## 更新说明

### v0.3.2
- **Chrome 扩展选择器工具** — 只需选择你需要的核心数据，可视化精准选取页面元素，构建精确的 CSS 选择器定位目标内容
- **AI 智能生成** — 基于你选择的数据，AI 自动扩展并发现关联字段，生成完整的数据抓取规则
- **使用你自己的大模型** — 生成的适配器保存到 `~/.ferrss/adapters/` 并自动加载，无需账号或云端服务

---

极速、安全的命令行工具 —— **一行命令快速获取任意网站信息**。覆盖 Bilibili、知乎、小红书、Twitter/X、Reddit、YouTube、HackerNews 等 [55+ 站点](#内置命令)，同时支持控制 Electron 桌面应用、集成本地 CLI 工具（`gh`、`docker`、`kubectl`），通过浏览器会话复用和 AI 原生发现能力驱动。

基于 [OpenCLI](https://github.com/jackwener/opencli)（TypeScript）用 **纯 Rust 完整重写**。功能对等，**最高快 12 倍**，**内存省 10 倍**，**单文件 4.7MB**，零运行时依赖。

**OpenClaw/Agent 的最佳搭档** —— 赋予你的 AI Agent 触达全网信息的能力，一行命令获取 55+ 站点的实时数据。
**为 AI Agent 而生：** 在 `AGENT.md` 或 `.cursorrules` 中让 Agent 调用 `ferrss --help`（全部站点）和 `ferrss <site> --help`（单站命令），它就能自行发现所有可用工具。本地 CLI 工具通过 `~/.ferrss/external-clis.yaml` 声明，你自己的命令也会变成 `ferrss` 的子命令。


## 🚀 性能对比

| 指标 | 🦀 ferrss (Rust) | 📦 opencli (Node.js) | 提升 |
|------|:-----------------:|:-----------------:|:----:|
| 💾 **内存占用 (Public 命令)** | 15 MB | 99 MB | **6.6x** |
| 💾 **内存占用 (Browser 命令)** | 9 MB | 95 MB | **10.6x** |
| 📏 **二进制大小** | 4.7 MB | ~50 MB (node_modules) | **10x** |
| 🔗 **运行时依赖** | 无 | Node.js 20+ | **零依赖** |
| ✅ **测试通过率** | 103/122 (84%) | 104/122 (85%) | 接近对等 |

**⚡ 实测命令耗时对比：**

| 命令 | 🦀 ferrss | 📦 opencli | 加速比 |
|------|:----------:|:-------:|:------:|
| `bilibili hot` | **1.66s** | 20.1s | 🔥 **12x** |
| `zhihu hot` | **1.77s** | 20.5s | 🔥 **11.6x** |
| `xueqiu search 茅台` | **1.82s** | 9.2s | ⚡ **5x** |
| `xiaohongshu search` | **5.1s** | 14s | ⚡ **2.7x** |

> 基于 122 个命令的自动化测试（55 个站点），macOS Apple Silicon 环境。

## 特性

- **55 个站点、333 个命令** —— 覆盖 Bilibili、Twitter、Reddit、知乎、小红书、YouTube、Hacker News 等
- **浏览器会话复用** —— 通过 Chrome 扩展复用已登录状态，无需管理 token
- **声明式 YAML Pipeline** —— 用 YAML 描述数据抓取流程，零代码新增适配器
- **AI 原生发现** —— `explore` 分析网站 API、`generate` 一键生成适配器、`cascade` 探测认证策略
- **AI 智能生成** —— `generate --ai` 使用你自己的大模型（本地或云端）分析任意网站，自动生成适配器
- **下载媒体和文章** —— 视频下载（yt-dlp）、文章导出为 Markdown 并本地化配图
- **外部 CLI 透传** —— 集成 GitHub CLI、Docker、Kubernetes 等工具
- **多格式输出** —— table、JSON、YAML、CSV、Markdown
- **单一二进制** —— 编译为 4MB 静态二进制，零运行时依赖

## 安装
> **只有一个文件，下载即可使用。** 无需 Node.js、Python 或任何运行时，放到 PATH 里就能跑。

### 一键安装脚本（macOS / Linux）

```bash
curl -fsSL https://raw.githubusercontent.com/oouxx/Ferrss/main/scripts/install.sh | sh
```

自动检测系统和架构，下载对应二进制，安装到 `/usr/local/bin/`。

### Windows (PowerShell)

```powershell
Invoke-WebRequest -Uri "https://github.com/oouxx/Ferrss/releases/latest/download/ferrss-x86_64-pc-windows-msvc.zip" -OutFile ferrss.zip
Expand-Archive ferrss.zip -DestinationPath .
Move-Item ferrss.exe "$env:LOCALAPPDATA\Microsoft\WindowsApps\"
```


### 手动下载（最简单）

从 [GitHub Releases](https://github.com/oouxx/Ferrss/releases/latest) 下载对应平台的文件：

| 平台 | 文件 |
|------|------|
| macOS (Apple Silicon) | `ferrss-aarch64-apple-darwin.tar.gz` |
| macOS (Intel) | `ferrss-x86_64-apple-darwin.tar.gz` |
| Linux (x86_64) | `ferrss-x86_64-unknown-linux-musl.tar.gz` |
| Linux (ARM64) | `ferrss-aarch64-unknown-linux-musl.tar.gz` |
| Windows (x64) | `ferrss-x86_64-pc-windows-msvc.zip` |

解压后将 `ferrss`（Windows 为 `ferrss.exe`）放到系统 PATH 中即可。

### 从源码编译

```bash
git clone https://github.com/oouxx/Ferrss.git
cd Ferrss
cargo build --release
cp target/release/ferrss /usr/local/bin/   # macOS / Linux
```

### 更新

重新运行安装命令或下载最新版本覆盖即可。

### Chrome 扩展安装（浏览器命令需要）

1. 从 [GitHub Releases](https://github.com/oouxx/Ferrss/releases/latest) 下载 `ferrss-chrome-extension.zip`
2. 解压到任意目录
3. 打开 Chrome，访问 `chrome://extensions`
4. 开启右上角「开发者模式」
5. 点击「加载已解压的扩展程序」，选择解压后的文件夹
6. 扩展安装后会自动连接 ferrss daemon

> Public 模式命令（hackernews、devto、lobsters 等）无需安装扩展即可使用。

## Skill 安装

一键为你的 AI Agent 安装 ferrss skill（本 fork 版本，已去除商业/云端依赖）：

```bash
# 查看 skill
npx skills add oouxx/Ferrss --list

# 全局安装到 Claude Code（非交互）
npx skills add oouxx/Ferrss -s ferrss -g -a claude-code -y

# 只装指定 skill
npx skills add oouxx/Ferrss -s ferrss
```

> 需要仓库已 push 且为 **public**。手动安装见 `skills/README.md`。

安装后，Agent 可直接自然语言调用 ferrss（例如"查B站今日热门"）。

## 快速开始

```bash
# 查看所有可用命令
ferrss --help

# 查看某个站点的命令
ferrss hackernews --help

# 获取 Hacker News 热门文章（公开 API，无需浏览器）
ferrss hackernews top --limit 10

# JSON 格式输出
ferrss hackernews top --limit 5 --format json

# 获取 Bilibili 热门视频（需要浏览器 + Cookie）
ferrss bilibili hot --limit 20

# 搜索 Twitter（需要浏览器 + 登录）
ferrss twitter search "rust lang" --limit 10

# 运行诊断
ferrss doctor

# 生成 Shell 补全
ferrss completion bash >> ~/.bashrc
ferrss completion zsh >> ~/.zshrc
ferrss completion fish > ~/.config/fish/completions/ferrss.fish
```

## AI 命令

适配器由你自己的大模型在本地生成 —— 无需账号，也不依赖云端服务。把 `ferrss` 指向任意 OpenAI 兼容接口即可，包括本地的 Ollama 或 LM Studio。

### 第一步：配置大模型

```bash
# 本地 Ollama
ferrss config-llm --provider ollama --model llama3

# 或任意 OpenAI 兼容接口
ferrss config-llm --provider https://api.openai.com/v1/chat/completions --model gpt-4o --api-key sk-...
```

已知 provider 名称（`openai`、`deepseek`、`qwen`、`moonshot`、`zhipu`、`groq`、`mistral`、`ollama`、`lmstudio`）会自动展开为默认接口；以 `http` 开头的值直接当作接口地址使用。配置保存在 `~/.ferrss/config.json`，`--provider` / `--model` / `--api-key` 可在单次调用时覆盖。

### 第二步：生成适配器

可以直接在命令行生成：

```bash
ferrss generate https://www.example.com --goal hot --ai
```

…也可以通过 Chrome 浏览器插件，精准选择特定网站上你需要的数据，点击生成按钮后，AI 会自动分析并生成页面，扩展相关数据并生成适配器：

<p align="center">
  <img src="assets/chrome_extension_demo.jpg" alt="ferrss" width="800" />
</p>

适配器会写入 `~/.ferrss/adapters/<site>/<name>.yaml` 并被自动发现，生成完成后即可用新命令检索需要的数据：

<p align="center">
  <img src="assets/ferrss_use.jpg" alt="ferrss" width="800" />
</p>

### 环境变量

| 变量 | 说明 | 默认值 |
|------|------|--------|
| `FERRSS_DAEMON_PORT` | 本地浏览器守护进程端口 | `19925` |
| `FERRSS_CDP_ENDPOINT` | 直接连接已有的 CDP 端点，而非启动 Chrome | 未设置 |
| `FERRSS_BROWSER_COMMAND_TIMEOUT` | 单条命令的浏览器超时（秒） | 命令默认值，否则 `60` |
| `FERRSS_VERBOSE` | 强制输出 debug 日志（等同 `-v`） | 未设置 |
| `RUST_LOG` | tracing 日志过滤 | `warn` |

旧变量名 `AUTOCLI_*` 仍会作为回退读取。

## 内置命令

运行 `ferrss --help` 查看所有可用命令。

| 站点 | 命令 | 模式 |
|------|------|------|
| **hackernews** | `top` `new` `best` `ask` `show` `jobs` `search` `user` | Public |
| **devto** | `top` `tag` `user` | Public |
| **lobsters** | `hot` `newest` `active` `tag` | Public |
| **stackoverflow** | `hot` `search` `bounties` `unanswered` | Public |
| **steam** | `top-sellers` | Public |
| **linux-do** | `hot` `latest` `search` `categories` `category` `topic` | Public |
| **arxiv** | `search` `paper` | Public |
| **wikipedia** | `search` `summary` `random` `trending` | Public |
| **apple-podcasts** | `search` `episodes` `top` | Public |
| **xiaoyuzhou** | `podcast` `podcast-episodes` `episode` | Public |
| **bbc** | `news` | Public |
| **hf** | `top` | Public |
| **sinafinance** | `news` | Public |
| **google** | `news` `search` `suggest` `trends` | Public / Browser |
| **v2ex** | `hot` `latest` `topic` `node` `user` `member` `replies` `nodes` `daily` `me` `notifications` | Public / Browser |
| **bloomberg** | `main` `markets` `economics` `industries` `tech` `politics` `businessweek` `opinions` `feeds` `news` | Public / Browser |
| **twitter** | `trending` `bookmarks` `profile` `search` `timeline` `thread` `following` `followers` `notifications` `post` `reply` `delete` `like` `article` `follow` `unfollow` `bookmark` `unbookmark` `download` `accept` `reply-dm` `block` `unblock` `hide-reply` | Browser |
| **bilibili** | `hot` `search` `me` `favorite` `history` `feed` `subtitle` `dynamic` `ranking` `following` `user-videos` `download` | Browser |
| **reddit** | `hot` `frontpage` `popular` `search` `subreddit` `read` `user` `user-posts` `user-comments` `upvote` `save` `comment` `subscribe` `saved` `upvoted` | Browser |
| **zhihu** | `hot` `search` `question` `download` | Browser |
| **xiaohongshu** | `search` `notifications` `feed` `user` `download` `publish` `creator-notes` `creator-note-detail` `creator-notes-summary` `creator-profile` `creator-stats` | Browser |
| **xueqiu** | `feed` `hot-stock` `hot` `search` `stock` `watchlist` `earnings-date` | Browser |
| **weibo** | `hot` `search` | Browser |
| **douban** | `search` `top250` `subject` `marks` `reviews` `movie-hot` `book-hot` | Browser |
| **weread** | `shelf` `search` `book` `highlights` `notes` `notebooks` `ranking` | Browser |
| **youtube** | `search` `video` `transcript` | Browser |
| **medium** | `feed` `search` `user` | Browser |
| **substack** | `feed` `search` `publication` | Browser |
| **sinablog** | `hot` `search` `article` `user` | Browser |
| **boss** | `search` `detail` `recommend` `joblist` `greet` `batchgreet` `send` `chatlist` `chatmsg` `invite` `mark` `exchange` `resume` `stats` | Browser |
| **jike** | `feed` `search` `create` `like` `comment` `repost` `notifications` `post` `topic` `user` | Browser |
| **facebook** | `feed` `profile` `search` `friends` `groups` `events` `notifications` `memories` `add-friend` `join-group` | Browser |
| **instagram** | `explore` `profile` `search` `user` `followers` `following` `follow` `unfollow` `like` `unlike` `comment` `save` `unsave` `saved` | Browser |
| **tiktok** | `explore` `search` `profile` `user` `following` `follow` `unfollow` `like` `unlike` `comment` `save` `unsave` `live` `notifications` `friends` | Browser |
| **yollomi** | `generate` `video` `edit` `upload` `models` `remove-bg` `upscale` `face-swap` `restore` `try-on` `background` `object-remover` | Browser |
| **yahoo-finance** | `quote` | Browser |
| **barchart** | `quote` `options` `greeks` `flow` | Browser |
| **linkedin** | `search` | Browser |
| **reuters** | `search` | Browser |
| **smzdm** | `search` | Browser |
| **ctrip** | `search` | Browser |
| **coupang** | `search` `add-to-cart` | Browser |
| **grok** | `ask` | Browser |
| **jimeng** | `generate` `history` | Browser |
| **chaoxing** | `assignments` `exams` | Browser |
| **weixin** | `download` | Browser |
| **doubao** | `status` `new` `send` `read` `ask` | Browser |
| **cursor** | `status` `send` `read` `new` `dump` `composer` `model` `extract-code` `ask` `screenshot` `history` `export` | Desktop |
| **codex** | `status` `send` `read` `new` `dump` `extract-diff` `model` `ask` `screenshot` `history` `export` | Desktop |
| **chatwise** | `status` `new` `send` `read` `ask` `model` `history` `export` `screenshot` | Desktop |
| **chatgpt** | `status` `new` `send` `read` `ask` | Desktop |
| **doubao-app** | `status` `new` `send` `read` `ask` `screenshot` `dump` | Desktop |
| **notion** | `status` `search` `read` `new` `write` `sidebar` `favorites` `export` | Desktop |
| **discord-app** | `status` `send` `read` `channels` `servers` `search` `members` | Desktop |
| **antigravity** | `status` `send` `read` `new` `dump` `extract-code` `model` `watch` | Desktop |

> **模式说明：** Public = 无需浏览器，直接调 API；Browser = 需要 Chrome + 扩展；Desktop = 需要桌面应用运行



## AI 发现能力

两种方式自动生成适配器：

```bash
# 🤖 AI 驱动（推荐）：你自己的大模型分析页面并生成适配器
ferrss generate https://www.example.com --goal hot --ai
# 保存到 ~/.ferrss/adapters/<site>/<name>.yaml，下次运行自动加载

# 🔧 规则驱动：无需 AI 的启发式分析
ferrss generate https://www.example.com --goal hot

# 探索网站 API（端点、框架、Store）
ferrss explore https://www.example.com --site mysite

# 交互式模糊测试（点击按钮触发隐藏 API）
ferrss explore https://www.example.com --auto --click "评论,字幕"

# 自动探测认证策略（PUBLIC → COOKIE → HEADER）
ferrss cascade https://api.example.com/hot
```

**发现能力：**
- `.json` 后缀探测（Reddit 风格 REST 发现）
- `__INITIAL_STATE__` 提取（Bilibili、小红书等 SSR 站点）
- Pinia/Vuex Store 发现和 Action 映射
- `--goal search` 自动发现搜索端点
- 框架检测（Vue/React/Next.js/Nuxt）

## 下载

下载支持站点的媒体和文章：

```bash
# 下载 B 站视频（需要 yt-dlp）
ferrss bilibili download BV1xxx --output ./videos --quality 1080p

# 下载知乎文章为 Markdown（含配图）
ferrss zhihu download "https://zhuanlan.zhihu.com/p/xxx" --output ./articles

# 下载微信公众号文章为 Markdown（含配图）
ferrss weixin download "https://mp.weixin.qq.com/s/xxx" --output ./articles

# 下载 Twitter/X 媒体（图片 + 视频）
ferrss twitter download nash_su --limit 10 --output ./twitter
ferrss twitter download --tweet-url "https://x.com/user/status/123" --output ./twitter
```

**下载特性：**
- 视频通过 yt-dlp 下载（自动从浏览器提取 cookies，无需系统授权）
- 文章导出为 Markdown + YAML 头信息（标题、作者、日期、来源）
- 配图自动下载并本地化（远程 URL 替换为本地 `images/img_001.jpg`）
- 输出结构：`output/文章标题/标题.md` + `output/文章标题/images/`

## 外部 CLI 集成

已集成的外部工具（透传执行）：

| 工具 | 说明 |
|------|------|
| `gh` | GitHub CLI |
| `docker` | Docker CLI |
| `kubectl` | Kubernetes CLI |
| `obsidian` | Obsidian 笔记管理 |
| `readwise` | Readwise 阅读管理 |
| `gws` | Google Workspace CLI |

```bash
# 透传到 GitHub CLI
ferrss gh repo list

# 透传到 kubectl
ferrss kubectl get pods
```

## 输出格式

通过 `--format` 全局参数切换输出格式：

```bash
ferrss hackernews top --format table    # ASCII 表格（默认）
ferrss hackernews top --format json     # JSON
ferrss hackernews top --format yaml     # YAML
ferrss hackernews top --format csv      # CSV
ferrss hackernews top --format md       # Markdown 表格
```

## REST API 与 RSS

把整个适配器注册表变成一个本地 HTTP 服务 —— 每个命令都会变成一个 JSON 接口
和一个 RSS 2.0 订阅源，任何阅读器（FreshRSS、NetNewsWire、Feedly、Inoreader 等）
都能直接订阅。

```bash
# 启动服务（默认 http://127.0.0.1:8787）
ferrss serve
ferrss serve --host 0.0.0.0 --port 8080   # 暴露到局域网
```

| 接口 | 说明 |
|------|------|
| `GET /` | HTML 索引页，列出所有可用订阅源 |
| `GET /health` | 健康检查 / 版本 |
| `GET /api/sites` | 列出站点及其命令 |
| `GET /api/commands` | 完整命令目录（含参数 schema） |
| `GET /api/sites/{site}` | 某个站点的所有命令 |
| `GET /api/run/{site}/{command}?limit=10` | 执行命令，返回 JSON（或 `?format=md\|csv\|yaml\|table`） |
| `GET /rss/{site}/{command}?limit=10` | 执行命令，返回 RSS 2.0 订阅源 |

查询参数使用与 CLI 完全相同的类型转换与校验规则，因此 `?limit=10` 会自动解析为整数。

浏览器模式的命令（依赖 Chrome 扩展的那些）**串行执行**：扩展只有一个自动化窗口，
并发请求会排队而不是直接失败。队列已满、或等待超过 4 分钟时返回 `503` 并带
`Retry-After` 头；纯 HTTP 命令不受影响，仍然并行执行。

```bash
# JSON
curl 'http://127.0.0.1:8787/api/run/hackernews/top?limit=5'

# RSS
curl 'http://127.0.0.1:8787/rss/hackernews/top?limit=20'

# 在阅读器中订阅
# https://news.ycombinator.com -> 订阅 http://<host>:8787/rss/hackernews/top
```

RSS 转换会自动识别常见字段（`title`/`name`、`url`/`link`、`description`/`summary`，
以及 `date`、`published_at`、`created_at` 等时间字段），其余字段会回退为
`key: value` 摘要。需要浏览器/桌面会话的命令同样可用，只要守护进程和 Chrome
扩展在运行。

## 认证策略

每个命令使用不同的认证策略：

| 策略 | 说明 | 是否需要浏览器 |
|------|------|--------------|
| `public` | 公开 API，无需认证 | 否 |
| `cookie` | 需要浏览器 Cookie | 是 |
| `header` | 需要特定请求头 | 是 |
| `intercept` | 需要拦截网络请求 | 是 |
| `ui` | 需要 UI 交互 | 是 |

## 自定义适配器

在 `~/.ferrss/adapters/` 下创建 YAML 文件即可添加自定义适配器：

```yaml
# ~/.ferrss/adapters/mysite/hot.yaml
site: mysite
name: hot
description: My site hot posts
strategy: public
browser: false

args:
  limit:
    type: int
    default: 20
    description: Number of items

columns: [rank, title, score]

pipeline:
  - fetch: https://api.mysite.com/hot
  - select: data.posts
  - map:
      rank: "${{ index + 1 }}"
      title: "${{ item.title }}"
      score: "${{ item.score }}"
  - limit: "${{ args.limit }}"
```

### Pipeline 步骤

| 步骤 | 功能 | 示例 |
|------|------|------|
| `fetch` | HTTP 请求 | `fetch: https://api.example.com/data` |
| `evaluate` | 浏览器中执行 JS | `evaluate: "document.title"` |
| `navigate` | 页面导航 | `navigate: https://example.com` |
| `click` | 点击元素 | `click: "#button"` |
| `type` | 输入文本 | `type: { selector: "#input", text: "hello" }` |
| `wait` | 等待 | `wait: 2000` |
| `select` | 选取嵌套数据 | `select: data.items` |
| `map` | 数据映射 | `map: { title: "${{ item.title }}" }` |
| `filter` | 数据过滤 | `filter: "item.score > 10"` |
| `sort` | 排序 | `sort: { by: score, order: desc }` |
| `limit` | 截断 | `limit: "${{ args.limit }}"` |
| `intercept` | 网络拦截 | `intercept: { pattern: "*/api/*" }` |
| `tap` | 状态管理桥接 | `tap: { action: "store.fetch", url: "*/api/*" }` |
| `download` | 下载 | `download: { type: media }` |

### 模板表达式

Pipeline 中使用 `${{ expression }}` 语法：

```yaml
# 变量访问
"${{ args.limit }}"
"${{ item.title }}"
"${{ index + 1 }}"

# 比较和逻辑
"${{ item.score > 10 }}"
"${{ item.title && !item.deleted }}"

# 三元表达式
"${{ item.active ? 'yes' : 'no' }}"

# 管道过滤器
"${{ item.title | truncate(30) }}"
"${{ item.tags | join(', ') }}"
"${{ item.name | lower | trim }}"

# 字符串插值
"https://api.com/${{ item.id }}.json"

# Fallback
"${{ item.subtitle || 'N/A' }}"

# 数学函数
"${{ Math.min(args.limit, 50) }}"
```

**内置过滤器（16 个）：** `default`, `join`, `upper`, `lower`, `trim`, `truncate`, `replace`, `keys`, `length`, `first`, `last`, `json`, `slugify`, `sanitize`, `ext`, `basename`

## 配置

### 环境变量

| 变量 | 默认值 | 说明 |
|------|--------|------|
| `OPENCLI_VERBOSE` | - | 启用详细输出 |
| `OPENCLI_DAEMON_PORT` | `19825` | Daemon 端口 |
| `OPENCLI_CDP_ENDPOINT` | - | CDP 直连端点（跳过 Daemon） |
| `OPENCLI_BROWSER_COMMAND_TIMEOUT` | `60` | 命令超时（秒） |
| `OPENCLI_BROWSER_CONNECT_TIMEOUT` | `30` | 浏览器连接超时（秒） |
| `OPENCLI_BROWSER_EXPLORE_TIMEOUT` | `120` | Explore 超时（秒） |

### 文件路径

| 路径 | 说明 |
|------|------|
| `~/.ferrss/adapters/` | 用户自定义适配器 |
| `~/.ferrss/plugins/` | 用户插件 |
| `~/.ferrss/external-clis.yaml` | 用户外部 CLI 注册表 |

## 架构

```
┌─────────────────────────────────────────────────────────────────┐
│                         用户 / AI Agent                         │
│                     ferrss <site> <command>                  │
└─────────────────────┬───────────────────────────────────────────┘
                      │
                      ▼
┌─────────────────────────────────────────────────────────────────┐
│                      CLI 层 (clap)                               │
│  main.rs → discovery → clap 动态子命令 → execution.rs            │
│  ┌───────────┐  ┌───────────────┐  ┌──────────────────┐        │
│  │ 内置命令   │  │ 站点适配器命令 │  │ 外部 CLI 透传     │        │
│  │ explore    │  │ bilibili hot  │  │ gh, docker, k8s  │        │
│  │ doctor     │  │ twitter feed  │  │                  │        │
│  └───────────┘  └───────┬───────┘  └──────────────────┘        │
└─────────────────────────┼───────────────────────────────────────┘
                          │
                          ▼
┌─────────────────────────────────────────────────────────────────┐
│                     执行引擎 (execution.rs)                      │
│               参数校验 → 能力路由 → 超时控制                      │
│                    ┌─────────┼─────────┐                        │
│                    ▼         ▼         ▼                        │
│              YAML Pipeline  Rust Func  External CLI              │
└────────────────┬────────────────────────────────────────────────┘
                 │
                 ▼
┌──────────────────────────────────────────────────────────────────┐
│  Pipeline 引擎                        浏览器桥接                  │
│  ┌────────────┐                      ┌─────────────────────┐    │
│  │ fetch      │                      │ BrowserBridge       │    │
│  │ evaluate   │  ──── IPage ────▶    │ DaemonClient (HTTP) │    │
│  │ navigate   │                      │ CdpPage (WebSocket) │    │
│  │ map/filter │                      └──────────┬──────────┘    │
│  │ sort/limit │                                 │               │
│  │ intercept  │                      Daemon (axum:19825)        │
│  │ tap        │                        HTTP + WebSocket          │
│  └────────────┘                                 │               │
│                                                 ▼               │
│  表达式引擎 (pest)                    Chrome 扩展 (CDP)          │
│  ${{ expr | filter }}                 chrome.debugger API        │
└──────────────────────────────────────────────────────────────────┘
```

### Workspace 结构

```
Ferrss/
├── crates/
│   ├── autocli-core/        # 核心数据模型：Strategy, CliCommand, Registry, IPage trait, Error
│   ├── autocli-pipeline/    # Pipeline 引擎：pest 表达式, 执行器, 14 种步骤
│   ├── autocli-browser/     # 浏览器桥接：Daemon, DaemonPage, CdpPage, DOM helpers
│   ├── autocli-output/      # 输出渲染：table, json, yaml, csv, markdown
│   ├── autocli-discovery/   # 适配器发现：YAML 解析, build.rs 编译时嵌入
│   ├── autocli-external/    # 外部 CLI：加载, 检测, 透传执行
│   ├── autocli-ai/          # AI 能力：explore, synthesize, cascade, generate
│   └── autocli-cli/         # CLI 入口：clap, 执行编排, doctor, completion
├── adapters/                   # 333 个 YAML 适配器定义
│   ├── hackernews/
│   ├── bilibili/
│   ├── twitter/
│   └── ...（55 个站点）
└── resources/
    └── external-clis.yaml      # 外部 CLI 注册表
```

### 相比 TypeScript 原版的改进

| 改进项 | 原版 (TypeScript) | ferrss (Rust) |
|--------|-------------------|-------------------|
| 分发方式 | Node.js + npm install (~100MB) | 单一二进制 (4.1MB) |
| 启动速度 | 读 manifest JSON → 解析 → 注册 | 编译时嵌入，零文件 I/O |
| 模板引擎 | JS eval (安全隐患) | pest PEG parser (类型安全) |
| 并发 fetch | 非浏览器模式 pool=5 | FuturesUnordered, 并发度 10 |
| 错误系统 | 单一 hint 字符串 | 结构化错误链 + 多条建议 |
| HTTP 连接 | 每次 new fetch | reqwest 连接池复用 |
| 内存安全 | GC | 所有权系统，零 GC 暂停 |

## 开发

```bash
# 构建
cargo build

# 测试（166 个测试）
cargo test --workspace

# Release 构建（启用 LTO，约 4MB）
cargo build --release

# 添加新适配器
# 1. 在 adapters/<site>/ 下创建 YAML 文件
# 2. 重新编译（build.rs 自动嵌入）
cargo build
```

## 支持的站点

<details>
<summary>展开查看全部 55 个站点</summary>

| 站点 | 命令数 | 策略 |
|------|--------|------|
| hackernews | 8 | public |
| bilibili | 12 | cookie |
| twitter | 24 | cookie/intercept |
| reddit | 15 | public/cookie |
| zhihu | 2 | cookie |
| xiaohongshu | 11 | cookie |
| douban | 7 | cookie |
| weibo | 2 | cookie |
| v2ex | 11 | public/cookie |
| bloomberg | 10 | cookie |
| youtube | 4 | cookie |
| wikipedia | 4 | public |
| google | 4 | public/cookie |
| facebook | 10 | cookie |
| instagram | 14 | cookie |
| tiktok | 15 | cookie |
| notion | 8 | ui |
| cursor | 12 | ui |
| chatgpt | 6 | public |
| stackoverflow | 4 | public |
| devto | 3 | public |
| lobsters | 4 | public |
| medium | 3 | cookie |
| substack | 3 | cookie |
| weread | 7 | cookie |
| xueqiu | 7 | cookie |
| boss | 14 | cookie |
| jike | 10 | cookie |
| 其他 27 个站点 | ... | ... |

</details>

## Star History

<a href="https://www.star-history.com/?repos=oouxx%2FFerrss&type=date&legend=top-left">
 <picture>
   <source media="(prefers-color-scheme: dark)" srcset="https://api.star-history.com/image?repos=oouxx/Ferrss&type=date&theme=dark&legend=top-left" />
   <source media="(prefers-color-scheme: light)" srcset="https://api.star-history.com/image?repos=oouxx/Ferrss&type=date&legend=top-left" />
   <img alt="Star History Chart" src="https://api.star-history.com/image?repos=oouxx/Ferrss&type=date&legend=top-left" />
 </picture>
</a>

## 致谢

本项目基于 [OpenCLI](https://github.com/jackwener/opencli)（作者 [jackwener](https://github.com/jackwener)）构建。感谢原始项目为本项目奠定的基础。

## 许可证

Apache-2.0
