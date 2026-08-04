//! Compact prompt templates for LLM-based conversation summarization.
//!
//! Provides the 9-section summary prompt, response parsing, and
//! post-compact message construction.

/// System prompt used for the compact LLM call.
pub const COMPACT_SYSTEM_PROMPT: &str = "你是一名负责总结对话的 AI 助手。";

/// Maximum output tokens for the compact LLM call.
pub const COMPACT_MAX_OUTPUT_TOKENS: u32 = 20_000;

// ── Prompt construction ─────────────────────────────────────────────────────

/// Build the 9-section compact prompt that asks the LLM to summarize.
pub fn build_compact_prompt() -> String {
    format!("{PREAMBLE}\n\n{BODY}\n\n{FORMAT_INSTRUCTIONS}\n\n{REMINDER}")
}

const PREAMBLE: &str = "\
重要：只能返回文本，绝对不要调用任何工具。
- 不要使用 Read、ExecCommand、Grep、Glob、Edit、Write 或任何其他工具。
- 上方对话已经包含你所需的全部上下文。
- 工具调用会被拒绝，并浪费你唯一的一轮响应，从而导致任务失败。
- 整个响应必须是纯文本：先输出一个 <analysis> 块，再输出一个 <summary> 块。";

const BODY: &str = "\
你的任务是详细总结迄今为止的对话，尤其关注用户的明确要求和你之前采取的操作。\
总结必须完整保留继续开发工作所需的技术细节、代码模式和架构决策。

在给出最终总结前，请将分析放入 <analysis> 标签中，以便梳理思路并确保内容完整。

总结应包含以下章节：

1. **主要请求与意图**：用户提出了哪些要求？包含对话中的全部明确请求。
2. **关键技术概念**：讨论过的重要技术细节、模式或架构决策。
3. **文件与代码区域**：所有查看或修改过的文件，并简述变更。
4. **错误与修复**：遇到的错误及其解决方式。
5. **问题处理进度**：每个问题的当前状态——哪些已解决，哪些仍待处理。
6. **全部用户消息**：总结每条非工具类用户消息，同时保留意图和上下文。
7. **待办任务**：尚未完成的所有任务。
8. **当前工作**：生成本总结前正在处理的工作。
9. **建议的下一步**：最合理的唯一下一步，且必须与用户最近一次明确请求直接一致。\
请逐字引用该请求，避免偏离目标。";

const FORMAT_INSTRUCTIONS: &str = "\
请严格按以下格式响应：

<analysis>
你对哪些信息最应保留的分析
</analysis>

<summary>
按上述 9 个章节组织的详细结构化总结
</summary>";

const REMINDER: &str = "\
再次提醒：不要调用任何工具。只能用纯文本响应——先输出 <analysis> 块，再输出 \
<summary> 块。工具调用会被拒绝，并导致任务失败。";

// ── Response parsing ────────────────────────────────────────────────────────

/// Parse the raw LLM response: strip `<analysis>`, extract `<summary>` content.
///
/// If no `<summary>` tags are found, returns the raw text as-is (graceful degradation).
pub fn format_compact_summary(raw: &str) -> String {
    // Step 1: remove <analysis>...</analysis>
    let without_analysis = strip_tag(raw, "analysis");

    // Step 2: extract <summary>...</summary> content
    if let Some(summary_content) = extract_tag_content(&without_analysis, "summary") {
        let trimmed = summary_content.trim();
        if trimmed.is_empty() {
            return collapse_blank_lines(&without_analysis).trim().to_string();
        }
        format!("总结：\n{trimmed}")
    } else {
        // Graceful degradation: use the text with analysis stripped
        collapse_blank_lines(&without_analysis).trim().to_string()
    }
}

// ── Post-compact message content ────────────────────────────────────────────

/// Build the user message content for the post-compact summary.
///
/// For autocompact (`is_auto = true`), appends an instruction telling the
/// model to continue seamlessly without acknowledging the compaction.
pub fn build_summary_content(formatted_summary: &str, is_auto: bool) -> String {
    let mut content = String::from("本会话从一个上下文已耗尽的先前对话继续。以下总结涵盖了对话的较早部分。\n\n");
    content.push_str(formatted_summary);

    if is_auto {
        content.push_str(
            "\n\n从中断处直接继续对话，不要再向用户提问。直接恢复工作——不要提及这份总结，\
             不要回顾之前发生的事情，也不要以“我会继续”等类似措辞开头。\
             像对话从未中断一样接着处理最后一项任务。",
        );
    }

    content
}

// ── Helpers ─────────────────────────────────────────────────────────────────

/// Remove `<tag>...</tag>` (first occurrence) from text.
///
/// If the closing tag appears before the opening tag (reversed order),
/// the text is returned unchanged to avoid producing duplicate content.
fn strip_tag(text: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let Some(start) = text.find(&open) else {
        return text.to_string();
    };
    let Some(end) = text.find(&close) else {
        return text.to_string();
    };

    // Guard: closing tag before opening tag → no-op
    if end < start {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    result.push_str(&text[..start]);
    result.push_str(&text[end + close.len()..]);
    collapse_blank_lines(&result)
}

/// Extract the content between `<tag>` and `</tag>` (first occurrence).
fn extract_tag_content<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");

    let start = text.find(&open)? + open.len();
    let end = text.find(&close)?;

    if start <= end { Some(&text[start..end]) } else { None }
}

/// Collapse consecutive blank lines into a single blank line.
fn collapse_blank_lines(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut prev_was_blank = false;

    for line in text.lines() {
        let is_blank = line.trim().is_empty();
        if is_blank && prev_was_blank {
            continue;
        }
        if !result.is_empty() {
            result.push('\n');
        }
        result.push_str(line);
        prev_was_blank = is_blank;
    }

    result
}

#[cfg(test)]
#[path = "prompt_test.rs"]
mod prompt_test;
