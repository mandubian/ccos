use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::runtime::error::RuntimeResult;
use rtfs_compiler::runtime::values::Value;

fn main() -> RuntimeResult<()> {
    let registry = CapabilityRegistry::new();
    let capability = registry
        .get_capability("ccos.system.get-env")
        .expect("capability registered");

    let result = (capability.func)(vec![Value::String(
        "OPENWEATHERMAP_ORG_API_KEY".to_string(),
    )])?;

    println!("{:?}", result);
    Ok(())
}
