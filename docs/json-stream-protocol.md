# TjuaeCLI JSON 流协议规范

> 本协议定义 TjuaeCLI（Rust CLI）与宿主客户端（例如 TjuaeUI Electron 应用）
> 之间通过 stdin/stdout JSON Lines 进行的通信。

## 概览

```text
┌──────────────┐   stdin（JSON Lines）    ┌──────────────────┐
│              │ ◄─────────────────────── │                  │
│   TjuaeCLI   │                          │     宿主客户端     │
│  （Rust CLI） │ ──────────────────────►  │ （TjuaeUI 等）     │
│              │   stdout（JSON Lines）   │                  │
└──────────────┘                          └──────────────────┘
     stderr → 诊断日志（不属于协议）
```

- **传输方式**：stdin/stdout，每行一个 JSON 对象（JSON Lines / NDJSON）。
- **编码**：UTF-8。
- **启用方式**：`tjuae-cli --json-stream [other flags]`。
- **生命周期**：每个对话使用一个进程；进程会保持运行以支持多轮交互。

## 1. 智能体 → 客户端事件（stdout）

每一行都是带有 `type` 字段的 JSON 对象。

### 1.1 `ready`

初始化完成后发出一次。客户端在发送消息前**必须**等待该事件。

```json
{
  "type": "ready",
  "version": "0.2.0",
  "session_id": "a1b2c3",
  "capabilities": {
    "tool_approval": true,
    "image_input": "supported",
    "thinking": true,
    "effort": false,
    "effort_levels": [],
    "modes": ["default", "auto_edit", "yolo"],
    "current_mode": "default",
    "mcp": true
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `version` | string | 协议版本，采用 semver |
| `session_id` | string? | 会话 ID；配置中禁用会话时省略 |
| `capabilities.tool_approval` | bool | 智能体是否支持暂停并等待工具审批 |
| `capabilities.image_input` | string | 已解析的图像输入能力：`supported`、`unsupported` 或 `unknown` |
| `capabilities.thinking` | bool | 当前提供商是否支持扩展思考 |
| `capabilities.effort` | bool | 当前提供商是否支持 `reasoning_effort` |
| `capabilities.effort_levels` | string[] | 有效的 effort 值，例如 `["low", "medium", "high"]`；不支持 effort 时为空 |
| `capabilities.modes` | string[] | `set_mode` 命令可用的审批模式 |
| `capabilities.current_mode` | string | 当前生效的审批模式 |
| `capabilities.mcp` | bool | MCP 工具是否可用 |

### 1.2 `stream_start`

新的响应轮次已经开始。

```json
{
  "type": "stream_start",
  "msg_id": "abc-123"
}
```

### 1.3 `text_delta`

流式增量文本输出。

```json
{
  "type": "text_delta",
  "text": "你好，",
  "msg_id": "abc-123"
}
```

### 1.4 `thinking`

模型的内部推理内容，仅在启用扩展思考时出现。

```json
{
  "type": "thinking",
  "text": "我先分析代码结构……",
  "msg_id": "abc-123"
}
```

### 1.5 `tool_request`

智能体希望调用工具，并需要客户端批准。收到 `tool_approve` 或 `tool_deny` 前，
智能体会**暂停**执行。

```json
{
  "type": "tool_request",
  "msg_id": "abc-123",
  "call_id": "tool-call-001",
  "tool": {
    "name": "Write",
    "category": "edit",
    "args": {
      "file_path": "/src/main.rs",
      "content": "fn main() { ... }"
    },
    "description": "写入 /src/main.rs"
  }
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `call_id` | string | 本次工具调用的唯一 ID |
| `tool.name` | string | 工具名称：`Read`、`Write`、`Edit`、`ExecCommand`、`Glob`、`Grep`、`Spawn` 或 MCP 工具名称 |
| `tool.category` | string | `"info"`（只读）、`"edit"`（修改文件）、`"exec"`（shell）或 `"mcp"`（MCP 工具） |
| `tool.args` | object | 工具参数 |
| `tool.description` | string | 便于阅读的单行说明 |

**内置工具的类别映射：**

| 工具 | 类别 | 原因 |
|------|------|------|
| `Read` | `info` | 只读文件访问 |
| `Glob` | `info` | 只读文件搜索 |
| `Grep` | `info` | 只读内容搜索 |
| `Write` | `edit` | 创建或覆盖文件 |
| `Edit` | `edit` | 修改文件内容 |
| `ExecCommand` | `exec` | 执行 shell 命令 |
| `Spawn` | `exec` | 启动子智能体 |
| MCP 工具 | `mcp` | 外部 MCP 服务器工具 |

> **注意：** 当 `auto_approve = true`（yolo 模式），或工具位于 `allow_list`
> 中时，智能体会立即执行并直接发出 `tool_running`，跳过 `tool_request`。

### 1.6 `tool_running`

工具在获得批准或自动批准后开始执行。

```json
{
  "type": "tool_running",
  "msg_id": "abc-123",
  "call_id": "tool-call-001",
  "tool_name": "Write"
}
```

### 1.7 `tool_result`

工具执行完成。

```json
{
  "type": "tool_result",
  "msg_id": "abc-123",
  "call_id": "tool-call-001",
  "tool_name": "Write",
  "status": "success",
  "output": "文件写入成功",
  "output_type": "text"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `status` | string | `"success"` 或 `"error"` |
| `output` | string | 工具输出；超过限制时会被截断 |
| `output_type` | string | `"text"`（默认）、`"diff"`（Edit 工具）或 `"image"`（base64） |

**Edit 工具的特殊输出**（`output_type: "diff"`）：

```json
{
  "type": "tool_result",
  "msg_id": "abc-123",
  "call_id": "tool-call-002",
  "tool_name": "Edit",
  "status": "success",
  "output": "--- a/src/main.rs\n+++ b/src/main.rs\n@@ -1,3 +1,3 @@\n-旧内容\n+新内容",
  "output_type": "diff",
  "metadata": {
    "file_path": "/src/main.rs"
  }
}
```

### 1.8 `tool_cancelled`

工具被客户端拒绝或取消。

```json
{
  "type": "tool_cancelled",
  "msg_id": "abc-123",
  "call_id": "tool-call-001",
  "reason": "用户已拒绝"
}
```

### 1.9 `stream_end`

当前响应轮次已经结束。

```json
{
  "type": "stream_end",
  "msg_id": "abc-123",
  "usage": {
    "input_tokens": 1500,
    "output_tokens": 320,
    "cache_read_tokens": 800,
    "cache_write_tokens": 200
  }
}
```

### 1.10 `error`

发生错误。智能体是否继续运行取决于错误严重程度。

```json
{
  "type": "error",
  "msg_id": "abc-123",
  "error": {
    "code": "provider_error",
    "message": "已超过速率限制",
    "retryable": true
  }
}
```

| 错误代码 | 说明 |
|----------|------|
| `provider_error` | LLM API 错误，例如限流或认证失败 |
| `tool_error` | 内置工具执行错误 |
| `config_error` | 配置或初始化错误 |
| `protocol_error` | 客户端命令无效 |
| `internal_error` | 意外的内部错误 |

### 1.11 `info`

非关键的信息消息，仅供显示。

```json
{
  "type": "info",
  "msg_id": "abc-123",
  "message": "流已中断，正在重试……（1/2）"
}
```

### 1.12 `config_changed`

处理 `set_config` 命令后发出。事件包含更新后的能力快照，反映当前提供商和模型
配置。

```json
{
  "type": "config_changed",
  "capabilities": {
    "tool_approval": true,
    "image_input": "unsupported",
    "thinking": false,
    "effort": true,
    "effort_levels": ["low", "medium", "high"],
    "modes": ["default", "auto_edit", "yolo"],
    "current_mode": "default",
    "mcp": true
  }
}
```

客户端应根据新能力更新 UI 控件，例如启用或禁用思考开关、填充 effort 下拉列表。

### 1.13 `mcp_ready`

动态注入的 MCP 服务器连接成功并完成工具注册后发出。

```json
{
  "type": "mcp_ready",
  "name": "my-tools",
  "tools": ["tool_a", "tool_b"]
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | string | 服务器名称，与 `add_mcp_server` 中提供的值相同 |
| `tools` | string[] | 从该服务器注册的工具名称列表 |

### 1.14 `pong`

对客户端 `ping` 命令的响应，用于心跳和存活检测。

```json
{
  "type": "pong"
}
```

没有其他字段。无论消息轮次是否正在进行，智能体收到 `ping` 后都会立即发出
`pong`。

## 2. 客户端 → 智能体命令（stdin）

每一行都是带有 `type` 字段的 JSON 对象。

### 2.1 `message`

发送用户消息。智能体会返回一系列流式事件。

```json
{
  "type": "message",
  "msg_id": "abc-123",
  "content": "读取 src/main.rs 并解释代码",
  "files": ["/path/to/attached/file.png"]
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `msg_id` | string | 是 | 客户端生成的唯一消息 ID |
| `content` | string | 是 | 用户消息文本 |
| `files` | string[] | 否 | 附件文件路径，例如图像或文档 |

### 2.2 `stop`

中止当前响应流。

```json
{
  "type": "stop"
}
```

智能体**必须**：

1. 取消正在进行的 LLM 请求。
2. 尽可能取消正在运行的工具。
3. 为当前 `msg_id` 发出 `stream_end`。

### 2.3 `tool_approve`

批准一项待处理的工具执行。

```json
{
  "type": "tool_approve",
  "call_id": "tool-call-001",
  "scope": "once"
}
```

| 字段 | 类型 | 说明 |
|------|------|------|
| `call_id` | string | 必须匹配待处理的 `tool_request` |
| `scope` | string | `"once"` 表示仅批准本次调用；`"always"` 表示本次会话中自动批准该工具与类别 |

当 `scope = "always"` 时，智能体会将工具类别加入会话白名单，使该类别后续调用
跳过审批。

### 2.4 `tool_deny`

拒绝一项待处理的工具执行。

```json
{
  "type": "tool_deny",
  "call_id": "tool-call-001",
  "reason": "不允许写入该文件"
}
```

智能体**必须**：

1. 发出 `tool_cancelled` 事件。
2. 将拒绝原因作为工具结果反馈给 LLM。
3. 继续对话，由 LLM 决定下一步操作。

### 2.5 `init_history`

注入之前的对话上下文，用于恢复对话。

```json
{
  "type": "init_history",
  "text": "之前的对话摘要：\n用户询问了 X……\n助手回复了 Y……"
}
```

该命令必须在第一条 `message` 命令**之前**发送。智能体会将文本加入对话上下文。

### 2.6 `set_mode`

修改本次会话的智能体审批模式。

```json
{
  "type": "set_mode",
  "mode": "yolo"
}
```

| 模式 | 行为 |
|------|------|
| `"default"` | 除白名单外，所有工具都需要批准 |
| `"auto_edit"` | 自动批准 `info` 和 `edit`；`exec` 与 `mcp` 仍需批准 |
| `"yolo"` | 自动批准所有工具 |

### 2.7 `set_config`

在运行时更新模型及宿主解析的能力或请求配置。

```json
{
  "type": "set_config",
  "model": "claude-opus-4",
  "image_input": "supported",
  "thinking": "enabled",
  "thinking_budget": 16000,
  "effort": "high",
  "compaction": "safe"
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `model` | string | 否 | 切换到另一个模型 |
| `image_input` | string | 否 | 宿主为所选模型解析的能力：`supported`、`unsupported` 或 `unknown` |
| `thinking` | string | 否 | `"enabled"` 或 `"disabled"` |
| `thinking_budget` | number | 否 | 启用思考时的 token 预算，默认为 10000；会发送给 Anthropic 请求，OpenAI-compatible 请求会忽略 |
| `effort` | string | 否 | 推理强度，例如 `"low"`、`"medium"`、`"high"` |
| `compaction` | string | 否 | 输出压缩级别：`"off"`、`"safe"` 或 `"full"` |

所有字段都是可选的，只有提供的字段会被更新。

修改 `model` 时，客户端**应当**在同一命令中发送对应的 `image_input`。如果提供
`model` 而不提供 `image_input`，智能体会将图像支持重置为 `unknown`，不会沿用
上一个模型的能力。

> **校验：** 智能体会根据当前提供商能力校验 `effort`。显式的 `thinking`
> 更新会作为请求意图应用；若提供商拒绝对应线协议字段，模型请求期间会返回该
> 提供商错误。处理完成后，智能体一定会发出带有最新能力的 `config_changed`。

### 2.8 `add_mcp_server`

在对话开始前动态注入 MCP 服务器。该命令只在**消息前阶段**接受，即收到 `ready`
之后、发送第一条 `message` 之前。第一条 `message` 之后发送的任何
`add_mcp_server` 都会被拒绝并返回错误。

```json
{
  "type": "add_mcp_server",
  "name": "my-tools",
  "transport": "stdio",
  "command": "node",
  "args": ["bridge.js", "--port", "9000"],
  "env": {"TOKEN": "abc123"}
}
```

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `name` | string | 是 | 唯一的服务器名称 |
| `transport` | string | 是 | `"stdio"`、`"sse"` 或 `"streamable-http"` |
| `command` | string | 仅 stdio | 要启动的可执行文件 |
| `args` | string[] | 否 | 命令参数 |
| `env` | object | 否 | 子进程环境变量 |
| `url` | string | 仅 sse/http | 服务器 URL |
| `headers` | object | 否 | HTTP 请求头，用于 sse/http |

动态 MCP 服务器使用默认的 30000 毫秒启动超时。若要自定义启动超时，应在配置中
定义服务器并设置 `startup_timeout_ms`。

**生命周期：**

```text
智能体   → stdout: {"type":"ready",...}
客户端   → stdin:  {"type":"add_mcp_server","name":"tools","transport":"stdio","command":"node","args":["bridge.js"]}
智能体   → stdout: {"type":"mcp_ready","name":"tools","tools":["tool_a","tool_b"]}
客户端   → stdin:  {"type":"message","msg_id":"m1","content":"你好"}
                     ↑ 第一条消息结束注入窗口
```

### 2.9 `ping`

心跳探测。智能体会立即返回 `pong` 事件。

```json
{
  "type": "ping"
}
```

空闲、处理消息或执行工具期间都可以发送。智能体始终返回 `{"type":"pong"}`。

第一条 `message` 之后的任何 `add_mcp_server` 命令都会被拒绝：

```json
{
  "type": "error",
  "error": {
    "code": "protocol_error",
    "message": "AddMcpServer 'name' 已拒绝：只允许在第一条 Message 之前使用",
    "retryable": false
  }
}
```

## 3. 生命周期

### 3.1 启动

```text
客户端启动：
  tjuae-cli --json-stream \
    --provider anthropic \
    --model claude-sonnet-4-20250514 \
    --max-tokens 8192 \
    --max-turns 30

客户端设置的环境变量：
  ANTHROPIC_API_KEY=sk-...
  # 或 OPENAI_API_KEY、AWS_REGION 等

智能体初始化 → stdout: {"type":"ready","session_id":"a1b2c3",...}
```

**消息前阶段（可选）：**

收到 `ready` 后、发送第一条 `message` 前，客户端可以通过 `add_mcp_server` 命令
注入 MCP 服务器。智能体连接每台服务器，并在就绪后发出 `mcp_ready`。发送第一条
`message` 时，该阶段结束。

**会话生命周期参数**（互斥）：

| 参数 | 说明 |
|------|------|
| `--session-id <ID>` | 使用指定会话 ID，而不是自动生成；ID 已存在时会报错 |
| `--resume <ID>` | 恢复之前的会话并加载对话历史；使用 `latest` 恢复最近一次会话 |

```bash
# 使用自定义 ID 创建新会话
tjuae-cli --json-stream --session-id my-conv-123 --provider openai --model gpt-4o

# 恢复现有会话
tjuae-cli --json-stream --resume my-conv-123 --provider openai --model gpt-4o
```

### 3.2 消息轮次

```text
客户端 → stdin:  {"type":"message","msg_id":"m1","content":"你好"}
智能体 → stdout: {"type":"stream_start","msg_id":"m1"}
智能体 → stdout: {"type":"text_delta","text":"你好！","msg_id":"m1"}
智能体 → stdout: {"type":"text_delta","text":"有什么可以帮你？","msg_id":"m1"}
智能体 → stdout: {"type":"stream_end","msg_id":"m1","usage":{...}}
```

### 3.3 工具审批流程

```text
客户端 → stdin:  {"type":"message","msg_id":"m2","content":"创建 hello.rs 文件"}
智能体 → stdout: {"type":"stream_start","msg_id":"m2"}
智能体 → stdout: {"type":"text_delta","text":"我来创建该文件。","msg_id":"m2"}
智能体 → stdout: {"type":"tool_request","msg_id":"m2","call_id":"t1","tool":{"name":"Write","category":"edit",...}}
  ← 智能体在此暂停，等待批准 →
客户端 → stdin:  {"type":"tool_approve","call_id":"t1","scope":"once"}
智能体 → stdout: {"type":"tool_running","msg_id":"m2","call_id":"t1","tool_name":"Write"}
智能体 → stdout: {"type":"tool_result","msg_id":"m2","call_id":"t1","status":"success",...}
智能体 → stdout: {"type":"text_delta","text":"文件已创建。","msg_id":"m2"}
智能体 → stdout: {"type":"stream_end","msg_id":"m2","usage":{...}}
```

### 3.4 多工具并行执行

当 LLM 在当前运行的一次轮次中请求多个工具时，智能体会发出多个
`tool_request` 事件。客户端可以分别批准或拒绝它们。

```text
智能体 → stdout: {"type":"tool_request","call_id":"t1","tool":{"name":"Read","category":"info",...}}
智能体 → stdout: {"type":"tool_request","call_id":"t2","tool":{"name":"Read","category":"info",...}}
客户端 → stdin:  {"type":"tool_approve","call_id":"t1","scope":"once"}
客户端 → stdin:  {"type":"tool_approve","call_id":"t2","scope":"once"}
智能体 → stdout: {"type":"tool_running","call_id":"t1",...}
智能体 → stdout: {"type":"tool_running","call_id":"t2",...}
智能体 → stdout: {"type":"tool_result","call_id":"t1",...}
智能体 → stdout: {"type":"tool_result","call_id":"t2",...}
```

### 3.5 关闭

客户端关闭 stdin（EOF）或发送 SIGTERM。智能体完成清理后退出。

## 4. 错误处理

### 4.1 无效命令

客户端发送格式错误的 JSON 或未知命令类型时：

```json
{
  "type": "error",
  "msg_id": null,
  "error": {
    "code": "protocol_error",
    "message": "未知命令类型：foo",
    "retryable": false
  }
}
```

### 4.2 提供商错误

智能体应发出错误，并在可能的情况下让对话继续：

```json
{
  "type": "error",
  "msg_id": "m3",
  "error": {
    "code": "provider_error",
    "message": "已超过速率限制，请在 30 秒后重试。",
    "retryable": true
  }
}
```

### 4.3 致命错误

对于无法恢复的错误，智能体会发出错误，并以非零状态退出：

```json
{
  "type": "error",
  "msg_id": null,
  "error": {
    "code": "config_error",
    "message": "未设置 ANTHROPIC_API_KEY",
    "retryable": false
  }
}
```

## 5. 通过 CLI 参数配置

以 `--json-stream` 模式启动时，所有配置均通过 CLI 参数和环境变量传入：

```bash
tjuae-cli --json-stream \
  --provider <anthropic|openai|bedrock|vertex> \
  --model <model-id> \
  --max-tokens <N> \
  --max-turns <N> \
  --base-url <URL> \
  --system-prompt <TEXT> \
  --auto-approve \
  --workspace <PATH>
```

其中 `--auto-approve` 表示以 yolo 模式启动，`--workspace` 用于指定文件操作的
工作目录。

**环境变量**（由客户端在启动进程前设置）：

| 提供商 | 变量 |
|--------|------|
| Anthropic | `ANTHROPIC_API_KEY`、`ANTHROPIC_BASE_URL` |
| OpenAI | `OPENAI_API_KEY`、`OPENAI_BASE_URL` |
| Bedrock | `AWS_REGION`、`AWS_ACCESS_KEY_ID`、`AWS_SECRET_ACCESS_KEY`、`AWS_PROFILE` |
| Vertex AI | `GOOGLE_APPLICATION_CREDENTIALS`、`VERTEX_PROJECT_ID`、`VERTEX_REGION` |

## 6. 协议版本

`ready` 事件包含 `version` 字段。客户端应检查版本兼容性。

- **次版本号递增**：增加新的可选事件类型或字段，保持向后兼容。
- **主版本号递增**：对现有事件或命令作出破坏性更改。

当前版本：`0.2.0`
