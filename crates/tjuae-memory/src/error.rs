use std::path::PathBuf;

/// Errors that can occur within the memory system.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// File I/O error.
    #[error("记忆 I/O 错误：{0}")]
    Io(#[from] std::io::Error),

    /// YAML frontmatter failed to parse.
    #[error("解析 {path} 中的 frontmatter 失败：{source}")]
    FrontmatterParse { path: PathBuf, source: serde_yaml::Error },

    /// Memory path failed security validation.
    #[error("路径验证失败：{0}")]
    PathValidation(String),
}

pub type Result<T> = std::result::Result<T, MemoryError>;

#[cfg(test)]
#[path = "error_test.rs"]
mod error_test;
