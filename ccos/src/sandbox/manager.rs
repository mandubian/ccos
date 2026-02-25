use crate::sandbox::config::{SandboxConfig, SandboxRuntimeType};
use crate::sandbox::filesystem::MountMode;
use crate::sandbox::network_proxy::NetworkProxy;
use crate::sandbox::secret_injection::SecretInjector;
use crate::sandbox::SandboxRuntime;
use crate::utils::fs::get_workspace_root;
use async_trait::async_trait;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::microvm::{
    ExecutionContext as MicroVMExecutionContext, ExecutionResult, MicroVMFactory, Program,
};
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use url::Url;
use uuid::Uuid;

pub struct SandboxManager {
    runtimes: HashMap<SandboxRuntimeType, Arc<dyn SandboxRuntime>>,
    secret_injector: Arc<SecretInjector>,
}

impl SandboxManager {
    pub fn new() -> Self {
        let microvm_runtime: Arc<dyn SandboxRuntime> = Arc::new(MicroVMSandboxRuntime::new());
        let mut runtimes = HashMap::new();
        runtimes.insert(SandboxRuntimeType::MicroVM, microvm_runtime);

        // Attempt to initialize BubblewrapSandbox and register if available
        if let Ok(bwrap) = crate::sandbox::BubblewrapSandbox::new() {
            let bwrap_arc: Arc<dyn SandboxRuntime> = Arc::new(bwrap);
            runtimes.insert(SandboxRuntimeType::Bubblewrap, Arc::clone(&bwrap_arc));
            // Also fall back to Bubblewrap for "process" type by default
            runtimes.insert(SandboxRuntimeType::Process, bwrap_arc);
        } else {
            log::warn!("BubblewrapSandbox is unvailable. `ccos.sandbox.python` will fail if requested with `bubblewrap`.");
        }

        let secret_store = crate::secrets::SecretStore::new(Some(get_workspace_root()))
            .or_else(|_| crate::secrets::SecretStore::new(None))
            .unwrap_or_else(|e| {
                log::warn!("SandboxManager SecretStore unavailable: {}", e);
                crate::secrets::SecretStore::empty()
            });
        let secret_injector = Arc::new(SecretInjector::new(Arc::new(secret_store)));

        Self {
            runtimes,
            secret_injector,
        }
    }

    pub async fn execute(
        &self,
        config: &SandboxConfig,
        program: Program,
        inputs: Vec<Value>,
    ) -> RuntimeResult<ExecutionResult> {
        self.enforce_host_allowlist(config, &inputs)?;
        self.enforce_port_allowlist(config, &inputs)?;

        let mut config_for_runtime = config.clone();
        let mut _secret_dir_guard = None;
        if !config.required_secrets.is_empty() {
            let secret_mount = self.secret_injector.inject_for_sandbox(
                config
                    .capability_id
                    .as_deref()
                    .unwrap_or("unknown-capability"),
                &config.required_secrets,
            )?;

            let temp_dir = tempfile::tempdir().map_err(|e| {
                RuntimeError::Generic(format!("Failed to create secrets dir: {}", e))
            })?;
            for (name, value) in &secret_mount.files {
                let path = temp_dir.path().join(name);
                std::fs::write(&path, value).map_err(|e| {
                    RuntimeError::Generic(format!("Failed to write secret {}: {}", name, e))
                })?;
            }

            config_for_runtime.secret_mount_dir = Some(
                temp_dir
                    .path()
                    .to_str()
                    .unwrap_or("/run/secrets")
                    .to_string(),
            );
            _secret_dir_guard = Some(temp_dir);
        }

        let runtime = self
            .runtimes
            .get(&config_for_runtime.runtime_type)
            .ok_or_else(|| {
                RuntimeError::Generic(format!(
                    "Sandbox runtime {:?} not available",
                    config_for_runtime.runtime_type
                ))
            })?;

        runtime.execute(&config_for_runtime, program, inputs).await
    }

    fn enforce_port_allowlist(
        &self,
        config: &SandboxConfig,
        inputs: &[Value],
    ) -> RuntimeResult<()> {
        if config.allowed_ports.is_empty() {
            return Ok(());
        }

        let url_str = extract_url_from_inputs(inputs).unwrap_or_default();
        if url_str.is_empty() {
            return Err(RuntimeError::Generic(
                "Port allowlist requires a URL input".to_string(),
            ));
        }

        let parsed = Url::parse(&url_str).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse URL for port check: {}", e))
        })?;
        let port = parsed
            .port_or_known_default()
            .ok_or_else(|| RuntimeError::Generic("No port available for URL".to_string()))?;

        if config.allowed_ports.contains(&port) {
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Port {} not allowed (allowed={:?})",
                port, config.allowed_ports
            )))
        }
    }

    fn enforce_host_allowlist(
        &self,
        config: &SandboxConfig,
        inputs: &[Value],
    ) -> RuntimeResult<()> {
        if config.allowed_hosts.is_empty() {
            return Ok(());
        }

        let url_str = extract_url_from_inputs(inputs).unwrap_or_default();
        if url_str.is_empty() {
            return Err(RuntimeError::Generic(
                "Host allowlist requires a URL input".to_string(),
            ));
        }

        let parsed = Url::parse(&url_str).map_err(|e| {
            RuntimeError::Generic(format!("Failed to parse URL for host check: {}", e))
        })?;
        let host = parsed
            .host_str()
            .ok_or_else(|| RuntimeError::Generic("No host available for URL".to_string()))?;

        if config.allowed_hosts.contains(host) {
            Ok(())
        } else {
            Err(RuntimeError::Generic(format!(
                "Host {} not allowed (allowed={:?})",
                host, config.allowed_hosts
            )))
        }
    }

    pub fn build_network_proxy(&self, config: &SandboxConfig) -> NetworkProxy {
        NetworkProxy::new(
            config.allowed_hosts.clone(),
            config.allowed_ports.clone(),
            Arc::clone(&self.secret_injector),
        )
    }
}

fn extract_url_from_inputs(inputs: &[Value]) -> Option<String> {
    for value in inputs {
        match value {
            Value::String(url) => return Some(url.clone()),
            Value::Map(map) => {
                for (key, val) in map {
                    let key_str = match key {
                        rtfs::ast::MapKey::Keyword(kw) => kw.0.as_str(),
                        rtfs::ast::MapKey::String(s) => s.as_str(),
                        _ => continue,
                    };
                    if key_str == "url" || key_str == "endpoint" {
                        if let Value::String(url) = val {
                            return Some(url.clone());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    None
}

struct MicroVMSandboxRuntime {
    factory: Arc<Mutex<MicroVMFactory>>,
}

impl MicroVMSandboxRuntime {
    fn new() -> Self {
        let mut factory = MicroVMFactory::new();
        let provider_names: Vec<String> = factory
            .list_providers()
            .iter()
            .map(|name| name.to_string())
            .collect();

        for provider_name in provider_names {
            if let Some(provider) = factory.get_provider_mut(&provider_name) {
                let _ = provider.initialize();
            }
        }

        Self {
            factory: Arc::new(Mutex::new(factory)),
        }
    }
}

#[async_trait]
impl SandboxRuntime for MicroVMSandboxRuntime {
    fn name(&self) -> &str {
        "microvm"
    }

    async fn execute(
        &self,
        config: &SandboxConfig,
        program: Program,
        inputs: Vec<Value>,
    ) -> RuntimeResult<ExecutionResult> {
        let mut factory = self.factory.lock().map_err(|e| {
            RuntimeError::Generic(format!("SandboxedExecutor factory mutex poisoned: {}", e))
        })?;
        let provider_name = config.provider.as_deref().unwrap_or("process");

        let vm_provider = factory.get_provider_mut(provider_name).ok_or_else(|| {
            RuntimeError::Generic(format!("Provider '{}' not available", provider_name))
        })?;

        let mut vm_config = rtfs::runtime::microvm::MicroVMConfig::default();
        if !config.allowed_hosts.is_empty() {
            vm_config.network_policy = rtfs::runtime::microvm::NetworkPolicy::AllowList(
                config.allowed_hosts.iter().cloned().collect(),
            );
        }
        if !config.allowed_ports.is_empty() {
            log::debug!(
                "Sandbox allowed ports are not enforced yet: {:?}",
                config.allowed_ports
            );
        }
        let mut read_only_paths = Vec::new();
        let mut read_write_paths = Vec::new();

        if let Some(secret_dir) = &config.secret_mount_dir {
            read_only_paths.push(secret_dir.clone());
            vm_config
                .env_vars
                .insert("CCOS_SECRET_DIR".to_string(), secret_dir.clone());
        }

        if let Some(filesystem) = config.filesystem.as_ref() {
            for mount in &filesystem.mounts {
                let host_path = mount.host_path.to_string_lossy().to_string();
                match mount.mode {
                    MountMode::ReadOnly => read_only_paths.push(host_path),
                    MountMode::ReadWrite => read_write_paths.push(host_path),
                }
            }
        }

        if !read_write_paths.is_empty() {
            let mut paths = read_write_paths;
            paths.extend(read_only_paths);
            vm_config.fs_policy = rtfs::runtime::microvm::FileSystemPolicy::ReadWrite(paths);
        } else if !read_only_paths.is_empty() {
            vm_config.fs_policy =
                rtfs::runtime::microvm::FileSystemPolicy::ReadOnly(read_only_paths);
        }

        if let Some(resources) = config.resources.as_ref() {
            if resources.timeout_ms > 0 {
                vm_config.timeout = Duration::from_millis(resources.timeout_ms);
            }
            if resources.memory_mb > 0 {
                vm_config.memory_limit_mb = resources.memory_mb;
            }
            if resources.cpu_shares > 0 {
                vm_config.cpu_limit = resources.cpu_shares as f64;
            }
        }

        let permissions = config
            .capability_id
            .as_ref()
            .map(|id| vec![id.clone()])
            .unwrap_or_default();

        let context = MicroVMExecutionContext {
            execution_id: Uuid::new_v4().to_string(),
            program: Some(program),
            capability_id: config.capability_id.clone(),
            capability_permissions: permissions,
            args: inputs,
            config: vm_config,
            runtime_context: None,
        };

        vm_provider.execute_program(context)
    }
}
