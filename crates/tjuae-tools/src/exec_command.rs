use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::{Value, json};

use tjuae_config::shell::{resolve_shell, shell_command_builder};
use tjuae_process::CommandRunner;
use tjuae_protocol::events::ToolCategory;
use tjuae_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

const DEFAULT_TIMEOUT_MS: u64 = 120_000;
const MAX_TIMEOUT_MS: u64 = 600_000;

pub struct ExecCommandTool {
    cwd: PathBuf,
    runtime_env: HashMap<String, String>,
}

impl ExecCommandTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self {
            cwd,
            runtime_env: HashMap::new(),
        }
    }

    pub fn new_with_env(cwd: PathBuf, runtime_env: Vec<(String, String)>) -> Self {
        Self {
            cwd,
            runtime_env: runtime_env.into_iter().collect(),
        }
    }
}

fn render_exit_result(exit_code: i32, stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    format!("退出码：{exit_code}\n标准输出：\n{stdout}\n标准错误：\n{stderr}")
}

fn render_timeout_result(timeout_ms: u64, stdout: &[u8], stderr: &[u8]) -> String {
    let stdout = String::from_utf8_lossy(stdout);
    let stderr = String::from_utf8_lossy(stderr);
    format!("命令在 {timeout_ms} 毫秒后超时\n标准输出：\n{stdout}\n标准错误：\n{stderr}")
}

#[async_trait]
impl Tool for ExecCommandTool {
    fn name(&self) -> &str {
        "ExecCommand"
    }

    fn description(&self) -> &str {
        "执行 shell 命令并返回其输出。\n\n\
         重要：如果已有专用工具，请不要使用 ExecCommand：\n\
         - 搜索文件：使用 Glob（不要使用 find 或 ls）\n\
         - 搜索内容：使用 Grep（不要使用 grep 或 rg）\n\
         - 读取文件：使用 Read（不要使用 cat、head 或 tail）\n\
         - 编辑文件：使用 Edit（不要使用 sed 或 awk）\n\
         - 写入文件：使用 Write（不要使用 echo 或带 heredoc 的 cat）\n\n\
         # 使用说明\n\
         - 使用绝对路径，避免混淆工作目录。\n\
         - 执行多个相互独立的命令时，应并行调用工具，不要把命令串联起来。\
         只有命令相互依赖时才使用 `&&`。\n\
         - 可指定以毫秒为单位的超时时间（默认 120000，最大 600000）。\n\n\
         # Git 安全\n\
         - 除非用户明确要求，否则绝不强制推送、执行 reset --hard 或使用 --no-verify。\n\
         - 优先创建新提交，不要修改已有提交。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "cmd": {
                    "type": "string",
                    "description": "要执行的命令"
                },
                "shell": {
                    "type": "string",
                    "description": "可选的 shell 覆盖值：auto、powershell、pwsh、cmd、bash、zsh、sh 或可执行文件路径"
                },
                "timeout": {
                    "type": "integer",
                    "description": "以毫秒为单位的超时时间（默认 120000，最大 600000）"
                }
            },
            "required": ["cmd"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(command) = input["cmd"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：cmd".to_string(),
                is_error: true,
            };
        };

        let shell = match resolve_shell(input["shell"].as_str()) {
            Ok(shell) => shell,
            Err(err) => {
                return ToolResult {
                    content: format!("无效的 shell：{}", err),
                    is_error: true,
                };
            }
        };

        tracing::info!(
            cwd = %self.cwd.display(),
            shell_kind = shell.kind.name(),
            shell_path = %shell.path.display(),
            "ExecCommandTool 正在执行"
        );

        let timeout_ms = input["timeout"]
            .as_u64()
            .unwrap_or(DEFAULT_TIMEOUT_MS)
            .min(MAX_TIMEOUT_MS);

        let timeout = Duration::from_millis(timeout_ms);

        let cwd = self.cwd.clone();
        let mut command_builder = shell_command_builder(&shell, command, false);
        command_builder.envs(&self.runtime_env).current_dir(&cwd);

        let result = CommandRunner::new(command_builder).timeout(timeout).run().await;

        match result {
            Ok(result) if result.timed_out => ToolResult {
                content: render_timeout_result(timeout_ms, &result.stdout, &result.stderr),
                is_error: true,
            },
            Ok(result) => {
                let exit_code = result.exit_code.unwrap_or(-1);
                ToolResult {
                    content: render_exit_result(exit_code, &result.stdout, &result.stderr),
                    is_error: exit_code != 0,
                }
            }
            Err(err) => ToolResult {
                content: format!("执行命令失败：{}", err),
                is_error: true,
            },
        }
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Exec
    }

    fn describe(&self, input: &Value) -> String {
        let cmd = input.get("cmd").and_then(|v| v.as_str()).unwrap_or("");
        format!("执行：{}", crate::truncate_utf8(cmd, 80))
    }
}

#[cfg(test)]
#[path = "exec_command_test.rs"]
mod exec_command_test;
