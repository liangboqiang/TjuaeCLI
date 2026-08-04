use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::Result;
use tjuae_config::config::{Config, McpServerConfig};
use tjuae_config::shell::{ResolvedShell, resolve_shell_config};
use tjuae_mcp::manager::McpManager;
use tjuae_mcp::tool_proxy::register_mcp_tools;
use tjuae_memory::paths::{ENTRYPOINT_NAME, auto_memory_dir};
use tjuae_providers::{LlmProvider, create_provider};
use tjuae_skills::loader::load_all_skills_with_managed;
use tjuae_skills::permissions::SkillPermissionChecker;
use tjuae_skills::types::SkillMetadata;
use tjuae_tools::edit::EditTool;
use tjuae_tools::exec_command::ExecCommandTool;
use tjuae_tools::file_cache::FileStateCache;
use tjuae_tools::glob::GlobTool;
use tjuae_tools::grep::GrepTool;
use tjuae_tools::read::ReadTool;
use tjuae_tools::registry::ToolRegistry;
use tjuae_tools::tool_search::ToolSearchTool;
use tjuae_tools::view_image::ViewImageTool;
use tjuae_tools::write::WriteTool;
use tjuae_types::runtime_asset::{
    RuntimeAssetRef, RuntimeAssetSnapshot, RuntimeBoundaryPhase, RuntimeBoundaryRecord, RuntimeMcpRef, RuntimeSkillRef,
};
use tracing::info;

use crate::context::{SystemPromptCache, build_system_prompt_with_shell_and_tool_policy};
use crate::context_usage::PromptUsage;
use crate::engine::AgentEngine;
use crate::output::OutputSink;
use crate::plan::tools::{EnterPlanModeTool, ExitPlanModeTool};
use crate::session::Session;
use crate::skill_tool::SkillTool;
use crate::spawn_tool::SpawnTool;
use crate::spawner::AgentSpawner;
use crate::tool_policy::ToolPolicy;

fn runtime_now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| i64::try_from(duration.as_millis()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

/// Result of bootstrapping an agent engine with all features initialized.
pub struct BootstrapResult {
    // Fully initialized runtime.
    pub engine: AgentEngine,

    // Shared provider dependency created or reused during bootstrap.
    pub provider: Arc<dyn LlmProvider>,

    // MCP runtime state discovered during bootstrap.
    pub mcp_managers: Vec<Arc<McpManager>>,
    pub has_mcp: bool,

    // Exact managed asset definitions accepted by this bootstrap.
    pub runtime_asset_snapshot: Option<RuntimeAssetSnapshot>,
}

/// Process-local callback for safe runtime-boundary records.
///
/// The callback receives a closed record type that cannot contain commands,
/// paths, environment values, credentials, payloads, or free-form errors.
pub type RuntimeBoundaryReporter = Arc<dyn Fn(RuntimeBoundaryRecord) + Send + Sync>;

/// Builder for creating a fully-initialized `AgentEngine`.
///
/// Encapsulates the complete initialization pipeline so all consumers
/// (CLI, backend, sub-agents) get consistent behavior:
///
/// - System prompt always includes model identity, working directory, date
/// - Tool usage guidance is always injected
/// - AGENTS.md is loaded from the workspace hierarchy
/// - Skills, MCP, plan mode, spawn are enabled based on `Config` fields
pub struct AgentBootstrap {
    // Bootstrap configuration.
    config: Config,
    workspace: PathBuf,
    extra_skill_dirs: Vec<PathBuf>,
    managed_assets: Option<ManagedAssetsInput>,

    // Output integration.
    output: Arc<dyn OutputSink>,

    // Optional externally supplied runtime state.
    provider: Option<Arc<dyn LlmProvider>>,
    resume_session: Option<Session>,
    runtime_env: Vec<(String, String)>,
    tool_policy: ToolPolicy,
    runtime_boundary_reporter: Option<RuntimeBoundaryReporter>,
}

struct ManagedAssetsInput {
    runtime_snapshot_id: String,
    core_assets: Vec<RuntimeAssetRef>,
    runtime_assets: Vec<RuntimeAssetRef>,
    skills: Vec<RuntimeSkillRef>,
    mcps: Vec<RuntimeMcpRef>,
}

struct BootstrapSkills {
    metadata: Vec<SkillMetadata>,
    runtime_asset_snapshot: Option<RuntimeAssetSnapshot>,
}

struct BootstrapEnvironment {
    // Workspace context.
    workspace: PathBuf,

    // Prompt context.
    resolved_shell: ResolvedShell,
    memory_dir: Option<PathBuf>,
}

#[derive(Default)]
struct McpBootstrap {
    // Active manager used for MCP-backed skills.
    manager: Option<Arc<McpManager>>,

    // Managers retained by the caller for lifecycle ownership.
    managers: Vec<Arc<McpManager>>,
    connected_server_names: HashSet<String>,
}

impl McpBootstrap {
    fn has_mcp(&self) -> bool {
        self.manager.is_some()
    }
}

impl AgentBootstrap {
    pub fn new(config: Config, workspace: impl Into<String>, output: Arc<dyn OutputSink>) -> Self {
        Self {
            config,
            workspace: PathBuf::from(workspace.into()),
            extra_skill_dirs: Vec::new(),
            managed_assets: None,
            output,
            provider: None,
            resume_session: None,
            runtime_env: Vec::new(),
            tool_policy: ToolPolicy::default(),
            runtime_boundary_reporter: None,
        }
    }

    /// Use a pre-created provider instead of creating one from config.
    pub fn provider(mut self, provider: Arc<dyn LlmProvider>) -> Self {
        self.provider = Some(provider);
        self
    }

    /// Resume from a previously saved session.
    pub fn resume(mut self, session: Session) -> Self {
        self.resume_session = Some(session);
        self
    }

    /// Inject process environment for tools/hooks/MCP subprocesses owned by this engine.
    pub fn runtime_env(mut self, runtime_env: Vec<(String, String)>) -> Self {
        self.runtime_env = runtime_env;
        self
    }

    /// Restrict which registered tools can be advertised and executed.
    pub fn tool_policy(mut self, tool_policy: ToolPolicy) -> Self {
        self.tool_policy = tool_policy;
        self
    }

    /// Observe lifecycle boundaries at the operation that proves them.
    /// Reporting is best-effort and cannot change bootstrap behavior.
    pub fn runtime_boundary_reporter(mut self, reporter: RuntimeBoundaryReporter) -> Self {
        self.runtime_boundary_reporter = Some(reporter);
        self
    }

    /// Add extra directories to scan for skills.
    pub fn extra_skill_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.extra_skill_dirs = dirs;
        self
    }

    /// Load an exact set of Core-managed skills for one runtime snapshot.
    ///
    /// The supplied roots remain process-local and are not included in the
    /// returned snapshot receipt.
    pub fn managed_skills(mut self, runtime_snapshot_id: impl Into<String>, skills: Vec<RuntimeSkillRef>) -> Self {
        self.managed_assets = Some(ManagedAssetsInput {
            runtime_snapshot_id: runtime_snapshot_id.into(),
            core_assets: Vec::new(),
            runtime_assets: Vec::new(),
            skills,
            mcps: Vec::new(),
        });
        self
    }

    /// Require exact runtime receipts for runtime-owned assets, managed skills,
    /// and MCP servers that completed initialization and tool discovery.
    pub fn managed_runtime_assets(
        mut self,
        runtime_snapshot_id: impl Into<String>,
        core_assets: Vec<RuntimeAssetRef>,
        runtime_assets: Vec<RuntimeAssetRef>,
        skills: Vec<RuntimeSkillRef>,
        mcps: Vec<RuntimeMcpRef>,
    ) -> Self {
        self.managed_assets = Some(ManagedAssetsInput {
            runtime_snapshot_id: runtime_snapshot_id.into(),
            core_assets,
            runtime_assets,
            skills,
            mcps,
        });
        self
    }

    /// Read-only access to the config (for session management before build).
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Build the fully-initialized engine.
    pub async fn build(mut self) -> Result<BootstrapResult> {
        let workspace = self.resolve_workspace_path();
        let provider = self.resolve_provider();
        let environment = self.resolve_environment(workspace)?;
        let mut registry = self.build_builtin_registry(&environment.workspace);

        let builtin_names = registry.tool_names();
        let mcp = self.connect_mcp(&mut registry, &builtin_names).await?;

        let skills = self.load_skills(&environment.workspace, mcp.manager.as_deref()).await?;
        let handshake_started_at = runtime_now_ms();
        let runtime_asset_snapshot =
            match self.runtime_asset_snapshot(skills.runtime_asset_snapshot, &mcp.connected_server_names) {
                Ok(snapshot) => {
                    let ended_at = runtime_now_ms();
                    self.report_managed_assets(
                        RuntimeBoundaryPhase::Handshake,
                        handshake_started_at,
                        ended_at,
                        None,
                        |input| &input.runtime_assets,
                    );
                    snapshot
                }
                Err(error) => {
                    let ended_at = runtime_now_ms();
                    self.report_managed_assets(
                        RuntimeBoundaryPhase::Handshake,
                        handshake_started_at,
                        ended_at,
                        Some("TJUAE_RUNTIME_ENGINE_HANDSHAKE_FAILED"),
                        |input| &input.runtime_assets,
                    );
                    return Err(error);
                }
            };
        let inject_started_at = runtime_now_ms();
        if let Some(invalid) = self
            .managed_assets
            .as_ref()
            .and_then(|input| input.core_assets.iter().find(|asset| asset.kind != "assistant"))
        {
            let ended_at = runtime_now_ms();
            self.report_boundary(
                RuntimeBoundaryPhase::Inject,
                inject_started_at,
                ended_at,
                Some(invalid),
                Some("TJUAE_RUNTIME_ASSISTANT_KIND_INVALID"),
            );
            anyhow::bail!("Core-managed runtime asset kind is unsupported for prompt injection");
        }
        let prompt_usage = self.configure_system_prompt(&environment, &skills.metadata);
        let inject_ended_at = runtime_now_ms();
        self.report_managed_assets(
            RuntimeBoundaryPhase::Inject,
            inject_started_at,
            inject_ended_at,
            None,
            |input| &input.core_assets,
        );

        self.register_agent_tools(&mut registry, &provider, &environment.workspace, skills.metadata);
        let plan_active_flag = self.register_plan_tools(&mut registry);
        self.register_tool_search(&mut registry);

        let has_mcp = mcp.has_mcp();
        let mcp_managers = mcp.managers;
        let engine = self.into_engine(
            provider.clone(),
            registry,
            plan_active_flag,
            environment.workspace,
            prompt_usage,
        );

        Ok(BootstrapResult {
            engine,
            provider,
            mcp_managers,
            has_mcp,
            runtime_asset_snapshot,
        })
    }

    fn resolve_workspace_path(&self) -> PathBuf {
        info!(
            target: "tjuae_agent",
            workspace = %self.workspace.display(),
            "智能体引导：已解析工作区当前目录",
        );

        self.workspace.clone()
    }

    fn resolve_environment(&self, workspace_path: PathBuf) -> Result<BootstrapEnvironment> {
        Ok(BootstrapEnvironment {
            resolved_shell: resolve_shell_config(&self.config.shell)?,
            memory_dir: auto_memory_dir(&workspace_path),
            workspace: workspace_path,
        })
    }

    fn resolve_provider(&mut self) -> Arc<dyn LlmProvider> {
        self.provider.take().unwrap_or_else(|| create_provider(&self.config))
    }

    fn build_builtin_registry(&self, workspace_path: &Path) -> ToolRegistry {
        let file_cache = self.build_file_cache();
        let mut registry = ToolRegistry::new();

        registry.register(Box::new(ReadTool::new(file_cache.clone())));
        registry.register(Box::new(WriteTool::new(file_cache.clone())));
        registry.register(Box::new(EditTool::new(file_cache)));
        registry.register(Box::new(ExecCommandTool::new_with_env(
            workspace_path.to_path_buf(),
            self.runtime_env.clone(),
        )));
        registry.register(Box::new(GrepTool::new(workspace_path.to_path_buf())));
        registry.register(Box::new(GlobTool::new(workspace_path.to_path_buf())));
        registry.register(Box::new(ViewImageTool::new()));

        registry
    }

    fn build_file_cache(&self) -> Option<Arc<RwLock<FileStateCache>>> {
        self.config
            .file_cache
            .enabled
            .then(|| Arc::new(RwLock::new(FileStateCache::new(&self.config.file_cache))))
    }

    async fn connect_mcp(&self, registry: &mut ToolRegistry, builtin_names: &[String]) -> Result<McpBootstrap> {
        let started_at = runtime_now_ms();
        let server_configs = self.mcp_servers_with_runtime_env();
        if server_configs.is_empty() {
            if self.managed_assets.as_ref().is_some_and(|input| !input.mcps.is_empty()) {
                self.report_managed_mcps(
                    RuntimeBoundaryPhase::Connect,
                    started_at,
                    runtime_now_ms(),
                    Some("TJUAE_RUNTIME_MCP_CONFIGURATION_MISSING"),
                );
                anyhow::bail!("managed MCP assets were requested but no MCP server configuration is available");
            }
            return Ok(McpBootstrap::default());
        }

        let manager = match McpManager::connect_all(&server_configs).await {
            Ok(manager) => Arc::new(manager),
            Err(err) => {
                self.output.emit_error(&format!("MCP 初始化错误：{err}"));
                if self.managed_assets.as_ref().is_some_and(|input| !input.mcps.is_empty()) {
                    self.report_managed_mcps(
                        RuntimeBoundaryPhase::Connect,
                        started_at,
                        runtime_now_ms(),
                        Some("TJUAE_RUNTIME_MCP_CONNECT_FAILED"),
                    );
                    return Err(err.into());
                }
                return Ok(McpBootstrap::default());
            }
        };

        let connected_server_names = manager.server_names().into_iter().collect::<HashSet<_>>();
        if let Some(input) = self.managed_assets.as_ref() {
            let mut expected_names = HashSet::new();
            for mcp in &input.mcps {
                if mcp.asset.kind != "mcp" || mcp.server_name.trim().is_empty() {
                    self.report_boundary(
                        RuntimeBoundaryPhase::Connect,
                        started_at,
                        runtime_now_ms(),
                        Some(&mcp.asset),
                        Some("TJUAE_RUNTIME_MCP_BINDING_INVALID"),
                    );
                    anyhow::bail!("managed MCP asset binding is invalid");
                }
                if !expected_names.insert(mcp.server_name.as_str()) {
                    self.report_boundary(
                        RuntimeBoundaryPhase::Connect,
                        started_at,
                        runtime_now_ms(),
                        Some(&mcp.asset),
                        Some("TJUAE_RUNTIME_MCP_BINDING_DUPLICATED"),
                    );
                    anyhow::bail!("managed MCP server binding is duplicated");
                }
                if !connected_server_names.contains(&mcp.server_name) {
                    self.report_boundary(
                        RuntimeBoundaryPhase::Connect,
                        started_at,
                        runtime_now_ms(),
                        Some(&mcp.asset),
                        Some("TJUAE_RUNTIME_MCP_NOT_CONNECTED"),
                    );
                    anyhow::bail!("managed MCP server did not complete initialization and tool discovery");
                }
            }
        }

        register_mcp_tools(registry, &manager, builtin_names, &server_configs);
        self.report_managed_mcps(RuntimeBoundaryPhase::Connect, started_at, runtime_now_ms(), None);

        Ok(McpBootstrap {
            manager: Some(Arc::clone(&manager)),
            managers: vec![manager],
            connected_server_names,
        })
    }

    fn mcp_servers_with_runtime_env(&self) -> HashMap<String, McpServerConfig> {
        let mut servers = self.config.mcp.servers.clone();
        if self.runtime_env.is_empty() {
            return servers;
        }

        for server in servers.values_mut() {
            let mut env: HashMap<String, String> = self.runtime_env.clone().into_iter().collect();
            if let Some(server_env) = server.env.take() {
                env.extend(server_env);
            }
            server.env = Some(env);
        }

        servers
    }

    async fn load_skills(&self, workspace: &Path, mcp_manager: Option<&McpManager>) -> Result<BootstrapSkills> {
        let started_at = runtime_now_ms();
        if self
            .managed_assets
            .as_ref()
            .is_some_and(|input| input.runtime_snapshot_id.trim().is_empty())
        {
            self.report_managed_skills(
                RuntimeBoundaryPhase::Inject,
                started_at,
                runtime_now_ms(),
                Some("TJUAE_RUNTIME_SKILL_SNAPSHOT_INVALID"),
            );
            anyhow::bail!("runtime snapshot id must not be empty");
        }
        let managed_skills = self
            .managed_assets
            .as_ref()
            .map(|input| input.skills.as_slice())
            .unwrap_or_default();
        let loaded =
            match load_all_skills_with_managed(workspace, &self.extra_skill_dirs, false, mcp_manager, managed_skills)
                .await
            {
                Ok(loaded) => loaded,
                Err(error) => {
                    self.report_managed_skills(
                        RuntimeBoundaryPhase::Inject,
                        started_at,
                        runtime_now_ms(),
                        Some("TJUAE_RUNTIME_SKILL_LOAD_FAILED"),
                    );
                    return Err(error.into());
                }
            };
        if managed_skills.iter().any(|skill| {
            !loaded.runtime_assets.iter().any(|asset| {
                asset.kind == skill.asset.kind
                    && asset.local_asset_id == skill.asset.local_asset_id
                    && asset.local_definition_digest == skill.asset.local_definition_digest
            })
        }) {
            self.report_managed_skills(
                RuntimeBoundaryPhase::Inject,
                started_at,
                runtime_now_ms(),
                Some("TJUAE_RUNTIME_SKILL_RECEIPT_INCOMPLETE"),
            );
            anyhow::bail!("managed skill loader receipt is incomplete");
        }
        let runtime_asset_snapshot = self
            .managed_assets
            .as_ref()
            .filter(|input| !input.skills.is_empty())
            .map(|input| RuntimeAssetSnapshot {
                runtime_snapshot_id: input.runtime_snapshot_id.clone(),
                assets: loaded.runtime_assets,
            });
        self.report_managed_skills(RuntimeBoundaryPhase::Inject, started_at, runtime_now_ms(), None);

        Ok(BootstrapSkills {
            metadata: loaded.skills,
            runtime_asset_snapshot,
        })
    }

    fn report_managed_skills(
        &self,
        phase: RuntimeBoundaryPhase,
        started_at_ms: i64,
        ended_at_ms: i64,
        error_code: Option<&'static str>,
    ) {
        let Some(input) = self.managed_assets.as_ref() else {
            return;
        };
        for skill in &input.skills {
            self.report_boundary(phase, started_at_ms, ended_at_ms, Some(&skill.asset), error_code);
        }
    }

    fn report_managed_mcps(
        &self,
        phase: RuntimeBoundaryPhase,
        started_at_ms: i64,
        ended_at_ms: i64,
        error_code: Option<&'static str>,
    ) {
        let Some(input) = self.managed_assets.as_ref() else {
            return;
        };
        for mcp in &input.mcps {
            self.report_boundary(phase, started_at_ms, ended_at_ms, Some(&mcp.asset), error_code);
        }
    }

    fn report_managed_assets<'a>(
        &'a self,
        phase: RuntimeBoundaryPhase,
        started_at_ms: i64,
        ended_at_ms: i64,
        error_code: Option<&'static str>,
        select: impl FnOnce(&'a ManagedAssetsInput) -> &'a [RuntimeAssetRef],
    ) {
        let Some(input) = self.managed_assets.as_ref() else {
            return;
        };
        for asset in select(input) {
            self.report_boundary(phase, started_at_ms, ended_at_ms, Some(asset), error_code);
        }
    }

    fn report_boundary(
        &self,
        phase: RuntimeBoundaryPhase,
        started_at_ms: i64,
        ended_at_ms: i64,
        asset: Option<&RuntimeAssetRef>,
        error_code: Option<&'static str>,
    ) {
        let Some(reporter) = self.runtime_boundary_reporter.as_ref() else {
            return;
        };
        let record = match error_code {
            Some(code) => RuntimeBoundaryRecord::failed(phase, started_at_ms, ended_at_ms, asset, code),
            None => RuntimeBoundaryRecord::succeeded(phase, started_at_ms, ended_at_ms, asset),
        };
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| reporter(record)));
    }

    fn runtime_asset_snapshot(
        &self,
        skill_snapshot: Option<RuntimeAssetSnapshot>,
        connected_server_names: &HashSet<String>,
    ) -> Result<Option<RuntimeAssetSnapshot>> {
        let Some(input) = self.managed_assets.as_ref() else {
            if skill_snapshot.is_some() {
                anyhow::bail!("runtime returned an unexpected managed skill receipt");
            }
            return Ok(None);
        };
        if input.runtime_assets.is_empty() && input.skills.is_empty() && input.mcps.is_empty() {
            if skill_snapshot.is_some() {
                anyhow::bail!("runtime returned an unexpected managed skill receipt");
            }
            return Ok(None);
        }
        let mut assets = input.runtime_assets.clone();
        for asset in &assets {
            if asset.kind != "engineAdapter" {
                anyhow::bail!("runtime-owned asset receipt kind is unsupported");
            }
        }
        if let Some(snapshot) = skill_snapshot {
            if snapshot.runtime_snapshot_id != input.runtime_snapshot_id {
                anyhow::bail!("managed skill receipt snapshot id does not match the request");
            }
            assets.extend(snapshot.assets);
        }
        for mcp in &input.mcps {
            if !connected_server_names.contains(&mcp.server_name) {
                anyhow::bail!("managed MCP server receipt is not backed by a live connection");
            }
            assets.push(mcp.asset.clone());
        }
        assets.sort_by(|left, right| (&left.kind, &left.local_asset_id).cmp(&(&right.kind, &right.local_asset_id)));
        Ok(Some(RuntimeAssetSnapshot {
            runtime_snapshot_id: input.runtime_snapshot_id.clone(),
            assets,
        }))
    }

    fn configure_system_prompt(&mut self, environment: &BootstrapEnvironment, skills: &[SkillMetadata]) -> PromptUsage {
        let mut prompt_cache = SystemPromptCache::new();
        let workspace = self.workspace.to_string_lossy();
        let system_prompt = build_system_prompt_with_shell_and_tool_policy(
            &mut prompt_cache,
            self.config.system_prompt.as_deref(),
            &workspace,
            &self.config.model,
            &environment.resolved_shell,
            skills,
            None,
            environment.memory_dir.as_deref(),
            false,
            self.config.compact.toon,
            &self.tool_policy,
        );
        let memory_prompt = prompt_cache.sections.get("memory").map(String::as_str);
        let skills_prompt = prompt_cache.sections.get("skills").map(String::as_str);
        let memory_files = if memory_prompt.is_some() {
            environment
                .memory_dir
                .as_ref()
                .map(|directory| directory.join(ENTRYPOINT_NAME))
                .filter(|path| path.is_file())
                .map(|path| path.display().to_string())
                .into_iter()
                .collect()
        } else {
            Vec::new()
        };
        let visible_skills = skills
            .iter()
            .filter(|skill| self.tool_policy.allows("Skill") && !skill.disable_model_invocation)
            .map(|skill| skill.name.clone())
            .collect();
        let prompt_usage = PromptUsage::from_sections(
            &system_prompt,
            memory_prompt,
            skills_prompt,
            memory_files,
            visible_skills,
        );
        self.config.system_prompt = Some(system_prompt);
        prompt_usage
    }

    fn register_agent_tools(
        &self,
        registry: &mut ToolRegistry,
        provider: &Arc<dyn LlmProvider>,
        workspace: &Path,
        skills: Vec<SkillMetadata>,
    ) {
        let skill_checker = SkillPermissionChecker::new(
            self.config.tools.skills.deny.clone(),
            self.config.tools.skills.allow.clone(),
            self.config.tools.auto_approve,
        );
        registry.register(Box::new(SkillTool::new(
            Arc::new(skills),
            self.workspace.to_path_buf(),
            skill_checker,
        )));

        let spawner = AgentSpawner::new_with_env(
            Arc::clone(provider),
            self.config.clone(),
            workspace.to_path_buf(),
            self.runtime_env.clone(),
            self.tool_policy.clone(),
        );
        registry.register(Box::new(SpawnTool::new(Arc::new(spawner))));
    }

    fn register_plan_tools(&self, registry: &mut ToolRegistry) -> Arc<AtomicBool> {
        let plan_active_flag = Arc::new(AtomicBool::new(false));

        if self.config.plan.enabled {
            registry.register(Box::new(EnterPlanModeTool::new(Arc::clone(&plan_active_flag))));
            registry.register(Box::new(ExitPlanModeTool::new(Arc::clone(&plan_active_flag))));
        }

        plan_active_flag
    }

    fn register_tool_search(&self, registry: &mut ToolRegistry) {
        let tool_defs_snapshot = registry.to_tool_defs_filtered(|tool| self.tool_policy.allows(tool.name()));
        registry.register(Box::new(ToolSearchTool::new(tool_defs_snapshot)));
    }

    fn into_engine(
        self,
        provider: Arc<dyn LlmProvider>,
        registry: ToolRegistry,
        plan_active_flag: Arc<AtomicBool>,
        workspace: PathBuf,
        prompt_usage: PromptUsage,
    ) -> AgentEngine {
        let runtime_env = self.runtime_env.clone();
        let mut engine = if let Some(session) = self.resume_session {
            AgentEngine::resume_with_provider_and_env(
                provider,
                self.config,
                registry,
                self.output,
                session,
                workspace,
                runtime_env,
            )
        } else {
            AgentEngine::new_with_provider_and_env(provider, self.config, registry, self.output, workspace, runtime_env)
        };
        engine.set_plan_active_flag(plan_active_flag);
        engine.set_tool_policy(self.tool_policy);
        engine.set_prompt_usage(prompt_usage);
        engine
    }
}

#[cfg(test)]
#[path = "bootstrap_test.rs"]
mod bootstrap_test;
