use crate::cli::CliContext;
use crate::cli::OutputFormat;
use crate::cli::OutputFormatter;
use crate::utils::value_conversion;
use clap::Args;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use serde_json::Value;
use std::sync::Arc;

#[derive(Args)]
pub struct CallArgs {
    /// Capability ID to execute
    pub capability_id: String,

    /// Arguments as JSON or key=value pairs
    #[arg(trailing_var_arg = true)]
    pub args: Vec<String>,
}

pub async fn execute(ctx: &mut CliContext, args: CallArgs) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    // Try to load agent config
    let config_path = "config/agent_config.toml";
    let agent_config = match crate::examples_common::builder::load_agent_config(config_path) {
        Ok(cfg) => {
            ctx.status(&format!("Loaded agent config from {}", config_path));
            Some(cfg)
        }
        Err(e) => {
            ctx.status(&format!(
                "Warning: Could not load agent config from {}: {}",
                config_path, e
            ));
            ctx.status("Using default configuration.");
            None
        }
    };

    // Initialize CCOS to get full capability set (including planner v2)
    // We use CCOS::new to ensure all components (arbiter, catalog, etc.) are wired up
    let debug_callback = if ctx.verbose {
        Some(Arc::new(|msg: String| {
            eprintln!("[DEBUG] {}", msg);
        }) as Arc<dyn Fn(String) + Send + Sync>)
    } else {
        None
    };

    let ccos = Arc::new(
        crate::ccos_core::CCOS::new_with_agent_config_and_configs_and_debug_callback(
            crate::intent_graph::IntentGraphConfig::default(),
            None,
            agent_config,
            debug_callback,
        )
        .await?,
    );
    ccos.init_v2_capabilities().await?;
    let marketplace = ccos.capability_marketplace.clone();

    // Load capabilities from approved servers directory
    let approved_dir = std::path::Path::new("capabilities/servers/approved");
    let mut total_loaded = 0;
    if approved_dir.exists() {
        let count = marketplace
            .import_capabilities_from_rtfs_dir_recursive(approved_dir)
            .await?;
        total_loaded += count;
    }

    // Load capabilities from samples directory (for testing)
    let samples_dir = std::path::Path::new("capabilities/samples");
    if samples_dir.exists() {
        let count = marketplace
            .import_capabilities_from_rtfs_dir_recursive(samples_dir)
            .await?;
        total_loaded += count;
    }

    // Register native capabilities (ccos.cli.*)
    crate::ops::native::register_native_capabilities(&marketplace).await?;

    // Show compact summary
    ctx.status(&format!("Loaded {} capabilities + natives", total_loaded));

    // Parse arguments
    let input_value = parse_args(&args.args)?;

    ctx.status(&format!("Calling capability: {}", args.capability_id));
    if ctx.verbose {
        ctx.status(&format!(
            "Input: {}",
            serde_json::to_string_pretty(&input_value).unwrap_or_default()
        ));
    }

    // Execute capability
    // Need to convert JSON value to RTFS value for marketplace execution
    // json_to_rtfs_value returns a generic Value, not Result, based on common patterns.
    // Wait, the error said it returns Result.
    let rtfs_input = value_conversion::json_to_rtfs_value(&input_value);

    // Check if it's a Result or not. The error said: found `&Result<...>`
    // So I need to handle it.
    let rtfs_input = match rtfs_input {
        Ok(v) => v,
        Err(e) => return Err(RuntimeError::Generic(format!("Conversion failed: {}", e))),
    };

    match marketplace
        .execute_capability(&args.capability_id, &rtfs_input)
        .await
    {
        Ok(result) => {
            let json_result = value_conversion::rtfs_value_to_json(&result)?;
            match ctx.output_format {
                OutputFormat::Json => {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json_result).unwrap_or_default()
                    );
                }
                _ => {
                    formatter.success("Execution successful");
                    formatter.section("Result");
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json_result).unwrap_or_default()
                    );
                }
            }
        }
        Err(e) => {
            return Err(RuntimeError::Generic(format!("Execution failed: {}", e)));
        }
    }

    Ok(())
}

fn parse_args(args: &[String]) -> RuntimeResult<Value> {
    if args.is_empty() {
        return Ok(Value::Object(serde_json::Map::new()));
    }

    // Check if first arg is JSON
    if args.len() == 1 {
        if let Ok(v) = serde_json::from_str::<Value>(&args[0]) {
            return Ok(v);
        }
    }

    // Parse key=value pairs (standard HTTP query param style: q=London)
    // Keys are automatically prefixed with ':' to become RTFS keywords
    let mut map = serde_json::Map::new();
    for arg in args {
        if let Some((key, val_str)) = arg.split_once('=') {
            let value = if let Ok(v) = serde_json::from_str::<Value>(val_str) {
                v
            } else {
                Value::String(val_str.to_string())
            };
            // Normalize key: add ':' prefix if not already present for RTFS keyword conversion
            let normalized_key = if key.starts_with(':') {
                key.to_string()
            } else {
                format!(":{}", key)
            };
            map.insert(normalized_key, value);
        } else {
            return Err(RuntimeError::Generic(format!(
                "Invalid argument format: '{}'. Use key=value format (e.g., q=London)",
                arg
            )));
        }
    }

    Ok(Value::Object(map))
}
