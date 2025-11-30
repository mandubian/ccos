// Test script to execute a plan from a JSON file independently

use std::error::Error;
use std::fs;
use std::sync::Arc;

use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use serde_json;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <plan_json_file>", args[0]);
        eprintln!("Example: {} demo_storage/plans/51/09/510983ace1aa57a5c8ded6245d5ff1821e85386f00235aa4cceb0622ab5df3aa.json", args[0]);
        std::process::exit(1);
    }

    let plan_file = &args[1];
    println!("📖 Loading plan from: {}", plan_file);

    // Read and parse the plan JSON
    let plan_json_str = fs::read_to_string(plan_file)?;
    let plan_json: serde_json::Value = serde_json::from_str(&plan_json_str)?;

    // Convert to ArchivablePlan and then to Plan
    let archivable_plan: ccos::archivable_types::ArchivablePlan =
        serde_json::from_value(plan_json)?;
    let plan = ccos::orchestrator::Orchestrator::archivable_plan_to_plan(&archivable_plan);

    println!("✅ Loaded plan: {}", plan.plan_id);
    println!("   Language: {:?}", plan.language);
    println!("   Capabilities required: {:?}", plan.capabilities_required);

    // Initialize CCOS (this triggers bootstrap which loads capabilities from disk)
    println!("🔧 Initializing CCOS...");
    let ccos = ccos::CCOS::new().await?;
    println!("✅ CCOS initialized (capabilities should be loaded via bootstrap)");

    // Give bootstrap a moment to complete (though it should be done by now)
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // List what capabilities are required by the plan
    println!("📋 Required capabilities: {:?}", plan.capabilities_required);
    println!("   ℹ️  If a capability is missing, check that it exists in capabilities/discovered or capabilities/generated");

    let marketplace = ccos.get_capability_marketplace();

    if let Some(cap) = marketplace
        .get_capability("mcp.github.github-mcp.list_issues")
        .await
    {
        println!(
            "ℹ️  Found base MCP capability with metadata keys: {:?}",
            cap.metadata.keys().collect::<Vec<_>>()
        );
    } else {
        println!(
            "ℹ️  Base MCP capability mcp.github.github-mcp.list_issues not currently registered"
        );
    }

    // Ensure MCP capabilities are imported (explicitly import discovered MCP GitHub tools)
    let mcp_github_dir = std::path::PathBuf::from("./capabilities/discovered/mcp/github");
    if mcp_github_dir.exists() {
        match marketplace
            .import_capabilities_from_rtfs_dir(&mcp_github_dir)
            .await
        {
            Ok(count) => println!(
                "📥 Imported {} capability file(s) from {}",
                count,
                mcp_github_dir.display()
            ),
            Err(e) => eprintln!(
                "⚠️  Failed to import MCP capabilities from {}: {}",
                mcp_github_dir.display(),
                e
            ),
        }
    } else {
        println!(
            "ℹ️  MCP GitHub directory not found: {}",
            mcp_github_dir.display()
        );
    }

    // Ensure synthesized/generated capabilities referenced by the plan are registered
    let generated_base = std::path::PathBuf::from("./capabilities/generated");
    for capability_id in &plan.capabilities_required {
        if marketplace.has_capability(capability_id).await {
            println!("✅ Capability already registered: {}", capability_id);
            continue;
        }

        let cap_dir = generated_base.join(capability_id);
        let mut loaded = false;
        if cap_dir.exists() {
            match marketplace
                .import_capabilities_from_rtfs_dir(&cap_dir)
                .await
            {
                Ok(count) if count > 0 => {
                    println!("📥 Imported {} from {}", capability_id, cap_dir.display());
                    loaded = true;
                }
                Ok(_) => {
                    println!("⚠️  No capability manifests found in {}", cap_dir.display());
                }
                Err(e) => {
                    eprintln!(
                        "⚠️  Failed to import {} from {}: {}",
                        capability_id,
                        cap_dir.display(),
                        e
                    );
                }
            }
        }

        if !loaded {
            if try_register_alias_capability(&marketplace, capability_id).await {
                loaded = true;
            }
        }

        if !loaded {
            println!(
                "⚠️  Generated capability directory missing: {}",
                cap_dir.display()
            );
        }
    }

    // Create runtime context with plan inputs
    // Use 'full' security level to allow all capabilities (for testing)
    // In production, use 'controlled' with specific allowed capabilities
    let mut context = RuntimeContext::full();

    // Extract inputs from plan's input_schema and set defaults
    // For this plan: owner, repository, language
    context
        .cross_plan_params
        .insert("owner".to_string(), Value::String("mandubian".to_string()));
    context
        .cross_plan_params
        .insert("repository".to_string(), Value::String("ccos".to_string()));
    context
        .cross_plan_params
        .insert("repo".to_string(), Value::String("ccos".to_string()));
    context
        .cross_plan_params
        .insert("language".to_string(), Value::String("rtfs".to_string()));
    context.cross_plan_params.insert(
        "language_filter".to_string(),
        Value::String("rtfs".to_string()),
    );

    println!("📋 Plan inputs:");
    for (key, value) in &context.cross_plan_params {
        println!("   {} = {:?}", key, value);
    }

    // Execute the plan using CCOS's public API
    println!("\n🚀 Executing plan...");
    match ccos.validate_and_execute_plan(plan, &context).await {
        Ok(result) => {
            println!("✅ Plan executed successfully!");
            println!("📊 Result: {:?}", result);
        }
        Err(e) => {
            eprintln!("❌ Plan execution failed");
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    }

    Ok(())
}

async fn try_register_alias_capability(
    marketplace: &Arc<ccos::capability_marketplace::CapabilityMarketplace>,
    alias_id: &str,
) -> bool {
    let tokens: Vec<&str> = alias_id.split('.').collect();
    if tokens.is_empty() {
        return false;
    }

    let existing_caps = marketplace.list_capabilities().await;
    let matching = existing_caps.iter().find(|manifest| {
        manifest.id != alias_id
            && tokens
                .iter()
                .all(|token| manifest.id.contains(token) || manifest.name.contains(token))
            && manifest.metadata.contains_key("mcp_server_url")
    });

    let Some(original) = matching else {
        return false;
    };

    let mut alias_manifest = original.clone();
    alias_manifest.id = alias_id.to_string();
    alias_manifest
        .metadata
        .insert("alias_of".to_string(), original.id.clone());
    alias_manifest.metadata.insert(
        "alias_created_by".to_string(),
        "test_plan_execution".to_string(),
    );
    alias_manifest.name = format!("{} (alias)", alias_manifest.name);

    match marketplace
        .register_capability_manifest(alias_manifest)
        .await
    {
        Ok(_) => {
            println!(
                "📦 Registered alias: {} → {}",
                alias_id,
                original.id.as_str()
            );
            true
        }
        Err(e) => {
            eprintln!(
                "⚠️  Failed to register alias {} (pointing to {}): {}",
                alias_id, original.id, e
            );
            false
        }
    }
}
