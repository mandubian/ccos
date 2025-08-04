//! MicroVM Factory for Provider Management

use std::collections::HashMap;
use crate::runtime::microvm::providers::MicroVMProvider;

/// Factory for creating and managing MicroVM providers
pub struct MicroVMFactory {
    providers: HashMap<String, Box<dyn MicroVMProvider>>,
}

impl MicroVMFactory {
    pub fn new() -> Self {
        let mut factory = Self {
            providers: HashMap::new(),
        };
        
        // Register default providers
        factory.register_default_providers();
        
        factory
    }

    fn register_default_providers(&mut self) {
        // Register mock provider (always available)
        self.register_provider("mock", Box::new(crate::runtime::microvm::providers::mock::MockMicroVMProvider::new()));
        
        // Register process provider (if available)
        let process_provider = crate::runtime::microvm::providers::process::ProcessMicroVMProvider::new();
        if process_provider.is_available() {
            self.register_provider("process", Box::new(process_provider));
        }
        
        // Register firecracker provider (if available)
        let firecracker_provider = crate::runtime::microvm::providers::firecracker::FirecrackerMicroVMProvider::new();
        if firecracker_provider.is_available() {
            self.register_provider("firecracker", Box::new(firecracker_provider));
        }
        
        // Register gvisor provider (if available)
        let gvisor_provider = crate::runtime::microvm::providers::gvisor::GvisorMicroVMProvider::default();
        if gvisor_provider.is_available() {
            self.register_provider("gvisor", Box::new(gvisor_provider));
        }
        
        // Register WASM provider (if available)
        let wasm_provider = crate::runtime::microvm::providers::wasm::WasmMicroVMProvider::new();
        if wasm_provider.is_available() {
            self.register_provider("wasm", Box::new(wasm_provider));
        }
    }

    /// Register a new MicroVM provider
    pub fn register_provider(&mut self, name: &str, provider: Box<dyn MicroVMProvider>) {
        self.providers.insert(name.to_string(), provider);
    }

    /// Get a provider by name
    pub fn get_provider(&self, name: &str) -> Option<&dyn MicroVMProvider> {
        self.providers.get(name).map(|p| p.as_ref())
    }

    /// Get a mutable reference to a provider by name
    pub fn get_provider_mut(&mut self, name: &str) -> Option<&mut Box<dyn MicroVMProvider>> {
        self.providers.get_mut(name)
    }

    /// List all registered provider names
    pub fn list_providers(&self) -> Vec<&str> {
        self.providers.keys().map(|k| k.as_str()).collect()
    }

    /// Get list of available providers (those that are available on the current system)
    pub fn get_available_providers(&self) -> Vec<&str> {
        self.providers
            .iter()
            .filter(|(_, provider)| provider.is_available())
            .map(|(name, _)| name.as_str())
            .collect()
    }

    /// Initialize a specific provider
    pub fn initialize_provider(&mut self, name: &str) -> crate::runtime::error::RuntimeResult<()> {
        if let Some(provider) = self.get_provider_mut(name) {
            provider.initialize()
        } else {
            Err(crate::runtime::error::RuntimeError::Generic(
                format!("Provider '{}' not found", name)
            ))
        }
    }

    /// Cleanup a specific provider
    pub fn cleanup_provider(&mut self, name: &str) -> crate::runtime::error::RuntimeResult<()> {
        if let Some(provider) = self.get_provider_mut(name) {
            provider.cleanup()
        } else {
            Err(crate::runtime::error::RuntimeError::Generic(
                format!("Provider '{}' not found", name)
            ))
        }
    }

    /// Cleanup all providers
    pub fn cleanup_all(&mut self) -> crate::runtime::error::RuntimeResult<()> {
        for (name, provider) in &mut self.providers {
            if let Err(e) = provider.cleanup() {
                eprintln!("Failed to cleanup provider '{}': {:?}", name, e);
            }
        }
        Ok(())
    }
}

impl Default for MicroVMFactory {
    fn default() -> Self {
        Self::new()
    }
}
