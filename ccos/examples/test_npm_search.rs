//! Test script to check NPM registry for @modelcontextprotocol/server-puppeteer
//!
//! Run with: cargo run --example test_npm_search

use rtfs::runtime::error::RuntimeResult;

#[tokio::main]
async fn main() -> RuntimeResult<()> {
    println!("🔍 Testing NPM registry search for puppeteer MCP...\n");

    let client = reqwest::Client::new();
    
    // Test 1: Direct package lookup
    println!("=== Test 1: Direct NPM package lookup ===");
    let package_name = "@modelcontextprotocol/server-puppeteer";
    let npm_url = format!("https://registry.npmjs.org/{}", package_name);
    
    match client.get(&npm_url).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(pkg) => {
                        println!("✅ Found package: {}", package_name);
                        if let Some(name) = pkg.get("name") {
                            println!("   Name: {}", name);
                        }
                        if let Some(description) = pkg.get("description") {
                            println!("   Description: {}", description);
                        }
                        if let Some(dist_tags) = pkg.get("dist-tags") {
                            if let Some(latest) = dist_tags.get("latest") {
                                println!("   Latest version: {}", latest);
                            }
                        }
                        if let Some(keywords) = pkg.get("keywords") {
                            if let Some(keywords_arr) = keywords.as_array() {
                                println!("   Keywords: {:?}", keywords_arr);
                            }
                        }
                    }
                    Err(e) => {
                        println!("❌ Failed to parse package JSON: {}", e);
                    }
                }
            } else {
                println!("❌ Package not found (status: {})", response.status());
            }
        }
        Err(e) => {
            println!("❌ Failed to fetch from NPM: {}", e);
        }
    }

    // Test 2: NPM search API
    println!("\n=== Test 2: NPM search API ===");
    let search_url = "https://registry.npmjs.org/-/v1/search";
    let search_params = [
        ("text", "modelcontextprotocol server-puppeteer"),
        ("size", "20"),
    ];
    
    match client.get(search_url).query(&search_params).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(results) => {
                        if let Some(objects) = results.get("objects") {
                            if let Some(objects_arr) = objects.as_array() {
                                println!("Found {} packages:", objects_arr.len());
                                for (i, obj) in objects_arr.iter().take(10).enumerate() {
                                    if let Some(package) = obj.get("package") {
                                        if let Some(name) = package.get("name") {
                                            println!("  {}. {}", i + 1, name);
                                        }
                                        if let Some(description) = package.get("description") {
                                            println!("     Description: {}", description);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("❌ Failed to parse search results: {}", e);
                    }
                }
            } else {
                println!("❌ Search failed (status: {})", response.status());
            }
        }
        Err(e) => {
            println!("❌ Failed to search NPM: {}", e);
        }
    }

    // Test 3: Search for just "puppeteer" in @modelcontextprotocol scope
    println!("\n=== Test 3: Search for puppeteer in @modelcontextprotocol scope ===");
    let search_params2 = [
        ("text", "scope:modelcontextprotocol puppeteer"),
        ("size", "20"),
    ];
    
    match client.get(search_url).query(&search_params2).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(results) => {
                        if let Some(objects) = results.get("objects") {
                            if let Some(objects_arr) = objects.as_array() {
                                println!("Found {} packages:", objects_arr.len());
                                for (i, obj) in objects_arr.iter().take(10).enumerate() {
                                    if let Some(package) = obj.get("package") {
                                        if let Some(name) = package.get("name") {
                                            println!("  {}. {}", i + 1, name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("❌ Failed to parse search results: {}", e);
                    }
                }
            } else {
                println!("❌ Search failed (status: {})", response.status());
            }
        }
        Err(e) => {
            println!("❌ Failed to search NPM: {}", e);
        }
    }

    // Test 4: List all @modelcontextprotocol packages
    println!("\n=== Test 4: List all @modelcontextprotocol packages ===");
    let search_params3 = [
        ("text", "scope:modelcontextprotocol"),
        ("size", "50"),
    ];
    
    match client.get(search_url).query(&search_params3).send().await {
        Ok(response) => {
            if response.status().is_success() {
                match response.json::<serde_json::Value>().await {
                    Ok(results) => {
                        if let Some(objects) = results.get("objects") {
                            if let Some(objects_arr) = objects.as_array() {
                                println!("Found {} @modelcontextprotocol packages:", objects_arr.len());
                                for (i, obj) in objects_arr.iter().take(20).enumerate() {
                                    if let Some(package) = obj.get("package") {
                                        if let Some(name) = package.get("name") {
                                            println!("  {}. {}", i + 1, name);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        println!("❌ Failed to parse search results: {}", e);
                    }
                }
            } else {
                println!("❌ Search failed (status: {})", response.status());
            }
        }
        Err(e) => {
            println!("❌ Failed to search NPM: {}", e);
        }
    }

    Ok(())
}
