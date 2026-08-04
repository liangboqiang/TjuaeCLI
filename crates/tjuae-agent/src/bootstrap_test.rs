use super::*;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use serde_json::{Value, json};
    use tempfile::TempDir;
    use tjuae_config::config::{CliArgs, McpServerConfig, TransportType};
    use tjuae_protocol::events::ToolCategory;
    use tjuae_tools::Tool;
    use tjuae_types::runtime_asset::{
        RuntimeAssetRef, RuntimeAssetSnapshot, RuntimeBoundaryPhase, RuntimeBoundaryStatus, RuntimeMcpRef,
        RuntimeSkillRef,
    };
    use tjuae_types::tool::ToolResult;

    use crate::output::OutputSink;
    use crate::output::null_sink::NullSink;
    use crate::tool_policy::ToolPolicy;

    use super::*;

    struct DeferredTestTool(&'static str);

    #[async_trait]
    impl Tool for DeferredTestTool {
        fn name(&self) -> &str {
            self.0
        }

        fn description(&self) -> &str {
            "deferred test tool"
        }

        fn input_schema(&self) -> Value {
            json!({"type": "object"})
        }

        fn is_concurrency_safe(&self, _input: &Value) -> bool {
            true
        }

        async fn execute(&self, _input: Value) -> ToolResult {
            ToolResult {
                content: "unused".to_string(),
                is_error: false,
            }
        }

        fn category(&self) -> ToolCategory {
            ToolCategory::Info
        }

        fn is_deferred(&self) -> bool {
            true
        }
    }

    fn test_config() -> Config {
        Config::resolve(&CliArgs {
            provider: Some("anthropic".to_string()),
            api_key: Some("sk-test".to_string()),
            base_url: None,
            model: Some("claude-sonnet-4-20250514".to_string()),
            max_tokens: Some(4096),
            thinking: None,
            thinking_budget: None,
            max_turns: None,
            max_tool_call_malformed_turns: None,
            max_tool_call_failure_turns: None,
            system_prompt: None,
            profile: None,
            auto_approve: false,
            project_dir: None,
        })
        .unwrap()
    }

    fn runtime_asset(id: &str, kind: &str, digest: char) -> RuntimeAssetRef {
        RuntimeAssetRef {
            local_asset_id: id.into(),
            kind: kind.into(),
            local_definition_digest: format!("sha256-{}", digest.to_string().repeat(64)),
            runtime_content_digest: format!("sha256-{}", digest.to_string().repeat(64)),
            upstream_package: None,
            upstream_asset_id: None,
            upstream_version: None,
            upstream_revision: None,
        }
    }

    #[test]
    fn mcp_servers_with_runtime_env_uses_server_env_as_override() {
        let mut config = test_config();
        config.mcp.servers.insert(
            "stdio".to_string(),
            McpServerConfig {
                transport: TransportType::Stdio,
                command: Some("server".to_string()),
                args: None,
                env: Some(HashMap::from([
                    ("OVERRIDE".to_string(), "server".to_string()),
                    ("SERVER_ONLY".to_string(), "1".to_string()),
                ])),
                url: None,
                headers: None,
                deferred: None,
                startup_timeout_ms: None,
            },
        );

        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(config, "/tmp", output).runtime_env(vec![
            ("OVERRIDE".to_string(), "runtime".to_string()),
            ("RUNTIME_ONLY".to_string(), "1".to_string()),
        ]);

        let servers = bootstrap.mcp_servers_with_runtime_env();
        let env = servers
            .get("stdio")
            .and_then(|server| server.env.as_ref())
            .expect("stdio server env should exist");

        assert_eq!(env.get("OVERRIDE").map(String::as_str), Some("server"));
        assert_eq!(env.get("SERVER_ONLY").map(String::as_str), Some("1"));
        assert_eq!(env.get("RUNTIME_ONLY").map(String::as_str), Some("1"));
    }

    #[tokio::test]
    async fn tool_search_snapshot_excludes_policy_denied_tools() {
        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(test_config(), "/tmp", output)
            .tool_policy(ToolPolicy::allow_only(["ToolSearch", "AllowedDeferred"]));
        let mut registry = ToolRegistry::new();
        registry.register(Box::new(DeferredTestTool("AllowedDeferred")));
        registry.register(Box::new(DeferredTestTool("DeniedDeferred")));

        bootstrap.register_tool_search(&mut registry);

        let tool_search = registry.get("ToolSearch").expect("ToolSearch should be registered");
        let allowed = tool_search.execute(json!({"query": "AllowedDeferred"})).await;
        let denied = tool_search.execute(json!({"query": "DeniedDeferred"})).await;

        assert!(allowed.content.contains("AllowedDeferred"));
        assert!(denied.content.starts_with("未找到"));
        assert!(!denied.content.contains("\"name\": \"DeniedDeferred\""));
    }

    #[tokio::test]
    async fn managed_skill_bootstrap_receipt_uses_actual_digest_without_root() {
        let temp = TempDir::new().unwrap();
        let skill_root = temp.path().join("managed-skill");
        std::fs::create_dir_all(&skill_root).unwrap();
        std::fs::write(skill_root.join("SKILL.md"), "---\ndescription: managed\n---\nbody").unwrap();
        let managed = RuntimeSkillRef {
            asset: RuntimeAssetRef {
                local_asset_id: "local-managed-1".to_string(),
                kind: "skill".to_string(),
                local_definition_digest: format!("sha256-{}", "a".repeat(64)),
                runtime_content_digest: "stale".to_string(),
                upstream_package: None,
                upstream_asset_id: None,
                upstream_version: None,
                upstream_revision: None,
            },
            root: skill_root.clone(),
        };
        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(test_config(), temp.path().to_string_lossy(), output)
            .managed_skills("runtime-snapshot-1", vec![managed]);

        let loaded = bootstrap.load_skills(temp.path(), None).await.unwrap();
        let receipt = loaded
            .runtime_asset_snapshot
            .expect("managed bootstrap should return a receipt");

        assert_eq!(receipt.runtime_snapshot_id, "runtime-snapshot-1");
        assert_eq!(receipt.assets.len(), 1);
        assert_eq!(receipt.assets[0].local_asset_id, "local-managed-1");
        assert!(receipt.assets[0].runtime_content_digest.starts_with("sha256-"));
        let serialized = serde_json::to_string(&receipt).unwrap();
        assert!(!serialized.contains(&skill_root.to_string_lossy().to_string()));
    }

    #[tokio::test]
    async fn managed_skill_boundary_is_emitted_at_the_loader_without_sensitive_fields() {
        let temp = TempDir::new().unwrap();
        let skill_root = temp.path().join("managed-boundary-skill");
        std::fs::create_dir_all(&skill_root).unwrap();
        std::fs::write(
            skill_root.join("SKILL.md"),
            "---\ndescription: managed\n---\nsecret body",
        )
        .unwrap();
        let managed = RuntimeSkillRef {
            asset: runtime_asset("local-managed-boundary", "skill", 'a'),
            root: skill_root.clone(),
        };
        let records = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&records);
        let reporter: RuntimeBoundaryReporter = Arc::new(move |record| captured.lock().unwrap().push(record));
        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(test_config(), temp.path().to_string_lossy(), output)
            .managed_skills("runtime-snapshot-boundary", vec![managed])
            .runtime_boundary_reporter(reporter);

        bootstrap.load_skills(temp.path(), None).await.unwrap();

        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        let record = &records[0];
        assert_eq!(record.phase, RuntimeBoundaryPhase::Inject);
        assert_eq!(record.status, RuntimeBoundaryStatus::Succeeded);
        assert_eq!(record.asset_kind.as_deref(), Some("skill"));
        assert_eq!(record.local_asset_id.as_deref(), Some("local-managed-boundary"));
        assert!(record.error_code.is_none());
        assert!(record.ended_at_ms >= record.started_at_ms);
        let serialized = serde_json::to_string(record).unwrap();
        assert!(!serialized.contains(&skill_root.to_string_lossy().to_string()));
        assert!(!serialized.contains("secret body"));
    }

    #[tokio::test]
    async fn managed_skill_boundary_reports_stable_failure_before_returning_error() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&records);
        let reporter: RuntimeBoundaryReporter = Arc::new(move |record| captured.lock().unwrap().push(record));
        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(test_config(), "/tmp", output)
            .managed_skills(
                "",
                vec![RuntimeSkillRef {
                    asset: runtime_asset("invalid-snapshot-skill", "skill", 'b'),
                    root: PathBuf::from("redacted"),
                }],
            )
            .runtime_boundary_reporter(reporter);

        assert!(bootstrap.load_skills(Path::new("/tmp"), None).await.is_err());

        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase, RuntimeBoundaryPhase::Inject);
        assert_eq!(records[0].status, RuntimeBoundaryStatus::Failed);
        assert_eq!(
            records[0].error_code.as_deref(),
            Some("TJUAE_RUNTIME_SKILL_SNAPSHOT_INVALID")
        );
    }

    #[tokio::test]
    async fn managed_mcp_boundary_reports_missing_configuration_before_returning_error() {
        let records = Arc::new(Mutex::new(Vec::new()));
        let captured = Arc::clone(&records);
        let reporter: RuntimeBoundaryReporter = Arc::new(move |record| captured.lock().unwrap().push(record));
        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(test_config(), "/tmp", output)
            .managed_runtime_assets(
                "snapshot-mcp",
                Vec::new(),
                Vec::new(),
                Vec::new(),
                vec![RuntimeMcpRef {
                    asset: runtime_asset("mcp-missing", "mcp", 'c'),
                    server_name: "missing-server".into(),
                }],
            )
            .runtime_boundary_reporter(reporter);
        let mut registry = ToolRegistry::new();

        assert!(bootstrap.connect_mcp(&mut registry, &[]).await.is_err());

        let records = records.lock().unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase, RuntimeBoundaryPhase::Connect);
        assert_eq!(records[0].status, RuntimeBoundaryStatus::Failed);
        assert_eq!(records[0].asset_kind.as_deref(), Some("mcp"));
        assert_eq!(records[0].local_asset_id.as_deref(), Some("mcp-missing"));
        assert_eq!(
            records[0].error_code.as_deref(),
            Some("TJUAE_RUNTIME_MCP_CONFIGURATION_MISSING")
        );
    }

    #[test]
    fn runtime_snapshot_only_attests_live_engine_skill_and_mcp_assets() {
        let engine = runtime_asset("engine-a", "engineAdapter", 'a');
        let skill = runtime_asset("skill-a", "skill", 'b');
        let mcp = runtime_asset("mcp-a", "mcp", 'c');
        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(test_config(), "/tmp", output).managed_runtime_assets(
            "snapshot-a",
            Vec::new(),
            vec![engine.clone()],
            vec![RuntimeSkillRef {
                asset: skill.clone(),
                root: PathBuf::from("redacted"),
            }],
            vec![RuntimeMcpRef {
                asset: mcp.clone(),
                server_name: "docs".into(),
            }],
        );
        let receipt = bootstrap
            .runtime_asset_snapshot(
                Some(RuntimeAssetSnapshot {
                    runtime_snapshot_id: "snapshot-a".into(),
                    assets: vec![skill.clone()],
                }),
                &HashSet::from(["docs".to_owned()]),
            )
            .unwrap()
            .unwrap();

        assert_eq!(receipt.assets, vec![engine, mcp, skill]);
    }

    #[test]
    fn runtime_snapshot_fails_closed_when_mcp_connection_is_not_live() {
        let output: Arc<dyn OutputSink> = Arc::new(NullSink);
        let bootstrap = AgentBootstrap::new(test_config(), "/tmp", output).managed_runtime_assets(
            "snapshot-a",
            Vec::new(),
            Vec::new(),
            Vec::new(),
            vec![RuntimeMcpRef {
                asset: runtime_asset("mcp-a", "mcp", 'a'),
                server_name: "docs".into(),
            }],
        );

        assert!(bootstrap.runtime_asset_snapshot(None, &HashSet::new()).is_err());
    }
}
