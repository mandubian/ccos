//! Runtime Lock resolution.

use autonoetic_types::runtime_lock::RuntimeLock;
use std::path::Path;

/// Parse and resolve a `runtime.lock` YAML file.
pub fn resolve_runtime_lock(path: &Path) -> anyhow::Result<RuntimeLock> {
    let contents = std::fs::read_to_string(path)?;
    let lock: RuntimeLock = serde_yaml::from_str(&contents)?;
    Ok(lock)
}
