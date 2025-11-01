use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rtfs_compiler::ast::{Keyword, MapKey};
use rtfs_compiler::ccos::capabilities::{MCPSessionHandler, SessionPoolManager};
use rtfs_compiler::ccos::capability_marketplace::types::ProviderType;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::ccos::synthesis::capability_synthesizer::CapabilitySynthesizer;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::values::Value;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Configure a minimal session pool so session-managed providers (e.g., MCP) don't warn
    let mut session_pool = SessionPoolManager::new();
    session_pool.register_handler("mcp", std::sync::Arc::new(MCPSessionHandler::new()));
    let session_pool = std::sync::Arc::new(session_pool);
    // Attach to marketplace for session-managed execution
    marketplace.set_session_pool(session_pool.clone()).await;

    // Compute export dir
    let mut export_dir = std::env::temp_dir();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let pid = std::process::id();
    export_dir.push(format!("ccos_discovery_exports_{}_{}", pid, ts));
    std::fs::create_dir_all(&export_dir)?;

    println!("Export directory: {}", export_dir.display());

    // We'll also persist RTFS capability files under ./capabilities
    let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
        .unwrap_or_else(|_| "./capabilities_ccos".to_string());
    let storage_path = std::path::Path::new(&storage_dir);
    std::fs::create_dir_all(storage_path)?;

    // 1) OpenAPI-based introspection (OpenWeather) via official synthesizer/introspector
    println!(
        "[OpenAPI] Introspecting OpenWeather via CapabilitySynthesizer (discovery → mock fallback)"
    );
    let synthesizer = CapabilitySynthesizer::new();
    let synthesis = synthesizer
        .synthesize_from_api_introspection("https://api.openweathermap.org", "openweather")
        .await;

    // Keep track of saved RTFS files so we can load them into the runtime for the smoke test
    let mut saved_rtfs_files: Vec<std::path::PathBuf> = Vec::new();

    match synthesis {
        Ok(result) => {
            println!(
                "[OpenAPI] Synthesized {} capabilities (overall_quality {:.2})",
                result.capabilities.len(),
                result.overall_quality_score
            );

            // Save each synthesized capability as RTFS and register in marketplace
            let api_introspector = synthesizer.get_introspector();
            for cap in &result.capabilities {
                let cap_id = &cap.capability.id;
                let saved_path = api_introspector.save_capability_to_rtfs(
                    &cap.capability,
                    &cap.implementation_code,
                    storage_path,
                );
                match saved_path {
                    Ok(path) => {
                        println!("  💾 Saved: {}", path.display());
                        saved_rtfs_files.push(path);
                    }
                    Err(e) => eprintln!("  ❌ Save failed for {}: {}", cap_id, e),
                }

                // Register manifest in marketplace (for export/import roundtrip)
                marketplace
                    .register_capability_manifest(cap.capability.clone())
                    .await?;
            }
        }
        Err(e) => eprintln!("[OpenAPI] Introspection failed: {}", e),
    }

    // Optional: small runtime smoke test using the RTFS implementation if API key is available
    if let Ok(api_key) = std::env::var("OPENWEATHERMAP_ORG_API_KEY") {
        if !api_key.trim().is_empty() {
            println!("[OpenAPI] Running a live call via RTFS runtime (get_current_weather)…");
            let files_to_load = saved_rtfs_files.clone();
            let handle = std::thread::spawn(move || {
                let builder = rtfs_compiler::ccos::environment::CCOSBuilder::new()
                    .http_mocking(false)
                    .http_allow_hosts(vec![
                        "openweathermap.org".to_string(),
                        "api.openweathermap.org".to_string(),
                    ])
                    .verbose(false);
                let env = builder.build();
                match env {
                    Ok(env) => {
                        // Load the generated RTFS capability files so the capability becomes available
                        for path in files_to_load.iter() {
                            match env.execute_file(path.to_string_lossy().as_ref()) {
                                Ok(_) => {
                                    // loaded
                                }
                                Err(e) => {
                                    eprintln!(
                                        "  ⚠️  Failed to load RTFS file {}: {:?}",
                                        path.display(),
                                        e
                                    );
                                }
                            }
                        }
                        let expr = r#"
                        ((call "openweather_api.get_current_weather") {
                            :q "Paris,FR"
                            :units "metric"
                        })
                        "#;
                        match env.execute_code(expr) {
                            Ok(outcome) => println!("  ✅ Runtime call executed: {:?}", outcome),
                            Err(e) => eprintln!("  ⚠️  Runtime call failed: {:?}", e),
                        }
                    }
                    Err(e) => eprintln!("  ⚠️  Failed to build RTFS environment: {:?}", e),
                }
            });
            let _ = handle.join();
        }
    }

    // 2) MCP discovery (generic MCP server) — optional, if endpoint provided
    //    On success: save RTFS, register manifests, and try a live call via the MCP executor.
    //    This validates end-to-end: discovery → persistence → execution.
    let mut discovered_mcp_caps: Vec<
        rtfs_compiler::ccos::capability_marketplace::types::CapabilityManifest,
    > = Vec::new();
    if let Ok(endpoint) = std::env::var("MCP_SERVER_URL") {
        println!("[MCP] Introspecting MCP server at {}", &endpoint);
        let name = std::env::var("MCP_SERVER_NAME").unwrap_or_else(|_| "mcp".to_string());
        let token_opt = std::env::var("MCP_AUTH_TOKEN").ok();

        // Prefer the official synthesizer path so we also get RTFS implementations
        let mcp_synth = if let Some(token) = token_opt.clone() {
            let mut headers = HashMap::new();
            // Use MCP_AUTH_TOKEN as-is for Authorization to avoid double-prefix issues.
            // If the server expects a scheme (e.g., "Bearer <token>"), the env var should include it.
            headers.insert("Authorization".to_string(), token);
            synthesizer
                .synthesize_from_mcp_introspection_with_auth(&endpoint, &name, Some(headers))
                .await
        } else {
            synthesizer
                .synthesize_from_mcp_introspection(&endpoint, &name)
                .await
        };

        match mcp_synth {
            Ok(result) => {
                println!(
                    "[MCP] Synthesized {} MCP capabilities (overall_quality {:.2})",
                    result.capabilities.len(),
                    result.overall_quality_score
                );

                let mcp_intro = synthesizer.get_mcp_introspector();
                for cap in &result.capabilities {
                    let cap_id = &cap.capability.id;
                    let saved_path = mcp_intro.save_capability_to_rtfs(
                        &cap.capability,
                        &cap.implementation_code,
                        storage_path,
                    );
                    match saved_path {
                        Ok(path) => println!("  💾 Saved MCP: {}", path.display()),
                        Err(e) => eprintln!("  ❌ Save failed for MCP {}: {}", cap_id, e),
                    }

                    marketplace
                        .register_capability_manifest(cap.capability.clone())
                        .await?;
                    discovered_mcp_caps.push(cap.capability.clone());
                }

                // Attempt a live MCP call through the marketplace executor if we can locate a simple tool.
                // Heuristic: prefer a tool named "list_issues" (common in GitHub MCP) with demo inputs; otherwise, skip.
                if !discovered_mcp_caps.is_empty() {
                    // Find a list_issues capability if present
                    if let Some(target) = discovered_mcp_caps
                        .iter()
                        .find(|m| m.id.contains("list_issues"))
                    {
                        println!("[MCP] Trying live call via MCPExecutor for {}", target.id);
                        // Minimal demo inputs; adjust via env if provided (generic names)
                        let owner = std::env::var("MCP_DEMO_OWNER")
                            .unwrap_or_else(|_| "octocat".to_string());
                        let repo = std::env::var("MCP_DEMO_REPO")
                            .unwrap_or_else(|_| "hello-world".to_string());
                        let mut args = std::collections::HashMap::new();
                        // Use keyword keys to satisfy the runtime type validator (expects :owner, :repo, ...)
                        args.insert(
                            MapKey::Keyword(Keyword("owner".to_string())),
                            Value::String(owner),
                        );
                        args.insert(
                            MapKey::Keyword(Keyword("repo".to_string())),
                            Value::String(repo),
                        );
                        // Small page size to keep response light if supported by the tool signature
                        args.insert(
                            MapKey::Keyword(Keyword("perPage".to_string())),
                            Value::Float(1.0),
                        );
                        let inputs = Value::Map(args);
                        match marketplace.execute_capability(&target.id, &inputs).await {
                            Ok(res) => println!("  ✅ MCP live call ok: type={}", res.type_name()),
                            Err(e) => {
                                eprintln!("  ⚠️  MCP live call failed for {}: {}", target.id, e)
                            }
                        }
                    } else {
                        println!("[MCP] Skipping live call demo: no 'list_issues' tool discovered");
                    }
                }
            }
            Err(e) => eprintln!("[MCP] Introspection failed: {}", e),
        }
    } else {
        eprintln!("[MCP] Skipping MCP introspection (set MCP_SERVER_URL to enable)");
    }

    // Export discovered/registered capabilities (both OpenWeather + optional MCP)
    let exported_rtfs = marketplace
        .export_capabilities_to_rtfs_dir(&export_dir)
        .await?;
    println!(
        "Exported {} RTFS files to {}",
        exported_rtfs,
        export_dir.display()
    );

    // If nothing was exported because synthesized capabilities use Local providers (non-serializable),
    // also copy the saved RTFS files into the export directory for inspection and manual reuse.
    if exported_rtfs == 0 && !saved_rtfs_files.is_empty() {
        let mut copied = 0usize;
        for src in &saved_rtfs_files {
            if let Some(name) = src.file_name() {
                let dest = export_dir.join(name);
                if let Err(e) = std::fs::copy(src, &dest) {
                    eprintln!(
                        "  ⚠️  Failed to copy {} to export dir: {}",
                        src.display(),
                        e
                    );
                } else {
                    copied += 1;
                }
            }
        }
        // Also write a small index to document why these files are here
        let idx_path = export_dir.join("LOCAL_RTFS_README.txt");
        let note = r#"These .rtfs files were synthesized via the official introspection/synthesis pipeline
but use a Local provider type, which the current marketplace export does not serialize.

They are copied here for transparency and reuse. You can load them into a runtime via execute_file.
"#;
        let _ = std::fs::write(idx_path, note);
        println!(
            "Copied {} synthesized RTFS files into export dir (Local provider artifacts)",
            copied
        );
    }

    let json_file = export_dir.join("capabilities.json");
    let exported_json = marketplace.export_capabilities_to_file(&json_file).await?;
    println!(
        "Exported {} capabilities to JSON: {}",
        exported_json,
        json_file.display()
    );

    // Reload into fresh marketplace
    let new_marketplace =
        CapabilityMarketplace::new(Arc::new(RwLock::new(CapabilityRegistry::new())));
    let imported = new_marketplace
        .import_capabilities_from_rtfs_dir(&export_dir)
        .await?;
    println!("Re-imported {} capabilities from RTFS dir", imported);

    // Try executing OpenWeather via marketplace for sanity: call operation getcurrentweather if any OpenAPI provider was registered
    for cap in new_marketplace.list_capabilities().await.into_iter() {
        if let ProviderType::OpenApi(_) = cap.provider {
            if cap.id.contains("openweather") {
                let mut params = HashMap::new();
                params.insert(
                    MapKey::String("q".to_string()),
                    Value::String("Berlin".to_string()),
                );
                params.insert(
                    MapKey::String("units".to_string()),
                    Value::String("metric".to_string()),
                );
                let mut input_map = HashMap::new();
                input_map.insert(
                    MapKey::String("operation".to_string()),
                    Value::String("getcurrentweather".to_string()),
                );
                input_map.insert(MapKey::String("params".to_string()), Value::Map(params));
                let val = Value::Map(input_map);
                match new_marketplace.execute_capability(&cap.id, &val).await {
                    Ok(res) => println!("[Re-exec] {} ok: type={}", cap.id, res.type_name()),
                    Err(e) => eprintln!("[Re-exec] {} failed: {}", cap.id, e),
                }
            }
        } else if let ProviderType::MCP(_) = cap.provider {
            // After re-import, try an MCP call again for a 'list_issues' capability if available
            if cap.id.contains("list_issues") {
                println!("[Re-exec][MCP] Trying MCPExecutor for {}", cap.id);
                let owner =
                    std::env::var("MCP_DEMO_OWNER").unwrap_or_else(|_| "octocat".to_string());
                let repo =
                    std::env::var("MCP_DEMO_REPO").unwrap_or_else(|_| "hello-world".to_string());
                let mut args = HashMap::new();
                // Use keyword keys to satisfy the runtime type validator (expects :owner, :repo, ...)
                args.insert(
                    MapKey::Keyword(Keyword("owner".to_string())),
                    Value::String(owner),
                );
                args.insert(
                    MapKey::Keyword(Keyword("repo".to_string())),
                    Value::String(repo),
                );
                args.insert(
                    MapKey::Keyword(Keyword("perPage".to_string())),
                    Value::Float(1.0),
                );
                let inputs = Value::Map(args);
                match new_marketplace.execute_capability(&cap.id, &inputs).await {
                    Ok(res) => println!("  ✅ [Re-exec] MCP call ok: type={}", res.type_name()),
                    Err(e) => eprintln!("  ⚠️  [Re-exec] MCP call failed for {}: {}", cap.id, e),
                }
            }
        }
    }

    println!("Files in {}:", export_dir.display());
    for entry in std::fs::read_dir(&export_dir)? {
        let e = entry?;
        println!("- {}", e.path().display());
    }

    // Reload from the saved RTFS storage directory and verify execution in a fresh runtime
    if let Ok(api_key) = std::env::var("OPENWEATHERMAP_ORG_API_KEY") {
        if !api_key.trim().is_empty() && !saved_rtfs_files.is_empty() {
            println!(
                "[Reload] Loading saved RTFS capabilities into a fresh runtime and executing…"
            );
            let files_to_load = saved_rtfs_files.clone();
            let handle = std::thread::spawn(move || {
                let builder = rtfs_compiler::ccos::environment::CCOSBuilder::new()
                    .http_mocking(false)
                    .http_allow_hosts(vec![
                        "openweathermap.org".to_string(),
                        "api.openweathermap.org".to_string(),
                    ])
                    .verbose(false);
                match builder.build() {
                    Ok(env) => {
                        for path in files_to_load.iter() {
                            if let Err(e) = env.execute_file(path.to_string_lossy().as_ref()) {
                                eprintln!(
                                    "  ⚠️  Reload: failed to load {}: {:?}",
                                    path.display(),
                                    e
                                );
                            }
                        }
                        let expr = r#"
                        ((call "openweather_api.get_forecast") {
                            :q "Berlin,DE"
                            :units "metric"
                        })
                        "#;
                        match env.execute_code(expr) {
                            Ok(outcome) => {
                                println!("  ✅ Reload runtime call executed: {:?}", outcome)
                            }
                            Err(e) => eprintln!("  ⚠️  Reload runtime call failed: {:?}", e),
                        }
                    }
                    Err(e) => eprintln!("  ⚠️  Reload: failed to build environment: {:?}", e),
                }
            });
            let _ = handle.join();
        }
    }

    Ok(())
}
