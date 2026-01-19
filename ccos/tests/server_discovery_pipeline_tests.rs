use std::sync::Arc;

use async_trait::async_trait;
use tempfile::tempdir;

use ccos::approval::queue::{DiscoverySource, ServerInfo};
use ccos::approval::storage_file::FileApprovalStorage;
use ccos::approval::UnifiedApprovalQueue;
use ccos::config::types::{AgentConfig, MissingCapabilityRuntimeConfig, ServerDiscoveryPipelineConfig};
use ccos::discovery::registry_search::{DiscoveryCategory, RegistrySearchResult};
use ccos::ops::server_discovery_pipeline::{DiscoveryQueryContext, DiscoveryStage, ServerDiscoveryPipeline};

#[tokio::test]
async fn parses_server_discovery_pipeline_config() {
    let toml = r#"
version = "0.1"
agent_id = "test-agent"
profile = "local"

[server_discovery_pipeline]
enabled = false
introspection_order = ["browser", "openapi"]

[server_discovery_pipeline.sources]
web_search = true
"#;

    let config: AgentConfig = toml::from_str(toml).expect("parse config");
    assert!(!config.server_discovery_pipeline.enabled);
    assert_eq!(
        config.server_discovery_pipeline.introspection_order,
        vec!["browser".to_string(), "openapi".to_string()]
    );
    assert_eq!(
        config.server_discovery_pipeline.sources.web_search,
        Some(true)
    );
}

#[tokio::test]
async fn preserves_stage_order_in_pipeline() {
    let mut pipeline_config = ServerDiscoveryPipelineConfig::default();
    pipeline_config.query_pipeline_order = vec![
        "llm_suggest".to_string(),
        "registry_search".to_string(),
        "limit".to_string(),
    ];
    pipeline_config.introspection_order = vec!["browser".to_string(), "openapi".to_string()];

    let temp = tempdir().expect("tempdir");
    let storage = FileApprovalStorage::new(temp.path().to_path_buf()).expect("storage");
    let queue = UnifiedApprovalQueue::new(Arc::new(storage));

    let pipeline = ServerDiscoveryPipeline::new(
        pipeline_config.clone(),
        MissingCapabilityRuntimeConfig::default(),
        queue,
    )
    .await
    .expect("pipeline");

    assert_eq!(
        pipeline.config().query_pipeline_order,
        pipeline_config.query_pipeline_order
    );
    assert_eq!(
        pipeline.config().introspection_order,
        pipeline_config.introspection_order
    );
}

struct DummyDiscoveryStage;

#[async_trait]
impl DiscoveryStage for DummyDiscoveryStage {
    fn name(&self) -> &str {
        "dummy"
    }

    async fn run(
        &self,
        _ctx: &DiscoveryQueryContext<'_>,
    ) -> rtfs::runtime::error::RuntimeResult<Vec<RegistrySearchResult>> {
        Ok(vec![RegistrySearchResult {
            source: DiscoverySource::Manual {
                user: "test".to_string(),
            },
            server_info: ServerInfo {
                name: "dummy".to_string(),
                endpoint: "https://example.com".to_string(),
                description: Some("dummy stage".to_string()),
                auth_env_var: None,
                capabilities_path: None,
                alternative_endpoints: Vec::new(),
                capability_files: None,
            },
            match_score: 1.0,
            alternative_endpoints: Vec::new(),
            category: DiscoveryCategory::WebApi,
        }])
    }
}

#[tokio::test]
async fn allows_custom_discovery_stage() {
    let stage = DummyDiscoveryStage;
    let config = ServerDiscoveryPipelineConfig::default();
    let ctx = DiscoveryQueryContext {
        query: "test",
        url_hint: None,
        config: &config,
    };

    let results = stage.run(&ctx).await.expect("run stage");
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].server_info.name, "dummy");
}
