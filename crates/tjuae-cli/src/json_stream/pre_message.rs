//! Pre-message phase of the JSON stream protocol.
//!
//! Before the first `Message` command arrives, the host is allowed to send
//! any number of `AddMcpServer` commands to connect additional MCP servers
//! up front. This phase drains those (and only those) commands, then hands
//! off the first non-`AddMcpServer` command it sees to the main dispatch
//! loop in `session.rs`.

use std::collections::HashMap;
use std::sync::Arc;

use tjuae_agent::engine::AgentEngine;
use tjuae_agent::output::OutputSink;
use tjuae_config::config::{McpServerConfig, TransportType};
use tjuae_mcp::manager::McpManager;
use tjuae_mcp::tool_proxy::register_single_server_tools;
use tjuae_protocol::commands::ProtocolCommand;
use tjuae_protocol::events::ProtocolEvent;
use tjuae_protocol::writer::{ProtocolEmitter, ProtocolWriter};
use tokio::sync::mpsc::UnboundedReceiver;

fn to_mcp_server_config(
    transport: &str,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<HashMap<String, String>>,
    url: Option<String>,
    headers: Option<HashMap<String, String>>,
) -> Result<McpServerConfig, String> {
    let transport_type = match transport {
        "stdio" => TransportType::Stdio,
        "sse" => TransportType::Sse,
        "streamable-http" | "streamable_http" => TransportType::StreamableHttp,
        other => return Err(format!("未知传输类型：{other}")),
    };
    Ok(McpServerConfig {
        transport: transport_type,
        command,
        args,
        env,
        url,
        headers,
        deferred: Some(false),
        startup_timeout_ms: None,
    })
}

/// Outcome of draining the pre-message phase.
pub(super) enum PreMessageOutcome {
    /// A `Stop` command was received before any `Message` — the caller
    /// should shut down immediately without entering the main loop.
    Stop,
    /// The phase ended because a non-`AddMcpServer` command arrived (or the
    /// channel closed). Carries any MCP managers connected during the phase
    /// plus the command that ended it (`None` if the channel closed).
    Continue {
        dynamic_managers: Vec<Arc<McpManager>>,
        next_command: Option<Box<ProtocolCommand>>,
    },
}

/// Drain `AddMcpServer` commands until a `Stop` or a different command
/// arrives.
pub(super) async fn run(
    cmd_rx: &mut UnboundedReceiver<ProtocolCommand>,
    engine: &mut AgentEngine,
    output: &Arc<dyn OutputSink>,
    writer: &Arc<ProtocolWriter>,
) -> PreMessageOutcome {
    let mut dynamic_managers: Vec<Arc<McpManager>> = Vec::new();

    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            ProtocolCommand::AddMcpServer {
                name,
                transport,
                command,
                args,
                env,
                url,
                headers,
            } => {
                tracing::info!(target: "tjuae_mcp", %name, %transport, ?command, "已收到 AddMcpServer");
                let config = match to_mcp_server_config(&transport, command, args, env, url, headers) {
                    Ok(c) => c,
                    Err(e) => {
                        output.emit_error(&format!("AddMcpServer '{name}' 配置无效：{e}"));
                        continue;
                    }
                };

                let mut single_configs = HashMap::new();
                single_configs.insert(name.clone(), config.clone());
                tracing::info!(target: "tjuae_mcp", %name, "正在连接 MCP 服务器");
                match McpManager::connect_all(&single_configs).await {
                    Ok(mgr) => {
                        let tool_names: Vec<String> = mgr.all_tools().iter().map(|(_, t)| t.name.clone()).collect();
                        tracing::info!(target: "tjuae_mcp", %name, tools = tool_names.len(), "MCP 服务器已连接");
                        let mgr_arc = Arc::new(mgr);
                        let builtin_names = engine.tool_names();
                        register_single_server_tools(
                            engine.registry_mut(),
                            &mgr_arc,
                            &name,
                            &builtin_names,
                            config.deferred.unwrap_or(true),
                        );
                        dynamic_managers.push(mgr_arc);
                        let _ = writer.emit(&ProtocolEvent::McpReady {
                            name,
                            tools: tool_names,
                        });
                    }
                    Err(e) => {
                        tracing::warn!(target: "tjuae_mcp", %name, error = %e, "MCP 服务器连接失败");
                        output.emit_error(&format!("AddMcpServer '{name}' 失败：{e}"));
                    }
                }
            }
            ProtocolCommand::Stop => return PreMessageOutcome::Stop,
            other => {
                return PreMessageOutcome::Continue {
                    dynamic_managers,
                    next_command: Some(Box::new(other)),
                };
            }
        }
    }

    PreMessageOutcome::Continue {
        dynamic_managers,
        next_command: None,
    }
}
