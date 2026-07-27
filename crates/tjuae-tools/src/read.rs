use std::path::Path;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::{Value, json};

use tjuae_protocol::events::ToolCategory;
use tjuae_types::file_state::FileState;
use tjuae_types::tool::{JsonSchema, ToolResult};

use crate::Tool;
use crate::file_cache::{FileStateCache, file_mtime_ms};

/// Stub returned when a file has not changed since the model last read it.
/// Saves tokens by avoiding re-sending identical content.
const FILE_UNCHANGED_STUB: &str = "文件自上次读取后没有变化。本次对话中先前 Read 工具结果里的内容仍然有效，\
     请直接参考该内容，不要重复读取。";

pub struct ReadTool {
    file_cache: Option<Arc<RwLock<FileStateCache>>>,
}

impl ReadTool {
    /// Create a ReadTool with optional file state cache for dedup.
    ///
    /// Pass `None` to disable caching (all reads return full content).
    pub fn new(file_cache: Option<Arc<RwLock<FileStateCache>>>) -> Self {
        Self { file_cache }
    }
}

#[async_trait]
impl Tool for ReadTool {
    fn name(&self) -> &str {
        "Read"
    }

    fn description(&self) -> &str {
        "从本地文件系统读取文件，并返回带行号的内容。\n\n\
         用法：\n\
         - file_path 参数必须是绝对路径，不能是相对路径。\n\
         - 默认读取整个文件。读取大型文件的一部分时请使用 offset 和 limit。\n\
         - 返回结果以从 1 开始的行号开头，行号与内容之间用制表符分隔。\n\
         - 二进制文件返回“（二进制文件，N 字节）”，而不是文件内容。\n\
         - 此工具只能读取文件，不能读取目录。要列出目录，请用 ExecCommand 执行 ls。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "待读取文件的绝对路径"
                },
                "offset": {
                    "type": "integer",
                    "description": "开始读取的行号（从 0 开始）"
                },
                "limit": {
                    "type": "integer",
                    "description": "最多读取的行数"
                }
            },
            "required": ["file_path"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let Some(file_path) = input["file_path"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：file_path".to_string(),
                is_error: true,
            };
        };

        let offset = input["offset"].as_u64().map(|v| v as usize);
        let limit = input["limit"].as_u64().map(|v| v as usize);

        // Get file mtime for dedup and cache.
        let mtime_ms = file_mtime_ms(Path::new(file_path));

        // Dedup check: if cache has the same file with matching offset/limit and mtime,
        // return a short stub instead of full content.
        if let (Some(cache_arc), Some(current_mtime)) = (&self.file_cache, mtime_ms)
            && let Ok(mut cache) = cache_arc.write()
            && let Some(cached) = cache.get(Path::new(file_path))
            && cached.offset == offset
            && cached.limit == limit
            && cached.mtime_ms == current_mtime
        {
            return ToolResult {
                content: FILE_UNCHANGED_STUB.to_string(),
                is_error: false,
            };
        }

        // Read file from disk.
        let content = match std::fs::read(file_path) {
            Ok(bytes) => bytes,
            Err(e) => {
                return ToolResult {
                    content: format!("读取文件 {} 失败：{}", file_path, e),
                    is_error: true,
                };
            }
        };

        // Check if binary.
        if content.iter().take(8192).any(|&b| b == 0) {
            return ToolResult {
                content: format!("（二进制文件，{} 字节）", content.len()),
                is_error: false,
            };
        }

        let text = String::from_utf8_lossy(&content);
        let lines: Vec<&str> = text.lines().collect();

        let effective_offset = offset.unwrap_or(0);
        let effective_limit = limit.unwrap_or(lines.len());

        let end = (effective_offset + effective_limit).min(lines.len());
        let slice = &lines[effective_offset.min(lines.len())..end];

        let numbered: Vec<String> = slice
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6}\t{}", effective_offset + i + 1, line))
            .collect();

        let result_content = numbered.join("\n");

        // Update cache after successful read.
        if let Some(cache_arc) = &self.file_cache
            && let (Ok(mut cache), Some(mtime)) = (cache_arc.write(), mtime_ms)
        {
            cache.insert(
                file_path.into(),
                FileState {
                    content: result_content.clone(),
                    mtime_ms: mtime,
                    offset,
                    limit,
                },
            );
        }

        ToolResult {
            content: result_content,
            is_error: false,
        }
    }

    fn max_result_size(&self) -> usize {
        100_000
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let path = input.get("file_path").and_then(|v| v.as_str()).unwrap_or("未知");
        format!("读取 {}", path)
    }
}

#[cfg(test)]
#[path = "read_test.rs"]
mod read_test;
