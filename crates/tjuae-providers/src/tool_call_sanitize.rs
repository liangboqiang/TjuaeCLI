#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DroppedToolCallReason {
    EmptyName,
    EmptyId,
}

impl DroppedToolCallReason {
    fn description(self) -> &'static str {
        match self {
            DroppedToolCallReason::EmptyName => "函数名为空",
            DroppedToolCallReason::EmptyId => "工具调用 ID 为空",
        }
    }

    fn reissue_field(self) -> &'static str {
        match self {
            DroppedToolCallReason::EmptyName => "name",
            DroppedToolCallReason::EmptyId => "id",
        }
    }

    pub(crate) fn log_reason(self) -> &'static str {
        match self {
            DroppedToolCallReason::EmptyName => "empty_name",
            DroppedToolCallReason::EmptyId => "empty_id",
        }
    }

    pub(crate) fn short_placeholder(self) -> &'static str {
        match self {
            DroppedToolCallReason::EmptyName => "[已跳过工具调用：格式错误（函数名为空）。]",
            DroppedToolCallReason::EmptyId => "[已跳过工具调用：格式错误（工具调用 ID 为空）。]",
        }
    }
}

/// Format a malformed tool_call as a human/model-readable line to embed in the
/// assistant content during projection. Shared by OpenAI and Anthropic
/// projection paths so the wording stays identical across providers.
/// `arguments` is the tool input, truncated to 100 chars on a char boundary.
pub(crate) fn format_dropped_tool_call(reason: DroppedToolCallReason, input: &serde_json::Value) -> String {
    let raw = serde_json::to_string(input).unwrap_or_default();
    let args = truncate_chars(&raw, 100);
    format!(
        "[已跳过工具调用：格式错误（{}）。arguments={}。该调用未执行；若仍需要，请使用有效的 {} 重新发起调用。]",
        reason.description(),
        args,
        reason.reissue_field()
    )
}

/// Truncate to at most `max` chars on a char boundary, appending `…` if cut.
fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let end = s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len());
    format!("{}…", &s[..end])
}

#[cfg(test)]
#[path = "tool_call_sanitize_test.rs"]
mod tool_call_sanitize_test;
