//! Config command - configuration management

use crate::cli::{CliContext, OutputFormat, OutputFormatter};
use clap::Subcommand;
use rtfs::runtime::error::RuntimeResult;

#[derive(Debug, Subcommand)]
pub enum ConfigCommand {
    /// Show current configuration
    Show {
        /// Show only a specific section
        #[arg(short, long)]
        section: Option<String>,
    },

    /// Validate configuration file
    Validate,

    /// Initialize a new configuration file
    Init {
        /// Output path for the configuration file
        #[arg(short, long, default_value = "agent_config.toml")]
        output: String,

        /// Overwrite existing file
        #[arg(short, long)]
        force: bool,
    },
}

pub async fn execute(ctx: &CliContext, command: ConfigCommand) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    match &command {
        ConfigCommand::Show { section } => {
            command.show_config(ctx, section.as_deref(), &formatter)
        }
        ConfigCommand::Validate => command.validate_config(ctx, &formatter),
        ConfigCommand::Init { output, force } => {
            command.init_config(output, *force, &formatter)
        }
    }
}

impl ConfigCommand {
    fn show_config(
        &self,
        ctx: &CliContext,
        section: Option<&str>,
        formatter: &OutputFormatter,
    ) -> RuntimeResult<()> {
        match ctx.output_format {
            OutputFormat::Json => {
                if let Some(section_name) = section {
                    match section_name {
                        "llm_profiles" | "llm-profiles" | "llm" => {
                            formatter.json(&ctx.config.llm_profiles);
                        }
                        "discovery" => {
                            formatter.json(&ctx.config.discovery);
                        }
                        "governance" => {
                            formatter.json(&ctx.config.governance);
                        }
                        "capabilities" => {
                            formatter.json(&ctx.config.capabilities);
                        }
                        "marketplace" => {
                            formatter.json(&ctx.config.marketplace);
                        }
                        "agent" => {
                            // Basic agent info
                            let info = serde_json::json!({
                                "agent_id": ctx.config.agent_id,
                                "profile": ctx.config.profile,
                                "version": ctx.config.version,
                            });
                            formatter.json(&info);
                        }
                        _ => {
                            formatter.error(&format!("Unknown section: {}", section_name));
                            formatter.list_item("Available sections: llm_profiles, discovery, governance, capabilities, marketplace, agent");
                            return Ok(());
                        }
                    }
                } else {
                    formatter.json(&ctx.config);
                }
            }
            _ => {
                formatter.section("Configuration");
                formatter.kv("Config file", ctx.config_path.to_string_lossy().as_ref());
                println!();

                if section.is_none() || section == Some("agent") {
                    formatter.section("Agent");
                    formatter.kv("Agent ID", &ctx.config.agent_id);
                    formatter.kv("Profile", &ctx.config.profile);
                    formatter.kv("Version", &ctx.config.version);
                    println!();
                }

                if section.is_none() || section == Some("llm_profiles") {
                    formatter.section("LLM Profiles");
                    if let Some(ref profiles_config) = ctx.config.llm_profiles {
                        if profiles_config.profiles.is_empty()
                            && profiles_config
                                .model_sets
                                .as_ref()
                                .map_or(true, |s| s.is_empty())
                        {
                            formatter.list_item("(none configured)");
                        } else {
                            // List explicit profiles
                            for profile in &profiles_config.profiles {
                                formatter.list_item(&format!(
                                    "{} ({}/{})",
                                    profile.name, profile.provider, profile.model
                                ));
                            }
                            // List model sets
                            if let Some(ref model_sets) = profiles_config.model_sets {
                                for set in model_sets {
                                    formatter.list_item(&format!(
                                        "[Model Set] {} ({} provider, {} models)",
                                        set.name,
                                        set.provider,
                                        set.models.len()
                                    ));
                                }
                            }
                        }
                        if let Some(ref default) = profiles_config.default {
                            formatter.kv("Default profile", default);
                        }
                    } else {
                        formatter.list_item("(no llm_profiles section)");
                    }
                    println!();
                }

                if section.is_none() || section == Some("discovery") {
                    formatter.section("Discovery");
                    formatter.kv(
                        "Match threshold",
                        &ctx.config.discovery.match_threshold.to_string(),
                    );
                    formatter.kv(
                        "Use embeddings",
                        &ctx.config.discovery.use_embeddings.to_string(),
                    );
                    if let Some(ref model) = ctx.config.discovery.embedding_model {
                        formatter.kv("Embedding model", model);
                    }
                    println!();
                }

                if section.is_none() || section == Some("governance") {
                    formatter.section("Governance");
                    if ctx.config.governance.policies.is_empty() {
                        formatter.list_item("(no policies configured)");
                    } else {
                        for (name, policy) in &ctx.config.governance.policies {
                            formatter.list_item(&format!(
                                "{}: risk_tier={}, approvals={}",
                                name, policy.risk_tier, policy.requires_approvals
                            ));
                        }
                    }
                    println!();
                }
            }
        }

        Ok(())
    }

    fn validate_config(&self, ctx: &CliContext, formatter: &OutputFormatter) -> RuntimeResult<()> {
        ctx.status(&format!(
            "Validating configuration: {:?}",
            ctx.config_path
        ));

        let warnings = ctx.validate_config()?;

        if warnings.is_empty() {
            formatter.success("Configuration is valid");
        } else {
            formatter.success("Configuration is valid with warnings:");
            for warning in warnings {
                formatter.warning(&warning);
            }
        }

        Ok(())
    }

    fn init_config(
        &self,
        output: &str,
        force: bool,
        formatter: &OutputFormatter,
    ) -> RuntimeResult<()> {
        let path = std::path::Path::new(output);

        if path.exists() && !force {
            formatter.error(&format!(
                "File already exists: {}. Use --force to overwrite.",
                output
            ));
            return Ok(());
        }

        let template = r#"# CCOS Agent Configuration
# Generated by: ccos config init

version = "1.0"
agent_id = "my-agent"
profile = "default"

# LLM Profiles
[llm_profiles]
default = "default"

[[llm_profiles.profiles]]
name = "default"
provider = "openrouter"
model = "anthropic/claude-sonnet-4-20250514"
api_key_env = "OPENROUTER_API_KEY"

# Discovery settings
[discovery]
match_threshold = 0.65
use_embeddings = false

# Governance settings
[governance]
enabled = true
default_policy = "balanced"
max_trust_level = 5
"#;

        std::fs::write(path, template).map_err(|e| {
            rtfs::runtime::error::RuntimeError::Generic(format!(
                "Failed to write config file: {}",
                e
            ))
        })?;

        formatter.success(&format!("Created configuration file: {}", output));
        Ok(())
    }
}
