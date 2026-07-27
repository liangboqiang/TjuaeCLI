use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use serde_json::{Value, json};

use tjuae_protocol::events::ToolCategory;
use tjuae_tools::Tool;
use tjuae_types::skill_types::{ContextModifier, PlanModeTransition};
use tjuae_types::tool::{JsonSchema, ToolResult};

// ---------------------------------------------------------------------------
// EnterPlanModeTool
// ---------------------------------------------------------------------------

/// Transitions the agent into Plan Mode.
///
/// While in plan mode the engine restricts the available tool set to
/// read-only (`Info`-category) tools so the LLM can focus on understanding
/// the codebase and composing an implementation plan.
pub struct EnterPlanModeTool {
    /// Shared flag indicating whether plan mode is currently active.
    /// Read by `execute()` to prevent double-entry.
    plan_active: Arc<AtomicBool>,
}

impl EnterPlanModeTool {
    pub fn new(plan_active: Arc<AtomicBool>) -> Self {
        Self { plan_active }
    }
}

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "EnterPlanMode"
    }

    fn description(&self) -> &str {
        "进入计划模式，专注于阅读代码和制定实施计划。\
         在计划模式中只能使用只读工具。计划准备完成后使用 ExitPlanMode。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(&self, _input: Value) -> ToolResult {
        if self.plan_active.load(Ordering::Acquire) {
            return ToolResult {
                content: "当前已处于计划模式。请先使用 ExitPlanMode 退出。".to_string(),
                is_error: true,
            };
        }

        ToolResult {
            content: "已进入计划模式。现在只能使用只读工具探索代码库并制定实施计划。\
                      计划准备完成后，使用 ExitPlanMode 退出计划模式并开始实施。"
                .to_string(),
            is_error: false,
        }
    }

    fn context_modifier_for(&self, _input: &Value) -> Option<ContextModifier> {
        Some(ContextModifier {
            plan_mode_transition: Some(PlanModeTransition::Enter),
            ..Default::default()
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, _input: &Value) -> String {
        "进入计划模式".to_string()
    }
}

// ---------------------------------------------------------------------------
// ExitPlanModeTool
// ---------------------------------------------------------------------------

/// Transitions the agent out of Plan Mode.
///
/// On exit the engine restores the full tool set and the allow-list
/// that was in effect before plan mode was entered.
pub struct ExitPlanModeTool {
    /// Shared flag indicating whether plan mode is currently active.
    /// Read by `execute()` to reject exit when not in plan mode.
    plan_active: Arc<AtomicBool>,
}

impl ExitPlanModeTool {
    pub fn new(plan_active: Arc<AtomicBool>) -> Self {
        Self { plan_active }
    }
}

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "ExitPlanMode"
    }

    fn description(&self) -> &str {
        "实施计划完成后退出计划模式。\
         这会恢复完整的工具访问权限，以便开始实施计划。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {},
            "required": []
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    fn is_deferred(&self) -> bool {
        true
    }

    async fn execute(&self, _input: Value) -> ToolResult {
        if !self.plan_active.load(Ordering::Acquire) {
            return ToolResult {
                content: "当前不在计划模式中。请先使用 EnterPlanMode 进入计划模式。".to_string(),
                is_error: true,
            };
        }

        ToolResult {
            content: "已退出计划模式并恢复完整工具访问权限。现在可以继续实施计划。".to_string(),
            is_error: false,
        }
    }

    fn context_modifier_for(&self, _input: &Value) -> Option<ContextModifier> {
        Some(ContextModifier {
            plan_mode_transition: Some(PlanModeTransition::Exit { plan_content: None }),
            ..Default::default()
        })
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, _input: &Value) -> String {
        "退出计划模式".to_string()
    }
}

#[cfg(test)]
#[path = "tools_test.rs"]
mod tools_test;
