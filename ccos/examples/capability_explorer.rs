//! Interactive Capability Explorer
//!
//! A smooth, elegant TUI for discovering, inspecting, and testing capabilities.
//!
//! Usage:
//!   cargo run --example capability_explorer -- --config config/agent_config.toml
//!
//! Features:
//! - Browse available registries (MCP servers, local, etc.)
//! - Search capabilities with hints/keywords
//! - Inspect schemas and metadata
//! - Test capabilities with live execution
//! - Beautiful colored output with progress indicators

use clap::Parser;
use colored::*;
use std::collections::HashMap;
use std::io::{self, Write};
use std::sync::Arc;

use ccos::capability_marketplace::mcp_discovery::MCPServerConfig;
use ccos::capability_marketplace::types::{CapabilityManifest, ProviderType};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::capabilities::registry::CapabilityRegistry;
use ccos::catalog::CatalogService;
use ccos::mcp::core::MCPDiscoveryService;
use ccos::mcp::types::DiscoveryOptions;
use rtfs::config::types::AgentConfig;
use tokio::sync::RwLock;

#[derive(Parser, Debug)]
#[command(name = "capability_explorer")]
#[command(about = "Interactive capability discovery and testing")]
struct Args {
    /// Path to agent config file
    #[arg(long, default_value = "config/agent_config.toml")]
    config: String,
    
    /// Start with a specific server
    #[arg(long)]
    server: Option<String>,
    
    /// Start with a search hint
    #[arg(long)]
    hint: Option<String>,
}

/// Main explorer state
struct CapabilityExplorer {
    discovery_service: Arc<MCPDiscoveryService>,
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
    discovered_tools: Vec<DiscoveredTool>,
    selected_capability: Option<CapabilityManifest>,
}

/// Discovered tool with metadata
#[derive(Clone)]
#[allow(dead_code)] // discovery_hint stored for potential future use
struct DiscoveredTool {
    manifest: CapabilityManifest,
    server_name: String,
    discovery_hint: Option<String>,
}

impl CapabilityExplorer {
    async fn new() -> Self {
        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let catalog = Arc::new(CatalogService::new());
        
        let discovery_service = Arc::new(
            MCPDiscoveryService::new()
                .with_marketplace(Arc::clone(&marketplace))
                .with_catalog(Arc::clone(&catalog))
        );
        
        Self {
            discovery_service,
            marketplace,
            catalog,
            discovered_tools: Vec::new(),
            selected_capability: None,
        }
    }
    
    fn print_banner(&self) {
        println!();
        println!("{}", "╔══════════════════════════════════════════════════════════════════════════════╗".cyan().bold());
        println!("{}", "║                     🔍 CCOS Capability Explorer 🔍                           ║".cyan().bold());
        println!("{}", "║                                                                              ║".cyan().bold());
        println!("{}", "║  Discover, inspect, and test capabilities from MCP servers and registries   ║".cyan().bold());
        println!("{}", "╚══════════════════════════════════════════════════════════════════════════════╝".cyan().bold());
        println!();
    }
    
    fn print_menu(&self) {
        println!("{}", "┌──────────────────────────────────────────────────────────────────────────────┐".white().dimmed());
        println!("│ {}                                                                       │", "Commands:".white().bold());
        println!("│                                                                              │");
        println!("│  {} - List available registries/servers                               │", "[1] servers".yellow());
        println!("│  {} - Discover capabilities from a server                             │", "[2] discover".yellow());
        println!("│  {} - Search capabilities by keyword/hint                             │", "[3] search".yellow());
        println!("│  {} - List discovered capabilities                                    │", "[4] list".yellow());
        println!("│  {} - Inspect a capability's details and schema                       │", "[5] inspect".yellow());
        println!("│  {} - Test/call a capability with inputs                              │", "[6] call".yellow());
        println!("│  {} - Show catalog statistics                                         │", "[7] stats".yellow());
        println!("│  {} - Display this menu                                               │", "[h] help".yellow());
        println!("│  {} - Exit the explorer                                               │", "[q] quit".yellow());
        println!("│                                                                              │");
        println!("{}", "└──────────────────────────────────────────────────────────────────────────────┘".white().dimmed());
        println!();
    }
    
    fn prompt(&self, msg: &str) -> String {
        print!("{} ", msg.green().bold());
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        input.trim().to_string()
    }
    
    async fn list_servers(&self) {
        println!();
        println!("{}", "📋 Available Registries & Servers".white().bold().underline());
        println!();
        
        let servers = self.discovery_service.list_known_servers();
        
        if servers.is_empty() {
            println!("  {} No servers configured.", "⚠".yellow());
            println!();
            println!("  {} Add servers in one of these ways:", "💡".cyan());
            println!("    • Edit {} to add MCP server configs", "config/mcp_introspection.toml".cyan());
            println!("    • Set environment variables like {}", "GITHUB_MCP_ENDPOINT".cyan());
            println!("    • Use {} with a custom endpoint", "--server <endpoint>".cyan());
        } else {
            println!("  {} {} server(s) found:", "✓".green(), servers.len());
            println!();
            
            for (i, server) in servers.iter().enumerate() {
                let auth_status = if server.auth_token.is_some() {
                    "🔐".to_string()
                } else {
                    "🔓".to_string()
                };
                
                println!("  {} [{}] {} {}", 
                    auth_status,
                    (i + 1).to_string().yellow(),
                    server.name.white().bold(),
                    format!("({})", server.endpoint).dimmed()
                );
            }
        }
        println!();
    }
    
    async fn discover_from_server(&mut self) {
        println!();
        println!("{}", "🔍 Discover Capabilities from Server".white().bold().underline());
        println!();
        
        let servers = self.discovery_service.list_known_servers();
        
        if servers.is_empty() {
            // Allow manual endpoint entry
            let endpoint = self.prompt("Enter server endpoint (or 'cancel'):");
            if endpoint == "cancel" || endpoint.is_empty() {
                return;
            }
            
            let name = self.prompt("Enter server name:");
            
            let config = MCPServerConfig {
                name: name.clone(),
                endpoint: endpoint.clone(),
                auth_token: std::env::var("MCP_AUTH_TOKEN").ok(),
                timeout_seconds: 30,
                protocol_version: "2024-11-05".to_string(),
            };
            
            self.perform_discovery(&config, None).await;
        } else {
            println!("  Select a server:");
            for (i, server) in servers.iter().enumerate() {
                println!("    [{}] {}", i + 1, server.name);
            }
            println!("    [0] Enter custom endpoint");
            println!();
            
            let choice = self.prompt("Server number:");
            
            if let Ok(idx) = choice.parse::<usize>() {
                if idx == 0 {
                    let endpoint = self.prompt("Enter server endpoint:");
                    let config = MCPServerConfig {
                        name: "custom".to_string(),
                        endpoint,
                        auth_token: std::env::var("MCP_AUTH_TOKEN").ok(),
                        timeout_seconds: 30,
                        protocol_version: "2024-11-05".to_string(),
                    };
                    self.perform_discovery(&config, None).await;
                } else if idx > 0 && idx <= servers.len() {
                    let config = servers[idx - 1].clone();
                    self.perform_discovery(&config, None).await;
                } else {
                    println!("  {} Invalid selection", "✗".red());
                }
            }
        }
    }
    
    async fn perform_discovery(&mut self, config: &MCPServerConfig, hint: Option<String>) {
        println!();
        println!("  {} Connecting to {}...", "⏳".yellow(), config.endpoint.cyan());
        
        let options = DiscoveryOptions {
            introspect_output_schemas: false,
            use_cache: true,
            register_in_marketplace: true,
            export_to_rtfs: false,
            export_directory: None,
            auth_headers: config.auth_token.as_ref().map(|token| {
                let mut headers = HashMap::new();
                headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                headers
            }),
            ..Default::default()
        };
        
        match self.discovery_service.discover_tools(config, &options).await {
            Ok(tools) => {
                println!("  {} Discovered {} tool(s)", "✓".green(), tools.len().to_string().white().bold());
                println!();
                
                // Filter by hint if provided
                let filtered_tools: Vec<_> = if let Some(ref h) = hint {
                    let h_lower = h.to_lowercase();
                    tools.iter()
                        .filter(|t| {
                            t.tool_name.to_lowercase().contains(&h_lower) ||
                            t.description.as_ref().map(|d| d.to_lowercase().contains(&h_lower)).unwrap_or(false)
                        })
                        .collect()
                } else {
                    tools.iter().collect()
                };
                
                if hint.is_some() && filtered_tools.len() < tools.len() {
                    println!("  {} Filtered to {} matching tool(s) for hint: '{}'", 
                        "🔎".cyan(),
                        filtered_tools.len().to_string().white().bold(),
                        hint.as_ref().unwrap().cyan()
                    );
                    println!();
                }
                
                // Convert to manifests and store
                for tool in &filtered_tools {
                    let manifest = self.discovery_service.tool_to_manifest(tool, config);
                    
                    // Print tool summary
                    println!("    {} {}", "•".green(), tool.tool_name.white().bold());
                    if let Some(desc) = &tool.description {
                        let short_desc = if desc.len() > 60 {
                            format!("{}...", &desc[..57])
                        } else {
                            desc.clone()
                        };
                        println!("      {}", short_desc.dimmed());
                    }
                    
                    // Store discovered tool
                    self.discovered_tools.push(DiscoveredTool {
                        manifest,
                        server_name: config.name.clone(),
                        discovery_hint: hint.clone(),
                    });
                }
                
                println!();
                println!("  {} Use '{}' to see all discovered capabilities", "💡".cyan(), "list".yellow());
            }
            Err(e) => {
                println!("  {} Discovery failed: {}", "✗".red(), e);
                println!();
                println!("  {} Possible causes:", "💡".cyan());
                println!("    • Server not running or unreachable");
                println!("    • Authentication required (set {})", "MCP_AUTH_TOKEN".cyan());
                println!("    • Invalid endpoint format");
            }
        }
        println!();
    }
    
    async fn search_capabilities(&mut self) {
        println!();
        println!("{}", "🔎 Search Capabilities".white().bold().underline());
        println!();
        
        let hint = self.prompt("Enter search hint (keyword, domain, or description):");
        if hint.is_empty() {
            return;
        }
        
        // First search in catalog
        let catalog_results = self.catalog.search_keyword(&hint, None, 20);
        
        if !catalog_results.is_empty() {
            println!();
            println!("  {} Found {} matching capability(ies) in catalog:", 
                "📚".cyan(), 
                catalog_results.len().to_string().white().bold()
            );
            println!();
            
            for (i, hit) in catalog_results.iter().enumerate() {
                println!("    [{}] {} {}", 
                    (i + 1).to_string().yellow(),
                    hit.entry.id.white().bold(),
                    format!("(score: {:.2})", hit.score).dimmed()
                );
                if let Some(ref desc) = hit.entry.description {
                    if !desc.is_empty() {
                        let short_desc = if desc.len() > 50 {
                            format!("{}...", &desc[..47])
                        } else {
                            desc.clone()
                        };
                        println!("        {}", short_desc.dimmed());
                    }
                }
            }
        } else {
            println!("  {} No matches in catalog. Try discovering from a server.", "⚠".yellow());
            println!();
            
            // Offer to discover
            let discover = self.prompt("Would you like to discover from available servers? (y/n):");
            if discover.to_lowercase() == "y" {
                let servers = self.discovery_service.list_known_servers();
                for config in &servers {
                    self.perform_discovery(config, Some(hint.clone())).await;
                }
            }
        }
        println!();
    }
    
    fn list_discovered(&self) {
        println!();
        println!("{}", "📦 Discovered Capabilities".white().bold().underline());
        println!();
        
        if self.discovered_tools.is_empty() {
            println!("  {} No capabilities discovered yet.", "⚠".yellow());
            println!("  {} Use '{}' to discover capabilities from a server.", "💡".cyan(), "discover".yellow());
        } else {
            println!("  {} {} capability(ies) discovered:", 
                "✓".green(), 
                self.discovered_tools.len().to_string().white().bold()
            );
            println!();
            
            // Group by server
            let mut by_server: HashMap<String, Vec<&DiscoveredTool>> = HashMap::new();
            for tool in &self.discovered_tools {
                by_server.entry(tool.server_name.clone()).or_default().push(tool);
            }
            
            for (server, tools) in &by_server {
                println!("  {} {} ({} tools)", "📡".cyan(), server.white().bold(), tools.len());
                for (i, tool) in tools.iter().enumerate() {
                    let domains = tool.manifest.domains.join(", ");
                    let categories = tool.manifest.categories.join(", ");
                    
                    println!("    [{}] {}", 
                        (i + 1).to_string().yellow(),
                        tool.manifest.name.white()
                    );
                    if !domains.is_empty() {
                        println!("        {} {}", "domains:".dimmed(), domains.cyan());
                    }
                    if !categories.is_empty() {
                        println!("        {} {}", "categories:".dimmed(), categories.magenta());
                    }
                }
                println!();
            }
        }
        println!();
    }
    
    async fn inspect_capability(&mut self) {
        println!();
        println!("{}", "🔬 Inspect Capability".white().bold().underline());
        println!();
        
        if self.discovered_tools.is_empty() {
            println!("  {} No capabilities to inspect. Discover some first!", "⚠".yellow());
            return;
        }
        
        // Show quick list
        for (i, tool) in self.discovered_tools.iter().enumerate() {
            println!("  [{}] {}", (i + 1).to_string().yellow(), tool.manifest.name);
        }
        println!();
        
        let choice = self.prompt("Select capability number (or name):");
        
        let selected = if let Ok(idx) = choice.parse::<usize>() {
            if idx > 0 && idx <= self.discovered_tools.len() {
                Some(&self.discovered_tools[idx - 1])
            } else {
                None
            }
        } else {
            // Search by name
            self.discovered_tools.iter().find(|t| t.manifest.name.contains(&choice))
        };
        
        if let Some(tool) = selected {
            self.print_capability_details(&tool.manifest);
            self.selected_capability = Some(tool.manifest.clone());
        } else {
            println!("  {} Capability not found", "✗".red());
        }
        println!();
    }
    
    fn print_capability_details(&self, manifest: &CapabilityManifest) {
        println!();
        println!("{}", "┌──────────────────────────────────────────────────────────────────────────────┐".cyan());
        println!("│ {} {:<67} │", "📦".cyan(), manifest.name.white().bold());
        println!("{}", "├──────────────────────────────────────────────────────────────────────────────┤".cyan());
        
        // ID and Version
        println!("│ {} {} {:<56} │", "ID:".dimmed(), manifest.id.cyan(), "");
        println!("│ {} {:<66} │", "Version:".dimmed(), manifest.version.yellow());
        
        // Description
        if !manifest.description.is_empty() {
            println!("{}", "├──────────────────────────────────────────────────────────────────────────────┤".cyan());
            let desc_lines = textwrap::wrap(&manifest.description, 70);
            for line in desc_lines {
                println!("│ {:<76} │", line);
            }
        }
        
        // Provider
        println!("{}", "├──────────────────────────────────────────────────────────────────────────────┤".cyan());
        let provider_str = match &manifest.provider {
            ProviderType::MCP(mcp) => format!("MCP: {} ({})", mcp.tool_name, mcp.server_url),
            ProviderType::Http(http) => format!("HTTP: {}", http.base_url),
            ProviderType::Local(_) => "Local".to_string(),
            ProviderType::OpenApi(api) => format!("OpenAPI: {}", api.base_url),
            ProviderType::A2A(a2a) => format!("A2A: {} ({})", a2a.agent_id, a2a.endpoint),
            _ => format!("{:?}", manifest.provider),
        };
        println!("│ {} {:<66} │", "Provider:".dimmed(), provider_str.green());
        
        // Domains & Categories
        if !manifest.domains.is_empty() {
            println!("│ {} {:<66} │", "Domains:".dimmed(), manifest.domains.join(", ").cyan());
        }
        if !manifest.categories.is_empty() {
            println!("│ {} {:<62} │", "Categories:".dimmed(), manifest.categories.join(", ").magenta());
        }
        
        // Input Schema
        if let Some(schema) = &manifest.input_schema {
            println!("{}", "├──────────────────────────────────────────────────────────────────────────────┤".cyan());
            println!("│ {} {:<68} │", "📥 INPUT SCHEMA".white().bold(), "");
            self.print_type_expr(schema, "│   ");
        }
        
        // Output Schema
        if let Some(schema) = &manifest.output_schema {
            println!("{}", "├──────────────────────────────────────────────────────────────────────────────┤".cyan());
            println!("│ {} {:<67} │", "📤 OUTPUT SCHEMA".white().bold(), "");
            self.print_type_expr(schema, "│   ");
        }
        
        println!("{}", "└──────────────────────────────────────────────────────────────────────────────┘".cyan());
    }
    
    fn print_type_expr(&self, type_expr: &rtfs::ast::TypeExpr, prefix: &str) {
        use rtfs::ast::TypeExpr;
        
        match type_expr {
            TypeExpr::Primitive(p) => {
                println!("{}{:<73} │", prefix, format!("{:?}", p).yellow());
            }
            TypeExpr::Any => {
                println!("{}{:<73} │", prefix, "any".yellow());
            }
            TypeExpr::Vector(inner) => {
                println!("{}{:<73} │", prefix, "vector of:".dimmed());
                self.print_type_expr(inner, &format!("{}  ", prefix));
            }
            TypeExpr::Map { entries, .. } => {
                println!("{}{:<73} │", prefix, "map:".dimmed());
                for entry in entries {
                    // entry.key is a Keyword, not MapKey
                    let key_str = format!(":{}", entry.key.0);
                    let opt = if entry.optional { " (optional)".dimmed().to_string() } else { "".to_string() };
                    println!("{}{:<73} │", prefix, format!("  {} →{}", key_str.cyan(), opt));
                    self.print_type_expr(&entry.value_type, &format!("{}    ", prefix));
                }
            }
            TypeExpr::Union(types) => {
                println!("{}{:<73} │", prefix, "union of:".dimmed());
                for t in types {
                    self.print_type_expr(t, &format!("{}  | ", prefix));
                }
            }
            TypeExpr::Tuple(types) => {
                println!("{}{:<73} │", prefix, format!("tuple ({} elements):", types.len()).dimmed());
                for (i, t) in types.iter().enumerate() {
                    println!("{}  [{}]", prefix, i);
                    self.print_type_expr(t, &format!("{}    ", prefix));
                }
            }
            TypeExpr::Alias(name) => {
                println!("{}{:<73} │", prefix, format!("{}", name.0).magenta());
            }
            TypeExpr::Function { param_types, return_type, .. } => {
                println!("{}{:<73} │", prefix, "function:".dimmed());
                println!("{}  params: {} types", prefix, param_types.len());
                println!("{}  returns:", prefix);
                self.print_type_expr(return_type, &format!("{}    ", prefix));
            }
            TypeExpr::Optional(inner) => {
                println!("{}{:<73} │", prefix, "optional:".dimmed());
                self.print_type_expr(inner, &format!("{}  ", prefix));
            }
            _ => {
                println!("{}{:<73} │", prefix, format!("{:?}", type_expr).dimmed());
            }
        }
    }
    
    async fn call_capability(&mut self) {
        println!();
        println!("{}", "▶️  Call Capability".white().bold().underline());
        println!();
        
        let manifest = if let Some(m) = &self.selected_capability {
            println!("  Using selected capability: {}", m.name.cyan());
            m.clone()
        } else if !self.discovered_tools.is_empty() {
            // Let user select
            for (i, tool) in self.discovered_tools.iter().enumerate() {
                println!("  [{}] {}", (i + 1).to_string().yellow(), tool.manifest.name);
            }
            println!();
            
            let choice = self.prompt("Select capability number:");
            if let Ok(idx) = choice.parse::<usize>() {
                if idx > 0 && idx <= self.discovered_tools.len() {
                    self.discovered_tools[idx - 1].manifest.clone()
                } else {
                    println!("  {} Invalid selection", "✗".red());
                    return;
                }
            } else {
                println!("  {} Invalid selection", "✗".red());
                return;
            }
        } else {
            println!("  {} No capabilities available. Discover some first!", "⚠".yellow());
            return;
        };
        
        println!();
        println!("  {} Building input parameters...", "⏳".yellow());
        println!();
        
        // Build inputs based on schema
        let inputs = self.build_inputs_from_schema(&manifest);
        
        if inputs.is_none() {
            println!("  {} Cancelled", "⚠".yellow());
            return;
        }
        
        let inputs = inputs.unwrap();
        
        println!();
        println!("  {} Calling capability with inputs:", "📤".cyan());
        println!("  {}", serde_json::to_string_pretty(&inputs).unwrap_or_default().dimmed());
        println!();
        
        // Execute the capability
        println!("  {} Executing...", "⏳".yellow());
        
        match self.marketplace.execute_capability(&manifest.id, &inputs).await {
            Ok(result) => {
                println!();
                println!("  {} Success!", "✓".green().bold());
                println!();
                println!("{}", "┌─ Result ──────────────────────────────────────────────────────────────────────┐".green());
                
                // Pretty print result
                let result_str = format!("{:?}", result);
                let lines = textwrap::wrap(&result_str, 76);
                for line in lines.iter().take(30) {
                    println!("│ {:<76} │", line);
                }
                if lines.len() > 30 {
                    println!("│ {:<76} │", format!("... ({} more lines)", lines.len() - 30).dimmed());
                }
                
                println!("{}", "└──────────────────────────────────────────────────────────────────────────────┘".green());
            }
            Err(e) => {
                println!();
                println!("  {} Execution failed: {}", "✗".red(), e);
                println!();
                println!("  {} This might be because:", "💡".cyan());
                println!("    • The capability requires authentication");
                println!("    • Required parameters are missing");
                println!("    • The server is not accessible");
            }
        }
        println!();
    }
    
    fn build_inputs_from_schema(&self, manifest: &CapabilityManifest) -> Option<rtfs::runtime::values::Value> {
        use rtfs::ast::TypeExpr;
        use rtfs::runtime::values::Value;
        
        if let Some(schema) = &manifest.input_schema {
            if let TypeExpr::Map { entries, .. } = schema {
                let mut map = std::collections::HashMap::new();
                
                println!("  Enter values for each parameter (or 'skip' to use default, 'cancel' to abort):");
                println!();
                
                for entry in entries {
                    // entry.key is a Keyword, not MapKey
                    let key_str = entry.key.0.clone();
                    
                    let type_hint = format!("{:?}", entry.value_type);
                    let optional_hint = if entry.optional { " (optional)" } else { "" };
                    
                    let prompt_str = format!("  {} [{}]{}: ", key_str.cyan(), type_hint.dimmed(), optional_hint.dimmed());
                    let value = self.prompt(&prompt_str);
                    
                    if value == "cancel" {
                        return None;
                    }
                    
                    if value == "skip" || (value.is_empty() && entry.optional) {
                        continue;
                    }
                    
                    // Parse value based on type
                    let parsed_value = self.parse_value(&value, &entry.value_type);
                    let map_key = rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(key_str));
                    map.insert(map_key, parsed_value);
                }
                
                return Some(Value::Map(map));
            }
        }
        
        // No schema - ask for raw JSON
        println!("  No schema available. Enter raw JSON input (or 'cancel'):");
        let input = self.prompt("  JSON:");
        
        if input == "cancel" || input.is_empty() {
            return None;
        }
        
        match serde_json::from_str::<serde_json::Value>(&input) {
            Ok(json) => Some(self.json_to_rtfs_value(&json)),
            Err(e) => {
                println!("  {} Invalid JSON: {}", "✗".red(), e);
                None
            }
        }
    }
    
    fn parse_value(&self, input: &str, type_expr: &rtfs::ast::TypeExpr) -> rtfs::runtime::values::Value {
        use rtfs::ast::{PrimitiveType, TypeExpr};
        use rtfs::runtime::values::Value;
        
        match type_expr {
            TypeExpr::Primitive(PrimitiveType::Int) => {
                input.parse::<i64>().map(Value::Integer).unwrap_or(Value::String(input.to_string()))
            }
            TypeExpr::Primitive(PrimitiveType::Float) => {
                input.parse::<f64>().map(Value::Float).unwrap_or(Value::String(input.to_string()))
            }
            TypeExpr::Primitive(PrimitiveType::Bool) => {
                Value::Boolean(input.to_lowercase() == "true" || input == "1")
            }
            TypeExpr::Primitive(PrimitiveType::String) => {
                Value::String(input.to_string())
            }
            _ => Value::String(input.to_string()),
        }
    }
    
    fn json_to_rtfs_value(&self, json: &serde_json::Value) -> rtfs::runtime::values::Value {
        use rtfs::runtime::values::Value;
        
        match json {
            serde_json::Value::Null => Value::Nil,
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::Nil
                }
            }
            serde_json::Value::String(s) => Value::String(s.clone()),
            serde_json::Value::Array(arr) => {
                Value::Vector(arr.iter().map(|v| self.json_to_rtfs_value(v)).collect())
            }
            serde_json::Value::Object(obj) => {
                let mut map = std::collections::HashMap::new();
                for (k, v) in obj {
                    let key = rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(k.clone()));
                    map.insert(key, self.json_to_rtfs_value(v));
                }
                Value::Map(map)
            }
        }
    }
    
    fn show_stats(&self) {
        println!();
        println!("{}", "📊 Catalog Statistics".white().bold().underline());
        println!();
        
        // Get basic stats from catalog
        let capability_search = self.catalog.search_keyword("", None, 1000);
        let total_capabilities = capability_search.len();
        
        println!("  {} Total catalog entries: {}", "•".cyan(), total_capabilities.to_string().white().bold());
        println!("  {} Discovered this session: {}", "🔍".cyan(), 
            self.discovered_tools.len().to_string().white().bold());
        
        // Group discovered by server
        let mut by_server: HashMap<String, usize> = HashMap::new();
        for tool in &self.discovered_tools {
            *by_server.entry(tool.server_name.clone()).or_default() += 1;
        }
        
        if !by_server.is_empty() {
            println!();
            println!("  {} By server:", "📡".cyan());
            for (server, count) in &by_server {
                println!("    • {}: {}", server, count);
            }
        }
        println!();
    }
    
    async fn run(&mut self, args: &Args) {
        self.print_banner();
        
        // Auto-discover if server specified
        if let Some(ref server) = args.server {
            let config = MCPServerConfig {
                name: server.clone(),
                endpoint: server.clone(),
                auth_token: std::env::var("MCP_AUTH_TOKEN").ok(),
                timeout_seconds: 30,
                protocol_version: "2024-11-05".to_string(),
            };
            self.perform_discovery(&config, args.hint.clone()).await;
        } else if args.hint.is_some() {
            // Search in known servers
            let servers = self.discovery_service.list_known_servers();
            for config in &servers {
                self.perform_discovery(config, args.hint.clone()).await;
            }
        }
        
        self.print_menu();
        
        loop {
            let cmd = self.prompt("explorer>");
            
            match cmd.as_str() {
                "1" | "servers" | "s" => self.list_servers().await,
                "2" | "discover" | "d" => self.discover_from_server().await,
                "3" | "search" => self.search_capabilities().await,
                "4" | "list" | "l" => self.list_discovered(),
                "5" | "inspect" | "i" => self.inspect_capability().await,
                "6" | "call" | "c" => self.call_capability().await,
                "7" | "stats" => self.show_stats(),
                "h" | "help" | "?" => self.print_menu(),
                "q" | "quit" | "exit" => {
                    println!();
                    println!("{}", "👋 Goodbye!".cyan());
                    println!();
                    break;
                }
                "" => continue,
                _ => {
                    println!("  {} Unknown command. Type '{}' for help.", "✗".red(), "h".yellow());
                }
            }
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    
    // Load config if available
    if let Ok(config_str) = std::fs::read_to_string(&args.config) {
        if let Ok(_config) = toml::from_str::<AgentConfig>(&config_str) {
            // Config loaded successfully
        }
    }
    
    let mut explorer = CapabilityExplorer::new().await;
    explorer.run(&args).await;
    
    Ok(())
}
