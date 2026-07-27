#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("HTTP 错误：{0}")]
    Http(#[from] reqwest::Error),
    #[error("API 错误 {status}：{message}")]
    Api { status: u16, message: String },
    #[error("SSE 解析错误：{0}")]
    Parse(String),
    // Display intentionally omits `body` — it may contain provider response
    // payload (potentially sensitive) and would leak into logs via
    // `tracing::error!("{err}")`. Consumers that need the body must pattern
    // match on the variant explicitly.
    #[error("请求受到限流，请在 {retry_after_ms} 毫秒后重试")]
    RateLimited { retry_after_ms: u64, body: Option<String> },
    #[error("提示词过长：{0}")]
    PromptTooLong(String),
    #[error("连接错误：{0}")]
    Connection(String),
}

impl ProviderError {
    pub fn is_retryable(&self) -> bool {
        matches!(self, ProviderError::RateLimited { .. } | ProviderError::Connection(_))
    }
}

#[cfg(test)]
#[path = "error_test.rs"]
mod error_test;
