//! CLI tool for resolving missing capability dependencies
//!
//! This tool implements advanced CLI commands for managing missing capability resolution,
//! monitoring, and observability as part of Phase 8 enhancements.

use ccos::capability_marketplace::types::{CapabilityManifest, LocalCapability, ProviderType};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::checkpoint_archive::CheckpointArchive;
use ccos::synthesis::feature_flags::MissingCapabilityConfig;
use ccos::synthesis::missing_capability_resolver::{MissingCapabilityResolver, ResolverConfig};
use clap::{Parser, Subcommand};
use rtfs::runtime::values::Value;
use serde_json;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use tokio::time::sleep;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
#[clap(name = "resolve-deps")]
#[clap(about = "CCOS Missing Capability Resolution Tool")]
struct Args {
    /// Command to execute
    #[clap(subcommand)]
    command: Command,

    /// Enable verbose logging
    #[clap(short, long, global = true)]
    verbose: bool,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Resolve missing capability dependencies
    Resolve {
        /// Capability ID to resolve
        #[clap(short, long)]
        capability_id: String,
        /// Force resolution even if capability exists
        #[clap(short, long)]
        force: bool,
    },
    /// Resume execution after capability resolution
    Resume {
        /// Checkpoint ID to resume from
        #[clap(short = 'k', long)]
        checkpoint_id: String,

        /// Capability ID that was resolved
        #[clap(short = 'c', long)]
        capability_id: String,
    },
    /// List pending capabilities awaiting resolution
    ListPending {
        /// Filter by status (pending/in_progress/failed)
        #[clap(short, long)]
        filter: Option<String>,
    },
    /// Show resolution statistics and metrics
    Stats {
        /// Time range for statistics (e.g., '1h', '1d', '7d')
        #[clap(short, long)]
        time_range: Option<String>,
    },
    /// Monitor resolution queue in real-time
    Monitor {
        /// Update interval in seconds
        #[clap(short, long, default_value = "5")]
        interval: u64,
        /// Run continuously until interrupted
        #[clap(short, long)]
        continuous: bool,
    },
    /// Validate a capability before registration
    Validate {
        /// Capability ID to validate
        #[clap(short, long)]
        capability_id: String,
        /// Security validation level (low/medium/high)
        #[clap(short, long)]
        security_level: Option<String>,
    },
    /// Search for capabilities across all sources
    Search {
        /// Search query
        #[clap(short, long)]
        query: String,
        /// Search source (mcp/registry/local/all)
        #[clap(short, long, default_value = "all")]
        source: String,
        /// Maximum number of results
        #[clap(short, long, default_value = "10")]
        limit: usize,
    },
    /// Export resolution data for analysis
    Export {
        /// Output format (json/csv/yaml)
        #[clap(short, long, default_value = "json")]
        format: String,
        /// Output file path
        #[clap(short, long)]
        output: Option<String>,
    },
    /// Show detailed capability information
    Info {
        /// Capability ID to inspect
        #[clap(short, long)]
        capability_id: String,
    },
    /// Clean up old resolution data
    Cleanup {
        /// Days of data to keep (default: 30)
        #[clap(short, long, default_value = "30")]
        days: u32,
        /// Dry run (don't actually delete)
        #[clap(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();

    // Initialize the capability marketplace and resolver
    let registry = Arc::new(RwLock::new(
        rtfs::runtime::capabilities::registry::CapabilityRegistry::new(),
    ));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));

    // Bootstrap the marketplace with some test capabilities
    bootstrap_test_capabilities(&marketplace).await?;

    let config = ResolverConfig {
        max_attempts: 3,
        auto_resolve: true,
        verbose_logging: true, // Force verbose logging to see what's happening
    };

    let checkpoint_archive = Arc::new(CheckpointArchive::new());

    // Enable missing capability resolution features for CLI tool
    let mut feature_config = MissingCapabilityConfig::from_env();
    feature_config.feature_flags.enabled = true;
    feature_config.feature_flags.runtime_detection = true;
    feature_config.feature_flags.auto_resolution = true;
    feature_config.feature_flags.mcp_registry_enabled = true;
    feature_config.feature_flags.importers_enabled = true;
    feature_config.feature_flags.http_wrapper_enabled = true;
    feature_config.feature_flags.llm_synthesis_enabled = true;
    feature_config.feature_flags.web_search_enabled = true;
    feature_config.feature_flags.continuous_resolution = true;
    feature_config.feature_flags.auto_resume_enabled = true;
    feature_config.feature_flags.audit_logging_enabled = true;
    feature_config.feature_flags.validation_enabled = true;
    feature_config.feature_flags.cli_tooling_enabled = true;

    let resolver = Arc::new(MissingCapabilityResolver::new(
        marketplace.clone(),
        checkpoint_archive,
        config,
        feature_config,
    ));

    match args.command {
        Command::Resolve {
            capability_id,
            force,
        } => {
            handle_resolve(&resolver, &marketplace, &capability_id, force).await?;
        }
        Command::Resume {
            checkpoint_id,
            capability_id,
        } => {
            handle_resume(&resolver, &checkpoint_id, &capability_id).await?;
        }
        Command::ListPending { filter } => {
            handle_list_pending(&resolver, filter.as_deref()).await?;
        }
        Command::Stats { time_range } => {
            handle_stats(&resolver, time_range.as_deref()).await?;
        }
        Command::Monitor {
            interval,
            continuous,
        } => {
            handle_monitor(&resolver, interval, continuous).await?;
        }
        Command::Validate {
            capability_id,
            security_level,
        } => {
            handle_validate(&resolver, &capability_id, security_level.as_deref()).await?;
        }
        Command::Search {
            query,
            source,
            limit,
        } => {
            handle_search(&resolver, &query, &source, limit).await?;
        }
        Command::Export { format, output } => {
            handle_export(&resolver, &format, output.as_deref()).await?;
        }
        Command::Info { capability_id } => {
            handle_info(&marketplace, &capability_id).await?;
        }
        Command::Cleanup { days, dry_run } => {
            handle_cleanup(&resolver, days, dry_run).await?;
        }
    }

    Ok(())
}

async fn handle_resolve(
    resolver: &Arc<MissingCapabilityResolver>,
    marketplace: &Arc<CapabilityMarketplace>,
    capability_id: &str,
    force: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!(
        "🔍 Resolving dependencies for capability: {}",
        capability_id
    );

    if force {
        println!("⚡ Force mode enabled - will attempt resolution even if capability exists");
    }

    // Simulate a missing capability request
    let mut context = HashMap::new();
    context.insert("plan_id".to_string(), "test_plan".to_string());
    context.insert("intent_id".to_string(), "test_intent".to_string());
    context.insert("force_resolution".to_string(), force.to_string());

    println!("📋 Adding missing capability to resolution queue...");
    println!(
        "🔍 DEBUG: Attempting to resolve capability '{}'",
        capability_id
    );

    // Check if capability already exists in marketplace and track initial state
    let existing_capabilities = marketplace.list_capabilities().await;
    let capability_ids: Vec<String> = existing_capabilities.iter().map(|c| c.id.clone()).collect();
    if capability_ids.contains(&capability_id.to_string()) {
        println!(
            "❌ ERROR: Capability '{}' already exists in marketplace!",
            capability_id
        );
        println!("📋 Available capabilities: {:?}", capability_ids);
        return Ok(());
    } else {
        println!(
            "✅ Capability '{}' is missing from marketplace - proceeding with resolution",
            capability_id
        );
    }

    let before_capability_ids: HashSet<String> = capability_ids.iter().cloned().collect();

    resolver.handle_missing_capability(
        capability_id.to_string(),
        vec![Value::String("test_arg".to_string())],
        context,
    )?;

    println!("⚙️ Processing resolution queue...");
    resolver.process_queue().await?;

    // Get statistics
    let stats = resolver.get_stats();
    println!("📊 Resolution Statistics:");
    println!("   Pending: {}", stats.pending_count);
    println!("   In Progress: {}", stats.in_progress_count);
    println!("   Failed: {}", stats.failed_count);
    println!("   Success Rate: {:.1}%", calculate_success_rate(&stats));

    let post_resolution_capabilities = marketplace.list_capabilities().await;
    let mut reported_path = false;
    for capability in &post_resolution_capabilities {
        if !before_capability_ids.contains(&capability.id) {
            if let Some(storage_path) = capability.metadata.get("storage_path") {
                println!(
                    "📁 Capability '{}' stored at: {}",
                    capability.id, storage_path
                );
                reported_path = true;
            }
        }
    }

    if !reported_path {
        if let Some(resolved_capability) = post_resolution_capabilities
            .iter()
            .find(|cap| cap.id == capability_id)
        {
            if let Some(storage_path) = resolved_capability.metadata.get("storage_path") {
                println!("📁 Capability stored at: {}", storage_path);
            }
        }
    }

    println!("✅ Dependency resolution completed!");
    Ok(())
}

async fn handle_resume(
    resolver: &Arc<MissingCapabilityResolver>,
    checkpoint_id: &str,
    capability_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔄 Resuming execution from checkpoint: {}", checkpoint_id);
    println!("📦 Resolved capability: {}", capability_id);

    // Check if checkpoint exists and capability is available
    let checkpoint_archive = resolver.get_checkpoint_archive();
    let checkpoints = checkpoint_archive.get_pending_auto_resume_checkpoints();

    if let Some(checkpoint) = checkpoints.iter().find(|c| c.plan_id == checkpoint_id) {
        println!("✅ Checkpoint found: {}", checkpoint.plan_id);
        println!(
            "   Missing capabilities: {:?}",
            checkpoint.missing_capabilities
        );
        println!("   Auto-resume enabled: {}", checkpoint.auto_resume_enabled);

        if checkpoint
            .missing_capabilities
            .contains(&capability_id.to_string())
        {
            println!(
                "🎯 Capability {} is required for this checkpoint",
                capability_id
            );
            println!("⚠️ Checkpoint resume logic not yet implemented - this is a placeholder");
        } else {
            println!(
                "⚠️ Capability {} not required for this checkpoint",
                capability_id
            );
        }
    } else {
        println!(
            "❌ Checkpoint {} not found or not pending resume",
            checkpoint_id
        );
    }

    Ok(())
}

async fn handle_list_pending(
    resolver: &Arc<MissingCapabilityResolver>,
    filter: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📋 Listing pending capabilities awaiting resolution...");

    let stats = resolver.get_stats();

    if let Some(filter) = filter {
        println!("🔍 Filter: {}", filter);
    }

    println!("📊 Pending Capabilities:");
    println!("   Pending: {}", stats.pending_count);
    println!("   In Progress: {}", stats.in_progress_count);
    println!("   Failed: {}", stats.failed_count);
    println!(
        "   Total: {}",
        stats.pending_count + stats.in_progress_count + stats.failed_count
    );

    // Show detailed breakdown if verbose
    if stats.pending_count > 0 {
        println!("\n📝 Pending Details:");
        println!("   • Capabilities awaiting initial resolution");
        println!("   • Checkpoint status: waiting for dependencies");
    }

    if stats.in_progress_count > 0 {
        println!("\n⚙️ In Progress Details:");
        println!("   • Capabilities currently being resolved");
        println!("   • Discovery and validation in progress");
    }

    if stats.failed_count > 0 {
        println!("\n❌ Failed Details:");
        println!("   • Capabilities that failed resolution");
        println!("   • May require manual intervention");
    }

    Ok(())
}

async fn handle_stats(
    resolver: &Arc<MissingCapabilityResolver>,
    time_range: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📊 Resolution Statistics and Metrics");

    if let Some(range) = time_range {
        println!("📅 Time Range: {}", range);
    } else {
        println!("📅 Time Range: All time");
    }

    let stats = resolver.get_stats();

    println!("\n🎯 Resolution Metrics:");
    println!("   Pending: {}", stats.pending_count);
    println!("   In Progress: {}", stats.in_progress_count);
    println!("   Failed: {}", stats.failed_count);
    println!("   Success Rate: {:.1}%", calculate_success_rate(&stats));

    let total = stats.pending_count + stats.in_progress_count + stats.failed_count;
    if total > 0 {
        println!("\n📈 Breakdown:");
        println!(
            "   Pending: {:.1}%",
            (stats.pending_count as f64 / total as f64) * 100.0
        );
        println!(
            "   In Progress: {:.1}%",
            (stats.in_progress_count as f64 / total as f64) * 100.0
        );
        println!(
            "   Failed: {:.1}%",
            (stats.failed_count as f64 / total as f64) * 100.0
        );
    }

    println!("\n⏱️ Performance Metrics:");
    println!("   Average Resolution Time: N/A (not yet implemented)");
    println!("   Peak Resolution Rate: N/A (not yet implemented)");
    println!("   Discovery Success Rate: N/A (not yet implemented)");

    Ok(())
}

async fn handle_monitor(
    resolver: &Arc<MissingCapabilityResolver>,
    interval: u64,
    continuous: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📡 Monitoring resolution queue (interval: {}s)", interval);

    if continuous {
        println!("🔄 Running continuously - press Ctrl+C to stop");
    }

    loop {
        let stats = resolver.get_stats();
        let timestamp = chrono::Utc::now().format("%H:%M:%S");

        println!("\n[{}] 📊 Queue Status:", timestamp);
        println!(
            "   Pending: {} | In Progress: {} | Failed: {}",
            stats.pending_count, stats.in_progress_count, stats.failed_count
        );

        if stats.pending_count > 0 || stats.in_progress_count > 0 {
            println!("   Status: Active");
        } else {
            println!("   Status: Idle");
        }

        if !continuous {
            break;
        }

        sleep(Duration::from_secs(interval)).await;
    }

    Ok(())
}

async fn handle_validate(
    resolver: &Arc<MissingCapabilityResolver>,
    capability_id: &str,
    security_level: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Validating capability: {}", capability_id);

    let security_level = security_level.unwrap_or("medium");
    println!("🛡️ Security Level: {}", security_level);

    // TODO: Implement actual validation logic
    println!("⚠️ Validation logic not yet implemented - this is a placeholder");
    println!("   Capability ID: {}", capability_id);
    println!("   Security Level: {}", security_level);
    println!("   Would perform:");
    println!("     • Static code analysis");
    println!("     • Security vulnerability scan");
    println!("     • Governance policy compliance check");
    println!("     • Performance impact assessment");

    Ok(())
}

async fn handle_search(
    resolver: &Arc<MissingCapabilityResolver>,
    query: &str,
    source: &str,
    limit: usize,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🔍 Searching for capabilities: '{}'", query);
    println!("📍 Source: {}", source);
    println!("📊 Limit: {}", limit);

    // TODO: Implement actual search logic across different sources
    println!("⚠️ Search functionality not yet implemented - this is a placeholder");

    match source {
        "mcp" => {
            println!("   Would search MCP Registry for: {}", query);
        }
        "registry" => {
            println!("   Would search local registry for: {}", query);
        }
        "local" => {
            println!("   Would search local capabilities for: {}", query);
        }
        "all" => {
            println!("   Would search all sources for: {}", query);
        }
        _ => {
            println!("   Unknown source: {}", source);
        }
    }

    Ok(())
}

async fn handle_export(
    resolver: &Arc<MissingCapabilityResolver>,
    format: &str,
    output: Option<&str>,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("📤 Exporting resolution data");
    println!("📋 Format: {}", format);

    if let Some(output_path) = output {
        println!("📁 Output: {}", output_path);
    } else {
        println!("📁 Output: stdout");
    }

    // TODO: Implement actual export logic
    println!("⚠️ Export functionality not yet implemented - this is a placeholder");

    let stats = resolver.get_stats();
    let export_data = serde_json::json!({
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "statistics": {
            "pending": stats.pending_count,
            "in_progress": stats.in_progress_count,
            "failed": stats.failed_count
        },
        "format": format
    });

    match format {
        "json" => {
            if let Some(output_path) = output {
                std::fs::write(output_path, serde_json::to_string_pretty(&export_data)?)?;
                println!("✅ Data exported to {}", output_path);
            } else {
                println!("{}", serde_json::to_string_pretty(&export_data)?);
            }
        }
        "csv" => {
            println!("   CSV export not yet implemented");
        }
        "yaml" => {
            println!("   YAML export not yet implemented");
        }
        _ => {
            println!("   Unknown format: {}", format);
        }
    }

    Ok(())
}

async fn handle_info(
    marketplace: &CapabilityMarketplace,
    capability_id: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("ℹ️ Capability Information: {}", capability_id);

    // Check if capability exists in marketplace
    if let Some(capability) = marketplace.get_capability(capability_id).await {
        println!("✅ Capability found in marketplace");
        println!("   Name: {}", capability.name);
        println!("   Description: {}", capability.description);
        println!("   Version: {}", capability.version);
        println!("   Provider: {:?}", capability.provider);

        if !capability.permissions.is_empty() {
            println!("   Permissions: {:?}", capability.permissions);
        }

        if !capability.effects.is_empty() {
            println!("   Effects: {:?}", capability.effects);
        }

        if !capability.metadata.is_empty() {
            println!("   Metadata: {:?}", capability.metadata);
        }

        if let Some(agent_metadata) = &capability.agent_metadata {
            println!("   Agent Metadata: {:?}", agent_metadata);
        }
    } else {
        println!("❌ Capability not found in marketplace");
        println!("   This capability may need to be resolved");
    }

    Ok(())
}

async fn handle_cleanup(
    resolver: &Arc<MissingCapabilityResolver>,
    days: u32,
    dry_run: bool,
) -> Result<(), Box<dyn std::error::Error>> {
    println!("🧹 Cleaning up old resolution data");
    println!("📅 Keeping data from last {} days", days);

    if dry_run {
        println!("🔍 DRY RUN - no data will be deleted");
    }

    // TODO: Implement actual cleanup logic
    println!("⚠️ Cleanup functionality not yet implemented - this is a placeholder");
    println!("   Would clean up:");
    println!("     • Old resolution attempts");
    println!("     • Expired checkpoints");
    println!("     • Stale audit logs");
    println!("     • Temporary discovery data");

    if dry_run {
        println!("   (DRY RUN - no actual cleanup performed)");
    } else {
        println!("   (Cleanup would be performed)");
    }

    Ok(())
}

fn calculate_success_rate(stats: &ccos::synthesis::missing_capability_resolver::QueueStats) -> f64 {
    let total = stats.pending_count + stats.in_progress_count + stats.failed_count;
    if total == 0 {
        100.0
    } else {
        let successful = total - stats.failed_count;
        (successful as f64 / total as f64) * 100.0
    }
}

/// Bootstrap the marketplace with some test capabilities for demonstration
async fn bootstrap_test_capabilities(
    marketplace: &CapabilityMarketplace,
) -> Result<(), Box<dyn std::error::Error>> {
    // Add some test primitive capabilities
    let test_capabilities = vec![
        ("travel.hotels", "Hotel booking capability"),
        ("travel.attractions", "Tourist attractions capability"),
        ("food.recommendations", "Restaurant recommendations"),
        ("weather.current", "Current weather information"),
        ("data.json.parse", "JSON parsing utility"),
    ];

    for (capability_id, description) in &test_capabilities {
        let capability_id = capability_id.to_string();
        let description = description.to_string();
        let manifest = CapabilityManifest {
            id: capability_id.clone(),
            name: description.clone(),
            description: format!("Test capability: {}", description),
            version: "1.0.0".to_string(),
            provider: ProviderType::Local(LocalCapability {
                handler: Arc::new(move |_args| {
                    Ok(Value::String(format!("Test result from {}", capability_id)))
                }),
            }),
            input_schema: None,
            output_schema: None,
            attestation: None,
            provenance: None,
            permissions: vec![],
            effects: vec![],
            metadata: HashMap::new(),
            agent_metadata: None,
        };

        marketplace.register_capability_manifest(manifest).await?;
    }

    println!(
        "🚀 Bootstrapped marketplace with {} test capabilities",
        test_capabilities.len()
    );
    Ok(())
}
