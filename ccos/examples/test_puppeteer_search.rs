//! Test script to check what the registry search finds for "puppeteer"
//!
//! Run with: cargo run --example test_puppeteer_search

use ccos::discovery::registry_search::RegistrySearcher;
use ccos::ops::server::search_servers;
use rtfs::runtime::error::RuntimeResult;

#[tokio::main]
async fn main() -> RuntimeResult<()> {
    println!("🔍 Testing search for 'puppeteer'...\n");

    // Test 1: Direct registry search
    println!("=== Test 1: Direct RegistrySearcher ===");
    let searcher = RegistrySearcher::new();
    match searcher.search("puppeteer").await {
        Ok(results) => {
            println!("Found {} results:", results.len());
            for (i, result) in results.iter().enumerate() {
                println!("\n  {}. {}", i + 1, result.server_info.name);
                println!("     Endpoint: {}", result.server_info.endpoint);
                println!("     Description: {:?}", result.server_info.description);
                println!("     Source: {:?}", result.source);
                if !result.server_info.alternative_endpoints.is_empty() {
                    println!("     Alternatives: {:?}", result.server_info.alternative_endpoints);
                }
            }
        }
        Err(e) => {
            println!("❌ Registry search failed: {}", e);
        }
    }

    println!("\n=== Test 2: search_servers (used by TUI) ===");
    match search_servers("puppeteer".to_string(), None, false, None).await {
        Ok(results) => {
            println!("Found {} results:", results.len());
            for (i, info) in results.iter().enumerate() {
                println!("\n  {}. {}", i + 1, info.name);
                println!("     Endpoint: {}", info.endpoint);
                println!("     Description: {:?}", info.description);
                println!("     Source: {:?}", info.source);
            }
        }
        Err(e) => {
            println!("❌ search_servers failed: {}", e);
        }
    }

    println!("\n=== Test 3: Search for 'puppeteer mcp' ===");
    match search_servers("puppeteer mcp".to_string(), None, false, None).await {
        Ok(results) => {
            println!("Found {} results:", results.len());
            for (i, info) in results.iter().enumerate() {
                println!("\n  {}. {}", i + 1, info.name);
                println!("     Endpoint: {}", info.endpoint);
                println!("     Description: {:?}", info.description);
                println!("     Source: {:?}", info.source);
            }
        }
        Err(e) => {
            println!("❌ search_servers failed: {}", e);
        }
    }

    println!("\n=== Test 3b: Search for 'modelcontextprotocol' ===");
    match search_servers("modelcontextprotocol".to_string(), None, false, None).await {
        Ok(results) => {
            println!("Found {} results:", results.len());
            for (i, info) in results.iter().enumerate() {
                println!("\n  {}. {}", i + 1, info.name);
                println!("     Endpoint: {}", info.endpoint);
                println!("     Description: {:?}", info.description);
            }
        }
        Err(e) => {
            println!("❌ search_servers failed: {}", e);
        }
    }

    println!("\n=== Test 4: Direct MCP Registry API call ===");
    use ccos::mcp::registry::MCPRegistryClient;
    let client = MCPRegistryClient::new();
    match client.search_servers("puppeteer").await {
        Ok(servers) => {
            println!("Found {} servers directly from MCP Registry:", servers.len());
            for (i, server) in servers.iter().enumerate() {
                println!("\n  {}. {}", i + 1, server.name);
                println!("     Description: {}", server.description);
                if let Some(remotes) = &server.remotes {
                    println!("     Remotes ({}):", remotes.len());
                    for remote in remotes {
                        println!("       - Type: {}, URL: {}", remote.r#type, remote.url);
                    }
                } else {
                    println!("     No remotes defined");
                }
                if let Some(packages) = &server.packages {
                    println!("     Packages:");
                    for pkg in packages {
                        println!("       - {:?}", pkg);
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ MCP Registry API call failed: {}", e);
        }
    }

    println!("\n=== Test 5: List all servers (to find puppeteer) ===");
    match client.list_all_servers().await {
        Ok(servers) => {
            println!("Total servers in registry: {}", servers.len());
            let puppeteer_servers: Vec<_> = servers
                .iter()
                .filter(|s| s.name.to_lowercase().contains("puppeteer") || 
                           s.description.to_lowercase().contains("puppeteer") ||
                           s.name.contains("modelcontextprotocol") ||
                           (s.packages.is_some() && s.packages.as_ref().unwrap().iter().any(|p| p.identifier.contains("puppeteer"))))
                .collect();
            println!("Servers with 'puppeteer' or 'modelcontextprotocol' in name/description/packages: {}", puppeteer_servers.len());
            for server in puppeteer_servers {
                println!("\n  - {}", server.name);
                println!("    Description: {}", server.description);
                if let Some(remotes) = &server.remotes {
                    for remote in remotes {
                        println!("    Remote: {} -> {}", remote.r#type, remote.url);
                    }
                }
                if let Some(packages) = &server.packages {
                    for pkg in packages {
                        println!("    Package: {:?}", pkg);
                        if pkg.identifier.contains("puppeteer") || pkg.identifier.contains("modelcontextprotocol") {
                            println!("      ^^^ This package matches!");
                        }
                    }
                }
            }
            
            // Also search for "modelcontextprotocol" specifically
            println!("\n=== Test 6: Search for servers with 'modelcontextprotocol' in packages ===");
            let mcp_servers: Vec<_> = servers
                .iter()
                .filter(|s| {
                    if let Some(packages) = &s.packages {
                        packages.iter().any(|p| p.identifier.contains("modelcontextprotocol") || p.identifier.contains("server-puppeteer"))
                    } else {
                        false
                    }
                })
                .collect();
            println!("Found {} servers with modelcontextprotocol packages:", mcp_servers.len());
            for server in mcp_servers {
                println!("\n  - {}", server.name);
                if let Some(packages) = &server.packages {
                    for pkg in packages {
                        if pkg.identifier.contains("modelcontextprotocol") || pkg.identifier.contains("server-puppeteer") {
                            println!("    Package: {} (type: {}, transport: {})", 
                                pkg.identifier, pkg.registry_type, pkg.transport.r#type);
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!("❌ Failed to list all servers: {}", e);
        }
    }

    Ok(())
}
