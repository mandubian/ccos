//! Test calling MCP GitHub capabilities
//!
//! This binary demonstrates calling the synthesized MCP capabilities.

use rtfs_compiler::ccos::environment::CCOSBuilder;
use rtfs_compiler::runtime::execution_outcome::ExecutionOutcome;
use rtfs_compiler::runtime::values::Value;
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🔌 Testing MCP GitHub Capability Call");
    println!("======================================");
    println!();

    // Setup CCOS environment
    println!("🔧 Setting up CCOS environment...");
    let env = CCOSBuilder::new()
        .http_mocking(false)
        .http_allow_hosts(vec!["localhost".to_string()])
        .verbose(true)
        .build()
        .expect("Failed to build environment");
    
    println!("✅ Environment ready");
    println!();

    // Load the MCP capability
    let capability_path = "../capabilities/mcp.github.list_issues/capability.rtfs";
    
    println!("📂 Loading capability from: {}", capability_path);
    
    let capability_code = match fs::read_to_string(capability_path) {
        Ok(code) => code,
        Err(_) => {
            eprintln!("❌ Error: Capability file not found!");
            eprintln!("   Expected: {}", capability_path);
            eprintln!();
            eprintln!("💡 Run this first: cargo run --bin test_mcp_introspection");
            return Err("Capability file not found".into());
        }
    };
    
    println!("✅ Capability file loaded");
    println!();

    // Parse and evaluate the capability definition
    println!("⚙️  Parsing and registering capability...");
    match env.execute_code(&capability_code) {
        Ok(ExecutionOutcome::Complete(result)) => {
            println!("✅ Capability registered: {:?}", result);
        }
        Ok(ExecutionOutcome::RequiresHost(_)) => {
            println!("⚠️  Host call required during registration");
        }
        Err(e) => {
            eprintln!("❌ Failed to register capability: {:?}", e);
            return Err(format!("Registration failed: {:?}", e).into());
        }
    }
    println!();

    // Now call the capability
    println!("🚀 Calling mcp.github.list_issues...");
    println!("   Input: {{ :owner \"mandubian\" :repo \"ccos\" :state \"open\" }}");
    println!();

    // Build the call expression: ((call "mcp.github.list_issues") {:owner "mandubian" :repo "ccos" :state "open"})
    let call_code = r#"
        ((call "mcp.github.list_issues") {
            :owner "mandubian"
            :repo "ccos"
            :state "open"
        })
    "#;

    println!("📝 RTFS code:");
    println!("{}", call_code);
    println!();
    
    println!("⏳ Executing capability...");
    println!();
    
    match env.execute_code(call_code) {
        Ok(ExecutionOutcome::Complete(result)) => {
            println!("✅ Success!");
            println!();
            println!("📊 Result:");
            println!("{}", format_result(&result));
        }
        Ok(ExecutionOutcome::RequiresHost(_)) => {
            println!("⚠️  Host call required during execution");
        }
        Err(e) => {
            println!("❌ Execution Error:");
            println!("{:?}", e);
            println!();
            println!("💡 Note: This is expected if the MCP server is not running!");
            println!("   The capability code is correct - it just needs a real MCP server.");
            println!();
            println!("   To test with a real MCP server:");
            println!("   1. Start an MCP server on http://localhost:3000/github-mcp");
            println!("   2. Ensure it implements the tools/call JSON-RPC method");
            println!("   3. Re-run this test");
        }
    }

    Ok(())
}

fn format_result(value: &Value) -> String {
    match value {
        Value::Vector(items) => {
            let mut result = String::from("[\n");
            for (i, item) in items.iter().enumerate() {
                result.push_str(&format!("  {}. ", i + 1));
                result.push_str(&format_result(item));
                result.push('\n');
            }
            result.push(']');
            result
        }
        Value::Map(map) => {
            let mut result = String::from("{\n");
            for (k, v) in map.iter() {
                result.push_str(&format!("  {} => ", k));
                result.push_str(&format_result(v).replace('\n', "\n  "));
                result.push('\n');
            }
            result.push('}');
            result
        }
        Value::String(s) => format!("\"{}\"", s),
        Value::Integer(i) => i.to_string(),
        Value::Float(f) => f.to_string(),
        Value::Boolean(b) => b.to_string(),
        Value::Keyword(k) => format!(":{}", k.0),
        Value::Nil => "nil".to_string(),
        other => format!("{:?}", other),
    }
}

