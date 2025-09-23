use std::sync::Arc;
use std::path::PathBuf;

use rtfs_compiler::ast::{Keyword, MapKey};
use rtfs_compiler::runtime::{IrRuntime, ModuleRegistry, RuntimeContext, Value, ExecutionOutcome};
use rtfs_compiler::ir::converter::IrConverter;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // --- Inputs ---
    let args: Vec<String> = std::env::args().collect();
    let city = args.get(1).cloned().unwrap_or_else(|| "Paris".to_string());
    let outfile = args
        .get(2)
        .cloned()
        .unwrap_or_else(|| "/tmp/weather.txt".to_string());

    println!("Running examples.mcp-and-fs/run with city='{}', outfile='{}'", city, outfile);

    // --- CCOS plumbing: registry, marketplace, causal chain, host, security ---
    // Set up the capability registry and marketplace for external tools (MCP, filesystem).
    let capability_registry = Arc::new(tokio::sync::RwLock::new(
        rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let marketplace = Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(
        capability_registry.clone(),
    ));

    // Register stub MCP tool: mcp.default_mcp_server.get-weather
    // This simulates an MCP server capability that fetches weather data.
    // Accepts a single map argument {:city "..."} and returns {:summary "..."}
    marketplace
        .register_local_capability(
            "mcp.default_mcp_server.get-weather".to_string(),
            "Weather (stub)".to_string(),
            "Returns a fake weather summary for a city".to_string(),
            Arc::new(|input| {
                // Expect Value::List([Value::Map({...})])
                let mut summary = String::from("Unknown");
                if let Value::List(list) = input {
                    if let Some(Value::Map(map)) = list.get(0) {
                        if let Some(Value::String(city)) = map.get(&MapKey::Keyword(Keyword("city".to_string()))) {
                            summary = format!("Sunny in {} with 22°C (stub)", city);
                        }
                    }
                }
                let mut out = std::collections::HashMap::new();
                out.insert(
                    MapKey::Keyword(Keyword("summary".to_string())),
                    Value::String(summary),
                );
                Ok(Value::Map(out))
            }),
        )
        .await?;

    // Register stub filesystem write capability: fs.write
    // This simulates a filesystem capability for writing files.
    // Accepts a single map {:path "..." :content "..."} and returns {:bytes-written int}
    marketplace
        .register_local_capability(
            "fs.write".to_string(),
            "Filesystem Write (stub)".to_string(),
            "Writes content to a local file (demo)".to_string(),
            Arc::new(|input| {
                // Expect Value::List([Value::Map({...})])
                let mut path_opt: Option<String> = None;
                let mut content_opt: Option<String> = None;
                if let Value::List(list) = input {
                    if let Some(Value::Map(map)) = list.get(0) {
                        if let Some(Value::String(p)) = map.get(&MapKey::Keyword(Keyword("path".to_string()))) {
                            path_opt = Some(p.clone());
                        }
                        if let Some(Value::String(c)) = map.get(&MapKey::Keyword(Keyword("content".to_string()))) {
                            content_opt = Some(c.clone());
                        }
                    }
                }
                let path = path_opt.ok_or_else(|| rtfs_compiler::runtime::error::RuntimeError::Generic(
                    "fs.write: missing :path".to_string(),
                ))?;
                let content = content_opt.ok_or_else(|| rtfs_compiler::runtime::error::RuntimeError::Generic(
                    "fs.write: missing :content".to_string(),
                ))?;
                std::fs::write(&path, &content).map_err(|e| rtfs_compiler::runtime::error::RuntimeError::Generic(format!(
                    "fs.write error: {}",
                    e
                )))?;
                let mut out = std::collections::HashMap::new();
                out.insert(
                    MapKey::Keyword(Keyword("bytes-written".to_string())),
                    Value::Integer(content.as_bytes().len() as i64),
                );
                Ok(Value::Map(out))
            }),
        )
        .await?;

    // Security: allow only our two demo capabilities to prevent unauthorized effects.
    let security_ctx = RuntimeContext::controlled(vec![
        "mcp.default_mcp_server.get-weather".to_string(),
        "fs.write".to_string(),
    ]);

    // Causal chain + Host: Set up the immutable audit ledger and the runtime host that manages effects.
    let causal_chain = Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::causal_chain::CausalChain::new().expect("create causal chain"),
    ));
    let host = Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain.clone(),
        marketplace.clone(),
        security_ctx.clone(),
    ));
    // Provide minimal execution context expected by Host (plan ID, intents, version).
    host.set_execution_context(
        "demo-plan".to_string(),
        vec!["intent-1".to_string()],
        "0".to_string(),
    );

    // --- Module loading ---
    // Load the RTFS module to make its functions available in the registry.
    let mut registry = ModuleRegistry::new();
    // Ensure the module path includes the repo root (one directory above this crate).
    let repo_root: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    registry.add_module_path(repo_root);

    // Ensure stdlib is loaded into the registry (needed for env.with_stdlib).
    rtfs_compiler::runtime::stdlib::load_stdlib(&mut registry)
        .map_err(|e| format!("Failed to load stdlib: {}", e))?;

    // Use the IR runtime to load modules (this compiles and registers the module's exports).
    let mut ir_runtime = IrRuntime::new(host.clone(), security_ctx.clone());
    let _module = registry
        .load_module("examples.mcp-and-fs", &mut ir_runtime)
        .map_err(|e| format!("Failed to load module examples.mcp-and-fs: {}", e))?;

    // Instead of resolving and binding the function manually, parse RTFS source directly that calls the function.
    // This demonstrates direct RTFS execution without manual IR construction.
    let rtfs_source = format!("(examples.mcp-and-fs/run {{:city \"{}\" :outfile \"{}\"}})", city, outfile);
    println!("Parsing and executing RTFS source: {}", rtfs_source);

    // Parse the RTFS source into AST expression.
    let ast = rtfs_compiler::parser::parse_expression(&rtfs_source)
        .map_err(|e| format!("Failed to parse RTFS: {:?}", e))?;

    // Compile AST to IR using IrConverter.
    let mut converter = IrConverter::with_module_registry(&registry);
    let ir = converter.convert(&ast)
        .map_err(|e| format!("Failed to compile to IR: {:?}", e))?;

    // Prepare IR environment from stdlib (module functions are resolved via qualified symbols in IR).
    let mut env = rtfs_compiler::runtime::environment::IrEnvironment::with_stdlib(&registry)
        .map_err(|e| format!("Failed to create IR environment with stdlib: {}", e))?;

    // Execute the IR node via IR runtime.
    match ir_runtime.execute_node(&ir, &mut env, false, &mut registry)? {
        ExecutionOutcome::Complete(v) => {
            println!("Result: {:?}", v);
        }
        ExecutionOutcome::RequiresHost(hc) => {
            eprintln!("Host call still required unexpectedly: {:?}", hc);
        }
    }

    println!("Done.");
    Ok(())
}
