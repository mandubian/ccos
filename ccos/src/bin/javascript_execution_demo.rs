use ccos::capability_marketplace::types::{
    ApprovalStatus, CapabilityManifest, EffectType, NativeCapability, ProviderType,
};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::config::types::SandboxConfig as GlobalSandboxConfig;
use ccos::sandbox::resources::ResourceLimits;
use ccos::sandbox::{BubblewrapSandbox, DependencyManager, SandboxConfig, SandboxRuntimeType};
use rtfs::ast::{Keyword, MapKey};
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("--- CCOS JavaScript Execution Demo ---");

    // 1. Setup Sandbox, Marketplace, and DependencyManager
    let sandbox = Arc::new(BubblewrapSandbox::new()?);

    // We need a DependencyManager for the library demo
    let dep_manager = Arc::new(DependencyManager::new(GlobalSandboxConfig::default()));

    use ccos::capabilities::registry::CapabilityRegistry;
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));

    // 2. Register JavaScript Execution Capability
    let sandbox_inner = Arc::clone(&sandbox);
    let dm_inner = Arc::clone(&dep_manager);

    let handler = Arc::new(move |inputs: &Value| {
        let inputs = inputs.clone();
        let sandbox = Arc::clone(&sandbox_inner);
        let dm = Arc::clone(&dm_inner);
        let fut = async move {
            let map = match &inputs {
                Value::Map(m) => m,
                _ => return Err(RuntimeError::Generic("Input must be a map".to_string())),
            };

            let code = map
                .get(&MapKey::Keyword(Keyword("code".to_string())))
                .and_then(|v| v.as_string())
                .ok_or_else(|| RuntimeError::Generic("Missing 'code'".to_string()))?
                .to_string();

            // Extract dependencies if present
            let dependencies = map
                .get(&MapKey::Keyword(Keyword("dependencies".to_string())))
                .and_then(|v| match v {
                    Value::Vector(v) => Some(
                        v.iter()
                            .filter_map(|x| x.as_string().map(|s| s.to_string()))
                            .collect::<Vec<_>>(),
                    ),
                    _ => None,
                });

            let timeout_ms = map
                .get(&MapKey::Keyword(Keyword("timeout_ms".to_string())))
                .and_then(|v| match v {
                    Value::Float(f) => Some(*f as u32),
                    Value::Integer(i) => Some(*i as u32),
                    _ => None,
                })
                .unwrap_or(30000);

            let max_memory_mb = map
                .get(&MapKey::Keyword(Keyword("max_memory_mb".to_string())))
                .and_then(|v| match v {
                    Value::Float(f) => Some(*f as u32),
                    Value::Integer(i) => Some(*i as u32),
                    _ => None,
                })
                .unwrap_or(512);

            let exec_config = SandboxConfig {
                runtime_type: SandboxRuntimeType::Process,
                capability_id: Some("ccos.execute.javascript".to_string()),
                resources: Some(ResourceLimits {
                    memory_mb: max_memory_mb as u64,
                    timeout_ms: timeout_ms as u64,
                    ..Default::default()
                }),
                ..Default::default()
            };

            let result = sandbox
                .execute_javascript(&code, &[], &exec_config, dependencies.as_deref(), Some(&dm))
                .await
                .map_err(|e| RuntimeError::Generic(format!("JS Execution failed: {}", e)))?;

            let mut out = HashMap::new();
            out.insert(
                MapKey::Keyword(Keyword("success".to_string())),
                Value::Boolean(result.success),
            );
            out.insert(
                MapKey::Keyword(Keyword("stdout".to_string())),
                Value::String(result.stdout),
            );
            out.insert(
                MapKey::Keyword(Keyword("stderr".to_string())),
                Value::String(result.stderr),
            );

            Ok(Value::Map(out))
        };
        Box::pin(fut)
            as futures::future::BoxFuture<'static, rtfs::runtime::error::RuntimeResult<Value>>
    });

    let manifest = CapabilityManifest {
        id: "ccos.execute.javascript".to_string(),
        name: "Execute Node.js Code".to_string(),
        description: "Execute Node.js snippets in a secure sandbox.".to_string(),
        provider: ProviderType::Native(NativeCapability {
            handler,
            security_level: "high".to_string(),
            metadata: HashMap::new(),
        }),
        version: "0.1.0".to_string(),
        input_schema: None,
        output_schema: None,
        attestation: None,
        provenance: None,
        permissions: vec![],
        effects: vec!["compute".to_string()],
        metadata: HashMap::new(),
        agent_metadata: None,
        domains: vec!["chat".to_string()],
        categories: vec!["demo".to_string()],
        effect_type: EffectType::default(),
        approval_status: ApprovalStatus::Approved,
    };

    marketplace.register_capability_manifest(manifest).await?;

    // 3. Test Cases

    // Case A: Basic Arithmetic
    println!("\n[Test A] Basic JavaScript Arithmetic");
    let input_a = Value::Map({
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("code".to_string())),
            Value::String("console.log(1 + 2 * 3);".to_string()),
        );
        m
    });
    let result_a = marketplace
        .execute_capability("ccos.execute.javascript", &input_a)
        .await?;
    println!("Result A: {:?}", result_a);

    // Case B: Memory Limit Enforcement (Try to allocate too much)
    println!("\n[Test B] Memory Limit Enforcement (50MB Limit)");
    let input_b = Value::Map({
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("code".to_string())),
            Value::String(
                "const buf = Buffer.alloc(100 * 1024 * 1024); console.log('Allocated 100MB');"
                    .to_string(),
            ),
        );
        m.insert(
            MapKey::Keyword(Keyword("max_memory_mb".to_string())),
            Value::Float(50.0),
        );
        m
    });
    let result_b = marketplace
        .execute_capability("ccos.execute.javascript", &input_b)
        .await;
    match result_b {
        Ok(res) => println!("Result B (Unexpected Success): {:?}", res),
        Err(e) => println!("Result B (Expected Failure): {}", e),
    }

    // Case C: Timeout Enforcement
    println!("\n[Test C] Timeout Enforcement (2s Limit)");
    let input_c = Value::Map({
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("code".to_string())),
            Value::String("while(true) {}".to_string()),
        );
        m.insert(
            MapKey::Keyword(Keyword("timeout_ms".to_string())),
            Value::Float(2000.0),
        );
        m
    });
    let result_c = marketplace
        .execute_capability("ccos.execute.javascript", &input_c)
        .await;
    match result_c {
        Ok(res) => println!("Result C (Unexpected Success): {:?}", res),
        Err(e) => println!("Result C (Expected Failure): {}", e),
    }

    // Case D: Dynamic Package Installation (lodash)
    println!("\n[Test D] Dynamic Package Installation (lodash)");
    let input_d = Value::Map({
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("code".to_string())),
            Value::String(
                "const _ = require('lodash'); console.log(_.chunk(['a', 'b', 'c', 'd'], 2));"
                    .to_string(),
            ),
        );
        m.insert(
            MapKey::Keyword(Keyword("dependencies".to_string())),
            Value::Vector(vec![Value::String("lodash".to_string())]),
        );
        m
    });
    let result_d = marketplace
        .execute_capability("ccos.execute.javascript", &input_d)
        .await;
    match result_d {
        Ok(res) => println!("Result D: {:?}", res),
        Err(e) => println!("Result D (Failure): {}", e),
    }

    println!("\nDemo completed.");
    Ok(())
}
