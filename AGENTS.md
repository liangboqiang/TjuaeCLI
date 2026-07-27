# AGENTS.md

本文件规定参与 TjuaeCLI 开发的 AI 助手与贡献者必须遵循的规则和约定。

## 项目概览

TjuaeCLI 是一个使用 Rust 编写的**多模型提供商 AI 智能体命令行程序**。它可连接
LLM 提供商（Anthropic、OpenAI、AWS Bedrock、Google Vertex AI），编排内置工具
（Read、Write、Edit、ExecCommand、Grep、Glob、Spawn），并支持 MCP 服务器、技能、
钩子和长期记忆。项目还提供 JSON 流协议，供宿主应用集成，例如基于 Electron 的
TjuaeUI。

技术栈：Rust 2024 edition、stable 工具链，以及位于 `crates/` 下的 Cargo 工作区。

## Crate 结构

依赖只能**向下流动**，不得引入循环依赖或向上依赖。

| 层级 | Crate | 职责 |
|------|-------|------|
| 底层 | `tjuae-types` | 提供商无关的共享数据类型（LLM、消息、工具），不依赖其他内部 crate |
| 底层 | `tjuae-compact` | 上下文压缩算法（折叠、清理、分词） |
| 中层 | `tjuae-config` | 配置、ProviderCompat、认证、钩子、日志（`create_file_layer`）及**跨平台 shell 辅助函数** |
| 中层 | `tjuae-protocol` | 面向宿主集成的 JSON 流协议（事件、命令、审批管理器） |
| 中层 | `tjuae-providers` | LLM 提供商实现（Anthropic、OpenAI、Bedrock、Vertex） |
| 中层 | `tjuae-tools` | 内置智能体工具（Read、Write、Edit、ExecCommand、Grep、Glob、Spawn） |
| 中层 | `tjuae-mcp` | MCP（Model Context Protocol）客户端 |
| 中层 | `tjuae-skills` | 技能系统（提示词片段、钩子、权限、shell 展开） |
| 中层 | `tjuae-memory` | 跨会话长期记忆（用户偏好、反馈、项目上下文） |
| 顶层 | `tjuae-agent` | 智能体引擎、会话管理和编排 |
| 顶层 | `tjuae-cli` | 命令行二进制入口 |

新增功能应放入语义上适用的**最低层 crate**。不得仅为一个共享函数创建新 crate。
依赖发生变化后，运行 `cargo metadata` 确认依赖关系仍符合上述结构。

## 构建与测试

```bash
cargo build            # 构建
cargo test             # 运行全部测试
cargo clippy           # 静态检查
cargo fmt --all        # 格式化（CI 强制执行）
```

**推送代码必须使用 `just push`，不得直接执行 `git push`。**
该命令会在推送前依次运行 fmt、clippy 和 test，以减少 CI 失败。它接受与
`git push` 相同的参数，例如 `just push -u origin branch`。

## 代码规范

实现完成后、暂存或提交前，必须依照本节自行检查。不要仅依赖测试或代码审查发现
风格偏差。

### 必做检查

- `cargo clippy` 必须无警告通过。
- `cargo fmt` 必须不产生差异。
- 代码注释和提交消息使用英文。
- 错误处理：
  - 公共 API 错误类型使用 `thiserror`，保证结构化且可匹配。
  - 内部或应用层错误传播使用 `anyhow`。
  - 不得静默吞掉错误；生产代码不得使用 `unwrap()`，除非不变量已经得到证明并
    通过注释说明。

### 可见性与导出

- 完成任务后，必须显式审查所有新增或修改的函数、类型、模块、常量、配置项、
  事件、协议项和 API 的可见性，确认它们确实需要对外暴露。
- 未跨模块、crate、进程或包使用的项目应保持私有或仅限文件/模块作用域。不得为
  方便而暴露。
- 必须暴露时，选择最窄的适用范围。Rust 中优先使用 `pub(crate)`、
  `pub(super)` 或 `pub(in ...)` 等受限可见性，而不是直接使用 `pub`。
- 公共接口必须具有明确调用方、稳定性预期和边界语义。若导出仅用于测试、临时
  接线或绕过模块边界，应重新设计或提供更窄的入口。

### Rust

- 不得在 `mod.rs` 或 `lib.rs` 中编写业务逻辑；这些文件仅用于声明模块可见性和
  重新导出。
- 字段较多的结构体应先按职责分组。相关的运行时状态、配置和依赖应放在一起，
  不得按加入时间或临时用途排序。
- 当前 `mod` 之外的类型、函数、trait、常量或值路径应优先通过 `use` 导入，再以
  本地名称引用。若使用路径包含两个或更多 `::` 段，例如
  `session::SessionManager::new` 或 `pre_message::PreMessageOutcome::Stop`，
  应导入相应项目，使调用处最多保留一个 `::`。
- 导入规则适用于类型位置、函数调用、值路径和 turbofish 语法。常见例外可以继续
  使用限定路径：`tracing::info!`、`tokio::select!`、`anyhow::bail!` 等宏；
  `#[tokio::main]` 等属性宏；为避免与标准名称冲突而使用的 `anyhow::Result`；
  以及 `env::current_dir`、`io::stdin` 等标准模块惯用写法。
- Rust 测试子模块必须拆分到同目录的 `*_test.rs` 文件。源文件只保留测试挂载，
  例如 `#[cfg(test)]` 和 `#[path = "..._test.rs"] mod ..._test;`。

### 智能体集成

- TjuaeCLI 是内置 SDK/库集成路径。修改 Tjuae 宿主集成代码时，不得将它建模为
  外部智能体子进程。
- 集成 Claude Code、Codex 或其他外部智能体前，必须先审查 ACP 协议、子进程生命
  周期、日志和安全边界。

## 日志

修改关键路径时，必须明确评估是否需要日志，以支持开发诊断和生产故障排查。使用
合适级别添加结构化日志：

- `debug`：高频、详细的内部流程，帮助开发阶段验证行为和诊断问题。
- `info`：低频、对生产环境有价值的生命周期边界。
- `warn`：格式异常或非预期但已安全处理的数据。
- `error`：契约违规或操作失败。

生产环境可见日志不得包含提示词、工具输入/输出、文件内容、命令正文、令牌、密钥
或原始提供商请求/响应等敏感载荷。若本地调试确实需要这些载荷，必须置于显式的
仅开发环境开关之后，且默认不得启用。

## 文件组织

- 每个模块（`.rs` 文件）遵循**单一职责原则**，每个文件只有一个明确用途。
- 文件应控制在 1000 行以内；接近上限时拆分子模块。
- 按领域职责组织文件，不按类型组织。

## 架构原则

### 不得硬编码提供商差异

**这是本代码库最重要的规则。**

提供商差异必须通过 **`ProviderCompat` 配置层**处理，不得写死条件判断。

```rust
// WRONG: hardcoded provider detection
if self.base_url.contains("api.openai.com") {
    body["max_completion_tokens"] = json!(max_tokens);
}

// CORRECT: read from compat config
let field = self.compat.max_tokens_field.as_deref().unwrap_or("max_tokens");
body[field] = json!(request.max_tokens);
```

需要新增兼容行为时：

1. 向 `ProviderCompat` 添加一个 `Option<T>` 字段。
2. 在对应的预设函数中设置默认值，例如 `openai_defaults()`。
3. 在提供商代码中通过 `self.compat.field_name` 使用它。

所有提供商都实现 `LlmProvider` trait。引擎只能看到提供商无关的类型
（`LlmRequest`、`LlmEvent`、`Message`、`ContentBlock`）。格式转换应在各提供商的
`build_messages()` 或 `build_request_body()` 内完成。

> **深入阅读：** 提供商配置、认证、别名和配置档继承见
> [docs/providers.md](docs/providers.md)。

### 集中处理平台差异

所有平台特定行为（路径、权限、shell 命令、换行符等）都必须封装在一个集中的
函数中。所有调用方统一使用该函数，不得在多个 crate 或模块中散布原始平台检测。
具体规则见[跨平台](#跨平台)。

### 不得跨 Crate 复制代码

若多个 crate 需要相同能力，应依据依赖图将其提取到适用的现有 crate，不得复制
粘贴或重复实现。目标 crate 应由功能语义和最小化依赖变化共同决定。

## 跨平台

CI 会在 macOS、Linux 和 **Windows** 上运行。本地开发只能测试当前平台对应的
`#[cfg(...)]` 分支，其他平台分支只能由 CI 验证。

### 路径

- 生产代码不得硬编码平台路径，例如 `/tmp/...` 或 `C:\...`。应使用
  `Path::join()`、`dirs::config_dir()`、`tempfile::tempdir()` 等 API。
- 测试中，若只进行纯字符串操作（拼接、显示）或不存在路径的错误处理，可以硬编
  码 Unix 路径，例如 `Path::new("/foo/...")`。只有路径会传给
  `is_absolute()`、`validate_memory_path()` 等平台敏感检查时，才添加
  `#[cfg(unix)]` 或 `#[cfg(windows)]` 变体。
- 检查路径深度时使用 `std::path::Component::Normal`，不得使用字节长度，因为
  各平台的前缀和根组件不同。

### Shell 执行

- 所有 shell 调用必须通过 `tjuae_config::shell` 模块中的 `shell_command()` 或
  `shell_command_builder()`。
- 不得直接调用 `Command::new("sh")`、`Command::new("bash")` 或
  `Command::new("cmd")`，这些命令具有平台差异。
- 在不同平台名称不同的外部 CLI 工具，例如 `grep` 和 `findstr`，必须使用
  `cfg!(windows)` 分支或等效的平台感知选择。

## 测试组织

| 位置 | 内容 |
|------|------|
| 由各 `.rs` 文件通过 `#[cfg(test)]` 与 `#[path = "..._test.rs"] mod ..._test;` 挂载的同目录 `*_test.rs` | 该模块内部逻辑的单元测试 |
| `crates/<crate>/tests/` | 该 crate 的集成测试 |

单元测试面向内部逻辑和代码路径。集成测试面向功能需求和公共 API，应依据规范编写，
不得从实现细节反推测试。

每个测试都必须验证有意义的行为或边界情况。不得只断言成功路径，而不覆盖边界、
错误条件或非显然逻辑。

## 文档

`docs/` 下的主要参考文档如下，请勿在本文件中重复其内容：

| 文档 | 内容 |
|------|------|
| [getting-started.md](docs/getting-started.md) | 安装、CLI 用法、配置格式和级联优先级 |
| [providers.md](docs/providers.md) | 提供商配置、认证、ProviderCompat、自定义别名和配置档 |
| [tools.md](docs/tools.md) | 内置工具参考和执行流程 |
| [skills.md](docs/skills.md) | 编写技能、front matter、shell 展开和条件激活 |
| [mcp.md](docs/mcp.md) | MCP 服务器集成、传输类型和延迟加载 |
| [advanced.md](docs/advanced.md) | 子智能体、钩子、日志、记忆、规划模式和上下文压缩 |
| [json-stream-protocol.md](docs/json-stream-protocol.md) | 面向宿主集成（如 TjuaeUI）的 JSON Lines 协议规范 |
| [troubleshooting.md](docs/troubleshooting.md) | 常见错误和解决方案 |
