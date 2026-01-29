//! Configurable server discovery pipeline
//!
//! Provides a modular, ordered pipeline for:
//! - candidate discovery (registry/LLM)
//! - introspection (MCP/OpenAPI/Browser)
//! - staging (RTFS artifact generation)
//! - approvals (UnifiedApprovalQueue)

use async_trait::async_trait;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::Path;
use std::sync::Arc;

use crate::approval::{
    queue::{DiscoverySource, RiskAssessment, RiskLevel, ServerInfo},
    storage_file::FileApprovalStorage,
    UnifiedApprovalQueue,
};
use crate::config::types::{
    AgentConfig, MissingCapabilityRuntimeConfig, ServerDiscoveryPipelineConfig,
    ServerDiscoverySourcesConfig,
};
use crate::discovery::llm_discovery::{ExternalApiResult, LlmDiscoveryService};
use crate::discovery::registry_search::{
    DiscoveryCategory, RegistrySearchResult, RegistrySearcher,
};
use crate::mcp::core::MCPDiscoveryService;
use crate::ops::browser_discovery::BrowserDiscoveryService;
use crate::ops::introspection_service::{
    IntrospectionResult, IntrospectionService, IntrospectionSource, RtfsGenerationResult,
};
use crate::utils::fs::{resolve_workspace_path, sanitize_filename};

/// Pipeline target input
#[derive(Debug, Clone)]
pub struct PipelineTarget {
    pub input: String,
    pub name: Option<String>,
    pub auth_env_var: Option<String>,
}

/// Query context for discovery stages
#[derive(Debug, Clone)]
pub struct DiscoveryQueryContext<'a> {
    pub query: &'a str,
    pub url_hint: Option<&'a str>,
    pub config: &'a ServerDiscoveryPipelineConfig,
}

/// Introspection context for introspection stages
#[derive(Debug, Clone)]
pub struct IntrospectionContext<'a> {
    pub target: &'a str,
    pub name: Option<String>,
    pub auth_env_var: Option<String>,
    pub config: &'a ServerDiscoveryPipelineConfig,
}

/// Staging context
#[derive(Debug, Clone)]
pub struct StagingContext<'a> {
    pub target: &'a str,
    pub server_name: &'a str,
    pub pending_base: &'a Path,
    pub result: &'a IntrospectionResult,
}

/// Approval context
#[derive(Debug, Clone)]
pub struct ApprovalContext<'a> {
    pub target: &'a str,
    pub server_name: &'a str,
    pub auth_env_var: Option<String>,
    pub result: &'a IntrospectionResult,
    pub capabilities_path: &'a str,
    pub capability_files: &'a [String],
    pub approvals_config: &'a crate::config::types::ServerDiscoveryApprovalsConfig,
}

/// Discovery candidate preview
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryEndpointPreview {
    pub id: String,
    pub name: String,
    pub method: Option<String>,
    pub path: Option<String>,
}

/// Introspection preview output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryPreviewResult {
    pub source: String,
    pub server_name: String,
    pub endpoints: Vec<DiscoveryEndpointPreview>,
    pub manifests_count: usize,
}

/// Stage-and-queue output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscoveryQueueResult {
    pub approval_id: String,
    pub pending_dir: String,
    pub capability_files: Vec<String>,
    pub source: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<DiscoveryPreviewResult>,
}

/// Discovery stage trait (query → candidates)
#[async_trait]
pub trait DiscoveryStage: Send + Sync {
    fn name(&self) -> &str;
    async fn run(
        &self,
        ctx: &DiscoveryQueryContext<'_>,
    ) -> RuntimeResult<Vec<RegistrySearchResult>>;
}

/// Introspection stage trait (target → introspection result)
#[async_trait]
pub trait IntrospectionStage: Send + Sync {
    fn name(&self) -> &str;
    async fn run(&self, ctx: &IntrospectionContext<'_>) -> RuntimeResult<IntrospectionResult>;
}

/// Staging stage trait (introspection → RTFS artifacts)
#[async_trait]
pub trait StagingStage: Send + Sync {
    fn name(&self) -> &str;
    async fn stage(&self, ctx: &StagingContext<'_>) -> RuntimeResult<RtfsGenerationResult>;
}

/// Approval stage trait (artifacts → approval id)
#[async_trait]
pub trait ApprovalStage: Send + Sync {
    fn name(&self) -> &str;
    async fn approve(&self, ctx: &ApprovalContext<'_>) -> RuntimeResult<String>;
}

/// Registry search stage (multi-source)
pub struct RegistrySearchStage {
    searcher: Arc<RegistrySearcher>,
    sources: ServerDiscoverySourcesConfig,
    missing_caps: MissingCapabilityRuntimeConfig,
}

impl RegistrySearchStage {
    pub fn new(
        searcher: Arc<RegistrySearcher>,
        sources: ServerDiscoverySourcesConfig,
        missing_caps: MissingCapabilityRuntimeConfig,
    ) -> Self {
        Self {
            searcher,
            sources,
            missing_caps,
        }
    }

    fn web_search_enabled(&self) -> bool {
        if let Some(enabled) = self.sources.web_search {
            return enabled;
        }
        self.missing_caps.web_search.unwrap_or(false)
    }

    fn filter_sources(&self, results: Vec<RegistrySearchResult>) -> Vec<RegistrySearchResult> {
        results
            .into_iter()
            .filter(|r| match r.source {
                DiscoverySource::McpRegistry { .. } => self.sources.mcp_registry,
                DiscoverySource::NpmRegistry { .. } => self.sources.npm,
                DiscoverySource::LocalOverride { .. } => self.sources.overrides,
                DiscoverySource::ApisGuru { .. } => self.sources.apis_guru,
                DiscoverySource::WebSearch { .. } => self.web_search_enabled(),
                DiscoverySource::LlmSuggestion { .. } => self.sources.llm_suggest,
                DiscoverySource::LocalConfig => self.sources.known_apis,
                _ => true,
            })
            .collect()
    }
}

#[async_trait]
impl DiscoveryStage for RegistrySearchStage {
    fn name(&self) -> &str {
        "registry_search"
    }

    async fn run(
        &self,
        ctx: &DiscoveryQueryContext<'_>,
    ) -> RuntimeResult<Vec<RegistrySearchResult>> {
        if !(self.sources.mcp_registry
            || self.sources.npm
            || self.sources.overrides
            || self.sources.apis_guru
            || self.web_search_enabled())
        {
            return Ok(vec![]);
        }
        let results = self.searcher.search(ctx.query).await?;
        crate::ccos_println!(
            "🔍 RegistrySearchStage found {} raw results from searcher for query '{}'",
            results.len(),
            ctx.query
        );
        for (i, r) in results.iter().enumerate() {
            crate::ccos_println!(
                "  [{}] Source: {:?}, Name: {}",
                i,
                r.source,
                r.server_info.name
            );
        }

        let filtered = self.filter_sources(results);
        crate::ccos_println!(
            "🔍 RegistrySearchStage after filter_sources: {} results",
            filtered.len()
        );
        Ok(filtered)
    }
}

/// LLM-based suggestion stage (query → candidates)
pub struct LlmSuggestStage {
    llm_discovery: Option<Arc<LlmDiscoveryService>>,
}

impl LlmSuggestStage {
    pub fn new(llm_discovery: Option<Arc<LlmDiscoveryService>>) -> Self {
        Self { llm_discovery }
    }

    fn to_registry_result(api: ExternalApiResult) -> RegistrySearchResult {
        let primary_endpoint = api.docs_url.clone().unwrap_or_else(|| api.endpoint.clone());
        let mut alternatives = Vec::new();
        if api.docs_url.is_some() && api.endpoint != primary_endpoint {
            alternatives.push(api.endpoint.clone());
        }
        RegistrySearchResult {
            source: DiscoverySource::LlmSuggestion {
                name: api.name.clone(),
            },
            server_info: ServerInfo {
                name: api.name,
                endpoint: primary_endpoint,
                description: Some(api.description),
                auth_env_var: api.auth_env_var,
                capabilities_path: None,
                alternative_endpoints: alternatives,
                capability_files: None,
            },
            match_score: 0.7,
            category: DiscoveryCategory::WebDoc,
            alternative_endpoints: Vec::new(),
        }
    }
}

#[async_trait]
impl DiscoveryStage for LlmSuggestStage {
    fn name(&self) -> &str {
        "llm_suggest"
    }

    async fn run(
        &self,
        ctx: &DiscoveryQueryContext<'_>,
    ) -> RuntimeResult<Vec<RegistrySearchResult>> {
        let Some(service) = &self.llm_discovery else {
            return Ok(vec![]);
        };
        crate::ccos_println!(
            "🔍 LlmSuggestStage: calling search_external_apis for '{}'...",
            ctx.query
        );
        match service.search_external_apis(ctx.query, ctx.url_hint).await {
            Ok(results) => {
                crate::ccos_println!("🔍 LlmSuggestStage: found {} suggestions.", results.len());
                Ok(results.into_iter().map(Self::to_registry_result).collect())
            }
            Err(e) => {
                crate::ccos_println!("❌ LlmSuggestStage: failed: {}", e);
                Err(e)
            }
        }
    }
}

/// Introspection stage (MCP)
pub struct McpIntrospectionStage {
    mcp_discovery: Arc<MCPDiscoveryService>,
    browser_discovery: Arc<BrowserDiscoveryService>,
}

impl McpIntrospectionStage {
    pub fn new(
        mcp_discovery: Arc<MCPDiscoveryService>,
        browser_discovery: Arc<BrowserDiscoveryService>,
    ) -> Self {
        Self {
            mcp_discovery,
            browser_discovery,
        }
    }
}

#[async_trait]
impl IntrospectionStage for McpIntrospectionStage {
    fn name(&self) -> &str {
        "mcp"
    }

    async fn run(&self, ctx: &IntrospectionContext<'_>) -> RuntimeResult<IntrospectionResult> {
        let name = ctx.name.clone();
        let target = ctx.target;
        let auth_token = ctx
            .auth_env_var
            .as_deref()
            .and_then(|env_var| std::env::var(env_var).ok());
        let output_dir = resolve_workspace_path(&ctx.config.staging.pending_subdir);
        let introspection_service =
            IntrospectionService::empty().with_browser_discovery(self.browser_discovery.clone());
        introspection_service
            .introspect_mcp(target, name, auth_token, &self.mcp_discovery, &output_dir)
            .await
    }
}

/// Introspection stage (OpenAPI)
pub struct OpenApiIntrospectionStage {
    introspection_service: IntrospectionService,
}

impl OpenApiIntrospectionStage {
    pub fn new(introspection_service: IntrospectionService) -> Self {
        Self {
            introspection_service,
        }
    }
}

#[async_trait]
impl IntrospectionStage for OpenApiIntrospectionStage {
    fn name(&self) -> &str {
        "openapi"
    }

    async fn run(&self, ctx: &IntrospectionContext<'_>) -> RuntimeResult<IntrospectionResult> {
        let server_name = ctx
            .name
            .clone()
            .unwrap_or_else(|| sanitize_filename(ctx.target));
        self.introspection_service
            .introspect_openapi(ctx.target, &server_name)
            .await
    }
}

/// Introspection stage (Browser docs)
pub struct BrowserIntrospectionStage {
    introspection_service: IntrospectionService,
}

impl BrowserIntrospectionStage {
    pub fn new(introspection_service: IntrospectionService) -> Self {
        Self {
            introspection_service,
        }
    }
}

#[async_trait]
impl IntrospectionStage for BrowserIntrospectionStage {
    fn name(&self) -> &str {
        "browser"
    }

    async fn run(&self, ctx: &IntrospectionContext<'_>) -> RuntimeResult<IntrospectionResult> {
        let server_name = ctx
            .name
            .clone()
            .unwrap_or_else(|| sanitize_filename(ctx.target));
        self.introspection_service
            .introspect_browser(ctx.target, &server_name)
            .await
    }
}

/// Default staging implementation
pub struct DefaultStagingStage {
    mcp_discovery: Arc<MCPDiscoveryService>,
}

impl DefaultStagingStage {
    pub fn new(mcp_discovery: Arc<MCPDiscoveryService>) -> Self {
        Self { mcp_discovery }
    }
}

#[async_trait]
impl StagingStage for DefaultStagingStage {
    fn name(&self) -> &str {
        "stage_rtfs"
    }

    async fn stage(&self, ctx: &StagingContext<'_>) -> RuntimeResult<RtfsGenerationResult> {
        let pending_dir = ctx.pending_base.join(sanitize_filename(ctx.server_name));
        match ctx.result.source {
            IntrospectionSource::Mcp | IntrospectionSource::McpStdio => {
                let server_config = crate::capability_marketplace::mcp_discovery::MCPServerConfig {
                    name: ctx.server_name.to_string(),
                    endpoint: ctx.target.to_string(),
                    auth_token: None,
                    timeout_seconds: 30,
                    protocol_version: "2024-11-05".to_string(),
                };
                let capability_files = self.mcp_discovery.export_manifests_to_rtfs_layout(
                    &server_config,
                    &ctx.result.manifests,
                    ctx.pending_base,
                )?;
                let server_rtfs_path = pending_dir.join("server.rtfs");
                Ok(RtfsGenerationResult {
                    output_dir: pending_dir,
                    capability_files,
                    server_json_path: server_rtfs_path,
                })
            }
            _ => {
                let introspection_service = IntrospectionService::empty();
                introspection_service.generate_rtfs_files(ctx.result, &pending_dir, ctx.target)
            }
        }
    }
}

/// Default approval implementation
pub struct DefaultApprovalStage {
    approval_queue: UnifiedApprovalQueue<FileApprovalStorage>,
}

impl DefaultApprovalStage {
    pub fn new(approval_queue: UnifiedApprovalQueue<FileApprovalStorage>) -> Self {
        Self { approval_queue }
    }

    fn risk_level_from_label(label: &str) -> RiskLevel {
        match label.to_ascii_lowercase().as_str() {
            "low" => RiskLevel::Low,
            "high" => RiskLevel::High,
            _ => RiskLevel::Medium,
        }
    }
}

#[async_trait]
impl ApprovalStage for DefaultApprovalStage {
    fn name(&self) -> &str {
        "approval_queue"
    }

    async fn approve(&self, ctx: &ApprovalContext<'_>) -> RuntimeResult<String> {
        let risk = Self::risk_level_from_label(&ctx.approvals_config.risk_default);
        let capabilities_path = ctx.capabilities_path.to_string();
        let capability_files = ctx.capability_files.to_vec();
        let server_info = ServerInfo {
            name: ctx.server_name.to_string(),
            endpoint: ctx.target.to_string(),
            description: Some(format!("Discovered via pipeline ({:?})", ctx.result.source)),
            auth_env_var: ctx.auth_env_var.clone(),
            capabilities_path: Some(capabilities_path),
            alternative_endpoints: vec![],
            capability_files: Some(capability_files),
        };
        let source = match ctx.result.source {
            IntrospectionSource::OpenApi => DiscoverySource::OpenApi {
                url: ctx.target.to_string(),
            },
            IntrospectionSource::Browser | IntrospectionSource::HtmlDocs => {
                DiscoverySource::HtmlDocs {
                    url: ctx.target.to_string(),
                }
            }
            IntrospectionSource::Mcp | IntrospectionSource::McpStdio => DiscoverySource::Mcp {
                endpoint: ctx.target.to_string(),
            },
            _ => DiscoverySource::Manual {
                user: "pipeline".to_string(),
            },
        };
        let approval_id = self
            .approval_queue
            .add_server_discovery(
                source,
                server_info,
                vec!["pipeline".to_string()],
                RiskAssessment {
                    level: risk,
                    reasons: vec!["server_discovery_pipeline".to_string()],
                },
                None,
                ctx.approvals_config.expiry_hours,
            )
            .await?;

        // Create approval_link.json to link filesystem artifacts to approval ID
        let base_path = std::path::Path::new(ctx.capabilities_path);
        let link_path = base_path.join("approval_link.json");
        let link_data = serde_json::json!({
            "approval_id": approval_id,
            "created_at": chrono::Utc::now().to_rfc3339()
        });
        if let Ok(content) = serde_json::to_string_pretty(&link_data) {
            let _ = std::fs::write(&link_path, &content);
        }

        Ok(approval_id)
    }
}

/// Main pipeline orchestrator
pub struct ServerDiscoveryPipeline {
    config: ServerDiscoveryPipelineConfig,
    missing_caps: MissingCapabilityRuntimeConfig,
    registry_searcher: Arc<RegistrySearcher>,
    llm_discovery: Option<Arc<LlmDiscoveryService>>,
    mcp_discovery: Arc<MCPDiscoveryService>,
    browser_discovery: Arc<BrowserDiscoveryService>,
    approval_queue: UnifiedApprovalQueue<FileApprovalStorage>,
}

impl ServerDiscoveryPipeline {
    /// Create a new pipeline (env-var based LLM initialization, for backwards compatibility)
    pub async fn new(
        config: ServerDiscoveryPipelineConfig,
        missing_caps: MissingCapabilityRuntimeConfig,
        approval_queue: UnifiedApprovalQueue<FileApprovalStorage>,
    ) -> RuntimeResult<Self> {
        let llm_discovery = if config.sources.llm_suggest {
            LlmDiscoveryService::new().await.ok().map(Arc::new)
        } else {
            None
        };
        Ok(Self {
            config,
            missing_caps,
            registry_searcher: Arc::new(RegistrySearcher::new()),
            llm_discovery,
            mcp_discovery: Arc::new(MCPDiscoveryService::new()),
            browser_discovery: Arc::new(BrowserDiscoveryService::new()),
            approval_queue,
        })
    }

    /// Create a new pipeline using LLM profiles from AgentConfig
    /// If `llm_service` is provided, it will be used directly; otherwise a new one is created.
    pub async fn from_config(
        agent_config: &AgentConfig,
        approval_queue: UnifiedApprovalQueue<FileApprovalStorage>,
        llm_service: Option<Arc<LlmDiscoveryService>>,
    ) -> RuntimeResult<Self> {
        let config = agent_config.server_discovery_pipeline.clone();
        let missing_caps = agent_config.missing_capabilities.clone();

        // Use provided service or create a new one
        let llm_discovery = if config.sources.llm_suggest {
            if let Some(service) = llm_service {
                Some(service)
            } else {
                LlmDiscoveryService::from_config(agent_config)
                    .await
                    .ok()
                    .map(Arc::new)
            }
        } else {
            None
        };

        Ok(Self {
            config,
            missing_caps,
            registry_searcher: Arc::new(RegistrySearcher::new()),
            llm_discovery,
            mcp_discovery: Arc::new(MCPDiscoveryService::new()),
            browser_discovery: Arc::new(BrowserDiscoveryService::new()),
            approval_queue,
        })
    }

    pub fn with_mcp_discovery(mut self, service: Arc<MCPDiscoveryService>) -> Self {
        self.mcp_discovery = service;
        self
    }

    pub fn with_browser_discovery(mut self, service: Arc<BrowserDiscoveryService>) -> Self {
        self.browser_discovery = service;
        self
    }

    pub fn with_llm_discovery(mut self, service: Option<Arc<LlmDiscoveryService>>) -> Self {
        self.llm_discovery = service;
        self
    }

    pub fn config(&self) -> &ServerDiscoveryPipelineConfig {
        &self.config
    }

    pub async fn discover_candidates(
        &self,
        query: &str,
        url_hint: Option<&str>,
    ) -> RuntimeResult<Vec<RegistrySearchResult>> {
        crate::ccos_println!(
            "🚀 ServerDiscoveryPipeline::discover_candidates started for query: '{}'",
            query
        );
        let ctx = DiscoveryQueryContext {
            query,
            url_hint,
            config: &self.config,
        };
        crate::ccos_println!(
            "🚀 ServerDiscoveryPipeline::discover_candidates - Order: {:?}",
            self.config.query_pipeline_order
        );
        let mut results: Vec<RegistrySearchResult> = Vec::new();
        for stage in &self.config.query_pipeline_order {
            match stage.as_str() {
                "registry_search" => {
                    let stage_impl = RegistrySearchStage::new(
                        self.registry_searcher.clone(),
                        self.config.sources.clone(),
                        self.missing_caps.clone(),
                    );
                    match stage_impl.run(&ctx).await {
                        Ok(stage_results) => {
                            results.extend(stage_results);
                        }
                        Err(e) => {
                            crate::ccos_println!(
                                "⚠️  Discovery stage 'registry_search' failed: {}",
                                e
                            );
                        }
                    }
                }
                "llm_suggest" => {
                    if self.config.sources.llm_suggest {
                        let stage_impl = LlmSuggestStage::new(self.llm_discovery.clone());
                        match stage_impl.run(&ctx).await {
                            Ok(stage_results) => {
                                results.extend(stage_results);
                            }
                            Err(e) => {
                                crate::ccos_println!(
                                    "⚠️  Discovery stage 'llm_suggest' failed: {}",
                                    e
                                );
                            }
                        }
                    }
                }
                "rank" => {
                    if let Some(service) = &self.llm_discovery {
                        if results.is_empty() {
                            continue;
                        }
                        let intent = service.analyze_goal(query).await.ok();
                        match service
                            .rank_results(query, intent.as_ref(), results.clone())
                            .await
                        {
                            Ok(ranked) => {
                                results = ranked
                                    .into_iter()
                                    .map(|r| {
                                        let mut result = r.result;
                                        result.match_score = r.llm_score as f32;
                                        result
                                    })
                                    .filter(|r| r.match_score as f64 >= self.config.threshold)
                                    .collect();
                            }
                            Err(e) => {
                                crate::ccos_println!("⚠️  Discovery stage 'rank' failed: {}", e);
                            }
                        }
                    }
                }
                "dedupe" => {
                    let mut seen = std::collections::HashSet::new();
                    results.retain(|r| seen.insert(r.server_info.endpoint.clone()));
                }
                "limit" => {
                    if results.len() > self.config.max_ranked {
                        results.truncate(self.config.max_ranked);
                    }
                }
                _ => {}
            }
        }
        Ok(results)
    }

    pub async fn preview(&self, target: PipelineTarget) -> RuntimeResult<DiscoveryPreviewResult> {
        let result = self.introspect_with_order(&target).await?;
        if !result.success {
            return Err(RuntimeError::Generic(
                result
                    .error
                    .unwrap_or_else(|| "Introspection failed".to_string()),
            ));
        }
        Ok(build_preview_from_introspection(&result))
    }

    pub async fn stage_and_queue(
        &self,
        target: PipelineTarget,
    ) -> RuntimeResult<DiscoveryQueueResult> {
        if !self.config.enabled {
            return Err(RuntimeError::Generic(
                "Server discovery pipeline is disabled by config".to_string(),
            ));
        }

        let result = self.introspect_with_order(&target).await?;
        if !result.success {
            return Err(RuntimeError::Generic(
                result
                    .error
                    .unwrap_or_else(|| "Introspection failed".to_string()),
            ));
        }

        let server_name = result.server_name.clone();
        let pending_base = resolve_workspace_path(&self.config.staging.pending_subdir);
        let pending_dir = pending_base.join(sanitize_filename(&server_name));

        let staging = DefaultStagingStage::new(self.mcp_discovery.clone());
        let staging_result = staging
            .stage(&StagingContext {
                target: &target.input,
                server_name: &server_name,
                pending_base: &pending_base,
                result: &result,
            })
            .await?;

        if !self.config.approvals.enabled {
            return Err(RuntimeError::Generic(
                "Approvals disabled for server discovery pipeline".to_string(),
            ));
        }

        let approval_stage = DefaultApprovalStage::new(self.approval_queue.clone());
        let server_rtfs_path = pending_dir.join("server.rtfs");
        let approval_id = approval_stage
            .approve(&ApprovalContext {
                target: &target.input,
                server_name: &server_name,
                auth_env_var: target.auth_env_var.clone(),
                result: &result,
                capabilities_path: &server_rtfs_path.to_string_lossy(),
                capability_files: &staging_result.capability_files,
                approvals_config: &self.config.approvals,
            })
            .await?;

        let preview = build_preview_from_introspection(&result);
        Ok(DiscoveryQueueResult {
            approval_id,
            pending_dir: staging_result.output_dir.to_string_lossy().to_string(),
            capability_files: staging_result.capability_files,
            source: format!("{:?}", result.source).to_lowercase(),
            preview: Some(preview),
        })
    }

    async fn introspect_with_order(
        &self,
        target: &PipelineTarget,
    ) -> RuntimeResult<IntrospectionResult> {
        let ctx = IntrospectionContext {
            target: &target.input,
            name: target.name.clone(),
            auth_env_var: target.auth_env_var.clone(),
            config: &self.config,
        };
        let mut last_error: Option<String> = None;
        for stage_name in &self.config.introspection_order {
            match stage_name.as_str() {
                "mcp" => {
                    if !self.config.introspection.mcp_http && !self.config.introspection.mcp_stdio {
                        continue;
                    }
                    let stage = McpIntrospectionStage::new(
                        self.mcp_discovery.clone(),
                        self.browser_discovery.clone(),
                    );
                    match stage.run(&ctx).await {
                        Ok(res) => {
                            if res.success {
                                return Ok(res);
                            }
                            last_error = res.error.clone();
                        }
                        Err(e) => {
                            last_error = Some(e.to_string());
                        }
                    }
                }
                "openapi" => {
                    if !self.config.introspection.openapi {
                        continue;
                    }
                    if !IntrospectionService::is_openapi_url(ctx.target) {
                        continue;
                    }
                    let service = IntrospectionService::empty()
                        .with_browser_discovery(self.browser_discovery.clone());
                    let stage = OpenApiIntrospectionStage::new(service);
                    let res = stage.run(&ctx).await?;
                    if res.success {
                        return Ok(res);
                    }
                    last_error = res.error.clone();
                }
                "browser" => {
                    if !self.config.introspection.browser {
                        continue;
                    }
                    if !ctx.target.starts_with("http") {
                        continue;
                    }
                    let mut service = IntrospectionService::empty()
                        .with_browser_discovery(self.browser_discovery.clone());
                    if let Some(llm) = &self.llm_discovery {
                        service = service.with_llm_discovery(llm.clone());
                    }
                    let stage = BrowserIntrospectionStage::new(service);
                    let res = stage.run(&ctx).await?;
                    if res.success {
                        return Ok(res);
                    }
                    last_error = res.error.clone();
                }
                _ => {}
            }
        }

        Ok(IntrospectionResult {
            success: false,
            source: IntrospectionSource::Unknown,
            server_name: target
                .name
                .clone()
                .unwrap_or_else(|| sanitize_filename(&target.input)),
            api_result: None,
            browser_result: None,
            manifests: Vec::new(),
            approval_id: None,
            error: Some(
                last_error.unwrap_or_else(|| "No introspection stages succeeded".to_string()),
            ),
        })
    }
}

#[allow(dead_code)]
fn dedupe_results(results: Vec<RegistrySearchResult>) -> Vec<RegistrySearchResult> {
    let mut seen = HashSet::new();
    results
        .into_iter()
        .filter(|r| {
            let key = format!("{}::{}", r.server_info.name, r.server_info.endpoint);
            seen.insert(key)
        })
        .collect()
}

fn build_preview_from_introspection(result: &IntrospectionResult) -> DiscoveryPreviewResult {
    let mut endpoints = Vec::new();
    if let Some(api) = &result.api_result {
        for ep in &api.endpoints {
            endpoints.push(DiscoveryEndpointPreview {
                id: ep.endpoint_id.clone(),
                name: ep.name.clone(),
                method: Some(ep.method.clone()),
                path: Some(ep.path.clone()),
            });
        }
    }
    if let Some(browser) = &result.browser_result {
        for ep in &browser.discovered_endpoints {
            endpoints.push(DiscoveryEndpointPreview {
                id: format!("{}_{}", ep.method, ep.path),
                name: format!("{} {}", ep.method, ep.path),
                method: Some(ep.method.clone()),
                path: Some(ep.path.clone()),
            });
        }
    }
    if !result.manifests.is_empty() {
        for manifest in &result.manifests {
            endpoints.push(DiscoveryEndpointPreview {
                id: manifest.id.clone(),
                name: manifest.name.clone(),
                method: None,
                path: None,
            });
        }
    }
    DiscoveryPreviewResult {
        source: format!("{:?}", result.source).to_lowercase(),
        server_name: result.server_name.clone(),
        endpoints,
        manifests_count: result.manifests.len(),
    }
}
