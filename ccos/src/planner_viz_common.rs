use std::{error::Error, fs};

use crate::config::types::AgentConfig;
use crossterm::style::Stylize;
use rtfs::config::profile_selection::expand_profiles;

pub fn load_agent_config(path: &str) -> Result<AgentConfig, Box<dyn Error>> {
    let data = fs::read_to_string(path)?;
    let config = if path.ends_with(".json") {
        serde_json::from_str(&data)?
    } else {
        toml::from_str(&data)?
    };
    Ok(config)
}

pub fn print_architecture_summary(config: &AgentConfig, profile_name: Option<&str>) {
    println!("\n{}", "═".repeat(80).bold());
    println!(
        "{}",
        "🏗️  CCOS Smart Assistant - Architecture Summary"
            .bold()
            .cyan()
    );
    println!("{}", "═".repeat(80).bold());

    println!("\n{}", "📋 Architecture Overview".bold());
    println!("  ┌─────────────────────────────────────────────────────────────┐");
    println!("  │ User Goal → Intent Extraction → Plan Generation → Execution │");
    println!("  └─────────────────────────────────────────────────────────────┘");
    println!("\n  {} Flow:", "1.".bold());
    println!("     • Natural language goal → Intent (constraints, preferences)");
    println!("     • Intent → Plan generation (delegating arbiter)");
    println!("     • Plan → Capability discovery (aliases → marketplace → MCP)");
    println!("     • Resolver timelines show how missing tools are synthesized");
    println!("     • Final plan executes via orchestrator");

    println!("\n  {} Key Components:", "2.".bold());
    println!(
        "     • {}: Governs intent extraction and plan synthesis",
        "DelegatingCognitiveEngine".cyan()
    );
    println!(
        "     • {}: Runs marketplace/MCP discovery pipeline",
        "MissingCapabilityResolver".cyan()
    );
    println!(
        "     • {}: Stores and ranks capabilities",
        "CapabilityMarketplace".cyan()
    );
    println!(
        "     • {}: Tracks intent relationships and checkpoints",
        "IntentGraph".cyan()
    );

    let discovery = &config.discovery;
    println!("\n  {} Discovery/Search Settings:", "3.".bold());
    if discovery.use_embeddings {
        let model = discovery
            .embedding_model
            .as_deref()
            .or(discovery.local_embedding_model.as_deref())
            .unwrap_or("unspecified model");
        println!(
            "     • Embedding search: {} ({})",
            "enabled".green(),
            model.cyan()
        );
    } else {
        println!(
            "     • Embedding search: {} (keyword + schema heuristics)",
            "disabled".yellow()
        );
    }
    println!("     • Match threshold: {:.2}", discovery.match_threshold);
    println!(
        "     • Action verb weight / threshold: {:.2} / {:.2}",
        discovery.action_verb_weight, discovery.action_verb_threshold
    );
    println!(
        "     • Capability class weight: {:.2}",
        discovery.capability_class_weight
    );

    if let Some(llm_profiles) = &config.llm_profiles {
        // Convert CCOS AgentConfig to RTFS AgentConfig for expand_profiles
        // (types are identical, just in different crates)
        let rtfs_config: rtfs::config::types::AgentConfig = serde_json::from_value(
            serde_json::to_value(config).expect("Failed to serialize AgentConfig"),
        )
        .expect("Failed to deserialize AgentConfig");
        let (profiles, _meta, _why) = expand_profiles(&rtfs_config);
        println!("\n  {} LLM Profile:", "4.".bold());
        let chosen = profile_name
            .map(|s| s.to_string())
            .or_else(|| llm_profiles.default.clone())
            .or_else(|| profiles.first().map(|p| p.name.clone()));

        if let Some(name) = chosen {
            if let Some(profile) = profiles.iter().find(|p| p.name == name) {
                println!("     • Active profile: {}", name.cyan());
                println!("     • Provider: {}", profile.provider.as_str().cyan());
                println!("     • Model: {}", profile.model.as_str().cyan());
                if let Some(base) = &profile.base_url {
                    println!("     • Base URL: {}", base);
                }
            } else {
                println!("     • Active profile name: {} (details unavailable)", name);
            }
        } else {
            println!("     • No LLM profile configured");
        }
    }

    println!("\n{}", "═".repeat(80).bold());
}
