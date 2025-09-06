//! GitHub MCP Capability Demo
//! 
//! This example demonstrates how to use the GitHub MCP capability to:
//! - List GitHub issues
//! - Create new issues
//! - Close existing issues
//! 
//! Usage:
//!   cargo run --example github_mcp_demo -- --token YOUR_GITHUB_TOKEN

use rtfs_compiler::runtime::capabilities::providers::github_mcp::GitHubMCPCapability;
use rtfs_compiler::runtime::capabilities::provider::{ExecutionContext, ProviderConfig, CapabilityProvider};
use rtfs_compiler::runtime::Value as RuntimeValue;
use rtfs_compiler::ast::{MapKey, Expression};
use std::collections::HashMap;
use std::env;
use std::time::Duration;
use clap::Parser;

#[derive(Parser)]
#[command(name = "github_mcp_demo")]
#[command(about = "Demo of GitHub MCP capability")]
struct Args {
    /// GitHub API token
    #[arg(long)]
    token: Option<String>,
    
    /// Repository owner
    #[arg(long, default_value = "mandubian")]
    owner: String,
    
    /// Repository name
    #[arg(long, default_value = "ccos")]
    repo: String,
    
    /// Action to perform: list, create, close
    #[arg(long, default_value = "list")]
    action: String,
    
    /// Issue number (for close action)
    #[arg(long)]
    issue_number: Option<u64>,
    
    /// Issue title (for create action)
    #[arg(long)]
    title: Option<String>,
    
    /// Issue body (for create action)
    #[arg(long)]
    body: Option<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    // Get GitHub token
    let token = args.token
        .or_else(|| env::var("GITHUB_TOKEN").ok())
        .expect("GitHub token required. Set GITHUB_TOKEN environment variable or use --token");
    
    println!("🔧 Initializing GitHub MCP Capability...");
    
    // Create GitHub MCP capability
    let mut capability = GitHubMCPCapability::new(Some(token));
    
    // Initialize the capability
    let config = ProviderConfig {
        config: Expression::Map(HashMap::new()),
    };
    capability.initialize(&config)?;
    
    // Check health
    let health = capability.health_check();
    println!("✅ Health Status: {:?}", health);
    
    // Show metadata
    let metadata = capability.metadata();
    println!("📋 Provider: {} v{}", metadata.name, metadata.version);
    println!("📝 Description: {}", metadata.description);
    
    // List available capabilities
    let capabilities = capability.list_capabilities();
    println!("🔧 Available capabilities:");
    for cap in &capabilities {
        println!("  - {}: {}", cap.id, cap.description);
    }
    
    // Create execution context
    let context = ExecutionContext {
        trace_id: "demo_request".to_string(),
        timeout: Duration::from_secs(30),
    };
    
    match args.action.as_str() {
        "list" => {
            println!("\n📋 Listing issues for {}/{}...", args.owner, args.repo);
            
            let inputs = RuntimeValue::Map({
                let mut map = HashMap::new();
                map.insert(MapKey::String("tool".to_string()), RuntimeValue::String("list_issues".to_string()));
                map.insert(MapKey::String("arguments".to_string()), RuntimeValue::Map({
                    let mut args_map = HashMap::new();
                    args_map.insert(MapKey::String("owner".to_string()), RuntimeValue::String(args.owner));
                    args_map.insert(MapKey::String("repo".to_string()), RuntimeValue::String(args.repo));
                    args_map.insert(MapKey::String("state".to_string()), RuntimeValue::String("open".to_string()));
                    args_map.insert(MapKey::String("per_page".to_string()), RuntimeValue::Integer(10));
                    args_map
                }));
                map
            });
            
            let result = capability.execute_capability("github.list_issues", &inputs, &context)?;
            println!("✅ Result: {}", result);
        }
        
        "create" => {
            let title = args.title.expect("--title required for create action");
            let body = args.body.unwrap_or_else(|| "Created via GitHub MCP Demo".to_string());
            
            println!("\n📝 Creating issue in {}/{}...", args.owner, args.repo);
            println!("   Title: {}", title);
            println!("   Body: {}", body);
            
            let inputs = RuntimeValue::Map({
                let mut map = HashMap::new();
                map.insert(MapKey::String("tool".to_string()), RuntimeValue::String("create_issue".to_string()));
                map.insert(MapKey::String("arguments".to_string()), RuntimeValue::Map({
                    let mut args_map = HashMap::new();
                    args_map.insert(MapKey::String("owner".to_string()), RuntimeValue::String(args.owner));
                    args_map.insert(MapKey::String("repo".to_string()), RuntimeValue::String(args.repo));
                    args_map.insert(MapKey::String("title".to_string()), RuntimeValue::String(title));
                    args_map.insert(MapKey::String("body".to_string()), RuntimeValue::String(body));
                    args_map.insert(MapKey::String("labels".to_string()), RuntimeValue::Vector(vec![
                        RuntimeValue::String("demo".to_string()),
                        RuntimeValue::String("mcp".to_string()),
                    ]));
                    args_map
                }));
                map
            });
            
            let result = capability.execute_capability("github.create_issue", &inputs, &context)?;
            println!("✅ Result: {}", result);
        }
        
        "close" => {
            let issue_number = args.issue_number.expect("--issue-number required for close action");
            
            println!("\n🔒 Closing issue #{} in {}/{}...", issue_number, args.owner, args.repo);
            
            let inputs = RuntimeValue::Map({
                let mut map = HashMap::new();
                map.insert(MapKey::String("tool".to_string()), RuntimeValue::String("close_issue".to_string()));
                map.insert(MapKey::String("arguments".to_string()), RuntimeValue::Map({
                    let mut args_map = HashMap::new();
                    args_map.insert(MapKey::String("owner".to_string()), RuntimeValue::String(args.owner));
                    args_map.insert(MapKey::String("repo".to_string()), RuntimeValue::String(args.repo));
                    args_map.insert(MapKey::String("issue_number".to_string()), RuntimeValue::Integer(issue_number as i64));
                    args_map.insert(MapKey::String("comment".to_string()), RuntimeValue::String("Closed via GitHub MCP Demo".to_string()));
                    args_map
                }));
                map
            });
            
            let result = capability.execute_capability("github.close_issue", &inputs, &context)?;
            println!("✅ Result: {}", result);
        }
        
        _ => {
            println!("❌ Unknown action: {}. Use: list, create, or close", args.action);
            println!("\nUsage examples:");
            println!("  # List issues");
            println!("  cargo run --example github_mcp_demo -- --action list");
            println!("\n  # Create issue");
            println!("  cargo run --example github_mcp_demo -- --action create --title 'Test Issue' --body 'Test body'");
            println!("\n  # Close issue");
            println!("  cargo run --example github_mcp_demo -- --action close --issue-number 123");
        }
    }
    
    println!("\n🎉 GitHub MCP Demo completed!");
    Ok(())
} 