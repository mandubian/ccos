use ccos::capabilities::registry::CapabilityRegistry;
use ccos::capability_marketplace::types::{CapabilityManifest, ProviderType, RegistryCapability};
use ccos::capability_marketplace::CapabilityMarketplace;
use ccos::causal_chain::CausalChain;
use ccos::host::RuntimeHost;
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::host_interface::HostInterface;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::RwLock;

#[tokio::test]
async fn test_runtime_host_normalization_flow() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));

    // 1. Register ccos.state.kv.put in marketplace with its schema
    let kv_put_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword("key".into()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("value".into()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: None,
    };

    let mut kv_put_manifest = CapabilityManifest::new(
        "ccos.state.kv.put".to_string(),
        "KV Put".to_string(),
        "Description".to_string(),
        ProviderType::Registry(RegistryCapability {
            capability_id: "ccos.state.kv.put".to_string(),
            registry: registry.clone(),
        }),
        "1.0.0".to_string(),
    );
    kv_put_manifest.input_schema = Some(kv_put_schema);

    marketplace
        .register_capability_manifest(kv_put_manifest)
        .await
        .expect("failed to register kv.put");

    // 2. Register ccos.io.write-file in marketplace with its schema
    let write_file_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword("path".into()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("content".into()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: None,
    };

    let mut write_file_manifest = CapabilityManifest::new(
        "ccos.io.write-file".to_string(),
        "Write File".to_string(),
        "Description".to_string(),
        ProviderType::Registry(RegistryCapability {
            capability_id: "ccos.io.write-file".to_string(),
            registry: registry.clone(),
        }),
        "1.0.0".to_string(),
    );
    write_file_manifest.input_schema = Some(write_file_schema);

    marketplace
        .register_capability_manifest(write_file_manifest)
        .await
        .expect("failed to register write-file");

    let causal_chain = Arc::new(Mutex::new(CausalChain::new().expect("causal_chain_failed")));
    let security_context = RuntimeContext::full();
    let host = Arc::new(RuntimeHost::new(
        causal_chain,
        marketplace,
        security_context,
    ));

    // Set a dummy execution context to satisfy host requirements
    host.set_execution_context(
        "test-plan".to_string(),
        vec!["test-intent".to_string()],
        "root-action".to_string(),
    );

    // Test CASE 1: ccos.state.kv.put
    // We call it with 2 positional arguments, and RTFS Host should normalize it to a map.
    let args = vec![Value::String("mykey".into()), Value::String("myval".into())];
    let result = host.execute_capability("ccos.state.kv.put", &args);

    assert!(
        result.is_ok(),
        "kv.put failed with error: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap(), Value::Boolean(true));

    // Test CASE 2: ccos.io.write-file
    let write_args = vec![
        Value::String("test_norm.txt".into()),
        Value::String("hello normalization".into()),
    ];
    let write_result = host.execute_capability("ccos.io.write-file", &write_args);

    assert!(
        write_result.is_ok(),
        "write-file failed with error: {:?}",
        write_result.err()
    );
    assert_eq!(write_result.unwrap(), Value::Boolean(true));
}
