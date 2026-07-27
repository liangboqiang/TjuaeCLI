use tjuae_providers::error::ProviderError;

#[derive(Debug, thiserror::Error)]
pub enum AgentError {
    #[error("API 错误：{0}")]
    ApiError(String),
    #[error("提供商反复返回格式错误的工具调用输出（{count}/{limit}）；为避免浪费 token，运行已停止")]
    ToolCallMalformed { count: usize, limit: usize },
    #[error("工具调用连续失败 {count}/{limit} 次后已停止；任务未能收敛。请调整请求或重试。")]
    ToolCallFailures { count: usize, limit: usize },
    #[error("提供商错误：{0}")]
    Provider(#[from] ProviderError),
    #[error("用户已中止会话")]
    UserAborted,
    #[error("上下文窗口即将用尽（已使用 {input_tokens} token，上限 {limit}）")]
    ContextTooLong { input_tokens: u64, limit: usize },
}
