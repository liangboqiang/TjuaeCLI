# 快速开始

## 安装

```bash
# 从源码构建
cargo build --release

# 二进制文件位置
./target/release/tjuae-cli
```

## 命令格式

```text
tjuae-cli [OPTIONS] [PROMPT]...
```

- 提供 `PROMPT`：以单次运行模式完成任务并退出。
- 不提供 `PROMPT`：进入交互式 REPL 模式。

> 运行 `tjuae-cli --help` 可查看完整的 CLI 参数列表。

### 子命令

管理操作使用名词子命令，而不是分散的顶层参数。子命令执行对应操作后便会退出，
不会进入智能体主流程。

| 子命令 | 说明 |
|--------|------|
| `tjuae-cli config init` | 生成默认的全局配置文件 |
| `tjuae-cli config path` | 输出全局配置文件路径 |
| `tjuae-cli auth login` | 使用 Anthropic 账户登录（OAuth 设备流程） |
| `tjuae-cli auth logout` | 退出登录（删除已保存的 OAuth 凭据） |
| `tjuae-cli session list` | 列出已保存的会话 |
| `tjuae-cli skills path` | 输出技能目录路径 |

### 主要参数

| 参数 | 说明 |
|------|------|
| `--provider <name>` | 提供商：`anthropic`、`openai`、`bedrock`、`vertex` 或自定义别名 |
| `--model <id>` | 模型名称 |
| `--profile <name>` | 配置文件中定义的命名配置档 |
| `--compaction <level>` | 输出压缩级别：`off`、`safe`（默认）或 `full` |
| `--toon` | 启用 TOON 表格编码（与 `full` 压缩配合使用） |
| `--max-turns <n>` | 每次运行的总体模型轮次上限；默认不设置，`0` 表示禁用 |
| `--max-tool-call-malformed-turns <n>` | 连续出现相同工具调用格式错误后停止；`0` 表示禁用 |
| `--max-tool-call-failure-turns <n>` | 连续出现相同工具调用失败后停止；`0` 表示禁用 |
| `--auto-approve` | 跳过所有工具确认 |
| `--json-stream` | 用于宿主集成的 JSON Lines 模式 |
| `--resume <id>` | 恢复之前的会话 |
| `--log-dir <path>` | 启用文件日志并写入指定目录 |
| `--log-level <filter>` | 日志级别过滤器，例如 `debug`、`info`、`tjuae_providers=debug` |

---

## 配置

### 三级级联

```text
<全局配置>                       （用户级；运行 `tjuae-cli config path` 查找）
    ↓ 被以下配置覆盖
./.tjuae.toml                    （项目级，位于工作目录）
    ↓ 被以下配置覆盖
CLI 参数 / 环境变量              （最高优先级）
```

### 生成默认配置

```bash
tjuae-cli config init
# 创建全局配置文件（运行 `tjuae-cli config path` 查看位置）
```

### 配置文件格式

```toml
# 全局配置文件（路径因操作系统而异，运行 `tjuae-cli config path` 查找）

[default]
provider = "anthropic"
# model = "claude-sonnet-4-20250514"
# max_tokens = 8192  # 可选的单次响应输出上限；省略时使用提供商/模型默认值
# max_turns = 20  # 可选的每次运行最大模型轮次数；省略或设为 0 表示禁用
max_tool_call_malformed_turns = 3  # 默认值；设为 0 可禁用该熔断器
max_tool_call_failure_turns = 3  # 默认值；设为 0 可禁用该熔断器

[providers.anthropic]
# api_key = "sk-ant-xxx"       # 也可使用环境变量 ANTHROPIC_API_KEY
# base_url = "https://api.anthropic.com"

[providers.openai]
# api_key = "sk-xxx"           # 也可使用环境变量 OPENAI_API_KEY
# base_url = "https://api.openai.com/v1"

# 自定义提供商别名
[providers.my-service]
provider = "openai"
model = "custom-model-v1"
api_key = "sk-xxx"
base_url = "https://my-service.example.com/api/openai"

# 命名配置档，通过 --profile <name> 切换
[profiles.deepseek]
provider = "openai"
model = "deepseek-chat"
api_key = "sk-xxx"
base_url = "https://api.deepseek.com/v1"

[profiles.deepseek-v4-pro]
provider = "openai"
model = "deepseek-v4-pro"
api_key = "sk-xxx"
base_url = "https://api.deepseek.com/v1"
max_tokens = 16384

[profiles.deepseek-v4-pro.compat]
supports_thinking = true

[profiles.ollama]
provider = "openai"
model = "qwen2.5:32b"
api_key = "ollama"
base_url = "http://localhost:11434"

[profiles.my-service]
provider = "my-service"

# 配置档名称由用户定义；这不是内置配置档。
[profiles.my-weak-provider]
provider = "openai"
max_tool_call_malformed_turns = 2
max_tool_call_failure_turns = 2

[tools]
auto_approve = false
allow_list = ["Read", "Grep", "Glob"]

[session]
enabled = true
directory = ".tjuae/sessions"
max_sessions = 20

[compact]
compaction = "safe"   # off | safe | full
toon = false          # 为 JSON 数组启用 TOON 编码
# autocompact_threshold_pct = 50  # 在上下文窗口达到 N% 时触发自动压缩

[file_cache]
enabled = true
max_entries = 100

[plan]
enabled = true
plan_directory = ".tjuae/plans"

# [logging]
# enabled = true              # 启用文件日志（默认：false）
# level = "info"              # 日志级别过滤器（默认："info"）
# dir = "/path/to/logs"       # 日志目录（默认：平台专属位置）
```

### 运行时限制

`max_turns` 是每次运行的总体模型轮次上限。默认不设置，因此除非显式配置，否则
运行没有总体模型轮次限制。将其设为 `0` 也可明确禁用总体限制。运行、轮次、工具
轮次和工具调用之间的区别参见[核心概念](core-concepts.md)。

`max_tool_call_malformed_turns` 限制提供商连续返回相同工具调用格式错误的轮次。
默认值为 `3`；设为 `0` 会禁用该熔断器。若已配置总体轮次上限，则改由
`max_turns` 负责终止运行。

`max_tool_call_failure_turns` 限制相同工具名称和输入模式连续失败的次数。助手的
解释文本不会重置计数，同一轮中其他成功调用也不会清除仍在重复的失败。完全没有
工具失败的轮次会重置精确调用和循环历史。默认值为 `3`。

同一保护机制还会在连续 3 个全错误轮次后发出警告，并在连续 8 个全错误轮次后
结束运行；它也会检测长度为 2～4 个轮次的重复调用循环，在重复 2 次后警告、
重复 3 次后结束。将 `max_tool_call_failure_turns` 设为 `0` 会禁用所有这些
工具失败保护。若已配置总体轮次上限，则改由 `max_turns` 负责终止运行。

优先级为 `CLI > 配置档 > 项目配置 > 全局配置 > 内置默认值 3`。单次运行可通过
`--max-tool-call-malformed-turns <n>` 或 `--max-tool-call-failure-turns <n>`
覆盖配置。

### API 密钥解析顺序

1. CLI 参数 `--api-key`
2. 配置文件中的 `providers.<name>.api_key`
3. 环境变量 `API_KEY`
4. 环境变量 `ANTHROPIC_API_KEY` 或 `OPENAI_API_KEY`（取决于提供商）
5. OAuth 凭据（通过 `tjuae-cli auth login` 获取）

> **注意：** `bedrock` 和 `vertex` 使用各自的云平台凭据，不需要传统 API 密钥。
> 详见[提供商与认证](providers.md)。

### 自定义提供商别名

如果某个后端兼容内置提供商的协议，可以在 `providers.<alias>` 下声明别名：

```toml
[default]
provider = "my-service"

[providers.my-service]
provider = "openai"
model = "custom-model-v1"
api_key = "sk-xxx"
base_url = "https://my-service.example.com/api/openai"
```

- `default.provider` 和 `profile.provider` 都可以使用别名。
- `providers.<alias>.provider` 必须声明底层类型，目前只能是 `anthropic`、
  `openai`、`bedrock` 或 `vertex`。
- 别名条目会覆盖相应底层提供商的默认配置。

---

## 快速上手

### 1. 初始化并配置

```bash
tjuae-cli config init
# 编辑配置文件（运行 `tjuae-cli config path` 查找），加入 API 密钥
```

### 2. 单次运行模式

```bash
tjuae-cli "读取并解释 crates/tjuae-agent/src/engine.rs"
```

### 3. 交互式 REPL

```text
$ tjuae-cli

> 读取 Cargo.toml 文件
     1  [package]
     2  name = "tjuae-cli"
     ...
[轮次：1 | token：输入 1234 / 输出 567]

> 将 serde_yaml 添加到依赖项
[工具] Write({"file_path":"Cargo.toml","content":"..."})
允许？[y]是 / [n]否 / [a]始终允许 / [q]退出 > y
[Write] 成功
[轮次：2 | token：输入 2345 / 输出 890]

> /quit
```

REPL 命令：输入 `/quit`、`/exit` 或空行可退出。

### 4. 切换配置档

```bash
tjuae-cli --profile deepseek "修复 main.rs 中的缺陷"
tjuae-cli --profile ollama "分析代码质量"
```

### 5. 环境变量

```bash
export ANTHROPIC_API_KEY=sk-ant-xxx
tjuae-cli "列出此项目中的所有 Rust 文件"
```

---

## 工具确认

具有破坏性的工具（Write、Edit、ExecCommand）在执行前会请求确认：

```text
[工具] Write({"file_path": "/tmp/test.rs", "content": "..."})
允许？[y]是 / [n]否 / [a]始终允许 / [q]退出 > y
```

| 选项 | 说明 |
|------|------|
| `y` / `yes` / 回车 | 允许本次执行 |
| `n` / `no` | 拒绝；LLM 会收到“已拒绝”错误 |
| `a` / `always` | 在本次会话的剩余时间内自动批准该工具 |
| `q` / `quit` | 中止整个智能体运行 |

- 只读工具（Read、Grep、Glob）默认自动批准。
- `--auto-approve` 会跳过所有确认。
- 可通过配置中的 `tools.allow_list` 自定义白名单。

---

## 会话管理

会话会自动保存到 `.tjuae/sessions/`。

```bash
# 列出已保存的会话
tjuae-cli session list

# 恢复最近一次会话
tjuae-cli --resume latest

# 恢复指定会话
tjuae-cli --resume a1b2c3

# 使用自定义 ID 创建会话
tjuae-cli --session-id my-conv-123
```

- `--session-id` 与 `--resume` 互斥。
- 若 ID 已存在，`--session-id` 会报错。
- 两个参数均可用于交互模式和 `--json-stream` 模式。
- 每个工具轮次结束后自动保存。
- 会话数量超过 `max_sessions` 时自动清理最早的会话。
