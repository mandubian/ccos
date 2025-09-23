use std::sync::Arc;
use std::path::PathBuf;

use rtfs_compiler::ast::{Keyword, MapKey};
use rtfs_compiler::runtime::{IrRuntime, ModuleRegistry, RuntimeContext, Value, ExecutionOutcome};
use rtfs_compiler::ir::core::{IrNode, IrType};

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
    let capability_registry = Arc::new(tokio::sync::RwLock::new(
        rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let marketplace = Arc::new(rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(
        capability_registry.clone(),
    ));

    // Register stub MCP tool: mcp.default_mcp_server.get-weather
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

    // Security: allow only our two demo capabilities
    let security_ctx = RuntimeContext::controlled(vec![
        "mcp.default_mcp_server.get-weather".to_string(),
        "fs.write".to_string(),
    ]);

    // Causal chain + Host
    let causal_chain = Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::causal_chain::CausalChain::new().expect("create causal chain"),
    ));
    let host = Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain.clone(),
        marketplace.clone(),
        security_ctx.clone(),
    ));
    // Provide minimal execution context expected by Host
    host.set_execution_context(
        "demo-plan".to_string(),
        vec!["intent-1".to_string()],
        "0".to_string(),
    );

    // --- Module loading ---
    let mut registry = ModuleRegistry::new();
    // Ensure the module path includes the repo root (one directory above this crate)
    let repo_root: PathBuf = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    registry.add_module_path(repo_root);

    // Ensure stdlib is loaded into the registry (needed for env.with_stdlib)
    rtfs_compiler::runtime::stdlib::load_stdlib(&mut registry)
        .map_err(|e| format!("Failed to load stdlib: {}", e))?;

    // Use the IR runtime to load modules
    let mut ir_runtime = IrRuntime::new(host.clone(), security_ctx.clone());
    let _module = registry
        .load_module("examples.mcp-and-fs", &mut ir_runtime)
        .map_err(|e| format!("Failed to load module examples.mcp-and-fs: {}", e))?;

    // Resolve exported function value (should be a Function or placeholder to IR lambda)
    let run_func_val = registry
        .resolve_qualified_symbol("examples.mcp-and-fs/run")
        .map_err(|e| format!("Failed to resolve run export: {}", e))?;

    // Prepare IR environment from stdlib and insert the exported function as a variable
    let mut env = rtfs_compiler::runtime::environment::IrEnvironment::with_stdlib(&registry)
        .map_err(|e| format!("Failed to create IR environment with stdlib: {}", e))?;
    // Insert the function value under a temp name and reference it as VariableRef
    env.define("__run".to_string(), run_func_val.clone());
    let call_ir = IrNode::Apply {
        id: 0,
        function: Box::new(IrNode::VariableRef { id: 0, name: "__run".to_string(), binding_id: 0, ir_type: IrType::Any, source_location: None }),
        arguments: vec![
            IrNode::Map {
                id: 0,
                entries: vec![
                    rtfs_compiler::ir::core::IrMapEntry {
                        key: IrNode::Literal { id: 0, value: rtfs_compiler::ast::Literal::Keyword(Keyword("city".into())), ir_type: IrType::Keyword, source_location: None },
                        value: IrNode::Literal { id: 0, value: rtfs_compiler::ast::Literal::String(city.clone()), ir_type: IrType::String, source_location: None },
                    },
                    rtfs_compiler::ir::core::IrMapEntry {
                        key: IrNode::Literal { id: 0, value: rtfs_compiler::ast::Literal::Keyword(Keyword("outfile".into())), ir_type: IrType::Keyword, source_location: None },
                        value: IrNode::Literal { id: 0, value: rtfs_compiler::ast::Literal::String(outfile.clone()), ir_type: IrType::String, source_location: None },
                    },
                ],
                ir_type: IrType::Any,
                source_location: None,
            }
        ],
        ir_type: IrType::Any,
        source_location: None,
    };

    // Execute the Apply node via IR runtime
    match ir_runtime.execute_node(&call_ir, &mut env, false, &mut registry)? {
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
