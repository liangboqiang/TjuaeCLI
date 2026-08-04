use std::path::PathBuf;

use async_trait::async_trait;
use serde_json::{Value, json};
use tokio::process::Command;

use tjuae_protocol::events::ToolCategory;
use tjuae_types::tool::{JsonSchema, ToolResult};

use crate::Tool;

pub struct GrepTool {
    cwd: PathBuf,
}

impl GrepTool {
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }
}

#[async_trait]
impl Tool for GrepTool {
    fn name(&self) -> &str {
        "Grep"
    }

    fn description(&self) -> &str {
        "使用正则表达式搜索文件内容（由 ripgrep 驱动）。\n\n\
         重要：搜索内容时始终使用 Grep 工具。绝不要通过 ExecCommand 运行 grep 或 rg。\n\n\
         - 支持完整的正则表达式语法（例如 \"log.*Error\"、\"fn\\\\s+\\\\w+\"）。\n\
         - 使用 glob 参数按文件模式筛选（例如 \"*.rs\"）。\n\
         - 输出最多保留 250 行。\n\
         - 将 case_insensitive 设为 true 可执行不区分大小写的搜索。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "要搜索的正则表达式"
                },
                "path": {
                    "type": "string",
                    "description": "搜索目录（默认：当前工作目录）"
                },
                "glob": {
                    "type": "string",
                    "description": "文件筛选模式，例如 \"*.rs\""
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "是否执行不区分大小写的搜索"
                }
            },
            "required": ["pattern"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(pattern) = input["pattern"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：pattern".to_string(),
                is_error: true,
            };
        };

        let raw_path = input["path"].as_str().unwrap_or(".");
        let path = if std::path::Path::new(raw_path).is_relative() {
            self.cwd.join(raw_path).to_string_lossy().into_owned()
        } else {
            raw_path.to_owned()
        };

        tracing::debug!(cwd = %self.cwd.display(), resolved_path = %path, pattern = %pattern, "GrepTool 正在搜索");

        let glob_pattern = input["glob"].as_str();
        let case_insensitive = input["case_insensitive"].as_bool().unwrap_or(false);

        // Try ripgrep first, fallback to grep
        let result = try_ripgrep(pattern, &path, glob_pattern, case_insensitive).await;

        match result {
            Ok(output) => output,
            Err(_) => {
                // Fallback to grep
                try_grep(pattern, &path, case_insensitive).await
            }
        }
    }

    fn max_result_size(&self) -> usize {
        20_000
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let pattern = input.get("pattern").and_then(|v| v.as_str()).unwrap_or("");
        let raw_path = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
        format!("在 {} 中搜索 '{}'", raw_path, pattern)
    }
}

async fn try_ripgrep(
    pattern: &str,
    path: &str,
    glob_pattern: Option<&str>,
    case_insensitive: bool,
) -> Result<ToolResult, std::io::Error> {
    let mut cmd = Command::new("rg");
    cmd.arg(pattern).arg(path).arg("-n");

    if let Some(g) = glob_pattern {
        cmd.arg("--glob").arg(g);
    }
    if case_insensitive {
        cmd.arg("-i");
    }

    let output = cmd.output().await?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    if output.status.code() == Some(1) && stdout.is_empty() {
        return Ok(ToolResult {
            content: "未找到匹配项".to_string(),
            is_error: false,
        });
    }

    if !output.status.success() && output.status.code() != Some(1) {
        return Ok(ToolResult {
            content: format!("rg 错误：{}", stderr),
            is_error: true,
        });
    }

    // Truncate to 250 lines (global limit, not per-file)
    let lines: Vec<&str> = stdout.lines().take(250).collect();
    Ok(ToolResult {
        content: lines.join("\n"),
        is_error: false,
    })
}

async fn try_grep(pattern: &str, path: &str, case_insensitive: bool) -> ToolResult {
    let mut cmd = if cfg!(windows) {
        let mut c = Command::new("findstr");
        c.arg("/S")
            .arg("/N")
            .arg("/R")
            .arg(pattern)
            .arg(format!("{}\\*", path.trim_end_matches(['\\', '/'])));
        if case_insensitive {
            c.arg("/I");
        }
        c
    } else {
        let mut c = Command::new("grep");
        c.arg("-rn").arg(pattern).arg(path);
        if case_insensitive {
            c.arg("-i");
        }
        c
    };

    match cmd.output().await {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.is_empty() {
                ToolResult {
                    content: "未找到匹配项".to_string(),
                    is_error: false,
                }
            } else {
                let lines: Vec<&str> = stdout.lines().take(250).collect();
                ToolResult {
                    content: lines.join("\n"),
                    is_error: false,
                }
            }
        }
        Err(e) => ToolResult {
            content: format!("grep 执行失败：{}", e),
            is_error: true,
        },
    }
}

#[cfg(test)]
#[path = "grep_test.rs"]
mod grep_test;
