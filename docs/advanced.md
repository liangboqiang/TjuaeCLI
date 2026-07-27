# 高级功能

## 子智能体

LLM 可以使用 Spawn 工具创建独立的子智能体，并行执行任务。每个子智能体都有自己
的对话上下文，继承父智能体的运行时工具策略，并与父智能体共用 LLM 提供商
（复用连接池）。fork 模式覆盖可以进一步限制继承的工具，但不能恢复父智能体已经
禁止的工具。

### 适用场景

- “同时搜索这 3 个文件并分别总结”
- “并行运行测试和 lint”
- “读取 Y 的同时，在代码库中搜索 X”

### 限制

| 设置 | 默认值 | 说明 |
|------|--------|------|
| 最大并行子智能体数 | 5 | 防止资源耗尽 |
| 子智能体最大轮次数 | 10 | 每次子智能体运行的轮次上限 |
| 子智能体最大 token 数 | 4096 | 每次子智能体响应的 token 上限 |

### 行为

- 子智能体自动批准所有工具调用，不显示确认提示。
- 子智能体不能超出父智能体的运行时工具策略。
- 子智能体不保存会话。
- 子智能体静默运行，不向标准输出写入内容。
- 所有结果会合并后返回父智能体。

---

## 钩子系统

事件驱动的钩子会在工具生命周期的特定阶段执行 shell 命令，可用于自动格式化、
lint、审计等操作。

### 钩子类型

| 类型 | 触发时机 | 行为 |
|------|----------|------|
| `pre_tool_use` | 工具执行前 | 非零退出码会阻止工具执行 |
| `post_tool_use` | 工具执行后 | 不阻塞；错误会写入日志 |
| `stop` | 智能体会话结束时 | 不阻塞 |

### 配置

```toml
# 修改 Rust 文件后自动格式化
[[hooks.post_tool_use]]
name = "rustfmt"
tool_match = ["Write", "Edit"]
file_match = ["*.rs"]
command = "rustfmt ${TOOL_INPUT_FILE_PATH}"

# 修改 TypeScript 文件后自动格式化
[[hooks.post_tool_use]]
name = "prettier"
tool_match = ["Write", "Edit"]
file_match = ["*.ts", "*.tsx"]
command = "npx prettier --write ${TOOL_INPUT_FILE_PATH}"

# 审计 ExecCommand 命令
[[hooks.post_tool_use]]
name = "audit-log"
tool_match = ["ExecCommand"]
command = "echo \"$(date): ${TOOL_INPUT_COMMAND}\" >> .tjuae/audit.log"

# 会话结束时运行 lint
[[hooks.stop]]
name = "final-lint"
command = "cargo clippy --quiet 2>&1 | tail -5"
```

### 环境变量

钩子命令可以通过 `${VAR}` 语法引用以下变量：

| 变量 | 说明 |
|------|------|
| `TOOL_NAME` | 工具名称 |
| `TOOL_INPUT` | 完整的工具输入 JSON |
| `TOOL_INPUT_FILE_PATH` | 文件路径（若工具具有 `file_path` 参数） |
| `TOOL_INPUT_COMMAND` | 命令（若工具具有 `command` 参数） |
| `TOOL_INPUT_PATTERN` | 搜索模式（若工具具有 `pattern` 参数） |
| `TOOL_OUTPUT` | 工具输出（仅用于 `post_tool_use`） |

### 匹配规则

- `tool_match`：匹配工具名称的 Glob 模式；为空时匹配所有工具。
- `file_match`：匹配文件路径的 Glob 模式；为空时匹配所有文件。
- 默认超时时间为 30 秒，可通过 `timeout_ms` 配置。

---

## 提示词缓存（Anthropic）

提示词缓存会将系统提示词和工具定义存储在 Anthropic 服务器上，使后续请求只需
处理发生变化的部分。

- **首次请求**：完整输入 token 成本，另加 25% 写入费用。
- **后续请求**：缓存部分只需 10% 的费用。
- **缓存 TTL**：5 分钟，每次命中时自动续期。

### 配置

```toml
[providers.anthropic]
api_key = "sk-ant-xxx"
prompt_caching = true   # 默认为 true（仅适用于 Anthropic）
```

### Token 统计

启用缓存后，统计信息会显示缓存数据：

```text
[轮次：3 | token：输入 100（缓存 5000）/ 输出 200 | 缓存：创建 5000，读取 5000]
```

---

## VCR 录制与重放

录制真实 API 交互并在测试中重放，无需 API 密钥或网络连接。

### 用法

```bash
# 录制模式
VCR_MODE=record VCR_CASSETTE=tests/cassettes/my_test.json \
  tjuae-cli -k sk-ant-xxx "读取 Cargo.toml"

# 重放模式（用于测试）
VCR_MODE=replay VCR_CASSETTE=tests/cassettes/my_test.json \
  tjuae-cli "读取 Cargo.toml"
```

### 功能

- 自动脱敏：录制时将敏感请求头（api-key、auth、token）替换为 `[REDACTED]`。
- 录像带文件采用 JSON 格式，可以手动编辑。
- 支持录制和重放 SSE 流式响应。

---

## 日志

基于 `tracing` crate 提供按天轮转的结构化 JSON 文件日志。所有内部事件，包括
LLM 请求/响应、工具执行、MCP 连接和压缩，都会以结构化字段记录。

### 启用方式

可以通过以下三种方式启用日志，优先级从高到低排列：

1. **CLI 参数**：`--log-dir /path/to/logs`，会自动启用日志。
2. **配置文件**：在全局或项目配置中添加 `[logging]`。
3. **默认行为**：除非显式配置，否则禁用日志。

```bash
# CLI——以 debug 级别将日志写入 /tmp/tjuae-logs
tjuae-cli --log-dir /tmp/tjuae-logs --log-level debug "读取 Cargo.toml"
```

### 配置

```toml
[logging]
enabled = true         # 启用文件日志（默认：false；设置 dir 时自动启用）
level = "info"         # tracing 过滤指令（默认："info"）
dir = "/path/to/logs"  # 日志目录（默认：平台专属位置，见下文）
```

`level` 字段接受标准的 tracing 过滤指令：

| 值 | 效果 |
|----|------|
| `"info"` | 所有 target 记录 info 及以上级别 |
| `"debug"` | 所有 target 记录 debug 及以上级别 |
| `"tjuae_providers=debug,info"` | 提供商记录 debug，其余 target 记录 info |

### 默认日志目录

未设置 `dir` 时，日志会写入各平台的默认位置：

| 平台 | 路径 |
|------|------|
| macOS | `~/Library/Logs/tjuae/` |
| Linux | `$XDG_STATE_HOME/tjuae/logs/` 或 `~/.local/state/tjuae/logs/` |
| Windows | `{data_local_dir}/tjuae/logs/` |

### 日志格式

每一行都是带有结构化字段的 JSON 对象：

```json
{"timestamp":"2026-05-13T12:12:52.431Z","level":"INFO","fields":{"message":"mcp server connected","server":"local-tools","tools":20},"target":"tjuae_mcp","spans":[{"name":"agent_run","session_id":"abc-123","msg_id":"msg-456"}]}
```

主要字段：

| 字段 | 说明 |
|------|------|
| `target` | 来源 crate，例如 `tjuae_agent`、`tjuae_providers`、`tjuae_mcp` |
| `spans[].session_id` | 用于关联同一对话中事件的会话 ID |
| `spans[].msg_id` | 用于关联同一轮次中事件的消息 ID |

### 会话关联

`engine.run()` 期间的所有事件，包括 LLM 流式输出、工具执行和压缩，都会封装在
携带 `session_id` 与 `msg_id` 的 `agent_run` span 中。借助这些字段，可以筛选
指定对话的全部日志：

```bash
# 查找指定会话的全部事件
grep '"session_id":"abc-123"' 2026-05-13.tjuae.log | jq .
```

### 库集成

将 TjuaeCLI 作为库使用时，例如嵌入后端服务器，`create_file_layer()` API
会提供可组合的 tracing layer：

```rust
use tjuae_config::logging::{ResolvedLogging, create_file_layer};

let resolved = ResolvedLogging {
    enabled: true,
    level: "tjuae_agent=debug,tjuae_providers=debug".to_string(),
    dir: log_dir.to_path_buf(),
};
let (layer, guard) = create_file_layer(&resolved)?;

// 与现有 subscriber 组合
tracing_subscriber::registry()
    .with(your_app_layer)
    .with(layer)  // TjuaeCLI 日志 → 独立的 tjuae-cli.log 文件
    .init();
```

宿主应用负责创建全局 subscriber；TjuaeCLI 的库 crate 只发出 tracing 事件，
不会自行初始化 subscriber。

---

## AGENTS.md 分层加载

AGENTS.md 文件提供项目专属指令，并自动注入系统提示词。系统会按层级发现这些文件，
再从最外层到最靠近工作目录的顺序合并：

1. **全局**：`<config_dir>/tjuae/AGENTS.md`，适用于所有项目的用户级指令。
2. **项目层级**：从当前工作目录向上遍历到 Git 根目录（或主目录），收集沿途发现的
   每个 `AGENTS.md`。

越靠近工作目录的文件在提示词中出现得越晚，因此会通过 LLM 的近因偏好获得更高
优先级。每个文件都会标注其绝对路径，方便追踪来源。

### `@include` 指令

AGENTS.md 文件可以使用 `@` 语法包含其他文件：

- `@FILENAME` 或 `@./relative/path`：相对于 AGENTS.md 所在目录。
- `@~/path`：相对于主目录。
- `@/absolute/path`：绝对路径。

围栏代码块中的路径会被忽略。包含操作可递归执行，最大深度为 5，并会检测循环
引用。不存在的文件和非文本文件会被静默跳过。

### 示例

给定以下目录结构：

```text
my-workspace/
├── .git/
├── AGENTS.md          ← 工作区规则
└── packages/
    └── server/
        └── AGENTS.md  ← 服务器专属规则
```

在 `packages/server/` 中运行 TjuaeCLI 时，系统提示词会同时包含两个文件：
先包含工作区规则，再包含服务器专属规则。

---

## 记忆系统

基于文件的持久记忆使智能体能够跨会话保留项目专属知识。对话开始时，记忆会自动
加载到系统提示词中。

### 记忆类型

| 类型 | 用途 |
|------|------|
| `user` | 用户的角色、目标、偏好和知识 |
| `feedback` | 对工作方式的纠正和确认 |
| `project` | 无法从代码或 Git 推导出的持续工作上下文 |
| `reference` | 指向外部系统和资源的引用 |

### 存储位置

记忆文件位于全局配置下按项目划分的目录中：

```text
<config_dir>/tjuae/projects/<sanitized-project-path>/memory/
├── MEMORY.md              # 索引（自动载入提示词，最多 200 行）
├── user_role.md
├── feedback_testing.md
└── project_auth_rewrite.md
```

每个记忆文件都使用 YAML front matter：

```markdown
---
name: 认证重写
description: 由合规要求推动的认证中间件重写
type: project
---

认证中间件重写由法律与合规要求推动。
```

### 配置

记忆默认启用，无需任何配置。记忆目录会根据当前工作目录自动解析。

可通过环境变量覆盖基础目录：

```bash
export TJUAE_MEMORY_DIR=/custom/path
```

### 工作原理

1. 智能体启动，根据项目路径解析记忆目录。
2. 将 `MEMORY.md` 索引载入系统提示词，最多 200 行或 25 KB。
3. 智能体使用标准 Read/Write 工具读写记忆文件。
4. 添加或删除记忆时，智能体会维护 `MEMORY.md` 索引。

---

## 计划模式

计划模式是一种只读探索模式。智能体会先理解代码库并制定实施计划，再进行任何
修改。

### 工作原理

1. 智能体调用 `EnterPlanMode`，工具访问被限制为只读工具（Read、Grep、Glob）。
2. 智能体探索代码、设计方案，并在响应中编写结构化计划。
3. 智能体调用 `ExitPlanMode`，恢复完整工具访问；计划可选择保存到磁盘。

### 配置

```toml
[plan]
enabled = true                    # 注册计划模式工具（默认：true）
plan_directory = ".tjuae/plans"  # 保存计划文件的位置
```

### 工作流阶段

进入计划模式后，智能体遵循四个阶段：

1. **理解**：使用只读工具探索代码库。
2. **设计**：确定要修改的文件和可复用的代码。
3. **编写计划**：形成清晰、可执行的实施计划。
4. **提交**：调用 `ExitPlanMode` 恢复完整工具访问。

---

## 上下文压缩

三级自动压缩策略可防止长对话超出上下文窗口。

### 层级

| 层级 | 触发条件 | 方法 | 调用 LLM |
|------|----------|------|----------|
| **Microcompact** | 工具结果数量超过阈值，或时间间隔过长 | 清除旧工具结果内容，保留最近 N 条 | 否 |
| **Autocompact** | 输入 token 接近上下文上限 | 由 LLM 总结对话 | 是 |
| **Emergency** | 输入 token 接近绝对上限 | 阻止后续 API 调用，要求用户重新开始 | 否 |

### 工作原理

- **Microcompact** 自动运行：将旧的 Read、ExecCommand、Grep、Glob、Write 和
  Edit 结果替换为 `[工具结果已清除]`，保留最近 5 条结果。可压缩结果数量超过 10，
  或距离上一条助手消息超过 1 小时时触发。

- **Autocompact** 在输入 token 达到阈值时触发。默认阈值为
  `context_window - output_reserve - autocompact_buffer`
  （200,000 - 20,000 - 13,000 = 167,000 token）。也可以设置
  `autocompact_threshold_pct`，按上下文窗口百分比触发，例如 `50` 表示在 200k
  上下文窗口达到 50%，即 100k token 时触发。智能体会调用 LLM 生成对话摘要，
  再用压缩边界标记替换历史记录。连续失败 3 次后，熔断器会停止重试。

- **Emergency** 是最后一道保护，阈值为
  `context_window - emergency_buffer`，默认即 197,000 token。无论配置如何，
  该机制始终启用。它会阻止 API 调用，并提示用户压缩上下文或开始新对话。

### 配置

```toml
[compact]
enabled = true              # 启用压缩系统（默认：true）
context_window = 200000     # 上下文窗口 token 数
output_reserve = 20000      # 为输出生成预留的 token 数
autocompact_buffer = 13000  # 触发自动压缩前的缓冲区
emergency_buffer = 3000     # 触发紧急阻断前的缓冲区
max_failures = 3            # 熔断阈值
micro_keep_recent = 5       # 保留最近 N 条工具结果
# autocompact_threshold_pct = 50  # 覆盖默认值：在上下文窗口达到 N% 时触发
```

---

## 文件状态缓存

LRU 缓存会跟踪智能体最近访问的文件，实现读取去重，并在写入后自动更新缓存。

- **读取去重**：智能体再次读取已经查看且未发生变化的文件时，直接由缓存提供内容，
  不再从磁盘读取。
- **Write/Edit 自动更新**：执行 Write 或 Edit 后，缓存会立即更新为最新内容。
- **双重淘汰**：条目数量或总字节数达到上限时，都会淘汰旧条目。

### 配置

```toml
[file_cache]
enabled = true                # 启用文件状态缓存（默认：true）
max_entries = 100             # 最大缓存文件数
max_size_bytes = 26214400     # 缓存总大小上限（25 MB）
```

---

## 输出压缩

工具输出经过后处理以减少 token 用量。压缩分为从轻到重的三个级别：

| 级别 | 转换 |
|------|------|
| `off` | 不进行转换 |
| `safe`（默认） | 删除 ANSI 转义码、合并连续空行、折叠以回车刷新的进度条 |
| `full` | 包含 `safe` 的全部操作，并折叠重复行、压缩 JSON 缩进 |

### TOON 编码

在 `full` 压缩下启用 TOON（Token-Oriented Object Notation）时，结构一致的
JSON 数组会编码成紧凑表格：

```text
[2]{id,name,role}:
  1,Alice,admin
  2,Bob,user
```

它等价于：

```json
[{"id":1,"name":"Alice","role":"admin"},{"id":2,"name":"Bob","role":"user"}]
```

系统提示词会注入 TOON 说明，使 LLM 能够理解该格式。

### 配置

```toml
[compact]
compaction = "safe"   # off | safe | full（默认：safe）
toon = false          # 启用 TOON 编码（默认：false）
```

### 运行时控制

在 `--json-stream` 模式中，可以通过 `set_config` 在运行时修改压缩级别：

```json
{"type": "set_config", "compaction": "full"}
```
