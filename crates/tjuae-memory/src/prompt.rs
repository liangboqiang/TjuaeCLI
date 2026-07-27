// Memory system prompt construction.
//
// Builds the behavioral instructions and MEMORY.md content that get
// injected into the agent's system prompt so it knows how to read,
// write, and manage the persistent memory system.

use std::path::Path;

use crate::index::{MAX_INDEX_LINES, read_index, truncate_index};
use crate::paths::ENTRYPOINT_NAME;

// ---------------------------------------------------------------------------
// Display name
// ---------------------------------------------------------------------------

const DISPLAY_NAME: &str = "自动记忆";

// ---------------------------------------------------------------------------
// Directory existence guidance
// ---------------------------------------------------------------------------

/// Guidance appended to the memory directory prompt line so the model
/// doesn't waste turns on `ls` / `mkdir -p` before writing.
const DIR_EXISTS_GUIDANCE: &str = "此目录已经存在——请直接使用 Write 工具写入，\
    不要运行 mkdir，也不要检查目录是否存在。";

// ---------------------------------------------------------------------------
// Type taxonomy (individual-only, no team/private scope tags)
// ---------------------------------------------------------------------------

const TYPES_SECTION: &str = "\
## 记忆类型

记忆系统可以存储以下几种彼此独立的记忆：

<types>
<type>
    <name>user</name>
    <description>记录用户的角色、目标、职责和知识背景。高质量的 user 记忆可以帮助你根据用户的偏好和视角调整未来行为。读写这些记忆的目标，是逐步理解用户是谁，以及怎样才能最有效地帮助对方。例如，与资深软件工程师协作的方式应不同于帮助第一次编程的学生。记住，目的始终是帮助用户；不要记录可能被视为负面评判，或与双方共同工作无关的内容。</description>
    <when_to_save>当你了解到用户角色、偏好、职责或知识背景的任何细节时。</when_to_save>
    <how_to_use>当工作方式需要结合用户画像或视角时使用。例如，用户要求解释代码时，应根据其最关注的细节和已有领域知识来组织回答，帮助对方建立合适的心智模型。</how_to_use>
    <examples>
    user: 我是一名数据科学家，正在调查项目现有的日志能力
    assistant: [保存 user 记忆：用户是数据科学家，目前关注可观测性和日志]

    user: 我写 Go 已经十年了，但这是第一次接触这个仓库的 React 部分
    assistant: [保存 user 记忆：用户精通 Go，但刚接触 React 和本项目的前端——解释前端概念时可类比后端]
    </examples>
</type>
<type>
    <name>feedback</name>
    <description>记录用户对工作方式的指导，包括应避免什么以及应继续做什么。这类记忆非常重要，可以让你在项目中保持一致，并持续采用用户期望的工作方式。既要记录失败，也要记录成功：如果只保存纠正意见，你虽然能避免重复错误，却可能偏离用户已经认可的方法，并变得过度谨慎。</description>
    <when_to_save>用户纠正你的方法（例如“不是这样”“不要”“停止做 X”），或确认一个并不显然的方法有效（例如“对，就是这样”“很好，继续这样做”，或没有反对一个特殊选择）时。纠正意见容易发现，肯定往往更隐晦，也要留意。两种情况都只保存适用于未来对话的信息，特别是那些出人意料或无法从代码中直接看出的内容，并写明原因，以便日后判断边界情况。</when_to_save>
    <how_to_use>让这些记忆指导你的行为，避免用户重复给出同样的要求。</how_to_use>
    <body_structure>先写规则本身，再写一行 **原因：**（用户给出的理由，通常是过往事件或强烈偏好）和一行 **应用方式：**（何时、何处适用）。了解原因可以帮助你判断边界情况，而不是机械执行规则。</body_structure>
    <examples>
    user: 这些测试不要模拟数据库——上季度模拟测试都通过了，但生产迁移失败，给我们造成了损失
    assistant: [保存 feedback 记忆：集成测试必须使用真实数据库，不能使用 mock。原因：此前 mock 与生产环境不一致，掩盖了损坏的迁移]

    user: 不要在每次响应结尾总结刚做过的事，我会看 diff
    assistant: [保存 feedback 记忆：用户希望响应简洁，不要附加结尾总结]

    user: 对，这里合成一个 PR 是正确选择，拆开只会徒增工作
    assistant: [保存 feedback 记忆：对此区域的重构，用户更偏好一个完整 PR，而不是多个小 PR。这是用户确认过的判断，并非纠正意见]
    </examples>
</type>
<type>
    <name>project</name>
    <description>记录无法从代码或 Git 历史中推导出的项目进行中工作、目标、计划、缺陷或事故。project 记忆帮助你理解用户在当前工作目录中开展工作的更广泛背景和动机。</description>
    <when_to_save>当你了解到谁在做什么、为什么做或何时完成时。此类状态变化较快，应尽量保持最新。保存时始终把用户消息中的相对日期转换为绝对日期（例如“周四”→“2026-03-05”），确保日后仍可理解。</when_to_save>
    <how_to_use>使用这些记忆更完整地理解用户请求背后的细节和含义，从而提出更可靠的建议。</how_to_use>
    <body_structure>先写事实或决定，再写一行 **原因：**（动机，通常是约束、截止日期或利益相关方要求）和一行 **应用方式：**（它应如何影响建议）。项目记忆过期较快，原因可以帮助未来的你判断这条记忆是否仍然重要。</body_structure>
    <examples>
    user: 周四之后冻结所有非关键合并——移动团队要切发布分支
    assistant: [保存 project 记忆：为切出移动端发布分支，合并冻结从 2026-03-05 开始。提醒所有计划在该日期之后进行的非关键 PR 工作]

    user: 删除旧认证中间件是因为法务指出它存储会话 token 的方式不符合新的合规要求
    assistant: [保存 project 记忆：认证中间件重写由会话 token 存储的法律与合规要求推动，而不是技术债清理——范围决策应优先满足合规，而非便利性]
    </examples>
</type>
<type>
    <name>reference</name>
    <description>记录可以在外部系统中找到信息的位置。这类记忆让你知道应到哪里查找项目目录之外的最新信息。</description>
    <when_to_save>当你了解到外部系统中的资源及其用途时。例如，缺陷记录在 Linear 的某个项目中，或反馈位于某个 Slack 频道。</when_to_save>
    <how_to_use>当用户提到外部系统，或信息可能位于外部系统中时使用。</how_to_use>
    <examples>
    user: 如果需要了解这些工单的背景，请查看 Linear 项目 \"INGEST\"，所有流水线缺陷都记录在那里
    assistant: [保存 reference 记忆：流水线缺陷记录在 Linear 项目 \"INGEST\" 中]

    user: 值班人员关注的是 grafana.internal/d/api-latency 看板——修改请求处理逻辑时，它可能触发告警
    assistant: [保存 reference 记忆：grafana.internal/d/api-latency 是值班延迟看板——编辑请求路径代码时应检查它]
    </examples>
</type>
</types>
";

// ---------------------------------------------------------------------------
// What NOT to save
// ---------------------------------------------------------------------------

const WHAT_NOT_TO_SAVE: &str = "\
## 不应保存到记忆中的内容

- 代码模式、约定、架构、文件路径或项目结构——这些可以通过读取项目当前状态获得。
- Git 历史、最近变更或谁修改了什么——应以 `git log` 和 `git blame` 为准。
- 调试方案或修复步骤——修复位于代码中，提交消息提供上下文。
- AGENTS.md 中已经记录的任何内容。
- 短期任务细节：进行中的工作、临时状态和当前对话上下文。

即使用户明确要求保存，上述排除规则仍然适用。如果用户要求保存 PR 列表或活动摘要，请询问其中哪些内容是*出人意料*或*不明显*的——只有这部分值得长期保留。";

// ---------------------------------------------------------------------------
// How to save (two-step process with MEMORY.md index)
// ---------------------------------------------------------------------------

fn how_to_save_section() -> String {
    format!(
        "\
## 如何保存记忆

保存记忆分为两个步骤：

**步骤 1**——使用以下 frontmatter 格式，把记忆写入独立文件（例如 `user_role.md`、`feedback_testing.md`）：

{FRONTMATTER_EXAMPLE}

**步骤 2**——在 `{ep}` 中添加指向该文件的链接。`{ep}` 是索引，不是记忆——每个条目只占一行且不超过约 150 个字符：`- [标题](file.md)——一句话提示`。它没有 frontmatter，绝不要把记忆正文直接写入 `{ep}`。

- `{ep}` 始终会载入对话上下文——超过 {max_lines} 行的内容会被截断，因此索引必须简洁
- 保持记忆文件中的 name、description 和 type 字段与正文同步
- 按主题语义组织记忆，不要按时间顺序组织
- 更新或删除后来发现错误或过时的记忆
- 不要写入重复记忆。新建前先检查能否更新已有记忆。",
        ep = ENTRYPOINT_NAME,
        max_lines = MAX_INDEX_LINES,
    )
}

// ---------------------------------------------------------------------------
// Frontmatter example
// ---------------------------------------------------------------------------

const FRONTMATTER_EXAMPLE: &str = "\
```markdown
---
name: {{记忆名称}}
description: {{单行说明——用于在未来对话中判断相关性，因此应具体明确}}
type: {{user, feedback, project, reference}}
---

{{记忆正文——feedback/project 类型应按“规则或事实”，然后是 **原因：** 和 **应用方式：** 两行来组织}}
```";

// ---------------------------------------------------------------------------
// When to access
// ---------------------------------------------------------------------------

const WHEN_TO_ACCESS: &str = "\
## 何时访问记忆
- 当记忆可能相关，或用户提到先前对话中的工作时。
- 用户明确要求检查、回忆或记住内容时，你必须访问记忆。
- 如果用户要求*忽略*或*不使用*记忆：按 MEMORY.md 为空来处理。不要应用、引用、比较或提及记忆中的事实。
- 记忆记录可能随时间过期。只能把它当作某一时刻真实情况的上下文。如果回答或假设完全依赖记忆，请先读取文件或资源的当前状态，确认记忆仍然正确且最新。记忆与当前信息冲突时，以当前观察为准，并更新或删除过时记忆，不要据其采取行动。";

// ---------------------------------------------------------------------------
// Before recommending from memory
// ---------------------------------------------------------------------------

const BEFORE_RECOMMENDING: &str = "\
## 根据记忆提出建议前

记忆中提到某个函数、文件或标志，只能证明它在*写入记忆时*存在。它可能已经重命名或删除，也可能从未合并。提出建议前：

- 如果记忆提到文件路径：检查文件是否存在。
- 如果记忆提到函数或标志：搜索确认。
- 如果用户即将按你的建议行动，而不只是询问历史：先验证。

“记忆中说 X 存在”不等于“X 现在存在”。

总结仓库状态的记忆（活动日志、架构快照）固定在过去某一时刻。用户询问*最近*或*当前*状态时，应优先查看 `git log` 或读取代码，而不是回忆快照。";

// ---------------------------------------------------------------------------
// Memory vs other persistence
// ---------------------------------------------------------------------------

const PERSISTENCE_SECTION: &str = "\
## 记忆与其他持久化方式
记忆是你在对话中协助用户时可用的多种持久化机制之一。其主要区别是记忆可以在未来对话中重新调用，因此不应保存只对当前对话有用的信息。
- 何时使用或更新计划而不是记忆：准备开始非简单实施任务，并希望先与用户就方法达成一致时，应使用计划，不要把这些信息保存为记忆。如果对话中已有计划而方法发生变化，应更新计划来记录变化，不要保存为记忆。
- 何时使用或更新任务而不是记忆：需要把当前对话中的工作拆分为独立步骤或跟踪进度时，应使用任务。任务适合记录当前对话中待完成的工作；记忆只应保留未来对话仍有用的信息。";

// ---------------------------------------------------------------------------
// Minimal memory prompt (lazy — saves ~2,500 tokens)
// ---------------------------------------------------------------------------

/// Compact summary of the memory system rules, without the full type taxonomy,
/// examples, or detailed save/access instructions. Enough for the LLM to
/// read existing memories and know the system exists; the full instructions
/// are injected on-demand when the LLM first writes to the memory directory.
const MINIMAL_RULES: &str = "\
随着时间推移逐步完善此记忆系统，使未来的对话能够完整了解用户是谁、用户希望如何与你协作、\
哪些行为应避免或重复，以及用户交付工作的背景。

如果用户明确要求记住某件事，请立即保存。如果用户要求忘记某件事，请找到并删除相关条目。

记忆类型包括 user、feedback、project 和 reference。每条记忆都是带 YAML frontmatter \
（name、description、type）的 Markdown 文件。MEMORY.md 是索引，每个条目占一行，\
绝不要直接把正文写入其中。

保存前先读取已有记忆，避免重复。根据记忆提出建议前，验证其中的文件或函数名称是否仍然存在。";

// ===========================================================================
// Public API
// ===========================================================================

/// Build a minimal memory prompt with just the path, compact rules,
/// and MEMORY.md index content. Omits the full type taxonomy and examples
/// to save ~2,500 tokens on the first turn.
pub fn build_memory_prompt_minimal(memory_dir: &Path) -> String {
    let dir_display = memory_dir.display();

    let mut parts = vec![
        format!("# {DISPLAY_NAME}"),
        String::new(),
        format!(
            "你在 `{dir_display}` 中拥有一个基于文件的持久记忆系统。\
             {DIR_EXISTS_GUIDANCE}"
        ),
        String::new(),
        MINIMAL_RULES.to_owned(),
        String::new(),
    ];

    // Append MEMORY.md index (same logic as the full version)
    let entrypoint = memory_dir.join(ENTRYPOINT_NAME);
    let raw = read_index(&entrypoint);
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        parts.push(format!("## {ENTRYPOINT_NAME}"));
        parts.push(String::new());
        parts.push(format!(
            "你的 {ENTRYPOINT_NAME} 当前为空。保存新记忆后，它们会显示在这里。"
        ));
    } else {
        let truncation = truncate_index(&raw);
        parts.push(format!("## {ENTRYPOINT_NAME}"));
        parts.push(String::new());
        parts.push(truncation.content);
    }

    parts.join("\n")
}

/// Build the complete memory system prompt including behavioral instructions
/// AND the current MEMORY.md content (or an empty-state message).
///
/// This is the all-in-one function used when the caller needs a single
/// string to inject into the system prompt.
pub fn build_memory_prompt(memory_dir: &Path) -> String {
    let mut lines = build_memory_instructions(memory_dir);

    let entrypoint = memory_dir.join(ENTRYPOINT_NAME);
    let raw = read_index(&entrypoint);
    let trimmed = raw.trim();

    if trimmed.is_empty() {
        lines.push(format!("## {ENTRYPOINT_NAME}"));
        lines.push(String::new());
        lines.push(format!(
            "你的 {ENTRYPOINT_NAME} 当前为空。保存新记忆后，它们会显示在这里。"
        ));
    } else {
        let truncation = truncate_index(&raw);
        lines.push(format!("## {ENTRYPOINT_NAME}"));
        lines.push(String::new());
        lines.push(truncation.content);
    }

    lines.join("\n")
}

/// Build only the behavioral instructions (without MEMORY.md content).
///
/// Returns a `Vec<String>` of logical prompt sections. The caller is
/// responsible for joining them with newlines and injecting any
/// additional content (e.g. MEMORY.md via a separate path).
pub fn build_memory_instructions(memory_dir: &Path) -> Vec<String> {
    let dir_display = memory_dir.display();

    vec![
        format!("# {DISPLAY_NAME}"),
        String::new(),
        format!(
            "你在 `{dir_display}` 中拥有一个基于文件的持久记忆系统。\
             {DIR_EXISTS_GUIDANCE}"
        ),
        String::new(),
        "随着时间推移逐步完善此记忆系统，使未来的对话能够完整了解用户是谁、\
         用户希望如何与你协作、哪些行为应避免或重复，以及用户交付工作的背景。"
            .to_owned(),
        String::new(),
        "如果用户明确要求记住某件事，请立即以最合适的类型保存。\
         如果用户要求忘记某件事，请找到并删除相关条目。"
            .to_owned(),
        String::new(),
        TYPES_SECTION.to_owned(),
        WHAT_NOT_TO_SAVE.to_owned(),
        String::new(),
        how_to_save_section(),
        String::new(),
        WHEN_TO_ACCESS.to_owned(),
        String::new(),
        BEFORE_RECOMMENDING.to_owned(),
        String::new(),
        PERSISTENCE_SECTION.to_owned(),
        String::new(),
    ]
}

/// Return the memory type descriptions as a standalone string.
///
/// Useful when only the type taxonomy is needed (e.g. for help text
/// or documentation), without the full behavioral instructions.
pub fn memory_type_descriptions() -> &'static str {
    TYPES_SECTION
}

#[cfg(test)]
#[path = "prompt_test.rs"]
mod prompt_test;
