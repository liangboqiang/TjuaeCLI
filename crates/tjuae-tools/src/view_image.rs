use std::path::Path;

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};

use tjuae_protocol::events::ToolCategory;
use tjuae_types::message::{ContentBlock, ImageUrl, extension_to_image_media_type};
use tjuae_types::tool::{JsonSchema, ToolResult};

use crate::{Tool, ToolExecutionOutput};

const MAX_IMAGE_SIZE_BYTES: u64 = 20 * 1024 * 1024;

fn detect_image_media_type(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

pub struct ViewImageTool;

impl ViewImageTool {
    pub fn new() -> Self {
        Self
    }

    async fn load_image(&self, input: &Value) -> Result<ImageUrl, String> {
        let file_path = input
            .get("file_path")
            .and_then(Value::as_str)
            .filter(|path| !path.trim().is_empty())
            .ok_or_else(|| "缺少必需参数：file_path".to_owned())?;
        let path = Path::new(file_path);
        if !path.is_absolute() {
            return Err("file_path 必须是绝对路径".to_owned());
        }

        let extension = path
            .extension()
            .and_then(|extension| extension.to_str())
            .ok_or_else(|| "图片路径必须包含受支持的扩展名".to_owned())?;
        let mime_type =
            extension_to_image_media_type(extension).ok_or_else(|| format!("不支持的图片扩展名：{extension}"))?;

        let metadata = tokio::fs::metadata(path)
            .await
            .map_err(|error| format!("读取图片元数据失败：{error}"))?;
        if !metadata.is_file() {
            return Err("图片路径不是普通文件".to_owned());
        }
        if metadata.len() > MAX_IMAGE_SIZE_BYTES {
            return Err(format!("图片超过 {} 字节的大小限制", MAX_IMAGE_SIZE_BYTES));
        }

        let bytes = tokio::fs::read(path)
            .await
            .map_err(|error| format!("读取图片失败：{error}"))?;
        if bytes.len() as u64 > MAX_IMAGE_SIZE_BYTES {
            return Err(format!("图片超过 {} 字节的大小限制", MAX_IMAGE_SIZE_BYTES));
        }
        let detected_mime_type = detect_image_media_type(&bytes)
            .ok_or_else(|| "文件内容不是受支持的 JPEG、PNG、GIF 或 WebP 图片".to_owned())?;
        if detected_mime_type != mime_type {
            return Err(format!(
                "图片内容类型 {detected_mime_type} 与扩展名类型 {mime_type} 不匹配"
            ));
        }

        let image_url = ImageUrl {
            url: format!("data:{detected_mime_type};base64,{}", STANDARD.encode(bytes)),
        };
        image_url
            .validate()
            .map_err(|error| format!("准备图片输入失败：{error}"))?;
        Ok(image_url)
    }

    fn success_result(file_path: &str) -> ToolResult {
        ToolResult {
            content: format!("已从 {file_path} 加载图片，并将其附加到下一轮模型输入。"),
            is_error: false,
        }
    }

    fn error_result(error: String) -> ToolResult {
        ToolResult {
            content: error,
            is_error: true,
        }
    }
}

impl Default for ViewImageTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Tool for ViewImageTool {
    fn name(&self) -> &str {
        "ViewImage"
    }

    fn description(&self) -> &str {
        "从本地绝对路径加载图片，并将其附加到下一轮模型输入。需要检查图片附件时使用此工具。"
    }

    fn input_schema(&self) -> JsonSchema {
        json!({
            "type": "object",
            "properties": {
                "file_path": {
                    "type": "string",
                    "description": "JPEG、PNG、GIF 或 WebP 图片的绝对路径"
                }
            },
            "required": ["file_path"]
        })
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(&self, input: Value) -> ToolResult {
        let file_path = input.get("file_path").and_then(Value::as_str).unwrap_or("未知");
        match self.load_image(&input).await {
            Ok(_) => Self::success_result(file_path),
            Err(error) => Self::error_result(error),
        }
    }

    async fn execute_with_follow_up(&self, input: Value) -> ToolExecutionOutput {
        let file_path = input.get("file_path").and_then(Value::as_str).unwrap_or("未知");
        match self.load_image(&input).await {
            Ok(image_url) => ToolExecutionOutput {
                result: Self::success_result(file_path),
                follow_up_blocks: vec![ContentBlock::Image { image_url }],
            },
            Err(error) => Self::error_result(error).into(),
        }
    }

    fn requires_image_input(&self) -> bool {
        true
    }

    fn category(&self) -> ToolCategory {
        ToolCategory::Info
    }

    fn describe(&self, input: &Value) -> String {
        let path = input.get("file_path").and_then(Value::as_str).unwrap_or("未知");
        format!("查看图片 {path}")
    }
}

#[cfg(test)]
#[path = "view_image_test.rs"]
mod view_image_test;
