
use rtfs::runtime::microvm::config::{FileSystemPolicy, MicroVMConfig, NetworkPolicy};
use rtfs::runtime::microvm::core::ExecutionContext;
use rtfs::runtime::microvm::providers::{
    process::ProcessMicroVMProvider, MicroVMProvider,
};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

fn base_context() -> ExecutionContext {
    ExecutionContext {
        execution_id: "test".to_string(),
        program: None,
        capability_id: None,
        capability_permissions: vec![],
        args: vec![],
        config: MicroVMConfig::default(),
        runtime_context: Some(RuntimeContext::full()),
    }
}

#[test]
fn deny_network_when_policy_denied() {
    let mut provider = ProcessMicroVMProvider::new();
    provider.initialize().unwrap();

    let mut ctx = base_context();
    ctx.capability_id = Some("ccos.network.http-fetch".to_string());
    ctx.capability_permissions = vec!["ccos.network.http-fetch".to_string()];
    ctx.args = vec![Value::String("https://api.example.com/data".to_string())];
    ctx.config.network_policy = NetworkPolicy::Denied;

    let res = provider.execute_program(ctx);
    assert!(res.is_err(), "network should be denied");
}

#[test]
fn allow_network_when_in_allowlist() {
    let mut provider = ProcessMicroVMProvider::new();
    provider.initialize().unwrap();

    let mut ctx = base_context();
    ctx.capability_id = Some("ccos.network.http-fetch".to_string());
    ctx.capability_permissions = vec!["ccos.network.http-fetch".to_string()];
    ctx.args = vec![Value::String("https://api.example.com/data".to_string())];
    ctx.config.network_policy = NetworkPolicy::AllowList(vec!["api.example.com".to_string()]);

    let res = provider.execute_program(ctx);
    assert!(res.is_ok(), "allowlisted host should be permitted");
}

#[test]
fn deny_fs_write_when_readonly() {
    let mut provider = ProcessMicroVMProvider::new();
    provider.initialize().unwrap();

    let mut ctx = base_context();
    ctx.capability_id = Some("ccos.io.write-line".to_string());
    ctx.capability_permissions = vec!["ccos.io.write-line".to_string()];
    ctx.args = vec![Value::String("/tmp/test.txt".to_string())];
    ctx.config.fs_policy = FileSystemPolicy::ReadOnly(vec!["/tmp".to_string()]);

    let res = provider.execute_program(ctx);
    assert!(res.is_err(), "write should be denied on read-only policy");
}

#[test]
fn allow_fs_read_when_readonly_path_matches() {
    let mut provider = ProcessMicroVMProvider::new();
    provider.initialize().unwrap();

    let mut ctx = base_context();
    ctx.capability_id = Some("ccos.io.read-line".to_string());
    ctx.capability_permissions = vec!["ccos.io.read-line".to_string()];
    ctx.args = vec![Value::String("/tmp/test.txt".to_string())];
    ctx.config.fs_policy = FileSystemPolicy::ReadOnly(vec!["/tmp".to_string()]);

    let res = provider.execute_program(ctx);
    assert!(res.is_ok(), "read should be allowed on read-only path");
}
