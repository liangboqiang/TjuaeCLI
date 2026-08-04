use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::{Value, json};

use crate::spawner::Spawner;
use tjuae_config::hooks::HooksConfig;
use tjuae_protocol::events::ToolCategory;
use tjuae_skills::context_modifier::ContextModifier;
use tjuae_skills::executor::{execute_fork, prepare_inline_content};
use tjuae_skills::hooks::{parse_skill_hooks, to_hook_defs};
use tjuae_skills::permissions::{SkillPermission, SkillPermissionChecker};
use tjuae_skills::types::{ExecutionContext, SkillMetadata};
use tjuae_types::tool::{JsonSchema, ToolResult};

use tjuae_tools::Tool;

/// A tool that allows the LLM to invoke named skills.
///
/// Each skill is looked up by name (exact match, leading `/` stripped),
/// its content is prepared with variable substitution and shell execution,
/// and returned as a `ToolResult`.  The Skill list is injected into the
/// system prompt in Phase 9; this tool's `description()` returns a fixed string.
pub struct SkillTool {
    skills: Arc<Vec<SkillMetadata>>,
    /// Working directory for shell command execution inside skill content.
    cwd: PathBuf,
    /// Permission checker for skill-level deny/allow rules.
    checker: SkillPermissionChecker,
    /// Session ID passed to prepare_inline_content for ${TJUAE_SESSION_ID} substitution.
    /// None if sessions are disabled or not yet initialised.
    session_id: Option<String>,
    /// Spawner for fork-mode skills. None when SkillTool is built without fork support.
    spawner: Option<Arc<dyn Spawner>>,
}

impl SkillTool {
    pub fn new(skills: Arc<Vec<SkillMetadata>>, cwd: PathBuf, checker: SkillPermissionChecker) -> Self {
        Self {
            skills,
            cwd,
            checker,
            session_id: None,
            spawner: None,
        }
    }

    /// Create a SkillTool with a known session ID.
    pub fn with_session_id(
        skills: Arc<Vec<SkillMetadata>>,
        cwd: PathBuf,
        checker: SkillPermissionChecker,
        session_id: Option<String>,
    ) -> Self {
        Self {
            skills,
            cwd,
            checker,
            session_id,
            spawner: None,
        }
    }

    /// Create a SkillTool with full fork-mode support.
    pub fn with_spawner(
        skills: Arc<Vec<SkillMetadata>>,
        cwd: PathBuf,
        checker: SkillPermissionChecker,
        session_id: Option<String>,
        spawner: Option<Arc<dyn Spawner>>,
    ) -> Self {
        Self {
            skills,
            cwd,
            checker,
            session_id,
            spawner,
        }
    }

    /// Find a skill by exact name (case-sensitive, leading `/` stripped).
    fn find_skill(&self, name: &str) -> Option<&SkillMetadata> {
        let name = name.trim_start_matches('/');
        self.skills.iter().find(|s| s.name == name)
    }

    /// Build a comma-separated list of available skill names for error messages.
    fn available_names(&self) -> String {
        self.skills
            .iter()
            .map(|s| s.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "Skill"
    }

    fn description(&self) -> &str {
        "按名称调用指定技能。必须使用系统提示词中列出的准确技能名称。\
         可选择以单个字符串传入参数。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "技能名称，例如 \"commit\"、\"review-pr\" 或 \"pdf\""
                },
                "args": {
                    "type": "string",
                    "description": "传给技能的可选参数"
                }
            },
            "required": ["skill"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        // Skills may modify context; conservatively mark as not concurrency-safe.
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(skill_name) = input["skill"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：skill".to_string(),
                is_error: true,
            };
        };

        let skill = match self.find_skill(skill_name) {
            Some(s) => s,
            None => {
                let available = self.available_names();
                return ToolResult {
                    content: format!("未找到技能 '{}'。可用技能：{}", skill_name, available),
                    is_error: true,
                };
            }
        };

        // Check skill-level permissions (applies to both inline and fork modes).
        match self.checker.check(skill) {
            SkillPermission::Deny => {
                return ToolResult {
                    content: format!("配置已禁止技能 '{}'。", skill.name),
                    is_error: true,
                };
            }
            SkillPermission::Ask { reason } => {
                return ToolResult {
                    content: format!(
                        "技能 '{}' 执行前需要用户批准。\
                         {} \
                         请让用户在配置中批准此技能。",
                        skill.name, reason
                    ),
                    is_error: true,
                };
            }
            SkillPermission::Allow => {}
        }

        let args = input["args"].as_str();

        match skill.execution_context {
            ExecutionContext::Inline => {
                match prepare_inline_content(skill, args, self.session_id.as_deref(), &self.cwd).await {
                    Ok(content) => ToolResult {
                        content,
                        is_error: false,
                    },
                    Err(e) => ToolResult {
                        content: e.to_string(),
                        is_error: true,
                    },
                }
            }
            ExecutionContext::Fork => {
                let spawner = match self.spawner.as_ref() {
                    Some(s) => s.as_ref(),
                    None => {
                        return ToolResult {
                            content: format!(
                                "技能 '{}' 需要 fork 执行上下文，\
                                 但当前没有可用的 AgentSpawner。\
                                 可通过 SkillTool::with_spawner() 启用 fork 支持。",
                                skill.name
                            ),
                            is_error: true,
                        };
                    }
                };
                match execute_fork(skill, args, self.session_id.as_deref(), &self.cwd, spawner).await {
                    Ok(content) => ToolResult {
                        content,
                        is_error: false,
                    },
                    Err(e) => ToolResult {
                        content: e,
                        is_error: true,
                    },
                }
            }
        }
    }

    fn context_modifier_for(&self, input: &serde_json::Value) -> Option<ContextModifier> {
        let skill_name = input["skill"].as_str()?;
        let skill = self.find_skill(skill_name)?;
        // Fork skills run in their own sub-agent context; modifiers must not
        // propagate back to the parent conversation.
        if skill.execution_context == ExecutionContext::Fork {
            return None;
        }
        tjuae_skills::context_modifier::from_skill(skill)
    }

    fn skill_hooks_for(&self, input: &serde_json::Value) -> Option<HooksConfig> {
        let skill_name = input["skill"].as_str()?;
        let skill = self.find_skill(skill_name)?;
        let config = parse_skill_hooks(skill.hooks_raw.as_ref(), &skill.name, skill.source)?;
        Some(to_hook_defs(&config, &skill.name))
    }

    fn category(&self) -> ToolCategory {
        // Inline mode returns skill content for the model to act on — categorised
        // as Info since it does not directly modify files or run commands.
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let name = input.get("skill").and_then(|v| v.as_str()).unwrap_or("?");
        match input.get("args").and_then(|v| v.as_str()) {
            Some(args) if !args.is_empty() => format!("技能 {name} {args}"),
            _ => format!("技能 {name}"),
        }
    }
}

#[cfg(test)]
#[path = "skill_tool_test.rs"]
mod skill_tool_test;
