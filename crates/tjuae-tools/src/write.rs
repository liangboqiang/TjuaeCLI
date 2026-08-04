use std::path::Path;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::{Value, json};

use tjuae_protocol::events::ToolCategory;
use tjuae_types::tool::{JsonSchema, ToolResult};

use crate::Tool;
use crate::file_cache::{FileStateCache, update_cache_after_write};

pub struct WriteTool {
    file_cache: Option<Arc<RwLock<FileStateCache>>>,
}

impl WriteTool {
    /// Create a WriteTool with optional file state cache.
    ///
    /// When cache is `Some`, the tool updates the cache after each successful
    /// write so that subsequent Edit/Read calls see the latest content and mtime.
    ///
    /// No "must Read first" guard: Write is intended for creating new files
    /// or complete rewrites.
    ///
    /// Pass `None` to disable cache integration (legacy behavior).
    pub fn new(file_cache: Option<Arc<RwLock<FileStateCache>>>) -> Self {
        Self { file_cache }
    }
}

#[async_trait]
impl Tool for WriteTool {
    fn name(&self) -> &str {
        "Write"
    }

    fn description(&self) -> &str {
        "将内容写入文件，并在需要时创建父目录。\n\n\
         用法：\n\
         - 此工具会完整覆盖现有文件，而不是追加内容。\n\
         - 如果文件已经存在，必须先使用 Read 查看当前内容。\n\
         - 修改现有文件时优先使用 Edit，因为 Edit 只发送差异。\n\
         - 仅在新建文件或完整重写时使用 Write。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "待写入文件的绝对路径"
                },
                "content": {
                    "type": "string",
                    "description": "要写入文件的内容"
                }
            },
            "required": ["file_path", "content"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        false
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(file_path) = input["file_path"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：file_path".to_string(),
                is_error: true,
            };
        };
        let Some(content) = input["content"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：content".to_string(),
                is_error: true,
            };
        };

        let path = Path::new(file_path);
        let existed = path.exists();

        // Create parent directories
        if let Some(parent) = path.parent().filter(|p| !p.exists()) {
            match std::fs::create_dir_all(parent) {
                Ok(()) => {}
                Err(e) => {
                    return ToolResult {
                        content: format!("创建目录失败：{}", e),
                        is_error: true,
                    };
                }
            }
        }

        // Write atomically: write to temp file, then rename
        let tmp_path = format!("{}.tmp.{}", file_path, std::process::id());
        if let Err(e) = std::fs::write(&tmp_path, content) {
            return ToolResult {
                content: format!("写入文件失败：{}", e),
                is_error: true,
            };
        }

        if let Err(e) = std::fs::rename(&tmp_path, file_path) {
            // Fallback: direct write if rename fails (cross-device)
            let _ = std::fs::remove_file(&tmp_path);
            if let Err(e) = std::fs::write(file_path, content) {
                return ToolResult {
                    content: format!("写入文件失败：{}", e),
                    is_error: true,
                };
            }
            if let Some(cache_arc) = &self.file_cache {
                update_cache_after_write(cache_arc, path, content);
            }

            return ToolResult {
                content: format!("已更新 {}（重命名失败：{}，已改为直接写入）", file_path, e),
                is_error: false,
            };
        }

        if let Some(cache_arc) = &self.file_cache {
            update_cache_after_write(cache_arc, path, content);
        }

        let line_count = content.lines().count();
        let action = if existed { "已更新" } else { "已创建" };
        ToolResult {
            content: format!("{} {}（{} 行）", action, file_path, line_count),
            is_error: false,
        }
    }

    fn max_result_size(&self) -> usize {
        10_000
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Edit
    }

    fn describe(&self, input: &Value) -> String {
        let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("未知");
        format!("写入 {}", path)
    }
}

#[cfg(test)]
#[path = "write_test.rs"]
mod write_test;
