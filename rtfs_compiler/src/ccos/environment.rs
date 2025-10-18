//! CCOS Execution Environment
//!
//! Provides a comprehensive execution environment for RTFS programs with:
//! - Multiple security levels
//! - Configurable capability access
//! - Progress tracking
//! - Resource management

use crate::ast::{Expression, TopLevel, DoExpr};
use crate::ccos::causal_chain::CausalChain;
use crate::ccos::{capability_marketplace::CapabilityMarketplace, host::RuntimeHost};
use crate::parser;
use crate::runtime::host_interface::HostInterface;
use crate::runtime::{
    error::{RuntimeError, RuntimeResult},
    execution_outcome::ExecutionOutcome,
    values::Value,
    Evaluator, RuntimeContext,
};
use std::sync::Arc;
// switched to Arc for ModuleRegistry
use crate::ast::{Keyword, MapKey};
use crate::ccos::working_memory::{InMemoryJsonlBackend, WorkingMemory, WorkingMemorySink};
#[allow(unused_imports)]
use std::cell::RefCell;
use std::collections::HashMap;
use std::env;
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
    /// Enable Working Memory ingestion from Causal Chain events
    pub enable_wm_ingestor: bool,
    /// Optional WM budgets
    pub wm_max_entries: Option<usize>,
    pub wm_max_tokens: Option<usize>,
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
            enable_wm_ingestor: true,
            wm_max_entries: Some(2000),
            wm_max_tokens: Some(200_000),
        }
    }
}

/// CCOS execution environment that manages the complete runtime
pub struct CCOSEnvironment {
    config: CCOSConfig,
    host: Arc<RuntimeHost>,
    evaluator: std::sync::Mutex<Evaluator>,
    #[allow(dead_code)]
    marketplace: Arc<CapabilityMarketplace>,
    // TODO: Remove this field once we have a proper capability marketplace
    registry: crate::ccos::capabilities::registry::CapabilityRegistry,
    /// Optional Working Memory exposed when WM ingestor is enabled
    wm: Option<Arc<Mutex<WorkingMemory>>>,
}

impl CCOSEnvironment {
    /// Create a new CCOS environment with the given configuration
    pub fn new(config: CCOSConfig) -> RuntimeResult<Self> {
        // Create capability registry
        let registry = Arc::new(tokio::sync::RwLock::new(
            crate::ccos::capabilities::registry::CapabilityRegistry::new(),
        ));
        // Create capability marketplace with integrated registry
        let marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));
        
        // Bootstrap the marketplace to register default capabilities
        let marketplace_for_bootstrap = marketplace.clone();
        let _: Result<(), Box<dyn std::error::Error + Send + Sync>> =
            futures::executor::block_on(async move {
                marketplace_for_bootstrap.bootstrap().await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
            });
        
        // Create causal chain for tracking
        let causal_chain = Arc::new(Mutex::new(CausalChain::new()?));

        // Optionally attach a Working Memory ingestor sink
        let enable_wm = config.enable_wm_ingestor
            || env::var("CCOS_ENABLE_WM_INGESTOR")
                .map(|v| {
                    let lv = v.to_lowercase();
                    lv == "1" || lv == "true"
                })
                .unwrap_or(false);

        let wm: Option<Arc<Mutex<WorkingMemory>>> = if enable_wm {
            let max_entries = env::var("CCOS_WM_MAX_ENTRIES")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .or(config.wm_max_entries);
            let max_tokens = env::var("CCOS_WM_MAX_TOKENS")
                .ok()
                .and_then(|s| s.parse::<usize>().ok())
                .or(config.wm_max_tokens);
            let backend = InMemoryJsonlBackend::new(None, max_entries, max_tokens);
            let wm = Arc::new(Mutex::new(WorkingMemory::new(Box::new(backend))));
            // Register sink
            if let Ok(mut chain) = causal_chain.lock() {
                let sink: Arc<dyn crate::ccos::event_sink::CausalChainEventSink> =
                    Arc::new(WorkingMemorySink::new(wm.clone()));
                chain.register_event_sink(sink);
            }
            Some(wm)
        } else {
            None
        };
        // Determine runtime context based on security level
        let runtime_context = match config.security_level {
            SecurityLevel::Minimal => RuntimeContext::pure(),
            SecurityLevel::Standard | SecurityLevel::Custom => RuntimeContext::full(),
            SecurityLevel::Paranoid => RuntimeContext::pure(),
        };
        // Create runtime host
        let host = Arc::new(RuntimeHost::new(
            causal_chain,
            marketplace.clone(),
            runtime_context.clone(),
        ));
        // Create module registry and load standard library
        let mut module_registry = crate::runtime::ModuleRegistry::new();
        crate::runtime::stdlib::load_stdlib(&mut module_registry)?;
        // Create delegation engine
        // Create evaluator
        let evaluator = std::sync::Mutex::new(Evaluator::new(
            std::sync::Arc::new(module_registry),
            runtime_context,
            host.clone(),
        ));

        // Register local capability: observability.ingestor:v1.ingest
        // Provides on-demand ingestion into Working Memory: modes single | batch | replay
        {
            let wm_for_cap = wm.clone();
            let host_for_cap = host.clone();
            let marketplace_for_cap = marketplace.clone();
            let handler = std::sync::Arc::new(move |input: &Value| -> RuntimeResult<Value> {
                // Closure helpers
                fn map_get<'a>(
                    m: &'a std::collections::HashMap<MapKey, Value>,
                    key: &str,
                ) -> Option<&'a Value> {
                    let k1 = MapKey::String(key.to_string());
                    let k2 = MapKey::Keyword(Keyword(key.to_string()));
                    m.get(&k1).or_else(|| m.get(&k2))
                }

                fn to_string_opt(v: &Value) -> Option<String> {
                    match v {
                        Value::String(s) => Some(s.clone()),
                        _ => None,
                    }
                }

                fn to_i64_opt(v: &Value) -> Option<i64> {
                    match v {
                        Value::Integer(i) => Some(*i),
                        _ => None,
                    }
                }

                fn parse_record(
                    v: &Value,
                ) -> Result<crate::ccos::working_memory::ingestor::ActionRecord, RuntimeError>
                {
                    let mut action_id = None;
                    let mut kind = "CapabilityCall".to_string();
                    let mut provider: Option<String> = None;
                    let mut timestamp_s: u64 = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs();
                    let mut summary = String::new();
                    let mut content = String::new();
                    let mut plan_id: Option<String> = None;
                    let mut intent_id: Option<String> = None;
                    let mut step_id: Option<String> = None;
                    let mut attestation_hash: Option<String> = None;
                    let mut content_hash: Option<String> = None;

                    match v {
                        Value::Map(map) => {
                            if let Some(val) =
                                map_get(map, "action_id").or_else(|| map_get(map, "action-id"))
                            {
                                action_id = to_string_opt(val);
                            }
                            if let Some(val) = map_get(map, "kind") {
                                if let Some(s) = to_string_opt(val) {
                                    kind = s;
                                }
                            }
                            if let Some(val) = map_get(map, "provider") {
                                provider = to_string_opt(val);
                            }
                            if let Some(val) =
                                map_get(map, "timestamp_s").or_else(|| map_get(map, "timestamp"))
                            {
                                if let Some(i) = to_i64_opt(val) {
                                    if i >= 0 {
                                        timestamp_s = i as u64;
                                    }
                                }
                            }
                            if let Some(val) = map_get(map, "summary") {
                                if let Some(s) = to_string_opt(val) {
                                    summary = s;
                                }
                            }
                            if let Some(val) = map_get(map, "content") {
                                if let Some(s) = to_string_opt(val) {
                                    content = s;
                                }
                            }
                            if let Some(val) =
                                map_get(map, "plan_id").or_else(|| map_get(map, "plan-id"))
                            {
                                plan_id = to_string_opt(val);
                            }
                            if let Some(val) =
                                map_get(map, "intent_id").or_else(|| map_get(map, "intent-id"))
                            {
                                intent_id = to_string_opt(val);
                            }
                            if let Some(val) =
                                map_get(map, "step_id").or_else(|| map_get(map, "step-id"))
                            {
                                step_id = to_string_opt(val);
                            }
                            if let Some(val) = map_get(map, "attestation_hash")
                                .or_else(|| map_get(map, "attestation-hash"))
                            {
                                attestation_hash = to_string_opt(val);
                            }
                            if let Some(val) = map_get(map, "content_hash")
                                .or_else(|| map_get(map, "content-hash"))
                            {
                                content_hash = to_string_opt(val);
                            }
                        }
                        other => {
                            return Err(RuntimeError::TypeError {
                                expected: "map".into(),
                                actual: other.type_name().into(),
                                operation: "observability.ingestor:v1.ingest/record".into(),
                            });
                        }
                    }

                    let action_id = action_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

                    Ok(crate::ccos::working_memory::ingestor::ActionRecord {
                        action_id,
                        kind,
                        provider,
                        timestamp_s,
                        summary,
                        content,
                        plan_id,
                        intent_id,
                        step_id,
                        attestation_hash,
                        content_hash,
                    })
                }

                // Parse high-level inputs
                let (mode, records): (
                    String,
                    Vec<crate::ccos::working_memory::ingestor::ActionRecord>,
                ) = match input {
                    // New calling convention: { :args [...] , :context ... }
                    Value::Map(m) => {
                        let args_val = map_get(m, "args").cloned().unwrap_or(Value::List(vec![]));
                        match args_val {
                            Value::List(args) => {
                                // Supported forms:
                                // ["single", <record>]
                                // ["batch", [<record>...]]
                                // ["replay"]
                                let mode = args
                                    .get(0)
                                    .and_then(|v| v.as_string())
                                    .unwrap_or("single")
                                    .to_string();
                                match mode.as_str() {
                                    "single" => {
                                        let rec_v = args.get(1).ok_or_else(|| {
                                            RuntimeError::Generic(
                                                "missing record for single mode".into(),
                                            )
                                        })?;
                                        let rec = parse_record(rec_v)?;
                                        (mode, vec![rec])
                                    }
                                    "batch" => {
                                        let list_v = args.get(1).ok_or_else(|| {
                                            RuntimeError::Generic(
                                                "missing records for batch mode".into(),
                                            )
                                        })?;
                                        let mut recs = Vec::new();
                                        if let Value::Vector(vs) | Value::List(vs) = list_v {
                                            for v in vs {
                                                recs.push(parse_record(v)?);
                                            }
                                        } else {
                                            return Err(RuntimeError::TypeError {
                                                expected: "list".into(),
                                                actual: list_v.type_name().into(),
                                                operation: "observability.ingestor:v1.ingest"
                                                    .into(),
                                            });
                                        }
                                        (mode, recs)
                                    }
                                    "replay" => (mode, vec![]),
                                    _ => {
                                        return Err(RuntimeError::Generic(format!(
                                            "unsupported mode: {}",
                                            mode
                                        )))
                                    }
                                }
                            }
                            other => {
                                return Err(RuntimeError::TypeError {
                                    expected: "list".into(),
                                    actual: other.type_name().into(),
                                    operation: "observability.ingestor:v1.ingest".into(),
                                })
                            }
                        }
                    }
                    // Back-compat: raw list
                    Value::List(args) => {
                        let mode = args
                            .get(0)
                            .and_then(|v| v.as_string())
                            .unwrap_or("single")
                            .to_string();
                        match mode.as_str() {
                            "single" => {
                                let rec_v = args.get(1).ok_or_else(|| {
                                    RuntimeError::Generic("missing record for single mode".into())
                                })?;
                                let rec = parse_record(rec_v)?;
                                (mode, vec![rec])
                            }
                            "batch" => {
                                let list_v = args.get(1).ok_or_else(|| {
                                    RuntimeError::Generic("missing records for batch mode".into())
                                })?;
                                let mut recs = Vec::new();
                                if let Value::Vector(vs) | Value::List(vs) = list_v {
                                    for v in vs {
                                        recs.push(parse_record(v)?);
                                    }
                                } else {
                                    return Err(RuntimeError::TypeError {
                                        expected: "list".into(),
                                        actual: list_v.type_name().into(),
                                        operation: "observability.ingestor:v1.ingest".into(),
                                    });
                                }
                                (mode, recs)
                            }
                            "replay" => (mode, vec![]),
                            _ => {
                                return Err(RuntimeError::Generic(format!(
                                    "unsupported mode: {}",
                                    mode
                                )))
                            }
                        }
                    }
                    other => {
                        return Err(RuntimeError::TypeError {
                            expected: "map or list".into(),
                            actual: other.type_name().into(),
                            operation: "observability.ingestor:v1.ingest".into(),
                        })
                    }
                };

                // Ensure WM is available
                let wm_arc = wm_for_cap.clone().ok_or_else(|| {
                    RuntimeError::Generic("Working Memory ingestor not enabled".into())
                })?;

                // Execute modes
                match mode.as_str() {
                    "single" | "batch" => {
                        let mut ingested = 0usize;
                        let mut wm_guard = wm_arc.lock().map_err(|_| {
                            RuntimeError::Generic("Failed to lock WorkingMemory".into())
                        })?;
                        for rec in &records {
                            if crate::ccos::working_memory::ingestor::MemoryIngestor::ingest_action(
                                &mut *wm_guard,
                                rec,
                            )
                            .is_ok()
                            {
                                ingested += 1;
                            }
                        }
                        let mut out = std::collections::HashMap::new();
                        out.insert(MapKey::String("mode".into()), Value::String(mode));
                        out.insert(
                            MapKey::String("ingested".into()),
                            Value::Integer(ingested as i64),
                        );
                        Ok(Value::Map(out))
                    }
                    "replay" => {
                        // Snapshot actions via host and rebuild WM
                        let actions = host_for_cap.snapshot_actions()?;
                        let mut records = Vec::with_capacity(actions.len());
                        for a in &actions {
                            let summary = a
                                .function_name
                                .clone()
                                .unwrap_or_else(|| format!("{:?}", a.action_type));
                            let mut content = format!(
                                "type={:?}; plan={}; intent={}; ts={}",
                                a.action_type, a.plan_id, a.intent_id, a.timestamp
                            );
                            if let Some(fn_name) = &a.function_name {
                                content.push_str(&format!("; fn={}", fn_name));
                            }
                            if let Some(args) = &a.arguments {
                                content.push_str(&format!("; args={}", args.len()));
                            }
                            if let Some(cost) = a.cost {
                                content.push_str(&format!("; cost={}", cost));
                            }
                            if let Some(d) = a.duration_ms {
                                content.push_str(&format!("; dur_ms={}", d));
                            }
                            let att = a.metadata.get("signature").and_then(|v| match v {
                                Value::String(s) => Some(s.clone()),
                                _ => None,
                            });
                            records.push(crate::ccos::working_memory::ingestor::ActionRecord {
                                action_id: a.action_id.clone(),
                                kind: format!("{:?}", a.action_type),
                                provider: a.function_name.clone(),
                                timestamp_s: a.timestamp,
                                summary,
                                content,
                                plan_id: Some(a.plan_id.clone()),
                                intent_id: Some(a.intent_id.clone()),
                                step_id: None,
                                attestation_hash: att,
                                content_hash: None,
                            });
                        }
                        let mut wm_guard = wm_arc.lock().map_err(|_| {
                            RuntimeError::Generic("Failed to lock WorkingMemory".into())
                        })?;
                        crate::ccos::working_memory::ingestor::MemoryIngestor::replay_all(
                            &mut *wm_guard,
                            &records,
                        )
                        .map_err(|e| RuntimeError::Generic(format!("WM replay failed: {:?}", e)))?;
                        let mut out = std::collections::HashMap::new();
                        out.insert(
                            MapKey::String("mode".into()),
                            Value::String("replay".into()),
                        );
                        out.insert(
                            MapKey::String("scanned_actions".into()),
                            Value::Integer(actions.len() as i64),
                        );
                        out.insert(
                            MapKey::String("ingested".into()),
                            Value::Integer(records.len() as i64),
                        );
                        Ok(Value::Map(out))
                    }
                    _ => Err(RuntimeError::Generic("unreachable mode".into())),
                }
            });

            let _: Result<(), Box<dyn std::error::Error + Send + Sync>> =
                futures::executor::block_on(async move {
                    marketplace_for_cap.register_local_capability(
                    "observability.ingestor:v1.ingest".to_string(),
                    "Observability WM Ingestor".to_string(),
                    "Ingest Working Memory entries from provided records or replay from Causal Chain".to_string(),
                    handler,
                ).await.map_err(|e| Box::new(e) as Box<dyn std::error::Error + Send + Sync>)
                });
        }
        Ok(Self {
            config,
            host,
            evaluator,
            marketplace,
            registry: crate::ccos::capabilities::registry::CapabilityRegistry::new(), // This field may be redundant now
            wm,
        })
    }

    /// Execute a single RTFS expression
    ///
    /// Note: this now returns an ExecutionOutcome so callers (the orchestrator / CCOS)
    /// can observe host invocation descriptors (RequiresHost) and decide delegation/resume.
    pub fn execute_expression(&self, expr: &Expression) -> RuntimeResult<ExecutionOutcome> {
        // Set up execution context for CCOS integration
        self.host.set_execution_context(
            "repl-session".to_string(),
            vec!["interactive".to_string()],
            "root-action".to_string(),
        );

        // Execute the expression and propagate the ExecutionOutcome upward.
        let result = {
            let evaluator = self.evaluator.lock().map_err(|_| RuntimeError::Generic("Failed to lock evaluator".to_string()))?;
            
            // Ensure hierarchical execution context is initialized
            {
                let mut cm = evaluator.context_manager.borrow_mut();
                if cm.current_context_id().is_none() {
                    cm.initialize(Some("repl-session".to_string()));
                }
            }
            
            // Evaluate expression
            evaluator.evaluate(expr)
        };

        // Clean up execution context
        self.host.clear_execution_context();

        result
    }

    /// Execute RTFS code from a string
    ///
    /// Returns an ExecutionOutcome describing either a completed Value or a host-call request.
    pub fn execute_code(&self, code: &str) -> RuntimeResult<ExecutionOutcome> {
        // Parse the code
        let parsed = parser::parse(code)
            .map_err(|e| RuntimeError::Generic(format!("Parse error: {:?}", e)))?;

        let mut last_result: ExecutionOutcome = ExecutionOutcome::Complete(Value::Nil);

        // Set up execution context for CCOS integration
        self.host.set_execution_context(
            "repl-execution".to_string(),
            vec!["repl-intent".to_string()],
            "root-action".to_string(),
        );

        // Execute each top-level item
        let execution_result = (|| -> RuntimeResult<ExecutionOutcome> {
            for item in parsed {
                let mut evaluator = self.evaluator.lock().map_err(|_| RuntimeError::Generic("Failed to lock evaluator".to_string()))?;
                
                // Ensure hierarchical execution context is initialized
                {
                    let mut cm = evaluator.context_manager.borrow_mut();
                    if cm.current_context_id().is_none() {
                        cm.initialize(Some("repl-execution".to_string()));
                    }
                }
                match item {
                    TopLevel::Expression(expr) => {
                        // Evaluate expression
                        last_result = evaluator.evaluate(&expr)?;
                        
                        // Special handling for function definitions to ensure they persist in the environment
                        if let ExecutionOutcome::Complete(Value::Function(_)) = last_result {
                            if let Expression::Defn(defn_expr) = expr {
                                // Manually define the function in the evaluator's environment
                                // This ensures the binding persists across evaluations
                                let function = Value::Function(crate::runtime::values::Function::new_closure(
                                    defn_expr.params.iter().map(|p| {
                                        match &p.pattern {
                                            crate::ast::Pattern::Symbol(s) => s.clone(),
                                            _ => panic!("Expected symbol pattern in defn parameter"),
                                        }
                                    }).collect(),
                                    defn_expr.params.iter().map(|p| p.pattern.clone()).collect(),
                                    defn_expr.variadic_param.as_ref().map(|p| {
                                        match &p.pattern {
                                            crate::ast::Pattern::Symbol(s) => s.clone(),
                                            _ => panic!("Expected symbol pattern in defn variadic parameter"),
                                        }
                                    }),
                                    Box::new(Expression::Do(DoExpr {
                                        expressions: defn_expr.body.clone(),
                                    })),
                                    Arc::new(evaluator.env.clone()),
                                    defn_expr.delegation_hint.clone(),
                                ));
                                evaluator.env.define(&defn_expr.name, function);
                            }
                        }
                        
                        if let ExecutionOutcome::RequiresHost(_) = last_result {
                            return Ok(last_result);
                        }
                    }
                    TopLevel::Module(module_def) => {
                        // Handle module definitions
                        if self.config.verbose {
                            println!("Processing module definition: {:?}", module_def.name);
                        }
                        // TODO: Implement module loading and execution
                        // For now, just process the definitions within the module
                        for def in &module_def.definitions {
                            match def {
                                crate::ast::ModuleLevelDefinition::Def(def_expr) => {
                                    let expr = Expression::Def(Box::new(def_expr.clone()));
                                    last_result = evaluator.evaluate(&expr)?;
                                    if let ExecutionOutcome::RequiresHost(_) = last_result {
                                        return Ok(last_result);
                                    }
                                }
                                crate::ast::ModuleLevelDefinition::Defn(defn_expr) => {
                                    let expr = Expression::Defn(Box::new(defn_expr.clone()));
                                    last_result = evaluator.evaluate(&expr)?;
                                    if let ExecutionOutcome::RequiresHost(_) = last_result {
                                        return Ok(last_result);
                                    }
                                }
                                crate::ast::ModuleLevelDefinition::Import(import_def) => {
                                    if self.config.verbose {
                                        println!("Import statement: {:?}", import_def.module_name);
                                    }
                                    
                                    // Resolve and bind the import
                                    if let Err(e) = self.resolve_and_bind_import(&mut evaluator, import_def) {
                                        if self.config.verbose {
                                            println!("Import resolution failed: {}", e);
                                        }
                                        return Err(e);
                                    }
                                }
                            }
                        }
                    }
                    _ => {
                        // For other top-level items, we could extend this to handle them
                        if self.config.verbose {
                            println!("Skipping non-expression top-level item: {:?}", item);
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
    pub fn execute_file(&self, file_path: &str) -> RuntimeResult<ExecutionOutcome> {
        let code = std::fs::read_to_string(file_path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to read file '{}': {}", file_path, e))
        })?;

        if self.config.verbose {
            println!("📖 Executing file: {}", file_path);
            println!("📊 File size: {} bytes", code.len());
        }

        self.execute_code(&code)
    }

    /// Execute a capability by id with RTFS Value args (helper wrapper around the host)
    pub fn execute_capability(&self, capability_id: &str, args: &[Value]) -> RuntimeResult<Value> {
        self.host.execute_capability(capability_id, args)
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
            self.registry
                .list_capabilities()
                .into_iter()
                .map(|s| s.to_string()),
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
        stats.insert(
            "security_level".to_string(),
            Value::String(format!("{:?}", self.config.security_level)),
        );
        stats.insert(
            "enabled_categories".to_string(),
            Value::Vector(
                self.config
                    .enabled_categories
                    .iter()
                    .map(|c| Value::String(format!("{:?}", c)))
                    .collect(),
            ),
        );
        stats.insert(
            "available_capabilities".to_string(),
            Value::Integer(self.list_capabilities().len() as i64),
        );
        stats
    }

    /// Returns the Working Memory instance if WM ingestor is enabled.
    pub fn working_memory(&self) -> Option<Arc<Mutex<WorkingMemory>>> {
        self.wm.clone()
    }

    /// Rebuild (replay) Working Memory from the current Causal Chain history.
    /// This is idempotent; existing entries derived from the same actions will be overwritten identically.
    pub fn rebuild_working_memory_from_chain(&self) -> RuntimeResult<()> {
        let wm_arc = if let Some(wm) = &self.wm {
            wm.clone()
        } else {
            return Ok(());
        };
        // Snapshot actions via host
        let actions: Vec<crate::ccos::types::Action> = self.host.snapshot_actions()?;
        let mut records = Vec::with_capacity(actions.len());
        for a in &actions {
            // Minimal mapping mirroring WorkingMemorySink
            let summary = a
                .function_name
                .clone()
                .unwrap_or_else(|| format!("{:?}", a.action_type));
            let mut content = format!(
                "type={:?}; plan={}; intent={}; ts={}",
                a.action_type, a.plan_id, a.intent_id, a.timestamp
            );
            if let Some(fn_name) = &a.function_name {
                content.push_str(&format!("; fn={}", fn_name));
            }
            if let Some(args) = &a.arguments {
                content.push_str(&format!("; args={}", args.len()));
            }
            if let Some(cost) = a.cost {
                content.push_str(&format!("; cost={}", cost));
            }
            if let Some(d) = a.duration_ms {
                content.push_str(&format!("; dur_ms={}", d));
            }
            let att = a.metadata.get("signature").and_then(|v| match v {
                crate::runtime::values::Value::String(s) => Some(s.clone()),
                _ => None,
            });
            records.push(crate::ccos::working_memory::ingestor::ActionRecord {
                action_id: a.action_id.clone(),
                kind: format!("{:?}", a.action_type),
                provider: a.function_name.clone(),
                timestamp_s: a.timestamp,
                summary,
                content,
                plan_id: Some(a.plan_id.clone()),
                intent_id: Some(a.intent_id.clone()),
                step_id: None,
                attestation_hash: att,
                content_hash: None,
            });
        }
        let mut wm = wm_arc
            .lock()
            .map_err(|_| RuntimeError::Generic("Failed to lock WorkingMemory".to_string()))?;
        crate::ccos::working_memory::ingestor::MemoryIngestor::replay_all(&mut *wm, &records)
            .map_err(|e| RuntimeError::Generic(format!("WM replay failed: {:?}", e)))
    }

    /// Resolve and bind an import statement to the current environment
    fn resolve_and_bind_import(
        &self,
        evaluator: &mut std::sync::MutexGuard<Evaluator>,
        import_def: &crate::ast::ImportDefinition,
    ) -> RuntimeResult<()> {
        let module_name = &import_def.module_name.0;
        
        if self.config.verbose {
            println!("Resolving import: {}", module_name);
        }

        // Try to load the module using the ModuleRegistry
        let module = match evaluator.module_registry().get_module(module_name) {
            Some(module) => module,
            None => {
                // Module not loaded yet, try to load it from filesystem
                if let Ok(loaded_module) = self.load_module_from_file(module_name, evaluator) {
                    loaded_module
                } else {
                    return Err(RuntimeError::ModuleNotFound(format!(
                        "Module '{}' not found in registry or filesystem.",
                        module_name
                    )));
                }
            }
        };

        // Get the module's exports
        let exports = module.exports.read()
            .map_err(|e| RuntimeError::InternalError(format!("Failed to read module exports: {}", e)))?;

        // Handle different import options
        match (&import_def.alias, &import_def.only) {
            (Some(alias), None) => {
                // Import with alias: (import [module :as alias])
                // Create a namespace-like binding for the alias
                let alias_name = &alias.0;
                let module_value = crate::runtime::values::Value::Map(
                    exports.iter()
                        .map(|(name, export)| {
                            (crate::ast::MapKey::Keyword(crate::ast::Keyword(name.clone())), export.value.clone())
                        })
                        .collect()
                );
                evaluator.env.define(&crate::ast::Symbol(alias_name.clone()), module_value);
                
                if self.config.verbose {
                    println!("Imported module '{}' as '{}' with {} exports", module_name, alias_name, exports.len());
                }
            }
            (None, Some(only_symbols)) => {
                // Import specific symbols: (import [module :only [sym1 sym2]])
                for symbol_ast in only_symbols {
                    let symbol_name = &symbol_ast.0;
                    if let Some(export) = exports.get(symbol_name) {
                        evaluator.env.define(&crate::ast::Symbol(symbol_name.clone()), export.value.clone());
                        if self.config.verbose {
                            println!("Imported symbol '{}' from module '{}'", symbol_name, module_name);
                        }
                    } else {
                        return Err(RuntimeError::SymbolNotFound(format!(
                            "Symbol '{}' not found in module '{}'",
                            symbol_name, module_name
                        )));
                    }
                }
            }
            (None, None) => {
                // Import all symbols, qualified by the full module name
                for (export_name, export) in exports.iter() {
                    let qualified_name = format!("{}/{}", module_name, export_name);
                    evaluator.env.define(&crate::ast::Symbol(qualified_name.clone()), export.value.clone());
                    if self.config.verbose {
                        println!("Imported qualified symbol '{}' from module '{}'", qualified_name, module_name);
                    }
                }
            }
            (Some(_), Some(_)) => {
                return Err(RuntimeError::ModuleError(
                    "Invalid import specification: cannot combine :as with :only".to_string()
                ));
            }
        }

        Ok(())
    }

    /// Load a module from a file
    fn load_module_from_file(
        &self,
        module_name: &str,
        evaluator: &mut std::sync::MutexGuard<Evaluator>,
    ) -> RuntimeResult<Arc<crate::runtime::module_runtime::Module>> {
        if self.config.verbose {
            println!("Loading module '{}' from filesystem", module_name);
        }

        // Convert module name to file path
        // For now, we'll look in a simple test_modules directory
        let file_path = format!("test_modules/{}.rtfs", module_name);
        
        // Read the module file
        let source_content = std::fs::read_to_string(&file_path)
            .map_err(|e| RuntimeError::ModuleError(format!(
                "Failed to read module file '{}': {}",
                file_path, e
            )))?;

        if self.config.verbose {
            println!("Read module source from '{}'", file_path);
        }

        // Parse the module source
        let parsed = crate::parser::parse(&source_content)
            .map_err(|e| RuntimeError::ModuleError(format!(
                "Failed to parse module file '{}': {:?}",
                file_path, e
            )))?;

        // Find the module definition
        let module_def = parsed.into_iter()
            .find_map(|top_level| {
                if let crate::ast::TopLevel::Module(module_def) = top_level {
                    Some(module_def)
                } else {
                    None
                }
            })
            .ok_or_else(|| RuntimeError::ModuleError(format!(
                "No module definition found in file '{}'",
                file_path
            )))?;

        if self.config.verbose {
            println!("Found module definition: {:?}", module_def.name);
        }

        // Create a simple module with the definitions
        // For now, we'll create a basic module structure
        // In a full implementation, this would use the ModuleRegistry's compilation logic
        let mut module_env = crate::runtime::stdlib::StandardLibrary::create_global_environment();
        
        // Execute the module definitions to populate the environment
        for def in &module_def.definitions {
            match def {
                crate::ast::ModuleLevelDefinition::Def(def_expr) => {
                    let expr = crate::ast::Expression::Def(Box::new(def_expr.clone()));
                    let result = evaluator.evaluate(&expr)?;
                    if let crate::runtime::execution_outcome::ExecutionOutcome::Complete(value) = result {
                        module_env.define(&def_expr.symbol, value);
                    }
                }
                crate::ast::ModuleLevelDefinition::Defn(defn_expr) => {
                    let expr = crate::ast::Expression::Defn(Box::new(defn_expr.clone()));
                    let result = evaluator.evaluate(&expr)?;
                    if let crate::runtime::execution_outcome::ExecutionOutcome::Complete(crate::runtime::values::Value::Function(_)) = result {
                        // Manually define the function in the module environment
                        let function = crate::runtime::values::Value::Function(crate::runtime::values::Function::new_closure(
                            defn_expr.params.iter().map(|p| {
                                match &p.pattern {
                                    crate::ast::Pattern::Symbol(s) => s.clone(),
                                    _ => panic!("Expected symbol pattern in defn parameter"),
                                }
                            }).collect(),
                            defn_expr.params.iter().map(|p| p.pattern.clone()).collect(),
                            defn_expr.variadic_param.as_ref().map(|p| {
                                match &p.pattern {
                                    crate::ast::Pattern::Symbol(s) => s.clone(),
                                    _ => panic!("Expected symbol pattern in defn variadic parameter"),
                                }
                            }),
                            Box::new(crate::ast::Expression::Do(crate::ast::DoExpr {
                                expressions: defn_expr.body.clone(),
                            })),
                            Arc::new(module_env.clone()),
                            defn_expr.delegation_hint.clone(),
                        ));
                        module_env.define(&defn_expr.name, function);
                    }
                }
                crate::ast::ModuleLevelDefinition::Import(_) => {
                    // Skip imports for now - they would be resolved recursively
                    if self.config.verbose {
                        println!("Skipping import in module '{}'", module_name);
                    }
                }
            }
        }

        // Create module exports from the environment
        let mut exports = std::collections::HashMap::new();
        for symbol_name in module_env.symbol_names() {
            if let Some(value) = module_env.lookup(&crate::ast::Symbol(symbol_name.clone())) {
                let export = crate::runtime::module_runtime::ModuleExport {
                    original_name: symbol_name.clone(),
                    export_name: symbol_name.clone(),
                    value: value.clone(),
                    ir_type: crate::ir::core::IrType::Any, // Simplified for now
                    export_type: match value {
                        crate::runtime::values::Value::Function(_) => crate::runtime::module_runtime::ExportType::Function,
                        _ => crate::runtime::module_runtime::ExportType::Variable,
                    },
                };
                exports.insert(symbol_name.clone(), export);
            }
        }

        if self.config.verbose {
            println!("Created module '{}' with {} exports: {:?}", 
                module_name, exports.len(), exports.keys().collect::<Vec<_>>());
        }

        // Create the module
        let module = crate::runtime::module_runtime::Module {
            metadata: crate::runtime::module_runtime::ModuleMetadata {
                name: module_name.to_string(),
                docstring: Some(format!("Module loaded from {}", file_path)),
                source_file: Some(file_path.into()),
                version: None,
                compiled_at: std::time::SystemTime::now(),
            },
            ir_node: crate::ir::core::IrNode::Module {
                id: 0, // Simplified
                name: module_name.to_string(),
                exports: exports.keys().cloned().collect(),
                definitions: vec![], // Simplified
                source_location: None,
            },
            exports: std::sync::RwLock::new(exports),
            namespace: Arc::new(std::sync::RwLock::new(crate::runtime::IrEnvironment::new())),
            dependencies: vec![],
        };

        // Register the module in the registry
        evaluator.module_registry().register_module(module.clone())
            .map_err(|e| RuntimeError::ModuleError(format!(
                "Failed to register module '{}': {:?}",
                module_name, e
            )))?;

        Ok(Arc::new(module))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ccos::working_memory::backend::QueryParams;
    use crate::runtime::host_interface::HostInterface;

    #[test]
    fn test_observability_ingestor_single() {
        std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");
        let env = CCOSBuilder::new().build().expect("env");
        let wm_arc = env.working_memory().expect("wm enabled");

        // Build a single record
        let mut rec_map = std::collections::HashMap::new();
        rec_map.insert(
            MapKey::String("action_id".into()),
            Value::String("t-1".into()),
        );
        rec_map.insert(
            MapKey::String("kind".into()),
            Value::String("CapabilityCall".into()),
        );
        rec_map.insert(MapKey::String("timestamp_s".into()), Value::Integer(123));
        rec_map.insert(
            MapKey::String("summary".into()),
            Value::String("demo".into()),
        );
        rec_map.insert(
            MapKey::String("content".into()),
            Value::String("hello".into()),
        );

        let args = vec![Value::String("single".into()), Value::Map(rec_map)];

        // Call capability via host
        let out = env
            .host
            .execute_capability("observability.ingestor:v1.ingest", &args)
            .expect("cap ok");
        if let Value::Map(m) = out {
            assert_eq!(
                m.get(&MapKey::String("ingested".into())),
                Some(&Value::Integer(1))
            );
        } else {
            panic!("unexpected output");
        }

        // Check WM has at least one entry
        let guard = wm_arc.lock().unwrap();
        let res = guard.query(&QueryParams::default()).unwrap();
        assert!(!res.entries.is_empty());
    }

    #[test]
    fn test_observability_ingestor_batch() {
        std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");
        let env = CCOSBuilder::new().build().expect("env");
        let wm_arc = env.working_memory().expect("wm enabled");

        // Build two records
        let mut rec1 = std::collections::HashMap::new();
        rec1.insert(
            MapKey::String("action_id".into()),
            Value::String("b-1".into()),
        );
        rec1.insert(MapKey::String("summary".into()), Value::String("s1".into()));
        rec1.insert(MapKey::String("content".into()), Value::String("c1".into()));

        let mut rec2 = std::collections::HashMap::new();
        rec2.insert(
            MapKey::String("action_id".into()),
            Value::String("b-2".into()),
        );
        rec2.insert(MapKey::String("summary".into()), Value::String("s2".into()));
        rec2.insert(MapKey::String("content".into()), Value::String("c2".into()));

        let args = vec![
            Value::String("batch".into()),
            Value::List(vec![Value::Map(rec1), Value::Map(rec2)]),
        ];

        let out = env
            .host
            .execute_capability("observability.ingestor:v1.ingest", &args)
            .expect("cap ok");
        if let Value::Map(m) = out {
            assert_eq!(
                m.get(&MapKey::String("ingested".into())),
                Some(&Value::Integer(2))
            );
        } else {
            panic!("unexpected output");
        }

        let guard = wm_arc.lock().unwrap();
        let res = guard.query(&QueryParams::default()).unwrap();
        assert!(res.entries.len() >= 2);
    }

    #[test]
    fn test_observability_ingestor_replay() {
        std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");
        let env = CCOSBuilder::new().build().expect("env");

        // First ingest a single entry so that there is something to replay alongside any prior chain state
        let mut rec_map = std::collections::HashMap::new();
        rec_map.insert(
            MapKey::String("action_id".into()),
            Value::String("r-1".into()),
        );
        rec_map.insert(
            MapKey::String("summary".into()),
            Value::String("hello".into()),
        );
        rec_map.insert(
            MapKey::String("content".into()),
            Value::String("payload".into()),
        );
        let args_single = vec![Value::String("single".into()), Value::Map(rec_map)];
        let _ = env
            .host
            .execute_capability("observability.ingestor:v1.ingest", &args_single)
            .expect("cap ok");

        // Now run replay mode
        let args_replay = vec![Value::String("replay".into())];
        let out = env
            .host
            .execute_capability("observability.ingestor:v1.ingest", &args_replay)
            .expect("cap ok");
        if let Value::Map(m) = out {
            // replay returns both ingested and scanned_actions
            assert!(
                matches!(m.get(&MapKey::String("ingested".into())), Some(Value::Integer(x)) if *x >= 0)
            );
            assert!(
                matches!(m.get(&MapKey::String("scanned_actions".into())), Some(Value::Integer(x)) if *x >= 0)
            );
        } else {
            panic!("unexpected output");
        }
    }

    #[test]
    fn test_observability_ingestor_metrics_increment() {
        std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");
        let env = CCOSBuilder::new().build().expect("env");

        // Before: metrics may be None or zero for this capability
        let before = env
            .host
            .get_capability_metrics("observability.ingestor:v1.ingest");
        let before_calls = before.as_ref().map(|m| m.total_calls).unwrap_or(0);

        // Call capability once
        let mut rec_map = std::collections::HashMap::new();
        rec_map.insert(
            MapKey::String("action_id".into()),
            Value::String("m-1".into()),
        );
        rec_map.insert(
            MapKey::String("summary".into()),
            Value::String("metrics".into()),
        );
        rec_map.insert(
            MapKey::String("content".into()),
            Value::String("payload".into()),
        );
        let args = vec![Value::String("single".into()), Value::Map(rec_map)];
        let _ = env
            .host
            .execute_capability("observability.ingestor:v1.ingest", &args)
            .expect("cap ok");

        // After: total_calls should have incremented by at least 1
        let after = env
            .host
            .get_capability_metrics("observability.ingestor:v1.ingest")
            .expect("metrics exist after call");
        assert!(after.total_calls >= before_calls + 1);
    }

    #[test]
    fn test_chain_recent_logs_and_function_metrics() {
        std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");
        let env = CCOSBuilder::new().build().expect("env");

        // Execute a capability to generate a CapabilityCall and a result record
        let mut rec_map = std::collections::HashMap::new();
        rec_map.insert(
            MapKey::String("action_id".into()),
            Value::String("l-1".into()),
        );
        rec_map.insert(
            MapKey::String("summary".into()),
            Value::String("logs".into()),
        );
        rec_map.insert(
            MapKey::String("content".into()),
            Value::String("payload".into()),
        );
        let args = vec![Value::String("single".into()), Value::Map(rec_map)];
        let _ = env
            .host
            .execute_capability("observability.ingestor:v1.ingest", &args)
            .expect("cap ok");

        // Record a delegation event via host helper
        let mut meta = std::collections::HashMap::new();
        meta.insert(
            "delegation.selected_agent".to_string(),
            Value::String("agent.alpha".to_string()),
        );
        env.host
            .record_delegation_event_for_test("auto-intent", "approved", meta)
            .expect("deleg event ok");

        // Fetch recent logs and assert they contain entries for both events
        let logs = env.host.get_recent_logs(32);
        // From host path we expect action_appended and action_result_recorded
        assert!(
            logs.iter()
                .any(|l| l.contains("action_appended")
                    && l.contains("observability.ingestor:v1.ingest"))
                || logs.iter().any(|l| l.contains("action_result_recorded")
                    && l.contains("observability.ingestor:v1.ingest"))
        );
        assert!(logs.iter().any(|l| l.contains("delegation_event")));

        // Function metrics should exist for the capability id
        let fm = env
            .host
            .get_function_metrics("observability.ingestor:v1.ingest")
            .expect("function metrics");
        assert!(fm.total_calls >= 1);
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
        self.config
            .custom_rules
            .insert(capability_id.to_string(), true);
        self
    }

    /// Deny specific capability
    pub fn deny_capability(mut self, capability_id: &str) -> Self {
        self.config
            .custom_rules
            .insert(capability_id.to_string(), false);
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
