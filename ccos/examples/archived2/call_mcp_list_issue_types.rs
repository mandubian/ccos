use ccos::capability_marketplace::types::{CapabilityManifest, MCPCapability, ProviderType};
use ccos::environment::{CCOSBuilder, CapabilityCategory, SecurityLevel};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::execution_outcome::ExecutionOutcome;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Build a CCOS environment that allows the GitHub MCP capability
    // and permits real network access to the Copilot MCP endpoint.
    let env = CCOSBuilder::new()
        .security_level(SecurityLevel::Custom)
        .enable_category(CapabilityCategory::Network)
        .allow_capability("mcp.github.github-mcp.list_issues")
        .http_mocking(false)
        .http_allow_hosts(vec!["api.githubcopilot.com".to_string()])
        .build()?;

    // Manually craft the capability manifest (mirrors the RTFS export).
    let mut manifest = CapabilityManifest::new(
        "mcp.github.github-mcp.list_issues".to_string(),
        "list_issues".to_string(),
        "List issues in a GitHub repository. For pagination, use the 'endCursor' in the 'after' parameter."
            .to_string(),
        ProviderType::MCP(MCPCapability {
            server_url: "https://api.githubcopilot.com/mcp/".to_string(),
            tool_name: "list_issues".to_string(),
            timeout_ms: 30_000,
        }),
        "1.0.0".to_string(),
    );

    manifest.input_schema = Some(TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword("owner".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("repo".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("after".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("direction".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("labels".to_string()),
                value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(
                    PrimitiveType::String,
                )))),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("orderBy".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("perPage".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Float)),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("since".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
            MapTypeEntry {
                key: Keyword("state".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
        ],
        wildcard: None,
    });
    manifest.output_schema = None; // Tool returns arbitrary JSON

    manifest.metadata.insert(
        "mcp_server_url".to_string(),
        "https://api.githubcopilot.com/mcp/".to_string(),
    );
    manifest
        .metadata
        .insert("mcp_tool_name".to_string(), "list_issues".to_string());
    manifest
        .metadata
        .insert("mcp_requires_session".to_string(), "auto".to_string());
    manifest
        .metadata
        .insert("mcp_auth_env_var".to_string(), "MCP_AUTH_TOKEN".to_string());
    manifest.metadata.insert(
        "capability_source".to_string(),
        "manual_example".to_string(),
    );

    // Register the capability with the marketplace.
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        env.marketplace()
            .register_capability_manifest(manifest)
            .await
    })?;

    println!("Capability registered in marketplace");
    let capabilities = rt.block_on(async { env.marketplace().list_capabilities().await });
    println!("Loaded capabilities before execution:");
    for cap in &capabilities {
        println!("  - {}", cap.id);
    }

    // Invoke the MCP capability directly via RTFS.
    {
        let _enter = rt.enter(); // ensure we execute inside a Tokio runtime context
        let expr =
            r#"(call "mcp.github.github-mcp.list_issues" {:owner "mandubian" :repo "ccos"})"#;
        match env.execute_code(expr)? {
            ExecutionOutcome::Complete(value) => {
                println!("✅ Capability call succeeded:\n{value:#?}");
            }
            other => {
                println!("⚠️ Capability call did not complete: {other:?}");
            }
        }
    }

    Ok(())
}
