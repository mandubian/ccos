use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rtfs_compiler::ast::{Keyword, MapKey};
use rtfs_compiler::ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use rtfs_compiler::ccos::capability_marketplace::mcp_discovery::{
    MCPDiscoveryProvider, MCPServerConfig,
};
use rtfs_compiler::ccos::synthesis::mcp_session::{MCPServerInfo, MCPSessionManager};
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::values::Value;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Marketplace with session pool for MCP session-managed execution
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace =
        rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry);

    // Configure minimal session pool for MCP
    let mut session_pool = SessionPoolManager::new();
    session_pool.register_handler("mcp", std::sync::Arc::new(MCPSessionHandler::new()));
    let session_pool = std::sync::Arc::new(session_pool);
    marketplace.set_session_pool(session_pool.clone()).await;

    // Compute export dir
    let mut export_dir = std::env::temp_dir();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let pid = std::process::id();
    export_dir.push(format!("ccos_discovery_exports_mcp_{}_{}", pid, ts));
    std::fs::create_dir_all(&export_dir)?;
    println!("Export directory: {}", export_dir.display());

    // Required MCP endpoint
    let endpoint = match std::env::var("MCP_SERVER_URL") {
        Ok(v) => v,
        Err(_) => {
            eprintln!("[MCP] Skipping MCP discovery (set MCP_SERVER_URL, e.g., https://api.githubcopilot.com/mcp/)");
            return Ok(());
        }
    };
    let name = std::env::var("MCP_SERVER_NAME").unwrap_or_else(|_| "mcp".to_string());
    println!(
        "[MCP] Registering MCP tool capability against {}",
        &endpoint
    );

    // Pick tool name from env or default to GitHub list_issues
    let tool_name = std::env::var("MCP_TOOL_NAME").unwrap_or_else(|_| "list_issues".to_string());

    // Early auth format check (do not print the secret)
    if let Ok(tok) = std::env::var("MCP_AUTH_TOKEN") {
        if !tok.trim_start().starts_with("Bearer ") {
            eprintln!("[MCP] Warning: MCP_AUTH_TOKEN doesn't start with 'Bearer '. If your server expects a bearer token, set it like: 'Bearer <token>'.");
        }
    }

    // Register a direct MCP capability so MCPExecutor is used (no HTTP wrapper)
    let cap_id = format!("mcp.{}.{}", name, tool_name);
    marketplace
        .register_mcp_capability(
            cap_id.clone(),
            tool_name.clone(),
            format!("MCP tool: {}", tool_name),
            endpoint.clone(),
            tool_name.clone(),
            30_000,
        )
        .await?;

    // Hint marketplace to use session pool for MCP (auth/header management) via metadata flag
    if let Some(mut manifest) = marketplace.get_capability(&cap_id).await {
        manifest
            .metadata
            .insert("mcp_requires_session".to_string(), "true".to_string());
        // Provide session handler with server URL and auth env variable
        manifest
            .metadata
            .insert("mcp_server_url".to_string(), endpoint.clone());
        manifest
            .metadata
            .insert("mcp_auth_env_var".to_string(), "MCP_AUTH_TOKEN".to_string());
        // Optional: allow overriding server URL via env without re-exporting
        manifest.metadata.insert(
            "mcp_server_url_override_env".to_string(),
            "MCP_SERVER_URL".to_string(),
        );

        // Optionally introspect input/output schemas via MCP session tools/list and convert to TypeExpr
        if std::env::var("MCP_SKIP_SCHEMA").is_err() {
            // Prepare optional Authorization header (verbatim, may already include "Bearer ")
            let mut auth_headers: std::collections::HashMap<String, String> =
                std::collections::HashMap::new();
            if let Ok(token) = std::env::var("MCP_AUTH_TOKEN") {
                auth_headers.insert("Authorization".to_string(), token);
            }

            // Initialize a short-lived MCP session to request schemas
            let session_manager = MCPSessionManager::new(if auth_headers.is_empty() {
                None
            } else {
                Some(auth_headers)
            });
            let client_info = MCPServerInfo {
                name: "ccos-discovery".to_string(),
                version: "1.0.0".to_string(),
            };

            match session_manager
                .initialize_session(&endpoint, &client_info)
                .await
            {
                Ok(session) => {
                    let resp = session_manager
                        .make_request(
                            &session,
                            "tools/list",
                            serde_json::json!({ "includeSchema": true }),
                        )
                        .await;

                    // Terminate regardless of result
                    let _ = session_manager.terminate_session(&session).await;

                    if let Ok(json) = resp {
                        if let Some(result) = json.get("result") {
                            if let Some(tools) = result.get("tools").and_then(|t| t.as_array()) {
                                if let Some(tool) = tools.iter().find(|t| {
                                    t.get("name").and_then(|n| n.as_str())
                                        == Some(tool_name.as_str())
                                }) {
                                    let input_schema_json = tool.get("inputSchema").cloned();
                                    let output_schema_json = tool.get("outputSchema").cloned();
                                    // Use MCPDiscoveryProvider helpers to convert JSON Schema -> RTFS Expr -> TypeExpr
                                    if input_schema_json.is_some() || output_schema_json.is_some() {
                                        let provider = MCPDiscoveryProvider::new(MCPServerConfig {
                                            name: name.clone(),
                                            endpoint: endpoint.clone(),
                                            auth_token: None, // not used for conversion
                                            timeout_seconds: 30,
                                            protocol_version: "2024-11-05".to_string(),
                                        });
                                        if let Ok(conv) = provider {
                                            // Build a synthetic MCPTool to reuse public converters
                                            let tool_desc = tool
                                                .get("description")
                                                .and_then(|v| v.as_str())
                                                .map(|s| s.to_string());
                                            let mcp_tool = rtfs_compiler::ccos::capability_marketplace::mcp_discovery::MCPTool {
                                                name: tool_name.clone(),
                                                description: tool_desc,
                                                input_schema: input_schema_json,
                                                output_schema: output_schema_json,
                                                metadata: None,
                                                annotations: None,
                                            };
                                            if let Ok(rtfs_cap) =
                                                conv.convert_tool_to_rtfs_format(&mcp_tool)
                                            {
                                                if let Ok(mm) =
                                                    conv.rtfs_to_capability_manifest(&rtfs_cap)
                                                {
                                                    manifest.input_schema = mm.input_schema;
                                                    manifest.output_schema = mm.output_schema;
                                                    if manifest.input_schema.is_some()
                                                        || manifest.output_schema.is_some()
                                                    {
                                                        println!(
                                                            "[MCP] Enriched schemas for {}",
                                                            tool_name
                                                        );
                                                    } else {
                                                        eprintln!("[MCP] No schemas present after conversion for {}", tool_name);
                                                    }
                                                }
                                            }
                                        } else {
                                            eprintln!("[MCP] Schema conversion provider init failed for {}", tool_name);
                                        }
                                    } else {
                                        eprintln!("[MCP] No input/output schema provided by server for {}", tool_name);
                                    }
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    eprintln!("[MCP] Session init failed for schema discovery: {}", e);
                }
            }
        }

        // Replace updated manifest (with metadata and possibly schemas)
        marketplace.register_capability_manifest(manifest).await?;
    }

    // Track discovered capability manifests (single target)
    let mut discovered_mcp_caps: Vec<
        rtfs_compiler::ccos::capability_marketplace::types::CapabilityManifest,
    > = Vec::new();
    if let Some(m) = marketplace.get_capability(&cap_id).await {
        discovered_mcp_caps.push(m);
    }

    // Try a live call: prefer a tool named "list_issues" when present
    if let Some(target) = discovered_mcp_caps
        .iter()
        .find(|m| m.id.ends_with(&tool_name))
    {
        println!("[MCP] Trying live call via MCPExecutor for {}", target.id);
        // Minimal demo inputs; adjust via env if provided (generic names)
        let owner = std::env::var("MCP_DEMO_OWNER").unwrap_or_else(|_| "octocat".to_string());
        let repo = std::env::var("MCP_DEMO_REPO").unwrap_or_else(|_| "hello-world".to_string());
        let mut args = std::collections::HashMap::new();
        // Use keyword keys to satisfy the runtime type validator
        args.insert(
            MapKey::Keyword(Keyword("owner".to_string())),
            Value::String(owner),
        );
        args.insert(
            MapKey::Keyword(Keyword("repo".to_string())),
            Value::String(repo),
        );
        // Keep response light
        args.insert(
            MapKey::Keyword(Keyword("perPage".to_string())),
            Value::Float(1.0),
        );
        let inputs = Value::Map(args);
        match marketplace.execute_capability(&target.id, &inputs).await {
            Ok(res) => println!("  ✅ MCP live call ok: type={}", res.type_name()),
            Err(e) => eprintln!("  ⚠️  MCP live call failed for {}: {}", target.id, e),
        }
    } else {
        println!(
            "[MCP] Skipping live call demo: tool '{}' not registered",
            tool_name
        );
    }

    // Export discovered MCP capabilities
    let exported_rtfs = marketplace
        .export_capabilities_to_rtfs_dir(&export_dir)
        .await?;
    println!(
        "Exported {} RTFS files to {}",
        exported_rtfs,
        export_dir.display()
    );

    let json_file = export_dir.join("capabilities.json");
    let exported_json = marketplace.export_capabilities_to_file(&json_file).await?;
    println!(
        "Exported {} capabilities to JSON: {}",
        exported_json,
        json_file.display()
    );

    // Re-import and list for sanity
    let new_marketplace = rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(
        Arc::new(RwLock::new(CapabilityRegistry::new())),
    );
    let imported = new_marketplace
        .import_capabilities_from_rtfs_dir(&export_dir)
        .await?;
    println!("Re-imported {} capabilities from RTFS dir", imported);

    println!("Files in {}:", export_dir.display());
    for entry in std::fs::read_dir(&export_dir)? {
        let e = entry?;
        println!("- {}", e.path().display());
    }

    Ok(())
}
