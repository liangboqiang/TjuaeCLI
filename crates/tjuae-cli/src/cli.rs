use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "tjuae-cli",
    about = "TjuaeCLI — 支持多模型提供商与工具编排的 AI 智能体命令行工具",
    version
)]
pub(crate) struct Cli {
    // --- Subcommand ---
    #[command(subcommand)]
    pub(crate) command: Option<Commands>,

    // --- Provider / model connection ---
    /// 模型提供商："anthropic" 或 "openai"
    #[arg(short, long, env = "PROVIDER")]
    pub(crate) provider: Option<String>,

    /// API 密钥
    #[arg(short = 'k', long, env = "API_KEY")]
    pub(crate) api_key: Option<String>,

    /// API 基础 URL
    #[arg(short, long, env = "BASE_URL")]
    pub(crate) base_url: Option<String>,

    /// 模型名称
    #[arg(short, long, env = "MODEL")]
    pub(crate) model: Option<String>,

    /// 每次响应的最大输出 token 数
    #[arg(long)]
    pub(crate) max_tokens: Option<u32>,

    /// 支持 thinking 请求对象的提供商所使用的思考模式：enabled 或 disabled
    #[arg(long)]
    pub(crate) thinking: Option<String>,

    /// 启用 --thinking 时使用的 token 预算；仅适用于 Anthropic，OpenAI 兼容请求会忽略此项
    #[arg(long)]
    pub(crate) thinking_budget: Option<u32>,

    // --- Runtime guards ---
    /// 每次运行的最大模型轮次。默认 20；设为 0 表示不限制
    #[arg(long)]
    pub(crate) max_turns: Option<usize>,

    /// 连续出现相同工具调用格式错误后停止的最大轮数。设为 0 表示不限制
    #[arg(long)]
    pub(crate) max_tool_call_malformed_turns: Option<usize>,

    /// 连续出现工具调用失败后停止的最大轮数。设为 0 表示不限制
    #[arg(long)]
    pub(crate) max_tool_call_failure_turns: Option<usize>,

    // --- Prompt / profile ---
    /// 自定义系统提示词
    #[arg(long)]
    pub(crate) system_prompt: Option<String>,

    /// 配置文件中的命名配置档
    #[arg(long)]
    pub(crate) profile: Option<String>,

    /// 自动批准所有工具执行（跳过确认）
    #[arg(long)]
    pub(crate) auto_approve: bool,

    /// 加载 .tjuae.toml 的项目目录（默认为当前工作目录）
    #[arg(long)]
    pub(crate) project_dir: Option<PathBuf>,

    // --- Session ---
    /// 恢复之前的会话
    #[arg(long)]
    pub(crate) resume: Option<String>,

    /// 使用指定的会话 ID（而非自动生成）
    #[arg(long)]
    pub(crate) session_id: Option<String>,

    // --- Output ---
    /// 禁用彩色输出
    #[arg(long)]
    pub(crate) no_color: bool,

    /// 启用用于宿主客户端集成的 JSON 流模式
    #[arg(long)]
    pub(crate) json_stream: bool,

    /// 输出压缩级别：off、safe（默认）或 full
    #[arg(long)]
    pub(crate) compaction: Option<String>,

    /// 对 JSON 数组启用 TOON 编码（会话级配置，对话中途不可更改）
    #[arg(long)]
    pub(crate) toon: bool,

    // --- Logging ---
    /// 日志目录（设置后启用文件日志）
    #[arg(long)]
    pub(crate) log_dir: Option<String>,

    /// 日志级别过滤器（例如 "info"、"debug"、"info,tjuae_providers=debug"）
    #[arg(long)]
    pub(crate) log_level: Option<String>,

    // --- Trailing prompt ---
    /// 初始提示词（省略时进入交互式 REPL 模式）
    #[arg(trailing_var_arg = true)]
    pub(crate) prompt: Vec<String>,
}

#[derive(Subcommand)]
pub(crate) enum Commands {
    /// 管理配置文件
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },
    /// 身份认证（Anthropic OAuth）
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    /// 管理会话
    Session {
        #[command(subcommand)]
        action: SessionAction,
    },
    /// 查看技能目录
    Skills {
        #[command(subcommand)]
        action: SkillsAction,
    },
}

#[derive(Subcommand)]
pub(crate) enum ConfigAction {
    /// 生成默认配置文件
    Init,
    /// 输出配置文件路径并退出
    Path,
}

#[derive(Subcommand)]
pub(crate) enum AuthAction {
    /// 使用 Anthropic 账户登录（OAuth 设备流程）
    Login,
    /// 退出登录（删除已保存的 OAuth 凭据）
    Logout,
}

#[derive(Subcommand)]
pub(crate) enum SessionAction {
    /// 列出已保存的会话
    List,
}

#[derive(Subcommand)]
pub(crate) enum SkillsAction {
    /// 输出技能目录路径并退出
    Path,
}

#[cfg(test)]
#[path = "cli_test.rs"]
mod cli_test;
