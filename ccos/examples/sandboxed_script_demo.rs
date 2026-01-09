use ccos::capability_marketplace::executors::{
    CapabilityExecutor, ExecutionContext, SandboxedExecutor,
};
use ccos::capability_marketplace::types::{ProviderType, SandboxedCapability};
use rtfs::ast::MapKey;
use rtfs::runtime::values::Value;
use std::collections::HashMap;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    println!("\n╔══════════════════════════════════════════════════════════════╗");
    println!("║           🛡️  Sandboxed Script Execution Demo                 ║");
    println!("╚══════════════════════════════════════════════════════════════╝\n");

    let executor = SandboxedExecutor::new();

    let mut input_map = HashMap::new();
    input_map.insert(
        MapKey::String("name".to_string()),
        Value::String("CCOS User".to_string()),
    );
    input_map.insert(
        MapKey::String("task".to_string()),
        Value::String("demonstrate generic sandboxing".to_string()),
    );
    let input = Value::Map(input_map);

    // 1. Python in Process
    println!("🚀 [1/2] Running Python script in local Process provider...");
    let python_process = ProviderType::Sandboxed(SandboxedCapability {
        runtime: "python".to_string(),
        source: "import json; import os; print(json.dumps({'status': 'success', 'provider': 'process', 'received': json.load(open(os.environ['RTFS_INPUT_FILE']))}))".to_string(),
        entry_point: None,
        provider: Some("process".to_string()),
    });

    let metadata = HashMap::new();
    let context = ExecutionContext::new("demo.python.process", &metadata, None);
    match executor.execute(&python_process, &input, &context).await {
        Ok(result) => println!("✅ Result: {:?}\n", result),
        Err(e) => println!("❌ Error: {:?}\n", e),
    }

    // 2. Python in Firecracker
    println!("🚀 [2/2] Running Python script in Firecracker MicroVM...");
    let python_fc = ProviderType::Sandboxed(SandboxedCapability {
        runtime: "python".to_string(),
        source: "import json; import os; print(json.dumps({'status': 'success', 'provider': 'firecracker', 'received': json.load(open(os.environ['RTFS_INPUT_FILE']))}))".to_string(),
        entry_point: None,
        provider: Some("firecracker".to_string()),
    });

    let metadata = HashMap::new();
    let context = ExecutionContext::new("demo.python.fc", &metadata, None);
    match executor.execute(&python_fc, &input, &context).await {
        Ok(result) => println!("✅ Result: {:?}\n", result),
        Err(e) => println!("❌ Error: {:?}\n", e),
    }

    // 3. Large Data Test
    println!("🚀 [3/3] Testing large data payload (100KB)...");
    let mut large_map = HashMap::new();
    let large_string = "A".repeat(100 * 1024);
    large_map.insert(
        MapKey::String("large_data".to_string()),
        Value::String(large_string),
    );
    let large_input = Value::Map(large_map);

    let python_large = ProviderType::Sandboxed(SandboxedCapability {
        runtime: "python".to_string(),
        source: "import json; import os; data = json.load(open(os.environ['RTFS_INPUT_FILE'])); print(json.dumps({'received_size': len(data['large_data'])}))".to_string(),
        entry_point: None,
        provider: Some("process".to_string()),
    });

    let metadata = HashMap::new();
    let context = ExecutionContext::new("demo.python.large", &metadata, None);
    match executor
        .execute(&python_large, &large_input, &context)
        .await
    {
        Ok(result) => println!("✅ Result: {:?}\n", result),
        Err(e) => println!("❌ Error: {:?}\n", e),
    }

    println!("✨ Demo complete!");

    Ok(())
}
