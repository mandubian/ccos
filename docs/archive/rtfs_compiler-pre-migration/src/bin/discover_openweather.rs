use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rtfs_compiler::ccos::synthesis::capability_synthesizer::CapabilitySynthesizer;
use rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Minimal marketplace (no session pool needed for OpenWeather)
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace =
        rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry);

    // Compute export dir
    let mut export_dir = std::env::temp_dir();
    let ts = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    let pid = std::process::id();
    export_dir.push(format!("ccos_discovery_exports_openweather_{}_{}", pid, ts));
    std::fs::create_dir_all(&export_dir)?;
    println!("Export directory: {}", export_dir.display());

    // Storage for synthesized RTFS artifacts
    let storage_dir = std::env::var("CCOS_CAPABILITY_STORAGE")
        .unwrap_or_else(|_| "./capabilities_ccos".to_string());
    let storage_path = std::path::Path::new(&storage_dir);
    std::fs::create_dir_all(storage_path)?;

    // OpenAPI-based introspection (OpenWeather) via official synthesizer/introspector
    println!(
        "[OpenAPI] Introspecting OpenWeather via CapabilitySynthesizer (discovery → mock fallback)"
    );
    let synthesizer = CapabilitySynthesizer::new();
    let synthesis = synthesizer
        .synthesize_from_api_introspection("https://api.openweathermap.org", "openweather")
        .await;

    // Keep track of saved RTFS files for runtime loading
    let mut saved_rtfs_files: Vec<std::path::PathBuf> = Vec::new();

    match synthesis {
        Ok(result) => {
            println!(
                "[OpenAPI] Synthesized {} capabilities (overall_quality {:.2})",
                result.capabilities.len(),
                result.overall_quality_score
            );

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

    // Optional runtime smoke test if API key is available
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
                        // Load synthesized RTFS
                        for path in files_to_load.iter() {
                            if let Err(e) = env.execute_file(path.to_string_lossy().as_ref()) {
                                eprintln!(
                                    "  ⚠️  Failed to load RTFS file {}: {:?}",
                                    path.display(),
                                    e
                                );
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

    // Export discovered OpenWeather capabilities
    let exported_rtfs = marketplace
        .export_capabilities_to_rtfs_dir(&export_dir)
        .await?;
    println!(
        "Exported {} RTFS files to {}",
        exported_rtfs,
        export_dir.display()
    );

    // If nothing exported due to Local provider, copy synthesized RTFS into export dir for inspection
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

    // Reload and re-execute an OpenWeather call through a fresh runtime
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

    println!("Files in {}:", export_dir.display());
    for entry in std::fs::read_dir(&export_dir)? {
        let e = entry?;
        println!("- {}", e.path().display());
    }

    Ok(())
}
