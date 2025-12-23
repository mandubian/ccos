//! Interactive Capability Explorer
//!
//! A smooth, elegant TUI for discovering, inspecting, and testing capabilities.
//!
//! Usage:
//!   cargo run --example capability_explorer -- --config config/agent_config.toml
//!   cargo run --example capability_explorer -- --rtfs "(call :ccos.discovery.discover {:server \"github\"})"
//!   cargo run --example capability_explorer -- --rtfs-file script.rtfs
//!
//! Features:
//! - Browse available registries (MCP servers, local, etc.)
//! - Search capabilities with hints/keywords
//! - Inspect schemas and metadata
//! - Test capabilities with live execution
//! - Beautiful colored output with progress indicators
//! - RTFS command-line mode for scripting and automation
//!
//! RTFS Capabilities:
//! - (call :ccos.discovery.servers {})                    - List available servers
//! - (call :ccos.discovery.discover {:server "github"})   - Discover from server
//! - (call :ccos.discovery.search {:hint "issues"})       - Search capabilities
//! - (call :ccos.discovery.list {})                       - List discovered capabilities
//! - (call :ccos.discovery.inspect {:id "mcp.github.list_issues"}) - Inspect capability
//! - (call :mcp.github.list_issues {:owner "x" :repo "y"}) - Call any capability

use clap::Parser;
use colored::*;
use std::collections::HashMap;
use std::io::{self, Read, Write};
use std::sync::Arc;

use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::mcp_discovery::MCPServerConfig;
use ccos::capability_marketplace::types::{CapabilityManifest, ProviderType};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::catalog::CatalogService;
use ccos::mcp::core::MCPDiscoveryService;
use ccos::mcp::types::DiscoveryOptions;
use rtfs::ast::{Keyword, MapKey};
use rtfs::config::types::AgentConfig;
use rtfs::runtime::values::Value;
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

    /// Execute a single RTFS expression and exit
    #[arg(long)]
    rtfs: Option<String>,

    /// Execute RTFS expressions from a file (one per line, or multi-line with (do ...))
    #[arg(long)]
    rtfs_file: Option<String>,

    /// Read RTFS expressions from stdin
    #[arg(long)]
    rtfs_stdin: bool,

    /// Output format for RTFS mode: "pretty", "json", or "rtfs"
    #[arg(long, default_value = "pretty")]
    output: String,

    /// Quiet mode - only output results, no banners
    #[arg(long, short)]
    quiet: bool,
}

/// Main explorer state
struct CapabilityExplorer {
    discovery_service: Arc<MCPDiscoveryService>,
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
    discovered_tools: Vec<DiscoveredTool>,
    selected_capability: Option<CapabilityManifest>,
    selected_server: Option<String>, // Track currently selected server
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

        // Load capabilities from approved servers directory
        let approved_dir = std::path::Path::new("capabilities/servers/approved");
        if approved_dir.exists() {
            if let Err(e) = marketplace
                .import_capabilities_from_rtfs_dir_recursive(approved_dir)
                .await
            {
                eprintln!("Failed to load capabilities from approved servers: {}", e);
            }
        }

        // Load locally generated capabilities (workspace only; do not traverse parent)
        let generated_dir = std::path::Path::new("capabilities/generated");
        if generated_dir.exists() {
            if let Err(e) = marketplace
                .import_capabilities_from_rtfs_dir_recursive(generated_dir)
                .await
            {
                eprintln!(
                    "Failed to load generated capabilities from {}: {}",
                    generated_dir.display(),
                    e
                );
            }
        }

        let discovery_service = Arc::new(
            MCPDiscoveryService::new()
                .with_marketplace(Arc::clone(&marketplace))
                .with_catalog(Arc::clone(&catalog)),
        );

        Self {
            discovery_service,
            marketplace,
            catalog,
            discovered_tools: Vec::new(),
            selected_capability: None,
            selected_server: None,
        }
    }

    /// Execute an RTFS expression and return the result as a Value
    async fn execute_rtfs(
        &mut self,
        expr: &str,
        output_format: &str,
        quiet: bool,
    ) -> Result<Value, String> {
        // Parse the RTFS expression to extract call information
        let trimmed = expr.trim();

        // Handle (call :capability-id {...}) pattern
        if let Some(call_content) = Self::extract_call(trimmed) {
            return self
                .handle_rtfs_call(&call_content, output_format, quiet)
                .await;
        }

        // Handle (do ...) for multiple expressions
        if trimmed.starts_with("(do") {
            return self.handle_rtfs_do(trimmed, output_format, quiet).await;
        }

        Err(format!(
            "Unsupported RTFS expression. Expected (call :capability-id {{...}}) or (do ...)"
        ))
    }

    /// Extract the content of a (call ...) expression
    fn extract_call(expr: &str) -> Option<String> {
        let trimmed = expr.trim();
        if trimmed.starts_with("(call ") && trimmed.ends_with(')') {
            Some(trimmed[6..trimmed.len() - 1].trim().to_string())
        } else {
            None
        }
    }

    /// Handle a single (call :capability-id {...}) expression
    async fn handle_rtfs_call(
        &mut self,
        call_content: &str,
        output_format: &str,
        quiet: bool,
    ) -> Result<Value, String> {
        // Parse capability ID and arguments
        // Format: :capability-id {...} or "capability-id" {...}
        let (cap_id, args_str) = Self::parse_call_content(call_content)?;

        // Check if it's a discovery capability
        match cap_id.as_str() {
            "ccos.discovery.servers" => {
                let servers = self.get_servers_as_value().await;
                if !quiet {
                    self.output_value(&servers, output_format);
                }
                Ok(servers)
            }
            "ccos.discovery.discover" => {
                let args = Self::parse_rtfs_map(&args_str)?;
                let server =
                    Self::get_string_arg(&args, "server").ok_or("Missing :server argument")?;
                let hint = Self::get_string_arg(&args, "hint");
                let result = self.discover_rtfs(&server, hint.as_deref(), quiet).await?;
                if !quiet {
                    self.output_value(&result, output_format);
                }
                Ok(result)
            }
            "ccos.discovery.search" => {
                let args = Self::parse_rtfs_map(&args_str)?;
                let hint = Self::get_string_arg(&args, "hint").ok_or("Missing :hint argument")?;
                let result = self.search_rtfs(&hint).await;
                if !quiet {
                    self.output_value(&result, output_format);
                }
                Ok(result)
            }
            "ccos.discovery.list" => {
                let result = self.list_rtfs();
                if !quiet {
                    self.output_value(&result, output_format);
                }
                Ok(result)
            }
            "ccos.discovery.inspect" => {
                let args = Self::parse_rtfs_map(&args_str)?;
                let id = Self::get_string_arg(&args, "id").ok_or("Missing :id argument")?;
                let result = self.inspect_rtfs(&id).await?;
                if !quiet {
                    self.output_value(&result, output_format);
                }
                Ok(result)
            }
            _ => {
                // It's a regular capability call - execute through marketplace
                let args = Self::parse_rtfs_map(&args_str)?;
                let result = self.call_capability_rtfs(&cap_id, args, quiet).await?;
                // In RTFS mode, always show the result so users see the output even with --quiet.
                self.output_value(&result, output_format);
                Ok(result)
            }
        }
    }

    /// Handle (do expr1 expr2 ...) - execute multiple expressions
    fn handle_rtfs_do<'a>(
        &'a mut self,
        expr: &'a str,
        output_format: &'a str,
        quiet: bool,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Value, String>> + 'a>> {
        Box::pin(async move {
            // Simple parsing: extract expressions between (do and final )
            let content = &expr[3..expr.len() - 1].trim();

            let mut results = Vec::new();
            let mut depth = 0;
            let mut current_expr = String::new();

            for c in content.chars() {
                match c {
                    '(' => {
                        depth += 1;
                        current_expr.push(c);
                    }
                    ')' => {
                        current_expr.push(c);
                        depth -= 1;
                        if depth == 0 && !current_expr.trim().is_empty() {
                            let result = self
                                .execute_rtfs(&current_expr, output_format, quiet)
                                .await?;
                            results.push(result);
                            current_expr.clear();
                        }
                    }
                    _ => {
                        if depth > 0 {
                            current_expr.push(c);
                        }
                    }
                }
            }

            // Return last result or nil
            let final_val = results.pop().unwrap_or(Value::Nil);
            // In RTFS mode, always show the result so users see the output even with --quiet.
            self.output_value(&final_val, output_format);
            Ok(final_val)
        })
    }

    /// Parse call content to extract capability ID and arguments
    fn parse_call_content(content: &str) -> Result<(String, String), String> {
        let trimmed = content.trim();

        // Handle :keyword-id {...}
        if trimmed.starts_with(':') {
            if let Some(space_idx) = trimmed.find(|c: char| c.is_whitespace()) {
                let cap_id = trimmed[1..space_idx].to_string();
                let args_str = trimmed[space_idx..].trim().to_string();
                return Ok((cap_id, args_str));
            } else {
                // No arguments
                return Ok((trimmed[1..].to_string(), "{}".to_string()));
            }
        }

        // Handle "string-id" {...}
        if trimmed.starts_with('"') {
            if let Some(end_quote) = trimmed[1..].find('"') {
                let cap_id = trimmed[1..end_quote + 1].to_string();
                let args_str = trimmed[end_quote + 2..].trim().to_string();
                return Ok((
                    cap_id,
                    if args_str.is_empty() {
                        "{}".to_string()
                    } else {
                        args_str
                    },
                ));
            }
        }

        Err(format!(
            "Invalid call format. Expected :capability-id or \"capability-id\""
        ))
    }

    /// Parse RTFS map syntax {:key value :key2 value2} into a HashMap
    fn parse_rtfs_map(map_str: &str) -> Result<HashMap<String, Value>, String> {
        let trimmed = map_str.trim();
        if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
            // Empty map or just a value
            if trimmed.is_empty() || trimmed == "{}" {
                return Ok(HashMap::new());
            }
            return Err(format!("Expected map {{...}}, got: {}", trimmed));
        }

        let content = &trimmed[1..trimmed.len() - 1];
        let mut map = HashMap::new();
        let mut chars = content.chars().peekable();

        while let Some(&c) = chars.peek() {
            // Skip whitespace
            if c.is_whitespace() {
                chars.next();
                continue;
            }

            // Expect :key
            if c != ':' {
                break;
            }
            chars.next(); // consume ':'

            // Read key
            let mut key = String::new();
            while let Some(&c) = chars.peek() {
                if c.is_whitespace() || c == '{' || c == '"' || c == ':' {
                    break;
                }
                key.push(c);
                chars.next();
            }

            // Skip whitespace
            while let Some(&c) = chars.peek() {
                if !c.is_whitespace() {
                    break;
                }
                chars.next();
            }

            // Read value
            let value = Self::parse_rtfs_value(&mut chars)?;
            map.insert(key, value);
        }

        Ok(map)
    }

    /// Parse a single RTFS value
    fn parse_rtfs_value(chars: &mut std::iter::Peekable<std::str::Chars>) -> Result<Value, String> {
        // Skip whitespace
        while let Some(&c) = chars.peek() {
            if !c.is_whitespace() {
                break;
            }
            chars.next();
        }

        match chars.peek() {
            Some('"') => {
                // String value
                chars.next(); // consume opening quote
                let mut s = String::new();
                while let Some(c) = chars.next() {
                    if c == '"' {
                        break;
                    }
                    if c == '\\' {
                        if let Some(escaped) = chars.next() {
                            match escaped {
                                'n' => s.push('\n'),
                                't' => s.push('\t'),
                                '\\' => s.push('\\'),
                                '"' => s.push('"'),
                                _ => s.push(escaped),
                            }
                        }
                    } else {
                        s.push(c);
                    }
                }
                Ok(Value::String(s))
            }
            Some(c) if c.is_ascii_digit() || *c == '-' => {
                // Number
                let mut num_str = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_ascii_digit() || c == '.' || c == '-' {
                        num_str.push(c);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if num_str.contains('.') {
                    num_str
                        .parse::<f64>()
                        .map(Value::Float)
                        .map_err(|e| format!("Invalid float: {}", e))
                } else {
                    num_str
                        .parse::<i64>()
                        .map(Value::Integer)
                        .map_err(|e| format!("Invalid integer: {}", e))
                }
            }
            Some(':') => {
                // Keyword as value
                chars.next(); // consume ':'
                let mut kw = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '}' || c == ')' {
                        break;
                    }
                    kw.push(c);
                    chars.next();
                }
                Ok(Value::Keyword(Keyword(kw)))
            }
            Some('[') => {
                // Vector
                chars.next(); // consume '['
                let mut items = Vec::new();
                loop {
                    // Skip whitespace
                    while let Some(&c) = chars.peek() {
                        if !c.is_whitespace() {
                            break;
                        }
                        chars.next();
                    }
                    if chars.peek() == Some(&']') {
                        chars.next();
                        break;
                    }
                    items.push(Self::parse_rtfs_value(chars)?);
                }
                Ok(Value::Vector(items))
            }
            Some('t') | Some('f') => {
                // Boolean
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '}' || c == ')' || c == ']' {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                match word.as_str() {
                    "true" => Ok(Value::Boolean(true)),
                    "false" => Ok(Value::Boolean(false)),
                    _ => Ok(Value::String(word)),
                }
            }
            Some('n') => {
                // nil
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || c == '}' || c == ')' || c == ']' {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                if word == "nil" {
                    Ok(Value::Nil)
                } else {
                    Ok(Value::String(word))
                }
            }
            _ => Ok(Value::Nil),
        }
    }

    /// Get a string argument from parsed map
    fn get_string_arg(args: &HashMap<String, Value>, key: &str) -> Option<String> {
        args.get(key).and_then(|v| match v {
            Value::String(s) => Some(s.clone()),
            Value::Keyword(k) => Some(k.0.clone()),
            _ => None,
        })
    }

    /// Output a value in the specified format
    fn output_value(&self, value: &Value, format: &str) {
        match format {
            "json" => {
                if let Ok(json) = self.value_to_json(value) {
                    println!(
                        "{}",
                        serde_json::to_string_pretty(&json).unwrap_or_default()
                    );
                }
            }
            "rtfs" => {
                println!("{:?}", value);
            }
            _ => {
                // Pretty format
                self.pretty_print_value(value, 0);
            }
        }
    }

    fn pretty_print_value(&self, value: &Value, indent: usize) {
        let pad = "  ".repeat(indent);
        match value {
            Value::Map(m) => {
                println!("{}{{", pad);
                for (k, v) in m {
                    let key_str = match k {
                        MapKey::Keyword(kw) => format!(":{}", kw.0),
                        MapKey::String(s) => format!("\"{}\"", s),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    print!("{}  {} ", pad, key_str.cyan());
                    self.pretty_print_inline(v);
                    println!();
                }
                println!("{}}}", pad);
            }
            Value::Vector(v) => {
                println!("{}[", pad);
                for item in v {
                    print!("{}  ", pad);
                    self.pretty_print_inline(item);
                    println!();
                }
                println!("{}]", pad);
            }
            _ => self.pretty_print_inline(value),
        }
    }

    fn pretty_print_inline(&self, value: &Value) {
        match value {
            Value::String(s) => print!("{}", format!("\"{}\"", s).green()),
            Value::Integer(i) => print!("{}", i.to_string().yellow()),
            Value::Float(f) => print!("{}", f.to_string().yellow()),
            Value::Boolean(b) => print!("{}", b.to_string().magenta()),
            Value::Keyword(k) => print!("{}", format!(":{}", k.0).cyan()),
            Value::Nil => print!("{}", "nil".dimmed()),
            Value::Map(m) => {
                print!("{{");
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        print!(" ");
                    }
                    let key_str = match k {
                        MapKey::Keyword(kw) => format!(":{}", kw.0),
                        MapKey::String(s) => format!("\"{}\"", s),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    print!("{} ", key_str);
                    self.pretty_print_inline(v);
                }
                print!("}}");
            }
            Value::Vector(v) => {
                print!("[");
                for (i, item) in v.iter().enumerate() {
                    if i > 0 {
                        print!(" ");
                    }
                    self.pretty_print_inline(item);
                }
                print!("]");
            }
            _ => print!("{:?}", value),
        }
    }

    fn value_to_json(&self, value: &Value) -> Result<serde_json::Value, String> {
        match value {
            Value::Nil => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Integer(i) => Ok(serde_json::json!(i)),
            Value::Float(f) => Ok(serde_json::json!(f)),
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Keyword(k) => Ok(serde_json::Value::String(format!(":{}", k.0))),
            Value::Vector(v) => {
                let items: Result<Vec<_>, _> = v.iter().map(|i| self.value_to_json(i)).collect();
                Ok(serde_json::Value::Array(items?))
            }
            Value::Map(m) => {
                let mut obj = serde_json::Map::new();
                for (k, v) in m {
                    let key = match k {
                        MapKey::Keyword(kw) => kw.0.clone(),
                        MapKey::String(s) => s.clone(),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    obj.insert(key, self.value_to_json(v)?);
                }
                Ok(serde_json::Value::Object(obj))
            }
            _ => Ok(serde_json::Value::String(format!("{:?}", value))),
        }
    }

    // RTFS-mode implementations

    async fn get_servers_as_value(&self) -> Value {
        let servers = self.discovery_service.list_known_servers();
        let server_values: Vec<Value> = servers
            .iter()
            .map(|s| {
                let mut map = HashMap::new();
                map.insert(
                    MapKey::Keyword(Keyword("name".to_string())),
                    Value::String(s.name.clone()),
                );
                map.insert(
                    MapKey::Keyword(Keyword("endpoint".to_string())),
                    Value::String(s.endpoint.clone()),
                );
                Value::Map(map)
            })
            .collect();
        Value::Vector(server_values)
    }

    async fn discover_rtfs(
        &mut self,
        server_name: &str,
        _hint: Option<&str>,
        quiet: bool,
    ) -> Result<Value, String> {
        let servers = self.discovery_service.list_known_servers();
        let config = servers
            .iter()
            .find(|s| s.name == server_name || s.endpoint.contains(server_name))
            .cloned()
            .ok_or_else(|| format!("Unknown server: {}", server_name))?;

        if !quiet {
            eprintln!(
                "  {} Discovering from {}...",
                "⏳".yellow(),
                config.endpoint
            );
        }

        let options = DiscoveryOptions {
            introspect_output_schemas: false,
            use_cache: false,
            register_in_marketplace: true,
            export_to_rtfs: true, // Enable RTFS export for discovered capabilities
            export_directory: None, // Uses default: ../capabilities/discovered
            auth_headers: None,
            retry_policy: Default::default(),
            rate_limit: Default::default(),
            max_parallel_discoveries: 5,
            lazy_output_schemas: true,
            ignore_approved_files: false,
            force_refresh: false,
        };

        // Use discover_and_export_tools to ensure RTFS files are created
        // Note: discover_and_export_tools already registers in marketplace if options.register_in_marketplace is true
        match self
            .discovery_service
            .discover_and_export_tools(&config, &options)
            .await
        {
            Ok(manifests) => {
                let count = manifests.len();

                for manifest in &manifests {
                    self.discovered_tools.push(DiscoveredTool {
                        manifest: manifest.clone(),
                        server_name: config.name.clone(),
                        discovery_hint: None,
                    });
                    // Don't register again - discover_and_export_tools already did it
                }

                if !quiet {
                    eprintln!("  {} Discovered {} capabilities", "✓".green(), count);
                }

                // Return list of capability IDs
                let ids: Vec<Value> = manifests
                    .iter()
                    .map(|m| Value::String(m.id.clone()))
                    .collect();
                Ok(Value::Vector(ids))
            }
            Err(e) => Err(format!("Discovery failed: {}", e)),
        }
    }

    async fn search_rtfs(&self, hint: &str) -> Value {
        let results = self.catalog.search_keyword(hint, None, 100);
        let values: Vec<Value> = results
            .iter()
            .map(|r| {
                let mut map = HashMap::new();
                map.insert(
                    MapKey::Keyword(Keyword("id".to_string())),
                    Value::String(r.entry.id.clone()),
                );
                map.insert(
                    MapKey::Keyword(Keyword("name".to_string())),
                    Value::String(r.entry.name.clone().unwrap_or_default()),
                );
                map.insert(
                    MapKey::Keyword(Keyword("score".to_string())),
                    Value::Float(r.score as f64),
                );
                Value::Map(map)
            })
            .collect();
        Value::Vector(values)
    }

    fn list_rtfs(&self) -> Value {
        let values: Vec<Value> = self
            .discovered_tools
            .iter()
            .map(|t| {
                let mut map = HashMap::new();
                map.insert(
                    MapKey::Keyword(Keyword("id".to_string())),
                    Value::String(t.manifest.id.clone()),
                );
                map.insert(
                    MapKey::Keyword(Keyword("name".to_string())),
                    Value::String(t.manifest.name.clone()),
                );
                map.insert(
                    MapKey::Keyword(Keyword("server".to_string())),
                    Value::String(t.server_name.clone()),
                );
                Value::Map(map)
            })
            .collect();
        Value::Vector(values)
    }

    async fn inspect_rtfs(&self, id: &str) -> Result<Value, String> {
        // Search in discovered tools
        if let Some(tool) = self
            .discovered_tools
            .iter()
            .find(|t| t.manifest.id == id || t.manifest.name == id)
        {
            return Ok(self.manifest_to_value(&tool.manifest));
        }

        // Try marketplace
        if let Some(manifest) = self.marketplace.get_capability(id).await {
            return Ok(self.manifest_to_value(&manifest));
        }

        Err(format!("Capability not found: {}", id))
    }

    fn manifest_to_value(&self, manifest: &CapabilityManifest) -> Value {
        let mut map = HashMap::new();
        map.insert(
            MapKey::Keyword(Keyword("id".to_string())),
            Value::String(manifest.id.clone()),
        );
        map.insert(
            MapKey::Keyword(Keyword("name".to_string())),
            Value::String(manifest.name.clone()),
        );
        map.insert(
            MapKey::Keyword(Keyword("description".to_string())),
            Value::String(manifest.description.clone()),
        );
        map.insert(
            MapKey::Keyword(Keyword("version".to_string())),
            Value::String(manifest.version.clone()),
        );

        // Add provider info
        let provider_str = match &manifest.provider {
            ProviderType::MCP(mcp) => format!("MCP: {} ({})", mcp.tool_name, mcp.server_url),
            ProviderType::Local(_) => "Local".to_string(),
            ProviderType::A2A(a) => format!("A2A: {}", a.agent_id),
            ProviderType::Http(h) => format!("HTTP: {}", h.base_url),
            ProviderType::OpenApi(o) => format!("OpenAPI: {}", o.base_url),
            _ => "Other".to_string(),
        };
        map.insert(
            MapKey::Keyword(Keyword("provider".to_string())),
            Value::String(provider_str),
        );

        // Add schema info
        if let Some(schema) = &manifest.input_schema {
            map.insert(
                MapKey::Keyword(Keyword("input_schema".to_string())),
                Value::String(format!("{:?}", schema)),
            );
        }

        Value::Map(map)
    }

    async fn call_capability_rtfs(
        &mut self,
        cap_id: &str,
        args: HashMap<String, Value>,
        quiet: bool,
    ) -> Result<Value, String> {
        // Convert args HashMap to RTFS Map
        let mut rtfs_map = HashMap::new();
        for (k, v) in args {
            rtfs_map.insert(MapKey::Keyword(Keyword(k)), v);
        }
        let input = Value::Map(rtfs_map);

        if !quiet {
            eprintln!("  {} Calling {}...", "⏳".yellow(), cap_id);
        }

        match self.marketplace.execute_capability(cap_id, &input).await {
            Ok(result) => {
                if !quiet {
                    eprintln!("  {} Success", "✓".green());
                }
                Ok(result)
            }
            Err(e) => Err(format!("Execution failed: {}", e)),
        }
    }

    fn print_banner(&self) {
        println!();
        println!(
            "{}",
            "╔══════════════════════════════════════════════════════════════════════════════╗"
                .cyan()
                .bold()
        );
        println!(
            "{}",
            "║                     🔍 CCOS Capability Explorer 🔍                           ║"
                .cyan()
                .bold()
        );
        println!(
            "{}",
            "║                                                                              ║"
                .cyan()
                .bold()
        );
        println!(
            "{}",
            "║  Discover, inspect, and test capabilities from MCP servers and registries   ║"
                .cyan()
                .bold()
        );
        println!(
            "{}",
            "╚══════════════════════════════════════════════════════════════════════════════╝"
                .cyan()
                .bold()
        );
        println!();
    }

    fn print_menu(&self) {
        println!(
            "{}",
            "┌──────────────────────────────────────────────────────────────────────────────┐"
                .white()
                .dimmed()
        );
        println!(
            "│ {}                                                                       │",
            "Commands:".white().bold()
        );
        println!(
            "│                                                                              │"
        );
        println!(
            "│  {} - List available registries/servers                               │",
            "[1] servers".yellow()
        );
        println!(
            "│  {} - Discover capabilities from a server                             │",
            "[2] discover".yellow()
        );
        println!(
            "│  {} - Search capabilities by keyword/hint                             │",
            "[3] search".yellow()
        );
        println!(
            "│  {} - List discovered capabilities                                    │",
            "[4] list".yellow()
        );
        println!(
            "│  {} - Inspect a capability's details and schema                       │",
            "[5] inspect".yellow()
        );
        println!(
            "│  {} - Test/call a capability with inputs                              │",
            "[6] call".yellow()
        );
        println!(
            "│  {} - Show catalog statistics                                         │",
            "[7] stats".yellow()
        );
        println!(
            "│  {} - Display this menu                                               │",
            "[h] help".yellow()
        );
        println!(
            "│  {} - Exit the explorer                                               │",
            "[q] quit".yellow()
        );
        println!(
            "│                                                                              │"
        );
        println!(
            "{}",
            "└──────────────────────────────────────────────────────────────────────────────┘"
                .white()
                .dimmed()
        );
        println!();
    }

    fn prompt(&self, msg: &str) -> String {
        print!("{} ", msg.green().bold());
        io::stdout().flush().unwrap();
        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        input.trim().to_string()
    }

    async fn list_servers(&mut self) {
        println!();
        println!(
            "{}",
            "📋 Available Registries & Servers"
                .white()
                .bold()
                .underline()
        );
        println!();

        let servers = self.discovery_service.list_known_servers();

        if servers.is_empty() {
            println!("  {} No servers configured.", "⚠".yellow());
            println!();
            println!("  {} Add servers in one of these ways:", "💡".cyan());
            println!(
                "    • Edit {} to add MCP server configs",
                "config/mcp_introspection.toml".cyan()
            );
            println!(
                "    • Set environment variables like {}",
                "GITHUB_MCP_ENDPOINT".cyan()
            );
            println!(
                "    • Use {} with a custom endpoint",
                "--server <endpoint>".cyan()
            );
        } else {
            println!("  {} {} server(s) found:", "✓".green(), servers.len());
            println!();

            for (i, server) in servers.iter().enumerate() {
                let auth_status = if server.auth_token.is_some() {
                    "🔐".to_string()
                } else {
                    "🔓".to_string()
                };

                let current_marker =
                    if self.selected_server.as_ref().map(|s| s.as_str()) == Some(&server.name) {
                        " ← current".cyan().to_string()
                    } else {
                        "".to_string()
                    };

                println!(
                    "  {} [{}] {} {}{}",
                    auth_status,
                    (i + 1).to_string().yellow(),
                    server.name.white().bold(),
                    format!("({})", server.endpoint).dimmed(),
                    current_marker
                );
            }

            if let Some(ref server_name) = self.selected_server {
                println!();
                println!(
                    "  {} Current server: {}",
                    "ℹ️".cyan(),
                    server_name.cyan().bold()
                );
            }

            println!();
            let choice = self.prompt(
                "Select server ID to set as current (0 to deselect, Enter to keep current):",
            );
            if !choice.is_empty() {
                if let Ok(idx) = choice.parse::<usize>() {
                    if idx == 0 {
                        self.selected_server = None;
                        println!("  {} Selection cleared", "✓".green());
                    } else if idx > 0 && idx <= servers.len() {
                        let config = &servers[idx - 1];
                        self.selected_server = Some(config.name.clone());
                        println!(
                            "  {} Selected server: {}",
                            "✓".green(),
                            config.name.cyan().bold()
                        );

                        // Reset environment to isolate the selected server
                        // This ensures we only explore capabilities from this server
                        let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
                        self.marketplace = Arc::new(CapabilityMarketplace::new(registry));
                        self.catalog = Arc::new(CatalogService::new());
                        self.discovery_service = Arc::new(
                            MCPDiscoveryService::new()
                                .with_marketplace(Arc::clone(&self.marketplace))
                                .with_catalog(Arc::clone(&self.catalog)),
                        );
                        self.discovered_tools.clear();

                        // Try to load from approved capabilities first
                        let sanitized_name = ccos::utils::fs::sanitize_filename(&config.name);
                        let approved_path = std::path::Path::new("capabilities/servers/approved")
                            .join(&sanitized_name);

                        let mut loaded_from_approved = false;
                        if approved_path.exists() {
                            println!(
                                "  {} Loading approved capabilities from {}...",
                                "⏳".yellow(),
                                approved_path.display()
                            );
                            match self
                                .marketplace
                                .import_capabilities_from_rtfs_dir_recursive(&approved_path)
                                .await
                            {
                                Ok(count) => {
                                    if count > 0 {
                                        println!(
                                            "  {} Loaded {} approved capability(ies)",
                                            "✓".green(),
                                            count
                                        );

                                        // Fetch them from marketplace to populate discovered_tools
                                        let all_caps = self.marketplace.list_capabilities().await;
                                        for cap in all_caps {
                                            self.discovered_tools.push(DiscoveredTool {
                                                manifest: cap,
                                                server_name: config.name.clone(),
                                                discovery_hint: None,
                                            });
                                        }

                                        if !self.discovered_tools.is_empty() {
                                            println!(
                                                "  {} Added {} capabilities to explorer list",
                                                "✓".green(),
                                                self.discovered_tools.len()
                                            );
                                            loaded_from_approved = true;
                                        }
                                    }
                                }
                                Err(e) => {
                                    eprintln!(
                                        "  {} Failed to load approved capabilities: {}",
                                        "✗".red(),
                                        e
                                    );
                                }
                            }
                        }

                        if !loaded_from_approved {
                            // Fallback to introspection if not approved or load failed
                            let options = DiscoveryOptions {
                                introspect_output_schemas: false,
                                use_cache: true,
                                register_in_marketplace: true,
                                export_to_rtfs: false, // Don't re-export when loading
                                export_directory: None,
                                auth_headers: config.auth_token.as_ref().map(|token| {
                                    let mut headers = HashMap::new();
                                    headers.insert(
                                        "Authorization".to_string(),
                                        format!("Bearer {}", token),
                                    );
                                    headers
                                }),
                                ..Default::default()
                            };

                            println!(
                                "  {} Loading capabilities via introspection...",
                                "⏳".yellow()
                            );
                            match self
                                .discovery_service
                                .discover_and_export_tools(config, &options)
                                .await
                            {
                                Ok(manifests) => {
                                    if !manifests.is_empty() {
                                        println!("  {} Loaded {} capability(ies) from server introspection", "✓".green(), manifests.len());
                                        // Store for exploration
                                        for manifest in &manifests {
                                            // Only add if not already present
                                            if !self
                                                .discovered_tools
                                                .iter()
                                                .any(|t| t.manifest.id == manifest.id)
                                            {
                                                self.discovered_tools.push(DiscoveredTool {
                                                    manifest: manifest.clone(),
                                                    server_name: config.name.clone(),
                                                    discovery_hint: None,
                                                });
                                            }
                                        }
                                    } else {
                                        println!("  {} No capabilities found via introspection. Use 'discover' (2) to try again.", "ℹ️".cyan());
                                    }
                                }
                                Err(e) => {
                                    log::debug!("Failed to auto-load capabilities: {}", e);
                                    println!("  {} Introspection failed. Use 'discover' (2) to try again with options.", "ℹ️".cyan());
                                }
                            }
                        }
                    } else {
                        println!("  {} Invalid selection", "✗".red());
                    }
                } else {
                    println!("  {} Invalid selection", "✗".red());
                }
            }
        }
        println!();
    }

    async fn discover_from_server(&mut self) {
        println!();
        println!(
            "{}",
            "🔍 Discover Capabilities from Server"
                .white()
                .bold()
                .underline()
        );
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

            // Store selected server
            self.selected_server = Some(config.name.clone());
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
                    // Store selected server
                    self.selected_server = Some(config.name.clone());
                    self.perform_discovery(&config, None).await;
                } else if idx > 0 && idx <= servers.len() {
                    let config = servers[idx - 1].clone();
                    // Store selected server
                    self.selected_server = Some(config.name.clone());
                    self.perform_discovery(&config, None).await;
                } else {
                    println!("  {} Invalid selection", "✗".red());
                }
            }
        }
    }

    async fn perform_discovery(&mut self, config: &MCPServerConfig, hint: Option<String>) {
        println!();
        println!(
            "  {} Connecting to {}...",
            "⏳".yellow(),
            config.endpoint.cyan()
        );

        let options = DiscoveryOptions {
            introspect_output_schemas: false,
            use_cache: true,
            register_in_marketplace: true,
            export_to_rtfs: true, // Enable RTFS export for discovered capabilities
            export_directory: None, // Uses default: ../capabilities/discovered
            auth_headers: config.auth_token.as_ref().map(|token| {
                let mut headers = HashMap::new();
                headers.insert("Authorization".to_string(), format!("Bearer {}", token));
                headers
            }),
            ..Default::default()
        };

        // Use discover_and_export_tools which also registers in marketplace
        match self
            .discovery_service
            .discover_and_export_tools(config, &options)
            .await
        {
            Ok(manifests) => {
                println!(
                    "  {} Discovered {} tool(s)",
                    "✓".green(),
                    manifests.len().to_string().white().bold()
                );
                println!();

                // Filter by hint if provided
                let filtered_manifests: Vec<_> = if let Some(ref h) = hint {
                    let h_lower = h.to_lowercase();
                    manifests
                        .iter()
                        .filter(|m| {
                            m.name.to_lowercase().contains(&h_lower)
                                || m.description.to_lowercase().contains(&h_lower)
                        })
                        .collect()
                } else {
                    manifests.iter().collect()
                };

                if hint.is_some() && filtered_manifests.len() < manifests.len() {
                    println!(
                        "  {} Filtered to {} matching tool(s) for hint: '{}'",
                        "🔎".cyan(),
                        filtered_manifests.len().to_string().white().bold(),
                        hint.as_ref().unwrap().cyan()
                    );
                    println!();
                }

                // Store discovered tools (already registered by discover_and_export_tools)
                for manifest in filtered_manifests {
                    // Print tool summary
                    println!("    {} {}", "•".green(), manifest.name.white().bold());
                    let short_desc = if manifest.description.len() > 60 {
                        format!("{}...", &manifest.description[..57])
                    } else {
                        manifest.description.clone()
                    };
                    if !short_desc.is_empty() {
                        println!("      {}", short_desc.dimmed());
                    }

                    // Store discovered tool (capability already registered by discover_and_export_tools)
                    self.discovered_tools.push(DiscoveredTool {
                        manifest: manifest.clone(),
                        server_name: config.name.clone(),
                        discovery_hint: hint.clone(),
                    });
                }

                // Verify registration
                let registered_count = self.marketplace.list_capabilities().await.len();
                println!();
                println!(
                    "  {} {} capabilities registered in marketplace",
                    "✓".green(),
                    registered_count.to_string().white().bold()
                );
                println!(
                    "  {} Use '{}' to see all discovered capabilities",
                    "💡".cyan(),
                    "list".yellow()
                );
            }
            Err(e) => {
                println!("  {} Discovery failed: {}", "✗".red(), e);
                println!();
                println!("  {} Possible causes:", "💡".cyan());
                println!("    • Server not running or unreachable");
                println!(
                    "    • Authentication required (set {})",
                    "MCP_AUTH_TOKEN".cyan()
                );
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
            println!(
                "  {} Found {} matching capability(ies) in catalog:",
                "📚".cyan(),
                catalog_results.len().to_string().white().bold()
            );
            println!();

            for (i, hit) in catalog_results.iter().enumerate() {
                println!(
                    "    [{}] {} {}",
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
            println!(
                "  {} No matches in catalog. Try discovering from a server.",
                "⚠".yellow()
            );
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
        println!(
            "{}",
            "📦 Discovered Capabilities".white().bold().underline()
        );
        println!();

        if self.discovered_tools.is_empty() {
            println!("  {} No capabilities discovered yet.", "⚠".yellow());
            println!(
                "  {} Use '{}' to discover capabilities from a server.",
                "💡".cyan(),
                "discover".yellow()
            );
        } else {
            println!(
                "  {} {} capability(ies) discovered:",
                "✓".green(),
                self.discovered_tools.len().to_string().white().bold()
            );
            println!();

            // Group by server
            let mut by_server: HashMap<String, Vec<&DiscoveredTool>> = HashMap::new();
            for tool in &self.discovered_tools {
                by_server
                    .entry(tool.server_name.clone())
                    .or_default()
                    .push(tool);
            }

            for (server, tools) in &by_server {
                println!(
                    "  {} {} ({} tools)",
                    "📡".cyan(),
                    server.white().bold(),
                    tools.len()
                );
                for (i, tool) in tools.iter().enumerate() {
                    let domains = tool.manifest.domains.join(", ");
                    let categories = tool.manifest.categories.join(", ");

                    println!(
                        "    [{}] {}",
                        (i + 1).to_string().yellow(),
                        tool.manifest.name.white()
                    );
                    if !domains.is_empty() {
                        println!("        {} {}", "domains:".dimmed(), domains.cyan());
                    }
                    if !categories.is_empty() {
                        println!(
                            "        {} {}",
                            "categories:".dimmed(),
                            categories.magenta()
                        );
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
            println!(
                "  {} No capabilities to inspect. Discover some first!",
                "⚠".yellow()
            );
            return;
        }

        // Show quick list
        for (i, tool) in self.discovered_tools.iter().enumerate() {
            println!(
                "  [{}] {}",
                (i + 1).to_string().yellow(),
                tool.manifest.name
            );
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
            self.discovered_tools
                .iter()
                .find(|t| t.manifest.name.contains(&choice))
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
        println!(
            "{}",
            "┌──────────────────────────────────────────────────────────────────────────────┐"
                .cyan()
        );
        println!("│ {} {:<67} │", "📦".cyan(), manifest.name.white().bold());
        println!(
            "{}",
            "├──────────────────────────────────────────────────────────────────────────────┤"
                .cyan()
        );

        // ID and Version
        println!("│ {} {} {:<56} │", "ID:".dimmed(), manifest.id.cyan(), "");
        println!(
            "│ {} {:<66} │",
            "Version:".dimmed(),
            manifest.version.yellow()
        );

        // Description
        if !manifest.description.is_empty() {
            println!(
                "{}",
                "├──────────────────────────────────────────────────────────────────────────────┤"
                    .cyan()
            );
            let desc_lines = textwrap::wrap(&manifest.description, 70);
            for line in desc_lines {
                println!("│ {:<76} │", line);
            }
        }

        // Provider
        println!(
            "{}",
            "├──────────────────────────────────────────────────────────────────────────────┤"
                .cyan()
        );
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
            println!(
                "│ {} {:<66} │",
                "Domains:".dimmed(),
                manifest.domains.join(", ").cyan()
            );
        }
        if !manifest.categories.is_empty() {
            println!(
                "│ {} {:<62} │",
                "Categories:".dimmed(),
                manifest.categories.join(", ").magenta()
            );
        }

        // Input Schema
        if let Some(schema) = &manifest.input_schema {
            println!(
                "{}",
                "├──────────────────────────────────────────────────────────────────────────────┤"
                    .cyan()
            );
            println!("│ {} {:<68} │", "📥 INPUT SCHEMA".white().bold(), "");
            self.print_type_expr(schema, "│   ");
        }

        // Output Schema
        if let Some(schema) = &manifest.output_schema {
            println!(
                "{}",
                "├──────────────────────────────────────────────────────────────────────────────┤"
                    .cyan()
            );
            println!("│ {} {:<67} │", "📤 OUTPUT SCHEMA".white().bold(), "");
            self.print_type_expr(schema, "│   ");
        }

        println!(
            "{}",
            "└──────────────────────────────────────────────────────────────────────────────┘"
                .cyan()
        );
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
                    let opt = if entry.optional {
                        " (optional)".dimmed().to_string()
                    } else {
                        "".to_string()
                    };
                    println!(
                        "{}{:<73} │",
                        prefix,
                        format!("  {} →{}", key_str.cyan(), opt)
                    );
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
                println!(
                    "{}{:<73} │",
                    prefix,
                    format!("tuple ({} elements):", types.len()).dimmed()
                );
                for (i, t) in types.iter().enumerate() {
                    println!("{}  [{}]", prefix, i);
                    self.print_type_expr(t, &format!("{}    ", prefix));
                }
            }
            TypeExpr::Alias(name) => {
                println!("{}{:<73} │", prefix, format!("{}", name.0).magenta());
            }
            TypeExpr::Function {
                param_types,
                return_type,
                ..
            } => {
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
                println!(
                    "  [{}] {}",
                    (i + 1).to_string().yellow(),
                    tool.manifest.name
                );
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
            println!(
                "  {} No capabilities available. Discover some first!",
                "⚠".yellow()
            );
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
        println!(
            "  {}",
            serde_json::to_string_pretty(&inputs)
                .unwrap_or_default()
                .dimmed()
        );
        println!();

        // Execute the capability
        println!("  {} Executing {} ...", "⏳".yellow(), manifest.id.cyan());

        // Debug: Check if capability is registered
        let registered_caps = self.marketplace.list_capabilities().await;
        let is_registered = registered_caps.iter().any(|c| c.id == manifest.id);
        println!(
            "  {} Capability in marketplace: {} (total: {})",
            if is_registered {
                "✓".green()
            } else {
                "✗".red()
            },
            is_registered,
            registered_caps.len()
        );

        match self
            .marketplace
            .execute_capability(&manifest.id, &inputs)
            .await
        {
            Ok(result) => {
                println!();
                println!("  {} Success!", "✓".green().bold());
                println!();
                println!("{}", "┌─ Result ──────────────────────────────────────────────────────────────────────┐".green());

                // Pretty print result as JSON
                let result_json = ccos::utils::value_conversion::rtfs_value_to_json(&result)
                    .unwrap_or(serde_json::Value::String(format!("{:?}", result)));

                let result_str = serde_json::to_string_pretty(&result_json).unwrap_or_default();
                let lines: Vec<&str> = result_str.lines().collect();

                for line in lines.iter().take(100) {
                    if line.len() > 76 {
                        println!("│ {:<76} │", &line[0..76]);
                        // Simple truncation for now to keep box intact
                    } else {
                        println!("│ {:<76} │", line);
                    }
                }
                if lines.len() > 100 {
                    println!(
                        "│ {:<76} │",
                        format!("... ({} more lines)", lines.len() - 100).dimmed()
                    );
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

    fn build_inputs_from_schema(
        &self,
        manifest: &CapabilityManifest,
    ) -> Option<rtfs::runtime::values::Value> {
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

                    let prompt_str = format!(
                        "  {} [{}]{}: ",
                        key_str.cyan(),
                        type_hint.dimmed(),
                        optional_hint.dimmed()
                    );
                    let value = self.prompt(&prompt_str);

                    if value == "cancel" {
                        return None;
                    }

                    if value == "skip" || (value.is_empty() && entry.optional) {
                        continue;
                    }

                    // Validate required fields are not empty
                    if value.is_empty() && !entry.optional {
                        println!(
                            "  {} Required parameter '{}' cannot be empty",
                            "✗".red(),
                            key_str
                        );
                        return None;
                    }

                    // Parse value based on type (with validation)
                    match self.parse_value(&value, &entry.value_type) {
                        Ok(parsed_value) => {
                            let map_key = rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(key_str));
                            map.insert(map_key, parsed_value);
                        }
                        Err(e) => {
                            println!("  {} Invalid value for '{}': {}", "✗".red(), key_str, e);
                            println!(
                                "  {} Please enter a valid value or 'cancel' to abort",
                                "💡".cyan()
                            );
                            // Retry this parameter
                            let retry_value = self.prompt(&prompt_str);
                            if retry_value == "cancel" {
                                return None;
                            }
                            if retry_value == "skip" && entry.optional {
                                continue;
                            }
                            match self.parse_value(&retry_value, &entry.value_type) {
                                Ok(parsed_value) => {
                                    let map_key =
                                        rtfs::ast::MapKey::Keyword(rtfs::ast::Keyword(key_str));
                                    map.insert(map_key, parsed_value);
                                }
                                Err(_) => {
                                    println!("  {} Invalid value again. Cancelling.", "✗".red());
                                    return None;
                                }
                            }
                        }
                    }
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

    fn parse_value(
        &self,
        input: &str,
        type_expr: &rtfs::ast::TypeExpr,
    ) -> Result<rtfs::runtime::values::Value, String> {
        use rtfs::ast::{PrimitiveType, TypeExpr};
        use rtfs::runtime::values::Value;

        match type_expr {
            TypeExpr::Primitive(PrimitiveType::Int) => input
                .parse::<i64>()
                .map(Value::Integer)
                .map_err(|_| format!("Expected integer, got: '{}'", input)),
            TypeExpr::Primitive(PrimitiveType::Float) => input
                .parse::<f64>()
                .map(Value::Float)
                .map_err(|_| format!("Expected float, got: '{}'", input)),
            TypeExpr::Primitive(PrimitiveType::Bool) => Ok(Value::Boolean(
                input.to_lowercase() == "true" || input == "1",
            )),
            TypeExpr::Primitive(PrimitiveType::String) => Ok(Value::String(input.to_string())),
            _ => Ok(Value::String(input.to_string())),
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

        println!(
            "  {} Total catalog entries: {}",
            "•".cyan(),
            total_capabilities.to_string().white().bold()
        );
        println!(
            "  {} Discovered this session: {}",
            "🔍".cyan(),
            self.discovered_tools.len().to_string().white().bold()
        );

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
        // RTFS mode - execute expressions without interactive TUI
        if args.rtfs.is_some() || args.rtfs_file.is_some() || args.rtfs_stdin {
            self.run_rtfs_mode(args).await;
            return;
        }

        if !args.quiet {
            self.print_banner();
        }

        // Auto-discover if server specified
        if let Some(ref server) = args.server {
            let config = MCPServerConfig {
                name: server.clone(),
                endpoint: server.clone(),
                auth_token: std::env::var("MCP_AUTH_TOKEN").ok(),
                timeout_seconds: 30,
                protocol_version: "2024-11-05".to_string(),
            };
            // Store selected server
            self.selected_server = Some(config.name.clone());
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
            // Show current server in prompt if one is selected
            let prompt = if let Some(ref server) = self.selected_server {
                format!("explorer [{}]>", server.cyan().bold())
            } else {
                "explorer>".to_string()
            };
            let cmd = self.prompt(&prompt);

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
                // Try to parse as RTFS expression in interactive mode
                expr if expr.starts_with('(') => {
                    match self.execute_rtfs(expr, &args.output, false).await {
                        Ok(_) => {}
                        Err(e) => println!("  {} {}", "✗".red(), e),
                    }
                }
                _ => {
                    println!(
                        "  {} Unknown command. Type '{}' for help.",
                        "✗".red(),
                        "h".yellow()
                    );
                    println!(
                        "  {} You can also enter RTFS expressions directly, e.g.:",
                        "💡".cyan()
                    );
                    println!("    (call :ccos.discovery.servers {{}})");
                }
            }
        }
    }

    /// Run in RTFS script mode (non-interactive)
    async fn run_rtfs_mode(&mut self, args: &Args) {
        // Collect expressions to execute
        let expressions: Vec<String> = if let Some(ref expr) = args.rtfs {
            vec![expr.clone()]
        } else if let Some(ref file_path) = args.rtfs_file {
            match std::fs::read_to_string(file_path) {
                Ok(content) => {
                    // If the file starts with (do, treat as single expression
                    let trimmed = content.trim();
                    if trimmed.starts_with("(do") {
                        vec![trimmed.to_string()]
                    } else {
                        // Otherwise, treat each non-comment line starting with ( as an expression
                        self.parse_rtfs_lines(&content)
                    }
                }
                Err(e) => {
                    eprintln!("Error reading file {}: {}", file_path, e);
                    return;
                }
            }
        } else if args.rtfs_stdin {
            let mut content = String::new();
            if let Err(e) = std::io::stdin().lock().read_to_string(&mut content) {
                eprintln!("Error reading from stdin: {}", e);
                return;
            }
            let trimmed = content.trim();
            if trimmed.starts_with("(do") {
                vec![trimmed.to_string()]
            } else {
                self.parse_rtfs_lines(&content)
            }
        } else {
            return;
        };

        // Execute each expression
        for expr in expressions {
            if !args.quiet {
                eprintln!("{} {}", "▶".cyan(), expr.dimmed());
            }

            match self.execute_rtfs(&expr, &args.output, args.quiet).await {
                Ok(_value) => {
                    // Value already printed by execute_rtfs
                }
                Err(e) => {
                    eprintln!("{} {}", "✗".red(), e);
                    // Continue with remaining expressions
                }
            }
        }
    }

    /// Parse RTFS lines from multi-line content
    fn parse_rtfs_lines(&self, content: &str) -> Vec<String> {
        let mut expressions = Vec::new();
        let mut current_expr = String::new();
        let mut depth = 0;

        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with(";;") {
                continue;
            }

            // Track parenthesis depth
            for c in trimmed.chars() {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
            }

            if !current_expr.is_empty() {
                current_expr.push(' ');
            }
            current_expr.push_str(trimmed);

            // Complete expression when depth returns to 0
            if depth == 0 && !current_expr.is_empty() {
                expressions.push(current_expr.clone());
                current_expr.clear();
            }
        }

        // Push any remaining incomplete expression
        if !current_expr.is_empty() {
            expressions.push(current_expr);
        }

        expressions
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
