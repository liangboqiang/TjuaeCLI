# 提供商与认证

## 支持的提供商

| 提供商 | 认证方式 | 说明 |
|--------|----------|------|
| Anthropic | API 密钥 / OAuth | 提示词缓存、流式输出、视觉能力 |
| OpenAI | API 密钥 | 兼容 DeepSeek、Qwen、Ollama 和 vLLM |
| AWS Bedrock | SigV4 | 区域端点、AWS 凭据链 |
| Google Vertex AI | GCP OAuth2 / 服务账户 | 自动检测元数据服务器 |

---

## 自定义提供商别名

如果后端兼容某个内置提供商的协议，可以为它定义自定义别名，而不必将 `provider`
直接写成内置名称。

```toml
[default]
provider = "my-service"

[providers.my-service]
provider = "openai"
model = "custom-model-v1"
api_key = "sk-xxx"
base_url = "https://my-service.example.com/api/openai"
```

规则：

- `provider = "my-service"` 是配置层别名。
- `[providers.my-service].provider` 必须指向底层内置提供商。
- 底层提供商目前只能是 `anthropic`、`openai`、`bedrock` 或 `vertex`。
- 别名条目的 `model`、`api_key`、`base_url` 和 `compat` 会覆盖底层提供商的
  默认配置。

这种方式适用于 DeepSeek 网关、内部 OpenAI-compatible 服务等场景。

---

## 配置档继承

配置档支持通过 `extends` 继承另一个配置档的设置，从而避免重复。

### 配置

```toml
# 基础配置档
[profiles.base-anthropic]
provider = "anthropic"
api_key = "sk-ant-xxx"

# 继承 base-anthropic 并覆盖模型
[profiles.claude-fast]
extends = "base-anthropic"
model = "claude-haiku-4-5-20251001"
max_tokens = 4096

[profiles.claude-deep]
extends = "base-anthropic"
model = "claude-opus-4-20250514"
max_tokens = 16384

# 配置档可以指定要使用的 MCP 服务器
[profiles.dev]
extends = "base-anthropic"
model = "claude-sonnet-4-20250514"
mcp_servers = ["filesystem", "github"]

# 配置档名称由用户定义；这不是内置配置档。
[profiles.my-weak-provider]
extends = "base-anthropic"
max_tool_call_malformed_turns = 2
max_tool_call_failure_turns = 2
```

### 用法

```bash
tjuae-cli --profile claude-fast "快速回答这个问题"
tjuae-cli --profile claude-deep "执行深入的安全审计"
tjuae-cli --profile dev "创建一个 GitHub issue"
```

- 支持多级继承链。
- 自动检测循环继承。
- 子配置档的设置会覆盖父配置档。
- 可为每个配置档设置 `max_tool_call_malformed_turns`。默认值为 `3`；设为 `0`
  会禁用工具调用格式错误轮次熔断器。若已配置总体轮次限制，则改由 `max_turns`
  负责终止运行。
- 可为每个配置档设置 `max_tool_call_failure_turns`。默认值为 `3`；设为 `0`
  会禁用工具调用失败轮次熔断器。若已配置总体轮次限制，则改由 `max_turns`
  负责终止运行。
- 单次运行可通过 `--max-tool-call-malformed-turns <n>` 或
  `--max-tool-call-failure-turns <n>` 覆盖这两个值。

---

## OpenAI Responses API

OpenAI 提供商默认使用 Chat Completions，以便现有的 OpenAI-compatible 服务
继续采用相同的请求和流式格式。只有模型或端点明确要求 Responses 时才启用：

```toml
[providers.openai]
api_key = "sk-xxx"
base_url = "https://api.openai.com/v1"

[providers.openai.compat]
openai_api_mode = "responses"
```

该设置会改变完整的线协议契约：端点改为 `/responses`，请求使用 `input` 条目
和平铺的 function tools，带类型的 SSE 事件则会解码为现有的提供商无关智能体
事件。工具调用和加密推理条目会持久化，并在后续工具轮次中重放。省略该设置时，
`chat_completions` 仍为默认值。

---

## OpenAI-compatible 思考模型

部分 OpenAI-compatible 提供商支持 `thinking` 请求对象，可与 OpenAI 的
`reasoning_effort` 同时使用或取而代之。当提供商或配置档需要向宿主 UI 声明
思考能力时，应设置相应的兼容能力：

```toml
[profiles.deepseek-v4-pro]
provider = "openai"
model = "deepseek-v4-pro"
api_key = "sk-xxx"
base_url = "https://api.deepseek.com/v1"
max_tokens = 16384

[profiles.deepseek-v4-pro.compat]
supports_thinking = true
```

随后可以从宿主协议通过 `set_config` 启用思考，也可以在启动时强制启用：

```bash
tjuae-cli --profile deepseek-v4-pro --thinking enabled
```

不使用配置档而临时启动 OpenAI-compatible 提供商时，等效命令如下：

```bash
tjuae-cli --json-stream \
  --provider openai \
  --model deepseek-v4-pro \
  --base-url https://api.deepseek.com/v1 \
  --max-tokens 16384 \
  --thinking enabled
```

`--thinking-budget` 只有与 `--thinking enabled` 一起使用时才生效，并且只会在
Anthropic 线协议路径中发送。OpenAI-compatible 请求目前只发送
`thinking.type`，因此该提供商路径会忽略配置的预算。

---

## AWS Bedrock

通过 AWS Bedrock 和 SigV4 认证访问 Claude 模型。

### 配置

```toml
[default]
provider = "bedrock"

[bedrock]
region = "us-east-1"
# 方式 1：显式凭据
access_key_id = "AKIA..."
secret_access_key = "..."
# session_token = "..."

# 方式 2：AWS 配置档
# profile = "my-profile"

# 方式 3：环境变量（AWS_ACCESS_KEY_ID、AWS_SECRET_ACCESS_KEY）
# 未配置凭据时自动使用

[profiles.bedrock-claude]
provider = "bedrock"
model = "anthropic.claude-sonnet-4-20250514-v1:0"
```

### 凭据优先级

1. 配置文件中的显式凭据
2. AWS 配置档
3. 环境变量（`AWS_ACCESS_KEY_ID`、`AWS_SECRET_ACCESS_KEY`、
   `AWS_SESSION_TOKEN`）

---

## Google Vertex AI

通过 Google Vertex AI 和 GCP OAuth2 认证访问 Claude 模型。

### 配置

```toml
[default]
provider = "vertex"

[vertex]
project_id = "my-gcp-project"
region = "us-central1"

# 方式 1：服务账户密钥文件
credentials_file = "/path/to/service-account.json"

# 方式 2：Application Default Credentials
# 运行：gcloud auth application-default login

# 方式 3：Metadata Server（在 GCE/GKE/Cloud Run 上自动使用）
# 位于 GCP 环境时自动使用

[profiles.vertex-claude]
provider = "vertex"
model = "claude-sonnet-4@20250514"
```

### 认证方式

| 方式 | 适用场景 |
|------|----------|
| 服务账户密钥 | CI/CD、服务端应用 |
| Application Default Credentials | 本地开发（需要 gcloud CLI） |
| Metadata Server | GCE/GKE/Cloud Run 及其他 GCP 环境 |

---

## OAuth 登录（Claude.ai）

直接使用 Claude.ai Pro、Team 或 Enterprise 订阅，无需 API 密钥。

### 登录

先向提供商注册 OAuth 客户端，再将客户端 ID 添加到全局配置文件。TjuaeCLI
不会内置或猜测客户端 ID：

```toml
[auth]
client_id = "your-registered-client-id"
```

```bash
tjuae-cli auth login
```

1. 命令会显示授权 URL 和代码。
2. 在浏览器中打开 URL 并输入代码。
3. 凭据会保存在全局配置旁边（运行 `tjuae-cli config path` 可查找目录）。
4. 后续运行会自动加载已保存的凭据，并自动刷新。

### 退出登录

```bash
tjuae-cli auth logout
```

### 配置 OAuth 端点

默认端点面向 Claude.ai。仅当已注册客户端使用不同端点时才覆盖它们：

```toml
[auth]
auth_url = "https://claude.ai/oauth"
token_url = "https://claude.ai/oauth/token"
client_id = "your-registered-client-id"
```
