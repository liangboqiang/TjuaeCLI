# TjuaeCLI

一个基于 Rust 的命令行 LLM 工具调用智能体。它连接 LLM API，自主调用文件 I/O、shell、搜索等本地工具，端到端完成任务。

## 功能

- **多提供商支持**——Anthropic、OpenAI（以及 DeepSeek、Ollama、Gemini 等兼容服务）、AWS Bedrock、Google Vertex AI
- **ProviderCompat 层**——通过配置适配提供商差异，不使用硬编码条件判断
- **推理模型支持**——支持 OpenAI `o1`/`o3` 推理模型及 `reasoning_effort` 控制
- **10 个常规内置工具**——Read、Write、Edit、ExecCommand、Grep、Glob、ViewImage、Skill、Spawn（子智能体）和 ToolSearch；启用计划模式时另注册 EnterPlanMode / ExitPlanMode
- **MCP 客户端**——连接任意[模型上下文协议](https://modelcontextprotocol.io/)服务器（stdio / SSE / streamable-http）
- **动态注入 MCP**——宿主客户端可通过 [JSON 流协议](docs/json-stream-protocol.md)在运行时注入 MCP 服务器
- **技能系统**——支持变量替换、shell 展开、条件激活，以及按技能覆盖模型和权限的命名提示词片段（参见 [docs/skills.md](docs/skills.md)）
- **钩子系统**——在工具生命周期中执行事件驱动的自动化操作（自动格式化、lint、审计）
- **子智能体生成**——通过 Spawn 工具并行执行任务
- **会话持久化**——保存和恢复对话历史
- **持久记忆**——带跨会话自动索引的项目专属记忆（参见 [docs/advanced.md](docs/advanced.md#记忆系统)）
- **计划模式**——编码前用于制定实施计划的只读探索模式（参见 [docs/advanced.md](docs/advanced.md#计划模式)）
- **上下文压缩**——三级自动压缩：microcompact、autocompact、emergency（参见 [docs/advanced.md](docs/advanced.md#上下文压缩)）
- **输出压缩**——支持 off/safe/full 级别和 TOON 编码的可配置输出压缩（参见 [docs/advanced.md](docs/advanced.md#输出压缩)）
- **文件状态缓存**——支持读取去重和写入跟踪的 LRU 缓存
- **提示词缓存**——使用 Anthropic `cache_control`，最高可降低 90% 成本
- **配置档继承**——命名配置档通过 `extends` 快速切换提供商和模型
- **OAuth 登录**——无需 API 密钥，直接使用 Claude.ai 订阅
- **AGENTS.md 注入**——分层加载项目指令，支持 `@include`

## 快速开始

```bash
# 从源码构建
cargo build --release

# 生成默认配置，然后添加 API 密钥
./target/release/tjuae-cli config init
# 编辑生成的配置（运行 `tjuae-cli config path` 查看路径）

# 单次运行模式
tjuae-cli "读取 Cargo.toml 并解释其中的依赖"

# 交互式 REPL
tjuae-cli

# 完整 CLI 帮助
tjuae-cli --help
```

## 运行限制

`max_turns` 是每次运行的总体模型轮次上限。默认不设置，因此除非显式配置，
否则运行没有总体轮次限制。设为 `0` 也可明确禁用此限制。
`max_tool_call_malformed_turns` 会提前终止反复出现的相同工具调用格式错误，
默认值为 `3`。`max_tool_call_failure_turns` 根据失败工具的名称和输入识别并终止
重复失败模式，不受助手文本或同一轮中其他成功调用的影响，默认值同样为 `3`。
失败保护还会检测连续全错误轮次和短周期重复调用。将任一保护设为 `0` 可禁用
相应熔断器；如果已配置总体轮次限制，则改由 `max_turns` 兜底。

运行、轮次、工具轮次和工具调用之间的区别参见[核心概念](docs/core-concepts.md)。

```toml
[default]
max_turns = 20  # 可选的总体模型轮次上限
max_tool_call_malformed_turns = 3
max_tool_call_failure_turns = 3

# 配置档名称由用户定义；这不是内置配置档。
[profiles.my-weak-provider]
max_turns = 10
max_tool_call_malformed_turns = 2
max_tool_call_failure_turns = 2
```

CLI 覆盖参数：

```bash
tjuae-cli --max-turns 10 "执行任务"
tjuae-cli --max-tool-call-malformed-turns 2 "执行任务"
tjuae-cli --max-tool-call-failure-turns 2 "执行任务"
```

## 架构

```
┌──────────────────────────────────────────────────────────────┐
│                      main.rs（CLI / REPL）                   │
├──────────────────────────────────────────────────────────────┤
│  配置             │  引擎（智能体循环）   │  会话管理器        │
│  （三级合并）     │  流式输出 + 工具      │  保存 / 恢复       │
├──────────────────┼───────────────────────┼───────────────────┤
│  提供商           │  工具注册表           │  钩子执行器         │
│  ├ Anthropic     │  ├ 常规内置工具（10） │  ├ pre_tool_use   │
│  ├ OpenAI        │  ├ MCP 工具（N）      │  ├ post_tool_use  │
│  ├ Bedrock       │  └ 计划模式工具       │  └ stop           │
│  └ Vertex AI     │                       │                   │
│                  │  MCP 客户端           │  记忆系统          │
│  ProviderCompat  │  ├ Stdio 传输         │  （按项目）        │
│  （兼容层）      │  ├ SSE 传输           │                   │
│                  │  └ HTTP 传输          │  子智能体          │
│  压缩引擎        │                       │  生成器            │
│  ├ Microcompact  │  文件状态缓存         │                   │
│  ├ Autocompact   │  （LRU）              │  输出压缩器        │
│  └ Emergency     │                       │  (off/safe/full)  │
└──────────────────┴───────────────────────┴───────────────────┘
```

## 文档

| 文档 | 说明 |
|----------|-------------|
| [快速入门](docs/getting-started.md) | 安装、CLI 参考、配置和使用示例 |
| [内置工具](docs/tools.md) | 10 个常规工具、计划模式工具与动态 MCP 工具参考 |
| [MCP 集成](docs/mcp.md) | 模型上下文协议客户端的设置与使用 |
| [提供商与认证](docs/providers.md) | 多提供商配置、配置档、Bedrock、Vertex 和 OAuth |
| [高级功能](docs/advanced.md) | 子智能体、钩子、提示词缓存、VCR 和 AGENTS.md |
| [故障排除](docs/troubleshooting.md) | 常见错误和解决方案 |
| [JSON 流协议](docs/json-stream-protocol.md) | 宿主集成协议（`--json-stream` 模式） |

## 支持的提供商

| 提供商 | 认证方式 | 说明 |
|----------|------|-------|
| Anthropic | API 密钥 / OAuth | 提示词缓存、流式输出、视觉能力 |
| OpenAI | API 密钥 | 推理模型（`o1`/`o3`），兼容 DeepSeek、Qwen、Ollama、Gemini、vLLM |
| AWS Bedrock | SigV4 | 区域端点、AWS 凭据链、schema 清理、可操作的错误提示 |
| Google Vertex AI | GCP OAuth2 / 服务账户 | 自动检测元数据服务器 |

## ProviderCompat

所有提供商专属行为均由 `ProviderCompat` 配置层驱动，不使用硬编码 URL 或模型名称判断。
每种提供商类型都有合理默认值，也可通过配置覆盖任意字段：

```toml
[providers.my-openai.compat]
max_tokens_field = "max_completion_tokens"   # 最大 token 数对应的字段名
merge_assistant_messages = true              # 合并连续的 assistant 消息
clean_orphan_tool_calls = true               # 删除缺少 tool_result 的 tool_use
dedup_tool_results = true                    # 对相同 tool_call_id 的结果去重
ensure_alternation = false                   # 为 user/assistant 交替插入占位消息
merge_same_role = false                      # 合并连续的同角色消息
sanitize_schema = false                      # 使用 Bedrock 风格的 schema 清理
strip_patterns = ["<think>", "</think>"]     # 从历史记录中删除文本模式
auto_tool_id = false                         # 自动生成缺失的工具 ID
api_path = "/v1/chat/completions"            # 自定义 chat completions 端点路径
```

提供商默认行为：**Anthropic/Vertex**——消息交替、合并、自动工具 ID；
**Bedrock**——前述行为加 schema 清理；**OpenAI**——合并 assistant 消息、清理孤立工具调用、结果去重。

## 发布流程

TjuaeCLI 使用可审计的标签发布流程，不依赖自动发布 PR：

1. 同步更新 `Cargo.toml`、`Cargo.lock` 和 `CHANGELOG.md` 中的版本。
2. 运行 `just verify`，通过格式、Clippy、测试、Hakari 和安全审计门禁。
3. 提交版本变更并用 `just push origin main` 更新主分支。
4. 创建并推送 `v<版本>` 标签。
5. `release.yml` 自动为 Linux、macOS、Windows 的 x64/arm64 目标构建六份归档，
   生成 `tjuae-cli-checksums.txt`，并发布到对应的 GitHub Release。

工作流也支持手动输入已有标签重新构建，用于恢复失败的发布；它不会修改版本或创建
额外分支。
