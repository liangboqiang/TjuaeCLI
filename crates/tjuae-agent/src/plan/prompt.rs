// Plan mode system prompt instructions.
//
// These prompts guide the LLM's behavior while in plan mode: what tools to
// use, what actions are forbidden, and how to structure the resulting plan.

/// Instructions injected into the system prompt when plan mode is active.
///
/// Guides the LLM through a structured planning workflow:
/// 1. Explore the codebase with read-only tools
/// 2. Design the implementation approach
/// 3. Write the plan
/// 4. Call ExitPlanMode when ready for user review
pub fn plan_mode_instructions() -> &'static str {
    r#"# 计划模式

计划模式已启用。严禁编辑文件、运行任何非只读工具，或以其他方式更改系统。

## 允许的操作
- 使用只读工具（Read、Grep、Glob）读取文件、搜索代码并探索代码库
- 在响应文本中编写实施计划
- 提出澄清问题

## 禁止的操作
- 编辑、新建或删除文件
- 运行会更改状态的 shell 命令
- 创建提交或推送更改

## 规划流程

### 阶段 1：理解
探索代码库，了解当前架构、相关文件和既有模式。阅读关键文件并搜索相关代码。

### 阶段 2：设计
基于你的理解设计实施方案：
- 确定需要新建或修改的文件
- 指出应复用的现有函数和工具
- 考虑边界情况和错误处理

### 阶段 3：编写计划
在响应中编写清晰、可执行的实施计划，内容包括：
- **背景**：简要说明为什么需要此变更
- **待修改文件**：逐一列出文件及所需变更
- **待复用代码**：注明现有函数、工具及其文件路径
- **验证方式**：说明如何端到端测试变更

### 阶段 4：提交审核
计划完成后，调用 ExitPlanMode 交给用户审核。不要询问“这个计划可以吗？”——调用 ExitPlanMode 即表示请求批准。"#
}

#[cfg(test)]
#[path = "prompt_test.rs"]
mod prompt_test;
