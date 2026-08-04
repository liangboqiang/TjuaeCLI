# MCP（Model Context Protocol）集成

## 概览

MCP 允许智能体连接外部工具服务器，将 8 个内置工具扩展到整个 MCP 服务器生态。

## 配置 MCP 服务器

在配置文件中声明 MCP 服务器：

```toml
# Stdio 传输：启动本地子进程
[mcp.servers.filesystem]
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-filesystem", "/Users/me/project"]

[mcp.servers.github]
transport = "stdio"
command = "npx"
args = ["-y", "@modelcontextprotocol/server-github"]
env = { GITHUB_TOKEN = "ghp_xxx" }
startup_timeout_ms = 30000

# SSE 传输：连接远程 SSE 服务器
[mcp.servers.database]
transport = "sse"
url = "http://localhost:3001/sse"

# Streamable HTTP 传输：通过 HTTP POST 通信
[mcp.servers.remote-tools]
transport = "streamable-http"
url = "https://tools.example.com/mcp"
headers = { Authorization = "Bearer xxx" }
```

## 传输类型

| 传输 | 说明 | 适用场景 |
|------|------|----------|
| `stdio` | 启动本地子进程，通过 stdin/stdout 通信 | 本地 MCP 服务器（npx、uvx） |
| `sse` | 使用 GET 建立 SSE 事件流，使用 POST 发送请求 | 远程 MCP 服务器 |
| `streamable-http` | 使用 HTTP POST，并支持 SSE 流式响应 | 远程 MCP 服务器 |

## 启动超时

启动时会并发连接全部已配置的 MCP 服务器。每台服务器的启动超时涵盖传输连接、
`initialize` 和 `tools/list`，默认值为 `30000` 毫秒。

```toml
[mcp.servers.slow-tools]
transport = "stdio"
command = "npx"
args = ["-y", "slow-mcp-server"]
startup_timeout_ms = 60000
```

若服务器首次设置、下载软件包、远程认证或网络握手需要更长时间，可增大
`startup_timeout_ms`。

## 延迟加载

MCP 工具可以注册为“延迟工具”：启动时不将其完整 schema 加载到系统提示词，从而
减少初始 token 用量。需要时，LLM 会通过 `ToolSearch` 发现延迟工具。

```toml
[mcp.servers.large-toolset]
transport = "stdio"
command = "npx"
args = ["-y", "my-mcp-server"]
deferred = true    # 启动时不加载工具 schema
```

| `deferred` | 行为 |
|------------|------|
| `false`（配置服务器的默认值） | 启动时将工具 schema 加入系统提示词 |
| `true` | 注册工具，但仅在需要时通过 ToolSearch 加载 schema |

对于工具数量较多的 MCP 服务器，建议设置 `deferred = true`，以缩小初始系统提示词。

## 工具命名

- 不存在冲突时，直接使用 MCP 工具名称。
- 与内置工具或其他 MCP 工具重名时，自动添加前缀：`mcp__{server}__{tool}`。

## 启动流程

1. 连接全部已配置的 MCP 服务器。
2. 分别执行 MCP 协议握手（`initialize`）。
3. 发现可用工具（`tools/list`）。
4. 将工具注册到工具注册表，智能体会像使用内置工具一样使用它们。
5. 退出时优雅关闭全部连接。
