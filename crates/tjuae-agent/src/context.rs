use std::collections::HashMap;
use std::path::Path;

use tjuae_config::shell::{ResolvedShell, default_shell, render_shell_prompt};
use tjuae_memory::prompt::build_memory_prompt_minimal;
use tjuae_skills::prompt::format_skills_within_budget;
use tjuae_skills::types::SkillMetadata;
use tjuae_types::message::{ContentBlock, Message, Role};

use crate::agents_md;
use crate::plan::prompt as plan_prompt;
use crate::tool_policy::ToolPolicy;

/// Session-scoped cache for system prompt sections.
///
/// Each section (intro, tool guidance, AGENTS.md, memory, skills) is cached
/// independently. The `joined` field holds the pre-joined full prompt string
/// and is invalidated whenever any section changes.
pub struct SystemPromptCache {
    /// Cached section strings, keyed by section name.
    pub(crate) sections: HashMap<&'static str, String>,
    /// Pre-joined full prompt. Invalidated on any section change.
    pub(crate) joined: Option<String>,
    /// Track last plan_mode_active value to detect changes.
    pub(crate) last_plan_mode: bool,
    /// Track last toon_enabled value to detect changes.
    pub(crate) last_toon_enabled: bool,
    /// Track shell prompt text to invalidate intro when shell resolution changes.
    pub(crate) last_shell_prompt: Option<String>,
    /// Track runtime tool authorization to keep guidance and skill reminders aligned.
    pub(crate) last_tool_policy: ToolPolicy,
}

impl SystemPromptCache {
    pub fn new() -> Self {
        Self {
            sections: HashMap::new(),
            joined: None,
            last_plan_mode: false,
            last_toon_enabled: false,
            last_shell_prompt: None,
            last_tool_policy: ToolPolicy::default(),
        }
    }

    /// Invalidate a specific section by name.
    pub fn invalidate(&mut self, section: &str) {
        self.sections.remove(section);
        self.joined = None;
    }

    /// Invalidate all cached sections (e.g., on /compact).
    pub fn invalidate_all(&mut self) {
        self.sections.clear();
        self.joined = None;
    }
}

impl Default for SystemPromptCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Return the tool-usage guidance section for the system prompt.
///
/// For unrestricted agents this includes the full cross-tool guidance. For
/// restricted agents it only names tools authorized by the runtime policy.
fn tool_usage_guidance(tool_policy: &ToolPolicy) -> String {
    if matches!(tool_policy, ToolPolicy::Unrestricted) {
        return "\
# 使用工具
 - 有专用工具可用时，不要使用 ExecCommand。专用工具能让用户更容易理解和审核你的工作：
   - 搜索文件：使用 Glob（不要使用 find 或 ls）
   - 搜索内容：使用 Grep（不要使用 grep 或 rg）
   - 读取文件：使用 Read（不要使用 cat、head 或 tail）
   - 编辑文件：使用 Edit（不要使用 sed 或 awk）
   - 写入文件：使用 Write（不要使用 echo 重定向或带 heredoc 的 cat）
 - 你可以在一次响应中调用多个工具。如果工具之间没有依赖关系，请并行发起所有独立调用。\
如果某个调用依赖前一个调用的结果，则按顺序执行。
 - 修改现有文件时优先使用 Edit，而不是 Write。Edit 只发送差异，更便于审核。
 - 编辑文件前始终先使用 Read。
 - 有些工具会延迟加载，只显示名称。调用延迟加载工具前，先使用 ToolSearch 加载完整 schema。"
            .to_string();
    }

    let mut guidance = vec!["# 使用工具".to_string()];
    let dedicated_tools = [
        ("Glob", "搜索文件：Glob"),
        ("Grep", "搜索内容：Grep"),
        ("Read", "读取文件：Read"),
        ("Edit", "编辑文件：Edit"),
        ("Write", "写入文件：Write"),
    ]
    .into_iter()
    .filter_map(|(name, text)| tool_policy.allows(name).then_some(text))
    .collect::<Vec<_>>();

    if !dedicated_tools.is_empty() {
        guidance.push(" - 如果适合当前任务，请使用可用的工作区专用工具：".to_string());
        guidance.extend(dedicated_tools.into_iter().map(|tool| format!("   - {tool}")));
    }

    guidance.push(
        " - 你可以在一次响应中调用多个工具。如果工具之间没有依赖关系，请并行发起所有独立调用；如果某个调用依赖前一个调用的结果，则按顺序执行。"
            .to_string(),
    );

    if tool_policy.allows("Edit") && tool_policy.allows("Write") {
        guidance.push(" - 修改现有文件时优先使用 Edit，而不是 Write。Edit 只发送差异，更便于审核。".to_string());
    }
    if tool_policy.allows("Read") && tool_policy.allows("Edit") {
        guidance.push(" - 编辑文件前始终先使用 Read。".to_string());
    }
    if tool_policy.allows("ToolSearch") {
        guidance.push(
            " - 有些工具会延迟加载，只显示名称。调用延迟加载工具前，先使用 ToolSearch 加载完整 schema。".to_string(),
        );
    }

    guidance.join("\n")
}

/// Build the system prompt from config and environment.
///
/// Sections are assembled in this order:
/// 1. Base intro (role, model identity, working directory, date)
/// 2. Tool usage guidance (dedicated tools, parallel calls, etc.)
/// 3. Custom prompt (user config)
/// 4. AGENTS.md (project instructions)
/// 5. Memory system prompt (behavioral instructions + MEMORY.md content)
/// 6. Plan mode instructions (when active)
/// 7. Skills reminder (available skills listing)
///
/// Session-permanent sections (intro, tool guidance, custom prompt, AGENTS.md)
/// are cached in `cache.sections` and reused across calls. The `joined` field
/// caches the final concatenated result; it is returned on subsequent calls
/// unless a dynamic input such as plan mode or tool policy has changed.
#[allow(clippy::too_many_arguments)]
pub fn build_system_prompt(
    cache: &mut SystemPromptCache,
    custom_prompt: Option<&str>,
    cwd: &str,
    model: &str,
    skills: &[SkillMetadata],
    context_window_tokens: Option<usize>,
    memory_dir: Option<&Path>,
    plan_mode_active: bool,
    toon_enabled: bool,
) -> String {
    let shell = default_shell();
    build_system_prompt_with_shell(
        cache,
        custom_prompt,
        cwd,
        model,
        &shell,
        skills,
        context_window_tokens,
        memory_dir,
        plan_mode_active,
        toon_enabled,
    )
}

/// Build the system prompt with an already-resolved shell.
#[allow(clippy::too_many_arguments)]
pub fn build_system_prompt_with_shell(
    cache: &mut SystemPromptCache,
    custom_prompt: Option<&str>,
    cwd: &str,
    model: &str,
    shell: &ResolvedShell,
    skills: &[SkillMetadata],
    context_window_tokens: Option<usize>,
    memory_dir: Option<&Path>,
    plan_mode_active: bool,
    toon_enabled: bool,
) -> String {
    build_system_prompt_with_shell_and_tool_policy(
        cache,
        custom_prompt,
        cwd,
        model,
        shell,
        skills,
        context_window_tokens,
        memory_dir,
        plan_mode_active,
        toon_enabled,
        &ToolPolicy::Unrestricted,
    )
}

/// Build the system prompt while keeping tool guidance consistent with the
/// runtime authorization policy.
#[allow(clippy::too_many_arguments)]
pub(crate) fn build_system_prompt_with_shell_and_tool_policy(
    cache: &mut SystemPromptCache,
    custom_prompt: Option<&str>,
    cwd: &str,
    model: &str,
    shell: &ResolvedShell,
    skills: &[SkillMetadata],
    context_window_tokens: Option<usize>,
    memory_dir: Option<&Path>,
    plan_mode_active: bool,
    toon_enabled: bool,
    tool_policy: &ToolPolicy,
) -> String {
    if cache.last_tool_policy != *tool_policy {
        cache.invalidate("tool_guidance");
        cache.invalidate("skills");
        cache.last_tool_policy = tool_policy.clone();
    }

    let shell_prompt = render_shell_prompt(shell);
    if cache.last_shell_prompt.as_deref() != Some(shell_prompt.as_str()) {
        cache.invalidate("intro");
        cache.last_shell_prompt = Some(shell_prompt.clone());
    }

    // Fast path: return cached joined result if nothing changed
    if let Some(ref joined) = cache.joined
        && cache.last_plan_mode == plan_mode_active
        && cache.last_toon_enabled == toon_enabled
    {
        return joined.clone();
    }

    let mut parts = Vec::new();

    // Section: intro (session permanent)
    let intro = cache.sections.entry("intro").or_insert_with(|| {
        format!(
            "你是一名可以使用工具协助完成任务的 AI 助手。\n\
             当前使用的模型：{model}\n\
             工作目录：{cwd}\n\
             当前日期：{}\n\
             操作系统：{}\n\
             系统架构：{}\n\
             {}",
            chrono::Local::now().format("%Y-%m-%d"),
            std::env::consts::OS,
            std::env::consts::ARCH,
            shell_prompt
        )
    });
    parts.push(intro.clone());

    // Section: tool guidance (session permanent)
    let guidance = cache
        .sections
        .entry("tool_guidance")
        .or_insert_with(|| tool_usage_guidance(tool_policy));
    parts.push(guidance.clone());

    // Section: custom prompt (session permanent)
    if let Some(custom) = custom_prompt {
        let custom_cached = cache.sections.entry("custom").or_insert_with(|| custom.to_string());
        parts.push(custom_cached.clone());
    }

    // Section: AGENTS.md (session permanent, hierarchical)
    let agents_section = cache.sections.entry("agents_md").or_insert_with(|| {
        let files = agents_md::collect_agents_md(cwd);
        agents_md::format_agents_md_section(&files)
    });
    if !agents_section.is_empty() {
        parts.push(agents_section.clone());
    }

    // Section: memory (cached, event-invalidated)
    // Uses the minimal prompt to save ~2,500 tokens — omits full type taxonomy
    // and examples. The full instructions are available via build_memory_prompt().
    if let Some(dir) = memory_dir {
        let memory_section = cache
            .sections
            .entry("memory")
            .or_insert_with(|| build_memory_prompt_minimal(dir));
        if !memory_section.is_empty() {
            parts.push(memory_section.clone());
        }
    }

    // Section: TOON format instructions (session permanent once enabled)
    if toon_enabled {
        let toon_section = cache
            .sections
            .entry("toon")
            .or_insert_with(|| tjuae_compact::toon_format_instructions().to_string());
        parts.push(toon_section.clone());
    }

    // Section: plan mode (NOT cached — rebuilt every call when active)
    if plan_mode_active {
        parts.push(plan_prompt::plan_mode_instructions().to_string());
    }

    // Section: skills (cached, event-invalidated)
    let visible_skills: Vec<SkillMetadata> = if tool_policy.allows("Skill") {
        skills.iter().filter(|s| !s.disable_model_invocation).cloned().collect()
    } else {
        Vec::new()
    };

    if !visible_skills.is_empty() {
        let skills_section = cache.sections.entry("skills").or_insert_with(|| {
            let listing = format_skills_within_budget(&visible_skills, context_window_tokens);
            if listing.is_empty() {
                String::new()
            } else {
                format!("<system-reminder>\n以下技能可通过 Skill 工具使用：\n\n{listing}\n</system-reminder>")
            }
        });
        if !skills_section.is_empty() {
            parts.push(skills_section.clone());
        }
    }

    let joined = parts.join("\n\n");
    cache.joined = Some(joined.clone());
    cache.last_plan_mode = plan_mode_active;
    cache.last_toon_enabled = toon_enabled;
    joined
}

/// Compact old messages to reduce context size.
/// Keeps first message (user input) and last `keep_tail` messages,
/// replaces middle with a summary.
pub fn compact_messages(messages: &mut Vec<Message>, keep_tail: usize) {
    let min_messages = keep_tail + 2; // first + summary + tail
    if messages.len() <= min_messages {
        return;
    }

    let tail_start = messages.len() - keep_tail;
    let summarized_count = tail_start - 1;

    let summary_text = format!(
        "[先前对话摘要：共交换 {} 条消息，包括工具调用及其结果。关键上下文已保留在最近的消息中。]",
        summarized_count
    );

    let summary_msg = Message::new(Role::User, vec![ContentBlock::Text { text: summary_text }]);

    let tail: Vec<Message> = messages.drain(tail_start..).collect();
    messages.truncate(1); // keep first message
    messages.push(summary_msg);
    messages.extend(tail);
}

#[cfg(test)]
#[path = "context_test.rs"]
mod context_test;
