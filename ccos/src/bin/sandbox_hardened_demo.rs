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
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt::init();

    println!("--- CCOS Hardened Sandbox Demo ---");

    // Setup
    let sandbox = Arc::new(BubblewrapSandbox::new()?);
    let dm = Arc::new(DependencyManager::new(GlobalSandboxConfig::default()));

    // Register JS executor for testing
    let registry = Arc::new(RwLock::new(
        ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry));

    let sandbox_inner = Arc::clone(&sandbox);
    let dm_inner = Arc::clone(&dm);

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
                .unwrap_or_default()
                .to_string();

            let timeout_ms = map
                .get(&MapKey::Keyword(Keyword("timeout_ms".to_string())))
                .and_then(|v| match v {
                    Value::Integer(i) => Some(*i as u64),
                    Value::Float(f) => Some(*f as u64),
                    _ => None,
                })
                .unwrap_or(30000);

            let memory_mb = map
                .get(&MapKey::Keyword(Keyword("memory_mb".to_string())))
                .and_then(|v| match v {
                    Value::Integer(i) => Some(*i as u64),
                    Value::Float(f) => Some(*f as u64),
                    _ => None,
                })
                .unwrap_or(512);

            let allowed_hosts = map
                .get(&MapKey::Keyword(Keyword("allowed_hosts".to_string())))
                .and_then(|v| match v {
                    Value::Vector(vec) => Some(
                        vec.iter()
                            .filter_map(|x| x.as_string().map(|s| s.to_string()))
                            .collect::<HashSet<_>>(),
                    ),
                    _ => {
                        // Also try mapping from Keywords
                        Some(HashSet::new())
                    }
                })
                .unwrap_or_default();

            let exec_config = SandboxConfig {
                runtime_type: SandboxRuntimeType::Process,
                resources: Some(ResourceLimits {
                    memory_mb,
                    timeout_ms,
                    ..Default::default()
                }),
                allowed_hosts,
                ..Default::default()
            };

            let result = sandbox
                .execute_javascript(&code, &[], &exec_config, None, Some(&dm))
                .await
                .map_err(|e| RuntimeError::Generic(format!("JS failed: {}", e)))?;

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
        id: "test.execute.js".to_string(),
        name: "Test JS".to_string(),
        description: "Test JS".to_string(),
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
        effects: vec![],
        metadata: HashMap::new(),
        agent_metadata: None,
        domains: vec![],
        categories: vec!["demo".to_string()],
        effect_type: EffectType::Effectful,
        approval_status: ApprovalStatus::Approved,
    };
    marketplace.register_capability_manifest(manifest).await?;

    // 1. OOM Test
    println!("\n[Test 1] Memory Limit Enforcement (OOM Expected)");
    // Try to allocate 100MB with 50MB limit
    // Node needs some memory to start, so let's try a tight limit
    let input1 = Value::Map({
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("code".to_string())),
            Value::String(
                "const a = Buffer.alloc(100*1024*1024); console.log('success');".to_string(),
            ),
        );
        m.insert(
            MapKey::Keyword(Keyword("memory_mb".to_string())),
            Value::Integer(50),
        );
        m
    });
    let result1 = marketplace
        .execute_capability("test.execute.js", &input1)
        .await;
    match result1 {
        Ok(res) => println!("Result 1: {:?}", res),
        Err(e) => println!("Result 1 Error: {}", e),
    }

    // 2. Timeout Test
    println!("\n[Test 2] Timeout Enforcement (Timeout Expected)");
    let input2 = Value::Map({
        let mut m = HashMap::new();
        m.insert(
            MapKey::Keyword(Keyword("code".to_string())),
            Value::String("while(true) {}".to_string()),
        );
        m.insert(
            MapKey::Keyword(Keyword("timeout_ms".to_string())),
            Value::Integer(2000),
        );
        m
    });
    let result2 = marketplace
        .execute_capability("test.execute.js", &input2)
        .await;
    match result2 {
        Ok(res) => println!("Result 2: {:?}", res),
        Err(e) => println!("Result 2 Error: {}", e),
    }

    // 3. Network Isolation Test (Block)
    println!("\n[Test 3] Network Isolation (Block Expected)");
    let input3 = Value::Map({
        let mut m = HashMap::new();
        // Trying to resolve google.com without network should fail
        m.insert(MapKey::Keyword(Keyword("code".to_string())), Value::String("require('dns').lookup('google.com', (err) => { if(err) console.log('blocked: ' + err.code); else console.log('accessible'); });".to_string()));
        m
    });
    let result3 = marketplace
        .execute_capability("test.execute.js", &input3)
        .await;
    match result3 {
        Ok(res) => println!("Result 3: {:?}", res),
        Err(e) => println!("Result 3 Error: {}", e),
    }

    // 4. Network Isolation Test (Allow)
    println!("\n[Test 4] Network Isolation (Allow list provided)");
    let input4 = Value::Map({
        let mut m = HashMap::new();
        m.insert(MapKey::Keyword(Keyword("code".to_string())), Value::String("require('dns').lookup('google.com', (err) => { if(err) console.log('blocked: ' + err.code); else console.log('accessible'); });".to_string()));
        m.insert(
            MapKey::Keyword(Keyword("allowed_hosts".to_string())),
            Value::Vector(vec![Value::String("google.com".to_string())]),
        );
        m
    });
    let result4 = marketplace
        .execute_capability("test.execute.js", &input4)
        .await;
    match result4 {
        Ok(res) => println!("Result 4: {:?}", res),
        Err(e) => println!("Result 4 Error: {}", e),
    }

    println!("\nDemo completed.");
    Ok(())
}
