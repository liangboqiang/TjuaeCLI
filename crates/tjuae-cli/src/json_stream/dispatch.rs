//! Top-level `ProtocolCommand` dispatch for the JSON stream main loop.
//!
//! Handles every command except `Message` (which needs the inner
//! select-loop machinery in `message.rs`) and the `AddMcpServer` pre-message
//! phase (handled in `pre_message.rs` before this loop starts).

use tjuae_agent::engine::AgentEngine;
use tjuae_protocol::ToolApprovalResult;
use tjuae_protocol::commands::ProtocolCommand;
use tjuae_protocol::events::ProtocolEvent;
use tjuae_protocol::writer::ProtocolEmitter;

use super::context::StreamContext;

/// Outcome of handling one top-level command.
pub(super) enum DispatchOutcome {
    /// Keep looping.
    Continue,
    /// A `Stop` command was received — shut down.
    Stop,
}

/// Handle a single top-level command (i.e. one that arrived outside of an
/// in-flight `Message`). `Message` itself is handled by the caller via
/// `message::handle`, not here.
pub(super) fn handle(cmd: ProtocolCommand, engine: &mut AgentEngine, ctx: &StreamContext) -> DispatchOutcome {
    match cmd {
        ProtocolCommand::Stop => return DispatchOutcome::Stop,
        ProtocolCommand::ToolApprove { call_id, scope: _ } => {
            ctx.approval_manager.resolve(&call_id, ToolApprovalResult::Approved);
        }
        ProtocolCommand::ToolDeny { call_id, reason } => {
            ctx.approval_manager
                .resolve(&call_id, ToolApprovalResult::Denied { reason });
        }
        ProtocolCommand::InitHistory { text } => {
            tracing::debug!(target: "tjuae_protocol", chars = text.len(), "已收到 InitHistory");
        }
        ProtocolCommand::SetMode { mode } => {
            let mode_str = format!("{mode:?}").to_lowercase();
            ctx.approval_manager.set_mode(mode);
            let _ = ctx.writer.emit(&ProtocolEvent::Info {
                msg_id: String::new(),
                message: format!("模式已更新：{}", ctx.approval_manager.current_mode()),
            });
            ctx.protocol_sink
                .emit_config_changed(engine.compat(), ctx.has_mcp, &ctx.approval_manager.current_mode());
            tracing::debug!(target: "tjuae_protocol", mode = %mode_str, "已应用 SetMode");
        }
        ProtocolCommand::SetConfig {
            model,
            image_input,
            thinking,
            thinking_budget,
            effort,
            compaction,
        } => {
            let changes = engine.apply_config_update(model, image_input, thinking, thinking_budget, effort, compaction);
            let message = if changes.is_empty() {
                "set_config：没有变更".to_string()
            } else {
                format!("配置已更新：{}", changes.join(", "))
            };
            let _ = ctx.writer.emit(&ProtocolEvent::Info {
                msg_id: String::new(),
                message,
            });
            ctx.protocol_sink
                .emit_config_changed(engine.compat(), ctx.has_mcp, &ctx.approval_manager.current_mode());
        }
        ProtocolCommand::AddMcpServer { name, .. } => {
            ctx.output.emit_error(&format!(
                "AddMcpServer '{name}'：已拒绝——只允许在第一条 Message 之前调用"
            ));
        }
        ProtocolCommand::Ping => {
            let _ = ctx.writer.emit(&ProtocolEvent::Pong);
        }
        ProtocolCommand::Message { .. } => {
            // `Message` is routed to `message::handle` by the caller before
            // reaching this dispatcher. Reaching here means the caller's
            // routing changed; log and ignore rather than panic.
            tracing::warn!(
                target: "tjuae_protocol",
                "Message 到达 dispatch::handle；预期应路由到 message::handle"
            );
        }
    }

    DispatchOutcome::Continue
}
