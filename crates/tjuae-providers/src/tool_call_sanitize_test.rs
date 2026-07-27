use super::*;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    // F1-8
    #[test]
    fn test_format_dropped_tool_call_template() {
        assert_eq!(
            format_dropped_tool_call(DroppedToolCallReason::EmptyName, &json!({})),
            "[已跳过工具调用：格式错误（函数名为空）。arguments={}。该调用未执行；若仍需要，请使用有效的 name 重新发起调用。]"
        );
        assert_eq!(
            format_dropped_tool_call(DroppedToolCallReason::EmptyName, &json!({"a":1})),
            "[已跳过工具调用：格式错误（函数名为空）。arguments={\"a\":1}。该调用未执行；若仍需要，请使用有效的 name 重新发起调用。]"
        );
    }

    // F2-8
    #[test]
    fn test_format_dropped_tool_call_empty_id_template() {
        assert_eq!(
            format_dropped_tool_call(DroppedToolCallReason::EmptyId, &json!({"command":"ls"})),
            "[已跳过工具调用：格式错误（工具调用 ID 为空）。arguments={\"command\":\"ls\"}。该调用未执行；若仍需要，请使用有效的 id 重新发起调用。]"
        );
    }

    // F1-6
    #[test]
    fn test_format_truncates_at_char_boundary() {
        // 150 multi-byte chars; must truncate to 100 chars with `…`, no panic.
        let big = "中".repeat(150);
        let out = format_dropped_tool_call(DroppedToolCallReason::EmptyId, &json!({"k": big}));
        assert!(out.contains('…'));
        assert!(out.starts_with("[已跳过工具调用："));
        // Pin the exact 100-char truncation boundary: the args segment between
        // `arguments=` and the `…` ellipsis must be exactly 100 chars.
        let after = out.split("arguments=").nth(1).unwrap();
        let args = after.split('…').next().unwrap();
        assert_eq!(args.chars().count(), 100);
    }
}
