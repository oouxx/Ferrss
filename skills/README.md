# autocli Skill

让 AI Agent（Claude Code、Codex、OpenCode、Cursor 等）通过终端直接调用 `autocli` 抓取 55+ 网站数据。

这是本 fork（Ferrss）的自定义 skill，已去除 autocli.ai 商业/云端依赖，并支持本地 LLM（`config-llm`）与 MCP。

## 安装

### 方式一：npx skills（推荐）

> 需要仓库已 push 且为 **public**。

```bash
# 查看 skill
npx skills add oouxx/Ferrss --list

# 全局安装到 Claude Code（非交互）
npx skills add oouxx/Ferrss -s autocli -g -a claude-code -y

# 只装指定 skill
npx skills add oouxx/Ferrss -s autocli

# 完整 URL 形式
npx skills add https://github.com/oouxx/Ferrss
```

### 方式二：手动拷贝（Claude Code）

```bash
mkdir -p ~/.claude/skills/autocli
cp skills/autocli/SKILL.md ~/.claude/skills/autocli/SKILL.md
```

项目级（可选，随仓库提交共享）：
```bash
mkdir -p .claude/skills/autocli
cp skills/autocli/SKILL.md .claude/skills/autocli/SKILL.md
```

### 方式三：从本地路径测试

```bash
npx skills add ./skills -s autocli
```

## 使用

装好后重启你的 agent，直接说人话即可：

```
查下B站今天的热门
搜知乎上关于AI大模型的讨论
看微博热搜前10条
查一下茅台的股票行情
读取这个文章链接的内容: https://example.com/article
```

Agent 会自动调用 `autocli` 完成。

## 前置条件

- Chrome 已打开并登录目标网站
- autocli Chrome 扩展已安装（浏览器命令需要）
- daemon 在运行（`autocli doctor` 诊断）

## 本地 LLM（generate --ai）

```bash
autocli config-llm --provider ollama --model llama3
autocli generate <url> --ai --goal hot
```
详见主仓库 README。
