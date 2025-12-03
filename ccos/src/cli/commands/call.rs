use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::cli::OutputFormat;
use clap::Args;
use rtfs::runtime::error::{RuntimeResult, RuntimeError};
use crate::capability_marketplace::CapabilityMarketplace;
use crate::capabilities::registry::CapabilityRegistry;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::Value;
use crate::utils::value_conversion;

#[derive(Args)]
pub struct CallArgs {
    /// Capability ID to execute
    pub capability_id: String,

    /// Arguments as JSON or key=value pairs
    #[arg(trailing_var_arg = true)]
    pub args: Vec<String>,
}

pub async fn execute(
    ctx: &mut CliContext,
    args: CallArgs,
) -> RuntimeResult<()> {
    let formatter = OutputFormatter::new(ctx.output_format);

    // Initialize marketplace
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));

    // Load capabilities from approved servers directory
    let approved_dir = std::path::Path::new("capabilities/servers/approved");
    if approved_dir.exists() {
        ctx.status("Loading capabilities from approved servers...");
        let count = marketplace.import_capabilities_from_rtfs_dir_recursive(approved_dir).await?;
        if count > 0 {
            ctx.status(&format!("Loaded {} capabilities from approved servers", count));
        } else {
            ctx.status("Warning: No capabilities loaded from approved servers");
        }
    }

    // Register native capabilities (ccos.cli.*)
    ctx.status("Registering native CLI capabilities...");
    crate::ops::native::register_native_capabilities(&marketplace).await?;

    // Parse arguments
    let input_value = parse_args(&args.args)?;

    ctx.status(&format!("Calling capability: {}", args.capability_id));
    if ctx.verbose {
        ctx.status(&format!("Input: {}", serde_json::to_string_pretty(&input_value).unwrap_or_default()));
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

    match marketplace.execute_capability(&args.capability_id, &rtfs_input).await {
        Ok(result) => {
            let json_result = value_conversion::rtfs_value_to_json(&result)?;
            match ctx.output_format {
                OutputFormat::Json => {
                    println!("{}", serde_json::to_string_pretty(&json_result).unwrap_or_default());
                }
                _ => {
                    formatter.success("Execution successful");
                    formatter.section("Result");
                    println!("{}", serde_json::to_string_pretty(&json_result).unwrap_or_default());
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
             return Err(RuntimeError::Generic(format!("Invalid argument format: '{}'. Use key=value format (e.g., q=London)", arg)));
        }
    }

    Ok(Value::Object(map))
}

