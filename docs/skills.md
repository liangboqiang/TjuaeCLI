# 技能

技能是智能体可按需调用的命名提示词片段。它可以将可复用指令、工作流或工具序列
封装为一个可调用名称。

## 概览

技能是带有 YAML front matter 头部的 Markdown 文件。智能体调用技能时会：

1. 按名称从已加载的技能列表中解析技能。
2. 替换变量（`$ARGUMENTS`、`$0`、`${TJUAE_SKILL_DIR}`）。
3. 展开 shell 命令（`` !`cmd` `` 语法）。
4. 将处理后的文本作为技能输出返回。

## 目录结构

技能按以下优先级顺序加载；名称重复时，最先匹配的技能生效：

| 优先级 | 路径 | 说明 |
|--------|------|------|
| 1 | `.tjuae/skills/` | 项目本地技能，可随仓库提交 |
| 2 | `<CONFIG_DIR>/tjuae/skills/` | 用户全局技能，位置见下文 |

> **各平台的 `<CONFIG_DIR>`：**
>
> - **macOS：** `~/Library/Application Support/`
> - **Linux：** `~/.config/`（或 `$XDG_CONFIG_HOME`）
> - **Windows：** `C:\Users\<USER>\AppData\Roaming\`
>
> 运行 `tjuae-cli skills path` 可查看当前计算机上的实际路径。

每个技能都是命名子目录中的一个 `SKILL.md` 文件：

```text
.tjuae/skills/
├── deploy/
│   └── SKILL.md          # 以 "deploy" 名称调用
├── review-pr/
│   └── SKILL.md          # 以 "review-pr" 名称调用
```

## 编写技能

### 最小技能

```markdown
---
name: greet
description: 输出问候语
---

你好！今天需要我做什么？
```

### 完整 front matter 参考

```yaml
---
# 必填
name: skill-name          # 唯一标识符，用于调用技能
description: 技能列表中显示的单行说明

# 可选——条件激活
paths:
  - "src/**/*.rs"         # 仅当工作路径匹配时激活技能

# 可选——技能运行时应用的上下文覆盖
model: claude-sonnet-4-20250514  # 覆盖当前模型
effort: high              # 推理强度：low | medium | high
allowed-tools:            # 限制技能可使用的工具
  - Read
  - Grep

# 可选——技能激活时注册的钩子
hooks:
  PreToolUse:
    - "echo '即将运行工具'"
  PostToolUse:
    - "echo '工具运行结束'"
  Stop:
    - "echo '会话已结束'"
---

技能正文写在这里。
```

### Front matter 字段

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | string | **必填。** 唯一的技能名称。 |
| `description` | string | **必填。** 在系统提示词的技能列表中显示。 |
| `paths` | string[] | Glob 模式；当前路径至少匹配一项时技能才会激活。 |
| `model` | string | 在技能运行期间覆盖当前模型。 |
| `effort` | string | 覆盖推理强度：`low`、`medium` 或 `high`。 |
| `allowed-tools` | string[] | 技能运行期间只能使用此列表中的工具。 |
| `hooks.PreToolUse` | string[] | 每次工具调用前运行的 shell 命令。 |
| `hooks.PostToolUse` | string[] | 每次工具调用后运行的 shell 命令。 |
| `hooks.Stop` | string[] | 会话结束时运行的 shell 命令。 |

Front matter 使用严格字段校验。`allowedTools`、`permissions` 等未支持字段会导致技能
被拒绝加载，而不会被静默忽略；全局技能授权规则应在 TjuaeCLI 配置中设置。

## 变量替换

技能正文中的以下变量会在运行时替换：

| 变量 | 替换内容 |
|------|----------|
| `$ARGUMENTS` | 调用技能时传入的完整参数字符串 |
| `$0` | 技能自身的名称 |
| `${TJUAE_SKILL_DIR}` | 包含该技能 `SKILL.md` 的目录绝对路径 |

示例：

```markdown
---
name: run-tests
description: 运行指定模块的测试
---

运行以下模块的测试套件：$ARGUMENTS

工作目录：${TJUAE_SKILL_DIR}
```

## Shell 命令展开

包含 `` !`cmd` `` 的行会在 shell 中执行 `cmd`，并将输出内联替换：

```markdown
---
name: git-status
description: 显示当前 Git 状态
---

当前分支：!`git rev-parse --abbrev-ref HEAD`

最近的提交：
!`git log --oneline -5`
```

## 条件激活

带有 `paths:` 字段的技能默认处于**休眠**状态，只有当前工作路径匹配其中一个
Glob 模式时才会**激活**：

```yaml
---
name: rust-review
description: Rust 专用代码审查清单
paths:
  - "**/*.rs"
  - "Cargo.toml"
---

审查 Rust 代码时，请检查：
- 库代码中没有 unwrap()
- 错误类型实现 std::error::Error
- 公共 API 具有文档注释
```

只有 `.rs` 文件或 `Cargo.toml` 位于当前工作范围时，该技能才会出现在系统提示词中。

## MCP 技能

技能也可以从 MCP 服务器加载。MCP 来源的技能与本地技能行为相同，但有一项限制：
为防止不受信任的来源执行任意代码，MCP 技能会禁用 **shell 命令展开
（`` !`cmd` ``）**。

在配置文件中声明 MCP 技能来源；服务器配置参见 [mcp.md](mcp.md)。

## 内置技能

二进制文件会编译进少量技能。内置技能：

- 无论文件系统技能目录如何，都始终可用。
- **不会因提示词预算而截断**；即使为满足 token 上限缩短技能列表，它们仍会保留。
- 不能被同名的用户技能覆盖。

## 提示词预算

所有技能说明的总大小超过提示词预算时，智能体会截断非内置技能列表，内置技能
始终保留。为避免浪费预算，请保持技能说明简洁。

## 故障排查

运行 `tjuae-cli skills path` 可以查看正在扫描的目录，以及它们是否存在：

```text
$ tjuae-cli skills path
用户：~/Library/Application Support/tjuae/skills     （存在）
项目：/path/to/repo/.tjuae/skills                    （存在）
```
