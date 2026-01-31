pub mod config;
pub mod filesystem;
pub mod manager;
pub mod network_proxy;
pub mod resources;
pub mod secret_injection;

pub use config::{SandboxConfig, SandboxRuntimeType};
pub use filesystem::{FilesystemMode, Mount, MountMode, VirtualFilesystem};
pub use manager::SandboxManager;
pub use network_proxy::{NetworkProxy, NetworkRequest, NetworkResponse};
pub use resources::{ResourceLimits, ResourceMetrics};
pub use secret_injection::{SecretInjector, SecretMount};

use async_trait::async_trait;
use rtfs::runtime::error::RuntimeResult;
use rtfs::runtime::microvm::{ExecutionResult, Program};
use rtfs::runtime::values::Value;

#[async_trait]
pub trait SandboxRuntime: Send + Sync {
    fn name(&self) -> &str;
    async fn execute(
        &self,
        config: &SandboxConfig,
        program: Program,
        inputs: Vec<Value>,
    ) -> RuntimeResult<ExecutionResult>;
}
