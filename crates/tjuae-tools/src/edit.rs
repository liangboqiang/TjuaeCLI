use std::borrow::Cow;
use std::path::Path;
use std::sync::{Arc, RwLock};

use async_trait::async_trait;
use serde_json::{Value, json};

use tjuae_protocol::events::ToolCategory;
use tjuae_types::tool::{JsonSchema, ToolResult};

use crate::Tool;
use crate::file_cache::{FileStateCache, file_mtime_ms, update_cache_after_write};

#[derive(Clone, Copy)]
enum LineEnding {
    Lf,
    Crlf,
}

struct OldStringMatch<'a> {
    old_string: Cow<'a, str>,
    line_ending: LineEnding,
    match_count: usize,
}

enum MatchSelectionError {
    NotFound,
    AmbiguousLineEndings,
}

fn detect_line_ending(content: &str) -> LineEnding {
    if content.contains("\r\n") {
        LineEnding::Crlf
    } else {
        LineEnding::Lf
    }
}

fn line_ending_in(text: &str) -> Option<LineEnding> {
    if text.contains("\r\n") {
        Some(LineEnding::Crlf)
    } else if text.contains('\n') {
        Some(LineEnding::Lf)
    } else {
        None
    }
}

fn select_old_string<'a>(content: &str, old_string: &'a str) -> Result<OldStringMatch<'a>, MatchSelectionError> {
    let exact_match_count = content.matches(old_string).count();
    let normalized_candidates = [
        (LineEnding::Lf, convert_line_endings(old_string, LineEnding::Lf)),
        (LineEnding::Crlf, convert_line_endings(old_string, LineEnding::Crlf)),
    ];

    if exact_match_count > 0 {
        // Exact bytes take priority. Other EOL forms are checked only to avoid
        // silently choosing one of multiple visually identical regions.
        let has_alternative_match = normalized_candidates
            .iter()
            .any(|(_, candidate)| candidate.as_ref() != old_string && content.contains(candidate.as_ref()));
        if has_alternative_match {
            return Err(MatchSelectionError::AmbiguousLineEndings);
        }

        return Ok(OldStringMatch {
            old_string: Cow::Borrowed(old_string),
            line_ending: line_ending_in(old_string).unwrap_or_else(|| detect_line_ending(content)),
            match_count: exact_match_count,
        });
    }

    let mut fallback_match = None;
    for (line_ending, candidate) in normalized_candidates {
        if candidate.as_ref() == old_string {
            continue;
        }

        let match_count = content.matches(candidate.as_ref()).count();
        if match_count == 0 {
            continue;
        }
        if fallback_match.is_some() {
            return Err(MatchSelectionError::AmbiguousLineEndings);
        }

        fallback_match = Some(OldStringMatch {
            old_string: candidate,
            line_ending,
            match_count,
        });
    }

    fallback_match.ok_or(MatchSelectionError::NotFound)
}

fn convert_line_endings(text: &str, line_ending: LineEnding) -> Cow<'_, str> {
    match line_ending {
        LineEnding::Lf => {
            if text.contains("\r\n") {
                Cow::Owned(text.replace("\r\n", "\n"))
            } else {
                Cow::Borrowed(text)
            }
        }
        LineEnding::Crlf => {
            if text.contains('\n') {
                let normalized = text.replace("\r\n", "\n");
                Cow::Owned(normalized.replace('\n', "\r\n"))
            } else {
                Cow::Borrowed(text)
            }
        }
    }
}

pub struct EditTool {
    file_cache: Option<Arc<RwLock<FileStateCache>>>,
}

impl EditTool {
    /// Create an EditTool with optional file state cache.
    ///
    /// When cache is `Some`, the tool enforces:
    /// - "Must Read first" guard (file must be in cache before editing)
    /// - Staleness detection (disk mtime must match cached mtime)
    /// - Post-write cache update (mtime + content refreshed after edit)
    ///
    /// Pass `None` to disable all cache-related guards (legacy behavior).
    pub fn new(file_cache: Option<Arc<RwLock<FileStateCache>>>) -> Self {
        Self { file_cache }
    }
}

#[async_trait]
impl Tool for EditTool {
    fn name(&self) -> &str {
        "Edit"
    }

    fn description(&self) -> &str {
        "在文件中执行精确字符串替换。\n\n\
         用法：\n\
         - 编辑文件前必须先使用 Read 工具。\n\
         - old_string 在文件中必须唯一。如果存在多个匹配项，编辑会失败。\
         请提供更多上下文使其唯一，或使用 replace_all 修改所有匹配项。\n\
         - 重命名变量或替换字符串的所有实例时使用 replace_all。\n\
         - 修改现有文件时优先使用 Edit，因为 Edit 只发送差异。\n\
         - 匹配 Read 输出中的文本时，必须保留精确缩进（制表符或空格）。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "待修改文件的绝对路径"
                },
                "old_string": {
                    "type": "string",
                    "description": "待替换文本"
                },
                "new_string": {
                    "type": "string",
                    "description": "替换后的文本"
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "是否替换所有匹配项（默认 false）"
                }
            },
            "required": ["file_path", "old_string", "new_string"]
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
        let Some(old_string) = input["old_string"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：old_string".to_string(),
                is_error: true,
            };
        };
        let Some(new_string) = input["new_string"].as_str() else {
            return ToolResult {
                content: "缺少必需参数：new_string".to_string(),
                is_error: true,
            };
        };
        let replace_all = input["replace_all"].as_bool().unwrap_or(false);

        let path = Path::new(file_path);

        // Cache guard: "must Read first" + staleness detection.
        if let Some(cache_arc) = &self.file_cache
            && let Ok(mut cache) = cache_arc.write()
        {
            let cached = cache.get(path);
            if cached.is_none() {
                return ToolResult {
                    content: format!(
                        "编辑前必须先读取 {}。请先使用 Read 工具，\
                         将文件内容加载到上下文中。",
                        file_path
                    ),
                    is_error: true,
                };
            }
            // Staleness check: compare cached mtime with current disk mtime.
            let cached_mtime = cached.map(|s| s.mtime_ms);
            let disk_mtime = file_mtime_ms(path);
            if let (Some(cached_mt), Some(disk_mt)) = (cached_mtime, disk_mtime)
                && cached_mt != disk_mt
            {
                return ToolResult {
                    content: format!(
                        "文件 {} 自上次读取后已被外部修改。\
                         编辑前请重新读取文件以查看当前内容。",
                        file_path
                    ),
                    is_error: true,
                };
            }
        }

        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                return ToolResult {
                    content: format!("读取文件 {} 失败：{}", file_path, e),
                    is_error: true,
                };
            }
        };

        // Prefer the exact bytes supplied by the caller. Only fall back to an
        // EOL-adapted old_string when the exact form is absent.
        let selected = match select_old_string(&content, old_string) {
            Ok(selected) => selected,
            Err(MatchSelectionError::NotFound) => {
                return ToolResult {
                    content: "文件中未找到 old_string".to_string(),
                    is_error: true,
                };
            }
            Err(MatchSelectionError::AmbiguousLineEndings) => {
                return ToolResult {
                    content: "换行符匹配不明确：old_string 同时匹配 LF 和 CRLF 文本。\
                              请提供更多上下文。"
                        .to_string(),
                    is_error: true,
                };
            }
        };
        let match_count = selected.match_count;
        let new_string = convert_line_endings(new_string, selected.line_ending);

        if match_count > 1 && !replace_all {
            return ToolResult {
                content: format!(
                    "找到多个匹配项（{} 个）。请使用 replace_all 或提供更多上下文。",
                    match_count
                ),
                is_error: true,
            };
        }

        let new_content = if replace_all {
            content.replace(selected.old_string.as_ref(), new_string.as_ref())
        } else {
            content.replacen(selected.old_string.as_ref(), new_string.as_ref(), 1)
        };

        if let Err(e) = std::fs::write(file_path, &new_content) {
            return ToolResult {
                content: format!("写入文件失败：{}", e),
                is_error: true,
            };
        }

        // Post-write cache update: refresh mtime and content.
        if let Some(cache_arc) = &self.file_cache {
            update_cache_after_write(cache_arc, path, &new_content);
        }

        ToolResult {
            content: format!("已编辑 {}：替换了 {} 处", file_path, match_count),
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
        format!("编辑 {}", path)
    }
}

#[cfg(test)]
#[path = "edit_test.rs"]
mod edit_test;
