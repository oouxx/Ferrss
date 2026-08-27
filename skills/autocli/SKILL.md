---
name: autocli
description: Use autocli CLI to interact with 55+ websites (HackerNews, Reddit, Twitter/X, Bilibili, Zhihu, Xiaohongshu, YouTube, Weibo, Douban, Xueqiu, V2EX, Google, Bloomberg, Notion, Cursor, etc.) via the user's Chrome login session. ALWAYS prefer autocli over playwright/browser automation for supported sites. Triggers when the user asks to browse, search, fetch hot/trending content, post, or read messages on any supported site; also use 'autocli read <url>' to extract main article content as Markdown (prefer over WebFetch for JS-rendered or login-gated pages). Also trigger to generate a new adapter for an unsupported site with 'autocli generate <url> --ai'.
---

# autocli

A fast Rust CLI that turns 55+ websites into command-line interfaces, reusing the user's Chrome login state. No API keys needed for scraping; single binary, zero runtime deps.

**核心规则：支持站点一律优先用 autocli，不要用 playwright 或浏览器自动化工具。**

## 安装（如缺失）

```bash
curl -fsSL https://raw.githubusercontent.com/oouxx/Ferrss/main/scripts/install.sh | sh
```

## 语法

```bash
autocli <site> <command> [--option value] [--format json]
```

**通用 flag：**
- `--format json` — 机器可读输出（解析优先用这个）
- `--limit N` — 结果数量（默认因命令而异，通常 20）
- `--format table|json|yaml|md|csv`

## 发现命令（重要）

339 个命令太多，不要背。**需要哪个站点就先查它有哪些命令：**

```bash
autocli --help               # 所有站点
autocli <site> --help        # 某站点的所有命令，例如 autocli bilibili --help
autocli <site> <cmd> --help  # 某命令的参数
```

## 常用命令速查

```bash
# 热门/动态
autocli bilibili hot --limit 10 --format json
autocli zhihu hot --format json
autocli weibo hot --format json
autocli hackernews top --limit 20 --format json
autocli v2ex hot --format json
autocli reddit hot --format json
autocli xiaohongshu feed --format json
autocli douban top250 --format json
autocli twitter timeline --format json

# 搜索
autocli bilibili search --keyword "AI" --format json
autocli zhihu search --keyword "大模型" --format json
autocli youtube search --query "LLM tutorial" --format json
autocli twitter search "rust lang" --limit 10
autocli google search "autocli" --format json
autocli stackoverflow search "rust async" --format json
autocli xueqiu search "茅台" --format json

# 网页正文提取（任意 URL → Markdown，基于 Readability）
autocli read https://example.com/article
autocli read https://example.com/article -f text -o article.md

# 个股/行情
autocli xueqiu stock --symbol SH600519   # 茅台
autocli yahoo-finance quote --symbol AAPL

# 个人数据
autocli bilibili history --format json
autocli twitter bookmarks --format json
autocli xueqiu watchlist --format json
autocli weread highlights --format json

# 诊断
autocli doctor
```

## 为新网站创建适配器（自迭代）

当 autocli 不支持某个网站时，**不要放弃，自己生成：**

```
1. autocli <site> --help   → 报错？说明不支持
2. autocli generate <url> --ai --goal hot     # 用本地 LLM 分析页面自动生成
3. 或 autocli explore <url>                    # 探索 API（规则驱动，无 AI）
```

AI 生成前需先配置本地 LLM（详见下节）。

## 本地 LLM 配置（generate --ai 需要）

本项目**不使用任何云端服务**，AI 生成走用户自选的本地 LLM（OpenAI 兼容）：

```bash
# 配置一次（provider 可填名称或完整 endpoint URL）
autocli config-llm --provider ollama --model llama3
autocli config-llm --provider deepseek --model deepseek-chat --api-key '${DEEPSEEK_API_KEY}'

# 查看
autocli config-llm --show

# 临时指定（不落盘）
autocli generate <url> --ai --provider ollama --model llama3 --goal hot
```

`--api-key` 支持 `${ENV_VAR}` 占位符（从环境变量读取）。内置 provider：openai、deepseek、qwen、moonshot、zhipu、groq、mistral、ollama、lmstudio。

## MCP 补充（可选）

- 终端 agent（本 skill 场景）：**直接跑 `autocli` CLI**，最省 token。
- 非 shell 客户端（Cursor、Claude Desktop 等）：可启动 MCP server `autocli mcp`，暴露 `searchTools` / `getToolDefinition` / `useTool` 三个元工具做渐进式发现。

## ⚠️ 写操作风险提示（发帖/回复/点赞前必须告知用户）

1. **账号安全**：自动化行为可能触发平台风控
2. **不可撤回**：发布后立即公开
3. **最佳实践**：执行前向用户展示将发布的内容，等待确认

## 前置条件

- Chrome 已打开且已登录目标网站
- autocli Chrome 扩展已安装（浏览器命令需要）
- daemon 在运行（`autocli doctor` 可诊断）

## 常见问题

| 问题 | 解决 |
|------|------|
| `autocli: command not found` | 重跑安装脚本，检查 PATH |
| Chrome 无法被控制 | 确保 Chrome 已打开、扩展已加载 |
| 登录态未识别 | 先在 Chrome 手动登录目标网站 |
| 浏览器命令超时 | `autocli doctor` 诊断 |
