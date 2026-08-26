use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tjuae_process::CommandRunner;

use crate::shell::{default_shell, shell_command_builder};

/// Hook system configuration
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct HooksConfig {
    #[serde(default)]
    pub pre_tool_use: Vec<HookDef>,
    #[serde(default)]
    pub post_tool_use: Vec<HookDef>,
    #[serde(default)]
    pub stop: Vec<HookDef>,
}

/// A single hook definition
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct HookDef {
    pub name: String,
    /// Tool name patterns to match (glob). Empty = match all.
    #[serde(default)]
    pub tool_match: Vec<String>,
    /// File path patterns to match (glob). Empty = match all.
    #[serde(default)]
    pub file_match: Vec<String>,
    /// Shell command to execute. Supports ${VAR} interpolation.
    pub command: String,
    /// Timeout in ms (default 30000)
    #[serde(default = "default_hook_timeout")]
    pub timeout_ms: u64,
}

fn default_hook_timeout() -> u64 {
    30_000
}

/// Event-driven hook engine
pub struct HookEngine {
    config: HooksConfig,
    cwd: PathBuf,
    runtime_env: HashMap<String, String>,
}

impl HookEngine {
    pub fn new(config: HooksConfig, cwd: PathBuf) -> Self {
        Self {
            config,
            cwd,
            runtime_env: HashMap::new(),
        }
    }

    pub fn new_with_env(config: HooksConfig, cwd: PathBuf, runtime_env: Vec<(String, String)>) -> Self {
        Self {
            config,
            cwd,
            runtime_env: runtime_env.into_iter().collect(),
        }
    }

    /// Run pre-tool-use hooks. Returns Err if any hook blocks execution.
    pub async fn run_pre_tool_use(&self, tool_name: &str, tool_input: &serde_json::Value) -> Result<(), HookError> {
        let matching: Vec<_> = self
            .config
            .pre_tool_use
            .iter()
            .filter(|h| matches_tool(h, tool_name, tool_input))
            .collect();

        for hook in matching {
            let env = self.build_hook_env(tool_name, tool_input);
            let input = self.codex_tool_payload("PreToolUse", tool_name, tool_input, None);
            let result = run_hook_command(&hook.command, &env, &input, hook.timeout_ms, &self.cwd).await?;
            if result.exit_code == 2 || hook_output_blocks(&result.output) {
                return Err(HookError::Blocked {
                    hook_name: hook.name.clone(),
                    output: result.output,
                });
            }
            if result.exit_code != 0 {
                tracing::warn!(
                    hook = %hook.name,
                    exit_code = result.exit_code,
                    "PreToolUse hook failed without blocking the tool"
                );
            }
        }
        Ok(())
    }

    /// Run post-tool-use hooks. Errors are logged but don't block.
    pub async fn run_post_tool_use(
        &self,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_output: &str,
    ) -> Vec<String> {
        let matching: Vec<_> = self
            .config
            .post_tool_use
            .iter()
            .filter(|h| matches_tool(h, tool_name, tool_input))
            .collect();

        let mut messages = Vec::new();
        for hook in matching {
            let mut env = self.build_hook_env(tool_name, tool_input);
            env.insert("TOOL_OUTPUT".to_string(), tool_output.to_string());
            let input = self.codex_tool_payload("PostToolUse", tool_name, tool_input, Some(tool_output));

            match run_hook_command(&hook.command, &env, &input, hook.timeout_ms, &self.cwd).await {
                Ok(result) => {
                    if !result.output.is_empty() {
                        messages.push(format!("[hook:{}] {}", hook.name, result.output.trim()));
                    }
                }
                Err(e) => {
                    messages.push(format!("[钩子：{}] 错误：{}", hook.name, e));
                }
            }
        }
        messages
    }

    /// Run stop hooks when agent session ends.
    pub async fn run_stop(&self) -> Vec<String> {
        let mut messages = Vec::new();
        for hook in &self.config.stop {
            let input = self.codex_base_payload("Stop");
            match run_hook_command(&hook.command, &self.runtime_env, &input, hook.timeout_ms, &self.cwd).await {
                Ok(result) => {
                    if !result.output.is_empty() {
                        messages.push(format!("[hook:{}] {}", hook.name, result.output.trim()));
                    }
                }
                Err(e) => {
                    messages.push(format!("[钩子：{}] 错误：{}", hook.name, e));
                }
            }
        }
        messages
    }

    /// Check if any hooks are configured
    pub fn has_hooks(&self) -> bool {
        !self.config.pre_tool_use.is_empty() || !self.config.post_tool_use.is_empty() || !self.config.stop.is_empty()
    }

    /// Merge additional hooks into the engine's config, skipping duplicates by name.
    /// Used by SkillTool to register skill-specific hooks at invocation time (idempotent).
    pub fn merge_hooks(&mut self, additional: HooksConfig) {
        merge_vec(&mut self.config.pre_tool_use, additional.pre_tool_use);
        merge_vec(&mut self.config.post_tool_use, additional.post_tool_use);
        merge_vec(&mut self.config.stop, additional.stop);
    }

    fn build_hook_env(&self, tool_name: &str, tool_input: &serde_json::Value) -> HashMap<String, String> {
        let mut env = self.runtime_env.clone();
        env.extend(build_env_vars(tool_name, tool_input));
        env
    }

    fn codex_base_payload(&self, event: &str) -> serde_json::Value {
        serde_json::json!({
            "session_id": self.runtime_env.get("TJUAE_HOOK_SESSION_ID").cloned().unwrap_or_default(),
            "transcript_path": self.runtime_env.get("TJUAE_HOOK_TRANSCRIPT_PATH").cloned().unwrap_or_default(),
            "cwd": self.cwd,
            "hook_event_name": event,
        })
    }

    fn codex_tool_payload(
        &self,
        event: &str,
        tool_name: &str,
        tool_input: &serde_json::Value,
        tool_response: Option<&str>,
    ) -> serde_json::Value {
        let mut payload = self.codex_base_payload(event);
        payload["tool_name"] = serde_json::Value::String(tool_name.to_owned());
        payload["tool_input"] = tool_input.clone();
        if let Some(output) = tool_response {
            payload["tool_response"] = serde_json::Value::String(output.to_owned());
        }
        payload
    }
}

/// Append `incoming` hooks into `existing`, skipping any whose name already exists.
fn merge_vec(existing: &mut Vec<HookDef>, incoming: Vec<HookDef>) {
    for hook in incoming {
        if !existing.iter().any(|h| h.name == hook.name) {
            existing.push(hook);
        }
    }
}

/// Environment variables available to hook commands
fn build_env_vars(tool_name: &str, tool_input: &serde_json::Value) -> HashMap<String, String> {
    let mut env = HashMap::new();
    env.insert("TOOL_NAME".to_string(), tool_name.to_string());
    env.insert("TOOL_INPUT".to_string(), tool_input.to_string());

    // Extract common fields for convenience
    if let Some(fp) = tool_input["file_path"].as_str() {
        env.insert("TOOL_INPUT_FILE_PATH".to_string(), fp.to_string());
    }
    if let Some(cmd) = tool_input["command"].as_str() {
        env.insert("TOOL_INPUT_COMMAND".to_string(), cmd.to_string());
    }
    if let Some(pattern) = tool_input["pattern"].as_str() {
        env.insert("TOOL_INPUT_PATTERN".to_string(), pattern.to_string());
    }

    env
}

fn matches_tool(hook: &HookDef, tool_name: &str, tool_input: &serde_json::Value) -> bool {
    // Check tool_match
    if !hook.tool_match.is_empty() {
        let matches = hook.tool_match.iter().any(|pattern| glob_match(pattern, tool_name));
        if !matches {
            return false;
        }
    }

    // Check file_match (if tool has a file_path input)
    if !hook.file_match.is_empty() {
        if let Some(file_path) = tool_input["file_path"].as_str() {
            let matches = hook.file_match.iter().any(|pattern| glob_match(pattern, file_path));
            if !matches {
                return false;
            }
        } else {
            return false; // file_match specified but tool has no file_path
        }
    }

    true
}

fn glob_match(pattern: &str, value: &str) -> bool {
    glob::Pattern::new(pattern).map(|p| p.matches(value)).unwrap_or(false)
}

/// Interpolate ${VAR} in a command string with provided env vars
fn interpolate_command(command: &str, env_vars: &HashMap<String, String>) -> String {
    let mut result = command.to_string();
    for (key, value) in env_vars {
        result = result.replace(&format!("${{{}}}", key), value);
    }
    result
}

struct HookResult {
    exit_code: i32,
    output: String,
}

fn hook_output_blocks(output: &str) -> bool {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(output.trim()) else {
        return false;
    };
    let decision = value.get("decision").and_then(serde_json::Value::as_str).or_else(|| {
        value
            .pointer("/hookSpecificOutput/permissionDecision")
            .and_then(serde_json::Value::as_str)
    });
    matches!(decision, Some("deny" | "block")) || value.get("continue") == Some(&serde_json::Value::Bool(false))
}

fn combine_output(stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout).to_string();
    let stderr = String::from_utf8_lossy(stderr).to_string();
    if stderr.is_empty() {
        stdout
    } else if stdout.is_empty() {
        stderr
    } else {
        format!("{}\n{}", stdout, stderr)
    }
}

async fn run_hook_command(
    command: &str,
    env_vars: &HashMap<String, String>,
    input: &serde_json::Value,
    timeout_ms: u64,
    cwd: &Path,
) -> Result<HookResult, HookError> {
    let interpolated = interpolate_command(command, env_vars);
    let timeout = Duration::from_millis(timeout_ms);

    let shell = default_shell();

    tracing::debug!(
        cwd = %cwd.display(),
        shell_kind = shell.kind.name(),
        shell_path = %shell.path.display(),
        "正在执行 hook"
    );

    let mut command_builder = shell_command_builder(&shell, &interpolated, false);
    command_builder.envs(env_vars).current_dir(cwd);

    let stdin = serde_json::to_vec(input).map_err(|error| HookError::ExecutionFailed(error.to_string()))?;
    match CommandRunner::new(command_builder)
        .stdin_bytes(stdin)
        .timeout(timeout)
        .run()
        .await
    {
        Ok(result) if result.timed_out => Err(HookError::Timeout {
            timeout_ms,
            output: combine_output(&result.stdout, &result.stderr),
        }),
        Ok(result) => {
            let exit_code = result.exit_code.unwrap_or(-1);
            Ok(HookResult {
                exit_code,
                output: combine_output(&result.stdout, &result.stderr),
            })
        }
        Err(e) => Err(HookError::ExecutionFailed(e.to_string())),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum HookError {
    #[error("钩子 '{hook_name}' 已阻止执行：{output}")]
    Blocked { hook_name: String, output: String },
    #[error("钩子执行失败：{0}")]
    ExecutionFailed(String),
    #[error("钩子在 {timeout_ms} 毫秒后超时\n{output}")]
    Timeout { timeout_ms: u64, output: String },
}

#[cfg(test)]
#[path = "hooks_test.rs"]
mod hooks_test;
