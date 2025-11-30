use crate::cli::CliContext;
use crate::cli::OutputFormatter;
use crate::cli::OutputFormat;
use clap::Args;
use rtfs::runtime::error::{RuntimeResult, RuntimeError};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::capabilities::registry::CapabilityRegistry;
use std::sync::Arc;
use tokio::sync::RwLock;
use serde_json::Value;

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

    // Parse arguments
    let input_value = parse_args(&args.args)?;

    ctx.status(&format!("Calling capability: {}", args.capability_id));
    if ctx.verbose {
        ctx.status(&format!("Input: {}", serde_json::to_string_pretty(&input_value).unwrap_or_default()));
    }

    // Execute capability
    // Need to convert JSON value to RTFS value for marketplace execution
    let rtfs_input = ccos::utils::value_conversion::json_to_rtfs_value(&input_value);
    
    match marketplace.execute_capability(&args.capability_id, &rtfs_input).await {
        Ok(result) => {
            let json_result = ccos::utils::value_conversion::rtfs_value_to_json(&result)?;
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

    // Parse key=value pairs
    let mut map = serde_json::Map::new();
    for arg in args {
        if let Some((key, val_str)) = arg.split_once('=') {
            let value = if let Ok(v) = serde_json::from_str::<Value>(val_str) {
                v
            } else {
                Value::String(val_str.to_string())
            };
            map.insert(key.to_string(), value);
        } else {
             return Err(RuntimeError::Generic(format!("Invalid argument format: {}", arg)));
        }
    }

    Ok(Value::Object(map))
}

