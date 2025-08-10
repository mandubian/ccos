//! CCOS Execution Environment
//!
//! Provides a comprehensive execution environment for RTFS programs with:
//! - Multiple security levels
//! - Configurable capability access
//! - Progress tracking
//! - Resource management

use crate::runtime::{
    Evaluator, RuntimeContext, 
    host::RuntimeHost,
    capability_marketplace::CapabilityMarketplace,
    values::Value,
    error::{RuntimeError, RuntimeResult},
};
use crate::ccos::{
    causal_chain::CausalChain,
    delegation::StaticDelegationEngine,
};
use crate::ast::{Expression, TopLevel};
use crate::parser;
use std::sync::Arc;
use std::rc::Rc;
#[allow(unused_imports)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Mutex;

/// Security levels for CCOS execution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityLevel {
    /// Minimal security - only basic functions
    Minimal,
    /// Standard security - most capabilities allowed
    Standard,
    /// Paranoid security - strict capability filtering
    Paranoid,
    /// Custom security - user-defined rules
    Custom,
}

/// Capability categories that can be enabled/disabled
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapabilityCategory {
    /// System operations (env vars, time, etc.)
    System,
    /// File I/O operations
    FileIO,
    /// Network operations
    Network,
    /// Agent operations
    Agent,
    /// AI/ML operations
    AI,
    /// Data processing operations
    Data,
    /// Logging operations
    Logging,
}

/// Configuration for CCOS execution environment
#[derive(Debug, Clone)]
pub struct CCOSConfig {
    /// Security level
    pub security_level: SecurityLevel,
    /// Enabled capability categories
    pub enabled_categories: Vec<CapabilityCategory>,
    /// Maximum execution time in milliseconds
    pub max_execution_time_ms: Option<u64>,
    /// Maximum memory usage in bytes
    pub max_memory_bytes: Option<u64>,
    /// Enable verbose logging
    pub verbose: bool,
    /// Custom security rules
    pub custom_rules: HashMap<String, bool>,
}

impl Default for CCOSConfig {
    fn default() -> Self {
        Self {
            security_level: SecurityLevel::Standard,
            enabled_categories: vec![
                CapabilityCategory::System,
                CapabilityCategory::Data,
                CapabilityCategory::Logging,
                CapabilityCategory::Agent,
            ],
            max_execution_time_ms: Some(30000), // 30 seconds
            max_memory_bytes: Some(100 * 1024 * 1024), // 100MB
            verbose: false,
            custom_rules: HashMap::new(),
        }
    }
}

/// CCOS execution environment that manages the complete runtime
pub struct CCOSEnvironment {
    config: CCOSConfig,
    host: Rc<RuntimeHost>,
    evaluator: Evaluator,
    #[allow(dead_code)]
    marketplace: Arc<CapabilityMarketplace>,
    // TODO: Remove this field once we have a proper capability marketplace
    registry: crate::runtime::capability_registry::CapabilityRegistry,
}

impl CCOSEnvironment {
    /// Create a new CCOS environment with the given configuration
    pub fn new(config: CCOSConfig) -> RuntimeResult<Self> {
        // Create capability registry
        let registry = Arc::new(tokio::sync::RwLock::new(crate::runtime::capability_registry::CapabilityRegistry::new()));
        // Create capability marketplace with integrated registry
        let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
        // Create causal chain for tracking
        let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));
        // Determine runtime context based on security level
        let runtime_context = match config.security_level {
            SecurityLevel::Minimal => RuntimeContext::pure(),
            SecurityLevel::Standard | SecurityLevel::Custom => RuntimeContext::full(),
            SecurityLevel::Paranoid => RuntimeContext::pure(),
        };
        // Create runtime host
        let host = Rc::new(RuntimeHost::new(
            causal_chain,
            marketplace.clone(),
            runtime_context.clone(),
        ));
        // Create module registry and load standard library
        let mut module_registry = crate::runtime::ModuleRegistry::new();
        crate::runtime::stdlib::load_stdlib(&mut module_registry)?;
        // Create delegation engine
        let delegation_engine = Arc::new(StaticDelegationEngine::new(HashMap::new()));
        // Create evaluator
        let evaluator = Evaluator::new(
            Rc::new(module_registry),
            delegation_engine,
            runtime_context,
            host.clone(),
        );
        Ok(Self {
            config,
            host,
            evaluator,
            marketplace,
            registry: crate::runtime::capability_registry::CapabilityRegistry::new(), // This field may be redundant now
        })
    }
    
    /// Execute a single RTFS expression
    pub fn execute_expression(&self, expr: &Expression) -> RuntimeResult<Value> {
        // Set up execution context for CCOS integration
        self.host.set_execution_context(
            "repl-session".to_string(),
            vec!["interactive".to_string()],
            "root-action".to_string()
        );

        // Ensure hierarchical execution context is initialized
        {
            let mut cm = self.evaluator.context_manager.borrow_mut();
            if cm.current_context_id().is_none() {
                cm.initialize(Some("repl-session".to_string()));
            }
        }
        
        // Execute the expression
        let result = self.evaluator.evaluate(expr);
        
        // Clean up execution context
        self.host.clear_execution_context();
        
        result
    }
    
    /// Execute RTFS code from a string
    pub fn execute_code(&self, code: &str) -> RuntimeResult<Value> {
        // Parse the code
        let parsed = parser::parse(code)
            .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;
        
        let mut last_result = Value::Nil;
        
        // Set up execution context for CCOS integration
        self.host.set_execution_context(
            "repl-execution".to_string(),
            vec!["repl-intent".to_string()],
            "root-action".to_string()
        );

        // Ensure hierarchical execution context is initialized
        {
            let mut cm = self.evaluator.context_manager.borrow_mut();
            if cm.current_context_id().is_none() {
                cm.initialize(Some("repl-execution".to_string()));
            }
        }
        
        // Execute each top-level item
        let execution_result = (|| -> RuntimeResult<Value> {
            for item in parsed {
                match item {
                    TopLevel::Expression(expr) => {
                        last_result = self.evaluator.evaluate(&expr)?;
                    }
                    _ => {
                        // For other top-level items, we could extend this to handle them
                        if self.config.verbose {
                            println!("Skipping non-expression top-level item");
                        }
                    }
                }
            }
            Ok(last_result)
        })();
        
        // Clean up execution context
        self.host.clear_execution_context();
        
        execution_result
    }
    
    /// Execute RTFS code from a file
    pub fn execute_file(&self, file_path: &str) -> RuntimeResult<Value> {
        let code = std::fs::read_to_string(file_path)
            .map_err(|e| RuntimeError::Generic(format!("Failed to read file '{}': {}", file_path, e)))?;
        
        if self.config.verbose {
            println!("📖 Executing file: {}", file_path);
            println!("📊 File size: {} bytes", code.len());
        }
        
        self.execute_code(&code)
    }
    
    /// Get current configuration
    pub fn config(&self) -> &CCOSConfig {
        &self.config
    }
    
    /// Update configuration (creates new environment)
    pub fn with_config(mut self, config: CCOSConfig) -> RuntimeResult<Self> {
        self.config = config;
        Self::new(self.config)
    }
    
    /// List available capabilities
    pub fn list_capabilities(&self) -> Vec<String> {
        let mut capabilities = Vec::new();
        
        // Add registry capabilities
        capabilities.extend(
            self.registry.list_capabilities()
                .into_iter()
                .map(|s| s.to_string())
        );
        
        // TODO: Add marketplace capabilities when we have async context
        
        capabilities.sort();
        capabilities
    }
    
    /// Check if a capability is available
    pub fn is_capability_available(&self, capability_id: &str) -> bool {
        self.registry.get_capability(capability_id).is_some()
    }
    
    /// Get execution statistics
    pub fn get_stats(&self) -> HashMap<String, Value> {
        let mut stats = HashMap::new();
        stats.insert("security_level".to_string(), Value::String(format!("{:?}", self.config.security_level)));
        stats.insert("enabled_categories".to_string(), Value::Vector(
            self.config.enabled_categories.iter()
                .map(|c| Value::String(format!("{:?}", c)))
                .collect()
        ));
        stats.insert("available_capabilities".to_string(), Value::Integer(self.list_capabilities().len() as i64));
        stats
    }
}

/// Builder for creating CCOS environments with specific configurations
pub struct CCOSBuilder {
    config: CCOSConfig,
}

impl CCOSBuilder {
    /// Create a new builder with default configuration
    pub fn new() -> Self {
        Self {
            config: CCOSConfig::default(),
        }
    }
    
    /// Set security level
    pub fn security_level(mut self, level: SecurityLevel) -> Self {
        self.config.security_level = level;
        self
    }
    
    /// Enable a capability category
    pub fn enable_category(mut self, category: CapabilityCategory) -> Self {
        if !self.config.enabled_categories.contains(&category) {
            self.config.enabled_categories.push(category);
        }
        self
    }
    
    /// Disable a capability category
    pub fn disable_category(mut self, category: CapabilityCategory) -> Self {
        self.config.enabled_categories.retain(|&c| c != category);
        self
    }
    
    /// Set maximum execution time
    pub fn max_execution_time(mut self, ms: u64) -> Self {
        self.config.max_execution_time_ms = Some(ms);
        self
    }
    
    /// Enable verbose logging
    pub fn verbose(mut self, verbose: bool) -> Self {
        self.config.verbose = verbose;
        self
    }
    
    /// Add custom security rule
    pub fn allow_capability(mut self, capability_id: &str) -> Self {
        self.config.custom_rules.insert(capability_id.to_string(), true);
        self
    }
    
    /// Deny specific capability
    pub fn deny_capability(mut self, capability_id: &str) -> Self {
        self.config.custom_rules.insert(capability_id.to_string(), false);
        self
    }
    
    /// Build the CCOS environment
    pub fn build(self) -> RuntimeResult<CCOSEnvironment> {
        CCOSEnvironment::new(self.config)
    }
}

impl Default for CCOSBuilder {
    fn default() -> Self {
        Self::new()
    }
}
