use async_trait::async_trait;

use super::{CommandContext, CommandResult, SlashCommand};
use crate::compact::auto;
use tjuae_types::compact::{CompactMetadata, CompactTrigger};
use tjuae_types::message::ContentBlock;

pub struct CompactCommand;

#[async_trait]
impl SlashCommand for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }

    fn description(&self) -> &str {
        "压缩对话上下文"
    }

    async fn execute(&self, ctx: &mut CommandContext<'_>, _args: &str) -> anyhow::Result<CommandResult> {
        if ctx.messages.len() <= 2 {
            ctx.output.emit_info("上下文已经足够紧凑");
            return Ok(CommandResult::Continue);
        }

        // Reset circuit breaker — manual intent overrides protection
        ctx.compact_state.consecutive_failures = 0;

        let pre_tokens = ctx.compact_state.last_input_tokens;

        match auto::autocompact(
            ctx.provider.as_ref(),
            ctx.messages,
            ctx.model,
            ctx.compact_config,
            ctx.compact_state,
        )
        .await
        {
            Ok(result) => {
                let msgs_summarized = result.messages_summarized;
                *ctx.messages = result.messages;

                if let Some(boundary) = ctx.messages.first_mut() {
                    for block in &mut boundary.content {
                        if let ContentBlock::Text { text } = block
                            && text.starts_with(auto::BOUNDARY_PREFIX)
                        {
                            let metadata = CompactMetadata {
                                trigger: CompactTrigger::Manual,
                                pre_compact_tokens: pre_tokens,
                                messages_summarized: msgs_summarized,
                            };
                            *text = format!(
                                "{}\n{}",
                                auto::BOUNDARY_PREFIX,
                                serde_json::to_string(&metadata).expect("metadata serialization cannot fail")
                            );
                        }
                    }
                }

                ctx.output.emit_info(&format!(
                    "上下文已压缩：{}k → compact（已总结 {} 条消息）",
                    pre_tokens / 1000,
                    msgs_summarized
                ));
                ctx.context_state.record_compact();
                return Ok(CommandResult::ContextChanged);
            }
            Err(e) => {
                ctx.output.emit_error(&format!("压缩失败：{}", e));
            }
        }

        Ok(CommandResult::Continue)
    }
}

#[cfg(test)]
#[path = "compact_test.rs"]
mod compact_test;
