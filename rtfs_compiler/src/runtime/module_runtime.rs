// Module Runtime - Comprehensive module system for RTFS
// Handles module loading, dependency resolution, namespacing, and import/export mechanisms

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use crate::ir::converter::{BindingInfo, BindingKind, IrConverter};
use crate::ir::core::{IrNode, IrType};
use crate::runtime::error::RuntimeResult;
use crate::runtime::{IrEnvironment, IrRuntime, RuntimeError, Value};
use sha2::{Sha256, Digest};
use crate::ccos::caching::l4_content_addressable::L4CacheClient;

/// Module registry that manages all loaded modules
#[derive(Debug)]
pub struct ModuleRegistry {
    /// Map from module name to compiled module
    modules: RwLock<HashMap<String, Arc<Module>>>,

    /// Map from module name to module namespace environment
    module_environments: RwLock<HashMap<String, Arc<RwLock<IrEnvironment>>>>,

    /// Module loading paths
    module_paths: Vec<PathBuf>,

    /// Currently loading modules (for circular dependency detection)
    loading_stack: RwLock<Vec<String>>,

    /// Optional L4 cache client for content-addressable bytecode reuse
    l4_cache: Option<Arc<L4CacheClient>>,

    /// Optional bytecode backend for compiling modules
    bytecode_backend: Option<Arc<dyn crate::bytecode::BytecodeBackend>>,
}

/// A compiled module with its metadata and runtime environment
#[derive(Debug)]
pub struct Module {
    /// Module metadata
    pub metadata: ModuleMetadata,

    /// Module's IR representation
    pub ir_node: IrNode,

    /// Module's exported symbols
    pub exports: RwLock<HashMap<String, ModuleExport>>,

    /// Module's private namespace
    pub namespace: Arc<RwLock<IrEnvironment>>,

    /// Module dependencies
    pub dependencies: Vec<String>,
}

impl Clone for Module {
    fn clone(&self) -> Self {
        // Clone exports by reading the lock and cloning the inner map
        let exports_cloned = match self.exports.read() {
            Ok(g) => g.clone(),
            Err(_) => HashMap::new(),
        };
        Module {
            metadata: self.metadata.clone(),
            ir_node: self.ir_node.clone(),
            exports: RwLock::new(exports_cloned),
            namespace: self.namespace.clone(),
            dependencies: self.dependencies.clone(),
        }
    }
}

/// Module metadata
#[derive(Debug, Clone)]
pub struct ModuleMetadata {
    /// Module name (e.g., "my.company/data/utils")
    pub name: String,

    /// Module documentation
    pub docstring: Option<String>,

    /// Source file path
    pub source_file: Option<PathBuf>,

    /// Module version
    pub version: Option<String>,

    /// Compilation timestamp
    pub compiled_at: std::time::SystemTime,
}

/// Exported symbol from a module
#[derive(Debug, Clone)]
pub struct ModuleExport {
    /// Original name in the module
    pub original_name: String,

    /// Exported name (may differ from original)
    pub export_name: String,

    /// Value being exported
    pub value: Value,

    /// Type of the exported value
    pub ir_type: IrType,

    /// Whether this is a function, variable, etc.
    pub export_type: ExportType,
}

/// Type of module export
#[derive(Debug, Clone, PartialEq)]
pub enum ExportType {
    Function,
    Variable,
    Type,
    Macro,
}

/// Import specification for module loading
#[derive(Debug, Clone)]
pub struct ImportSpec {
    /// Module name to import from
    pub module_name: String,

    /// Local alias for the module (e.g., "utils" for "my.company/utils")
    pub alias: Option<String>,

    /// Specific symbols to import (None = import all)
    pub symbols: Option<Vec<SymbolImport>>,

    /// Whether to import all symbols into current namespace
    pub refer_all: bool,
}

/// Individual symbol import specification
#[derive(Debug, Clone)]
pub struct SymbolImport {
    /// Original name in the exporting module
    pub original_name: String,

    /// Local name in the importing module
    pub local_name: Option<String>,
}

impl ModuleRegistry {
    /// Create a new module registry
    pub fn new() -> Self {
        ModuleRegistry {
            modules: RwLock::new(HashMap::new()),
            module_environments: RwLock::new(HashMap::new()),
            module_paths: vec![PathBuf::from(".")],
            loading_stack: RwLock::new(Vec::new()),
            l4_cache: None,
            bytecode_backend: None,
        }
    }

    /// Attach an L4 cache client; returns self for chaining
    pub fn with_l4_cache(mut self, cache: Arc<L4CacheClient>) -> Self {
        self.l4_cache = Some(cache);
        self
    }

    /// Accessor for the optional L4 cache
    pub fn l4_cache(&self) -> Option<&Arc<L4CacheClient>> {
        self.l4_cache.as_ref()
    }

    /// Attach a bytecode backend; returns self for chaining
    pub fn with_bytecode_backend(mut self, backend: Arc<dyn crate::bytecode::BytecodeBackend>) -> Self {
        self.bytecode_backend = Some(backend);
        self
    }

    /// Accessor for the optional bytecode backend
    pub fn bytecode_backend(&self) -> Option<&Arc<dyn crate::bytecode::BytecodeBackend>> {
        self.bytecode_backend.as_ref()
    }

    /// Add a module search path
    pub fn add_module_path(&mut self, path: PathBuf) {
        if !self.module_paths.contains(&path) {
            self.module_paths.push(path);
        }
    }
    /// Register a compiled module
    pub fn register_module(&self, module: Module) -> RuntimeResult<()> {
        let module_name = module.metadata.name.clone();

        // Store the module environment
        self.module_environments
            .write()
            .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?
            .insert(module_name.clone(), module.namespace.clone());

        // Register the module
        self.modules
            .write()
            .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?
            .insert(module_name, Arc::new(module));

        Ok(())
    }
    /// Load and compile a module
    pub fn load_module(
        &mut self,
        module_name: &str,
        ir_runtime: &mut IrRuntime,
    ) -> RuntimeResult<Arc<Module>> {
        // If already loaded, return it.
        if let Some(module) = self.modules.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.get(module_name) {
            return Ok(module.clone());
        }

        // Check for circular dependency.
        if self
            .loading_stack
            .read()
            .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?
            .contains(&module_name.to_string())
        {
            // It's a cycle. Create and register a temporary placeholder module to break the cycle.
            let placeholder_metadata = ModuleMetadata {
                name: module_name.to_string(),
                docstring: Some("placeholder for circular dependency".to_string()),
                source_file: None,
                version: None,
                compiled_at: std::time::SystemTime::now(),
            };
            let placeholder_module = Arc::new(Module {
                metadata: placeholder_metadata,
                ir_node: IrNode::Do {
                    id: 0,
                    ir_type: IrType::Any,
                    expressions: vec![],
                    source_location: None,
                },
                exports: RwLock::new(HashMap::new()),
                namespace: Arc::new(RwLock::new(IrEnvironment::new())),
                dependencies: Vec::new(),
            });

            // Register the placeholder to allow dependent modules to compile.
            self.modules
                .write()
                .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?
                .insert(module_name.to_string(), placeholder_module.clone());

            return Ok(placeholder_module);
        }

        // Push module onto loading stack
        {
            let mut guard = self.loading_stack.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
            guard.push(module_name.to_string());
        }

        // Compile the module from source, getting back the module structure and the bindings map.
        let (compiled_module, bindings) = match self.load_module_from_file(module_name, ir_runtime) {
            Ok(result) => result,
            Err(e) => {
                // Pop loading stack on error
                let _ = { let mut guard = self.loading_stack.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?; guard.pop(); };
                return Err(e);
            }
        };

        // Execute the module's IR to populate its namespace.
        {
            let mut ns_guard = compiled_module.namespace.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
            match ir_runtime.execute_node(&compiled_module.ir_node, &mut *ns_guard, false, self) {
                Ok(_) => {}
                Err(e) => {
                    let _ = { let mut guard = self.loading_stack.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?; guard.pop(); };
                    return Err(e);
                }
            }
        }

        // After execution, populate the exports using the bindings map and the populated environment.
        if let IrNode::Module { exports: export_names, .. } = &compiled_module.ir_node {
            let mut exports_map = compiled_module.exports.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
            let module_env_borrow = compiled_module.namespace.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;

            // DEBUG: Print bindings and environment keys before export population
            #[cfg(debug_assertions)]
            {
                println!("[DEBUG] Export population for module: {}", module_name);
                println!("[DEBUG] Export names: {:?}", export_names);
                println!("[DEBUG] Bindings: {{");
                for (k, v) in &bindings {
                    println!("  {} => id {}", k, v.binding_id);
                }
                println!("}}");
                println!("[DEBUG] Environment: {:?}", *module_env_borrow);
            }

            for export_name in export_names {
                if let Some(binding_info) = bindings.get(export_name) {
                    if let Some(value) = module_env_borrow.get(export_name) {
                        let export = ModuleExport {
                            original_name: export_name.to_string(),
                            export_name: export_name.to_string(),
                            value: value.clone(),
                            ir_type: binding_info.ir_type.clone(),
                            export_type: match value { Value::Function(_) => ExportType::Function, _ => ExportType::Variable },
                        };
                        exports_map.insert(export_name.to_string(), export);
                    } else {
                        let _ = { let mut guard = self.loading_stack.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?; guard.pop(); };
                        return Err(RuntimeError::ModuleError(format!(
                            "Exported symbol '{}' not found in module '{}' environment after execution.",
                            export_name, module_name
                        )));
                    }
                }
            }
        }

        // Pop the loading stack after successful compilation
        {
            let mut guard = self.loading_stack.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
            let _ = guard.pop();
        }

        // Register the definitive, fully-loaded module. This will overwrite any placeholders.
        self.modules
            .write()
            .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?
            .insert(module_name.to_string(), compiled_module.clone());
        self.module_environments
            .write()
            .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?
            .insert(module_name.to_string(), compiled_module.namespace.clone());

        // ----- L4 cache publishing prototype -----
        if let (Some(cache), Some(backend)) = (&self.l4_cache, &self.bytecode_backend) {
            use crate::ccos::caching::l4_content_addressable::RtfsModuleMetadata;
            // Compile IR to bytecode via backend
            let bytecode = backend.compile_module(&compiled_module.ir_node);

            // Interface hash = SHA256(sorted export names)::hex
            let mut export_names: Vec<String> = compiled_module.exports.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.keys().cloned().collect();
            export_names.sort();
            let joined = export_names.join("::");
            let mut hasher = Sha256::new();
            hasher.update(joined.as_bytes());
            let interface_hash = format!("{:x}", hasher.finalize());
            // Semantic embedding unavailable here; leave empty.
            let metadata = RtfsModuleMetadata::new(Vec::<f32>::new(), interface_hash, String::new());
            // Ignore errors in the prototype.
            let _ = cache.publish_module(bytecode, metadata);
        }

        Ok(compiled_module)
    }

    /// Load and compile a module from a source file
    fn load_module_from_file(
        &mut self,
        module_name: &str,
        ir_runtime: &mut IrRuntime,
    ) -> RuntimeResult<(Arc<Module>, HashMap<String, BindingInfo>)> {
        // Resolve module path from module name
        let module_path = self.resolve_module_path(module_name)?;

        // Read the source file
        let source_content = self.read_module_source(&module_path)?;

        // Parse the module source
        let parsed_ast = self.parse_module_source(&source_content, &module_path)?;

        // Convert module AST to IR and compile
        let compiled_result =
            self.compile_module_ast(module_name, parsed_ast, &module_path, ir_runtime)?;

        Ok(compiled_result)
    }
    /// Compile module AST to a CompiledModule
    fn compile_module_ast(
        &mut self,
        module_name: &str,
        module_def: crate::ast::ModuleDefinition,
        source_path: &std::path::Path,
        ir_runtime: &mut IrRuntime,
    ) -> RuntimeResult<(Arc<Module>, HashMap<String, BindingInfo>)> {
        use std::collections::HashMap;

        // Create module metadata
        let metadata = ModuleMetadata {
            name: module_name.to_string(),
            docstring: None, // Could extract from module comments
            source_file: Some(source_path.to_path_buf()),
            version: None, // Could extract from module metadata
            compiled_at: std::time::SystemTime::now(),
        };
        // Create module namespace environment with stdlib as parent, wrapped for interior mutability
    let stdlib_env = Arc::new(IrEnvironment::with_stdlib(self)?);
    let module_env = Arc::new(RwLock::new(IrEnvironment::with_parent(stdlib_env)));

        // Process module dependencies first
        let mut dependencies = Vec::new();
        let mut loaded_dependencies = HashMap::new();

        for definition in &module_def.definitions {
            if let crate::ast::ModuleLevelDefinition::Import(import_def) = definition {
                let dep_module_name = import_def.module_name.0.clone();

                if !loaded_dependencies.contains_key(&dep_module_name) {
                    let loaded_dep_module = self.load_module(&dep_module_name, ir_runtime)?;
                    loaded_dependencies.insert(dep_module_name.clone(), loaded_dep_module);
                }
                if !dependencies.contains(&dep_module_name) {
                    dependencies.push(dep_module_name);
                }
            }
        }

        let mut ir_converter = IrConverter::with_module_registry(self);

        for definition in &module_def.definitions {
            if let crate::ast::ModuleLevelDefinition::Import(import_def) = definition {
                let dep_module_name = &import_def.module_name.0;
                let loaded_dep_module = loaded_dependencies.get(dep_module_name).unwrap();

                // Import symbols into the ir_converter's scope
                match (&import_def.alias, &import_def.only) {
                    (Some(alias), None) => {
                        // Import with alias: (import [module :as alias])
                        for (export_name, export) in loaded_dep_module.exports.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.iter() {
                            let qualified_name = format!("{}/{}", alias.0, export_name);
                            let binding_id = ir_converter.next_id();
                            let binding_kind = match export.export_type {
                                ExportType::Function => BindingKind::Function,
                                ExportType::Variable => BindingKind::Variable,
                                ExportType::Type => BindingKind::Variable,
                                ExportType::Macro => BindingKind::Variable,
                            };
                            let binding_info = BindingInfo {
                                name: qualified_name.clone(),
                                binding_id,
                                ir_type: export.ir_type.clone(),
                                kind: binding_kind,
                            };
                            ir_converter.define_binding(qualified_name, binding_info);
                        }
                    }
                    (None, Some(only_symbols)) => {
                        // Import specific symbols: (import [module :only [sym1 sym2]])
                        for symbol_ast in only_symbols {
                            let export_name = &symbol_ast.0;
                            if let Some(export) =
                                loaded_dep_module.exports.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.get(export_name)
                            {
                                let binding_id = ir_converter.next_id();
                                let binding_kind = match export.export_type {
                                    ExportType::Function => BindingKind::Function,
                                    ExportType::Variable => BindingKind::Variable,
                                    ExportType::Type => BindingKind::Variable,
                                    ExportType::Macro => BindingKind::Variable,
                                };
                                let binding_info = BindingInfo {
                                    name: export_name.clone(),
                                    binding_id,
                                    ir_type: export.ir_type.clone(),
                                    kind: binding_kind,
                                };
                                ir_converter.define_binding(export_name.clone(), binding_info);
                            } else {
                                return Err(RuntimeError::ModuleError(format!(
                                    "Symbol '{}' not exported by module '{}'",
                                    export_name, dep_module_name
                                )));
                            }
                        }
                    }
                    (None, None) => {
                        // Import all symbols, qualified by the full module name
                        for (export_name, export) in loaded_dep_module.exports.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.iter() {
                            let qualified_name = format!("{}/{}", dep_module_name, export_name);
                            let binding_id = ir_converter.next_id();
                            let binding_kind = match export.export_type {
                                ExportType::Function => BindingKind::Function,
                                ExportType::Variable => BindingKind::Variable,
                                ExportType::Type => BindingKind::Variable,
                                ExportType::Macro => BindingKind::Variable,
                            };
                            let binding_info = BindingInfo {
                                name: qualified_name.clone(),
                                binding_id,
                                ir_type: export.ir_type.clone(),
                                kind: binding_kind,
                            };
                            ir_converter.define_binding(qualified_name, binding_info);
                        }
                    }
                    (Some(_), Some(_)) => {
                        return Err(RuntimeError::ModuleError(
                            "Invalid import specification: cannot combine :as with :only"
                                .to_string(),
                        ));
                    }
                }
            }
        }

        // Convert module definitions to IR
        let mut ir_definitions = Vec::new();

        for definition in &module_def.definitions {
            match definition {
                crate::ast::ModuleLevelDefinition::Import(_) => {
                    // Already processed above
                    continue;
                }
                crate::ast::ModuleLevelDefinition::Def(def_expr) => {
                    // Convert def expression to Expression and then to IR
                    let expr = crate::ast::Expression::Def(Box::new(def_expr.clone()));
                    let ir_node = ir_converter.convert_expression(expr).map_err(|e| {
                        RuntimeError::ModuleError(format!("IR conversion failed: {:?}", e))
                    })?;
                    ir_definitions.push(ir_node);
                }
                crate::ast::ModuleLevelDefinition::Defn(defn_expr) => {
                    // Convert defn expression to Expression and then to IR
                    let expr = crate::ast::Expression::Defn(Box::new(defn_expr.clone()));
                    let ir_node = ir_converter.convert_expression(expr).map_err(|e| {
                        RuntimeError::ModuleError(format!("IR conversion failed: {:?}", e))
                    })?;
                    ir_definitions.push(ir_node);
                }
            }
        }

        // Get the list of exported symbol names from the AST
        let export_names = match &module_def.exports {
            Some(symbols) => symbols.iter().map(|s| s.0.clone()).collect(),
            None => Vec::new(),
        };

        let module_ir_node = IrNode::Module {
            id: ir_converter.next_id(),
            name: module_name.to_string(),
            exports: export_names,
            definitions: ir_definitions,
            source_location: None,
        };

        let mut bindings = ir_converter.into_bindings();

        // Overwrite/ensure bindings for top-level definitions are correct.
        // This is crucial for the export mechanism to find the right values
        // in the environment after execution.
        if let IrNode::Module { definitions, .. } = &module_ir_node {
            for def in definitions {
                match def {
                    IrNode::FunctionDef {
                        id, name, ir_type, ..
                    } => {
                        let binding_info = BindingInfo {
                            name: name.clone(),
                            binding_id: *id,
                            ir_type: ir_type.clone(),
                            kind: BindingKind::Function,
                        };
                        bindings.insert(name.clone(), binding_info);
                    }
                    IrNode::VariableDef {
                        id, name, ir_type, ..
                    } => {
                        let binding_info = BindingInfo {
                            name: name.clone(),
                            binding_id: *id,
                            ir_type: ir_type.clone(),
                            kind: BindingKind::Variable,
                        };
                        bindings.insert(name.clone(), binding_info);
                    }
                    _ => {}
                }
            }
        }

        let compiled_module = Module {
            metadata,
            ir_node: module_ir_node,
            exports: RwLock::new(HashMap::new()),
            namespace: module_env,
            dependencies,
        };

        Ok((Arc::new(compiled_module), bindings))
    }

    pub fn get_module(&self, module_name: &str) -> Option<Arc<Module>> {
        self.modules.read().map_err(|_e| ()).ok().and_then(|m| m.get(module_name).cloned())
    }

    pub fn loaded_modules(&self) -> std::collections::HashMap<String, Arc<Module>> {
        match self.modules.read() {
            Ok(guard) => guard.clone(),
            Err(_) => HashMap::new(),
        }
    }
    pub fn is_qualified_symbol(name: &str) -> bool {
        if let Some(slash_pos) = name.find('/') {
            // Must have non-empty module name and non-empty symbol name
            slash_pos > 0 && slash_pos < name.len() - 1
        } else {
            false
        }
    }

    pub fn resolve_qualified_symbol(&mut self, qualified_name: &str) -> RuntimeResult<Value> {
        let parts: Vec<&str> = qualified_name.splitn(2, '/').collect();
        if parts.len() != 2 {
            return Err(RuntimeError::SymbolNotFound(format!(
                "Invalid qualified symbol format: {}",
                qualified_name
            )));
        }
        let module_name = parts[0];
        let symbol_name = parts[1];

        if let Some(module) = self.get_module(module_name) {
            if let Some(export) = module.exports.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.get(symbol_name) {
                Ok(export.value.clone())
            } else {
                Err(RuntimeError::SymbolNotFound(format!(
                    "Symbol '{}' not found in module '{}'",
                    symbol_name, module_name
                )))
            }
        } else {
            // Before failing, try to load the module
            // Create a temporary runtime with default host and security context
            let capability_registry = Arc::new(tokio::sync::RwLock::new(crate::ccos::capabilities::registry::CapabilityRegistry::new()));
            let capability_marketplace = Arc::new(crate::ccos::capability_marketplace::CapabilityMarketplace::new(capability_registry.clone()));
            let causal_chain = Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().expect("Failed to create causal chain")));
            let security_context = crate::runtime::security::RuntimeContext::pure();
            let host: Arc<dyn crate::runtime::host_interface::HostInterface> = Arc::new(crate::ccos::host::RuntimeHost::new(causal_chain.clone(), capability_marketplace.clone(), security_context.clone()));
            let mut ir_runtime = IrRuntime::new(host, security_context); // Temporary runtime
            match self.load_module(module_name, &mut ir_runtime) {
                Ok(module) => {
                    if let Some(export) = module.exports.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.get(symbol_name) {
                        Ok(export.value.clone())
                    } else {
                        Err(RuntimeError::SymbolNotFound(format!(
                            "Symbol '{}' not found in newly loaded module '{}'",
                            symbol_name, module_name
                        )))
                    }
                }
                Err(_) => Err(RuntimeError::ModuleNotFound(module_name.to_string())),
            }
        }
    }

    pub fn import_symbols(
        &mut self,
        import_spec: &ImportSpec,
        _env: &mut IrEnvironment,
        ir_runtime: &mut IrRuntime,
    ) -> RuntimeResult<()> {
        self.load_module(&import_spec.module_name, ir_runtime)?;
        Ok(())
    }

    /// Resolve module name to file path
    fn resolve_module_path(&self, module_name: &str) -> RuntimeResult<PathBuf> {
        // Convert module name like "math.utils" to file path like "math/utils.rtfs"
        let file_name = module_name.replace('.', "/") + ".rtfs";

        for search_path in &self.module_paths {
            let full_path = search_path.join(&file_name);
            if full_path.exists() {
                return Ok(full_path);
            }
        }

        Err(RuntimeError::ModuleNotFound(format!(
            "Module '{}' not found in module paths: {:?}",
            module_name, self.module_paths
        )))
    }

    /// Read module source from file
    fn read_module_source(&self, module_path: &PathBuf) -> RuntimeResult<String> {
        std::fs::read_to_string(module_path).map_err(|e| {
            RuntimeError::ModuleError(format!(
                "Failed to read module file '{}': {}",
                module_path.display(),
                e
            ))
        })
    }

    /// Parse module source into AST
    fn parse_module_source(
        &self,
        source_content: &str,
        module_path: &PathBuf,
    ) -> RuntimeResult<crate::ast::ModuleDefinition> {
        // Parse the source content using the existing parser
        let parsed = crate::parser::parse(source_content).map_err(|e| {
            RuntimeError::ModuleError(format!(
                "Failed to parse module file '{}': {:?}",
                module_path.display(),
                e
            ))
        })?;

        // Find the module definition in the parsed AST
        for top_level in parsed {
            if let crate::ast::TopLevel::Module(module_def) = top_level {
                return Ok(module_def);
            }
        }

        Err(RuntimeError::ModuleError(format!(
            "No module definition found in file '{}'",
            module_path.display()
        )))
    }
}

impl Clone for ModuleRegistry {
    fn clone(&self) -> Self {
        let modules_map = match self.modules.read() {
            Ok(g) => g.clone(),
            Err(_) => HashMap::new(),
        };
        let module_envs = match self.module_environments.read() {
            Ok(g) => g.clone(),
            Err(_) => HashMap::new(),
        };
        let loading = match self.loading_stack.read() {
            Ok(g) => g.clone(),
            Err(_) => Vec::new(),
        };

        ModuleRegistry {
            modules: RwLock::new(modules_map),
            module_environments: RwLock::new(module_envs),
            module_paths: self.module_paths.clone(),
            loading_stack: RwLock::new(loading),
            l4_cache: self.l4_cache.clone(),
            bytecode_backend: self.bytecode_backend.clone(),
        }
    }
}

/// Module-aware runtime that extends IrRuntime
pub struct ModuleAwareRuntime {
    /// Core IR runtime
    pub ir_runtime: IrRuntime,

    /// Module registry
    pub module_registry: ModuleRegistry,
}

impl ModuleAwareRuntime {
    /// Create a new module-aware runtime
    pub fn new() -> Self {
        let module_registry = ModuleRegistry::new();
        ModuleAwareRuntime {
            ir_runtime: {
                // Create a default runtime with minimal host
                let capability_registry = Arc::new(tokio::sync::RwLock::new(crate::ccos::capabilities::registry::CapabilityRegistry::new()));
                let capability_marketplace = Arc::new(crate::ccos::capability_marketplace::CapabilityMarketplace::new(capability_registry.clone()));
                let causal_chain = Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().expect("Failed to create causal chain")));
                let security_context = crate::runtime::security::RuntimeContext::pure();
                let host: Arc<dyn crate::runtime::host_interface::HostInterface> = Arc::new(crate::ccos::host::RuntimeHost::new(causal_chain.clone(), capability_marketplace.clone(), security_context.clone()));
                IrRuntime::new(host, security_context)
            },
            module_registry,
        }
    }

    /// Execute a module-aware program
    pub fn execute_program(&mut self, program: &IrNode) -> RuntimeResult<Value> {
        // Pre-process the program to handle modules
        match program {
            IrNode::Program { forms, .. } => {
                let mut last_value = Value::Nil;

                for form in forms {
                    last_value = self.execute_top_level_form(form)?;
                }

                Ok(last_value)
            }
            _ => {
                // IrRuntime now returns ExecutionOutcome; unwrap Complete or map RequiresHost to error
                match self.ir_runtime.execute_program(program, &mut self.module_registry) {
                    Ok(super::execution_outcome::ExecutionOutcome::Complete(v)) => Ok(v),
                    Ok(super::execution_outcome::ExecutionOutcome::RequiresHost(_host_call)) => Err(RuntimeError::Generic("Host call required during module-level program execution".to_string())),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Execute a top-level form (module, import, def, etc.)
    fn execute_top_level_form(&mut self, form: &IrNode) -> RuntimeResult<Value> {
        match form {
            IrNode::Module { .. } => self.execute_module_definition(form),
            IrNode::Import { .. } => self.execute_import(form),
            _ => {
                // Regular IR node execution
                let mut env = IrEnvironment::new();
                match self.ir_runtime.execute_node(form, &mut env, false, &mut self.module_registry) {
                    Ok(super::execution_outcome::ExecutionOutcome::Complete(v)) => Ok(v),
                    Ok(super::execution_outcome::ExecutionOutcome::RequiresHost(_)) => Err(RuntimeError::Generic("Host call required during module top-level form execution".to_string())),
                    Err(e) => Err(e),
                }
            }
        }
    }

    /// Execute a module definition
    fn execute_module_definition(&mut self, module_node: &IrNode) -> RuntimeResult<Value> {
        if let IrNode::Module {
            name,
            exports,
            definitions,
            ..
        } = module_node
        {
            // Create module environment
            let module_env: Arc<RwLock<IrEnvironment>> = Arc::new(RwLock::new(IrEnvironment::new()));
            let mut module_exports = HashMap::new();

            // First, execute all definitions and collect them in the environment
            for definition in definitions {
                match definition {
                    IrNode::Import {
                        module_name,
                        alias,
                        imports,
                        ..
                    } => {
                        let import_spec = ImportSpec {
                            module_name: module_name.clone(),
                            alias: alias.clone(),
                            symbols: imports.as_ref().map(|syms| {
                                syms.iter()
                                    .map(|s| SymbolImport {
                                        original_name: s.clone(),
                                        local_name: None,
                                    })
                                    .collect()
                            }),
                            refer_all: false, // This needs to be determined from AST
                        };
                        {
                            let mut guard = module_env.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
                            self.module_registry.import_symbols(&import_spec, &mut *guard, &mut self.ir_runtime)?;
                        }
                    }
                    IrNode::FunctionDef {
                        name: func_name,
                        lambda,
                        ..
                    } => {
                        {
                            let mut guard = module_env.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
                            let func_outcome = self.ir_runtime.execute_node(lambda, &mut *guard, false, &mut self.module_registry)?;
                            let func_value = match func_outcome {
                                super::execution_outcome::ExecutionOutcome::Complete(v) => v,
                                super::execution_outcome::ExecutionOutcome::RequiresHost(_)=> return Err(RuntimeError::Generic("Host call required during module function definition".to_string())),
                            };
                            guard.define(func_name.clone(), func_value.clone());
                        }
                    }
                    IrNode::VariableDef {
                        name: var_name,
                        init_expr,
                        ..
                    } => {
                        {
                            let mut guard = module_env.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
                            let var_outcome = self.ir_runtime.execute_node(init_expr, &mut *guard, false, &mut self.module_registry)?;
                            let var_value = match var_outcome {
                                super::execution_outcome::ExecutionOutcome::Complete(v) => v,
                                super::execution_outcome::ExecutionOutcome::RequiresHost(_)=> return Err(RuntimeError::Generic("Host call required during module variable definition".to_string())),
                            };
                            guard.define(var_name.clone(), var_value.clone());
                        }
                    }
                    _ => {
                        {
                            let mut guard = module_env.write().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
                            self.ir_runtime.execute_node(definition, &mut *guard, false, &mut self.module_registry)?;
                        }
                    }
                }
            }

            // Now, register all exports listed in the exports vector
            for export_name in exports {
                let guard = module_env.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?;
                if let Some(value) = guard.get(export_name) {
                    let export_type = match &value {
                        Value::Function(_) => ExportType::Function,
                        _ => ExportType::Variable,
                    };
                    module_exports.insert(
                        export_name.clone(),
                        ModuleExport {
                            original_name: export_name.clone(),
                            export_name: export_name.clone(),
                            value: value.clone(),
                            ir_type: IrType::Any,
                            export_type,
                        },
                    );
                }
            }

            // Create compiled module
            let compiled_module = Module {
                metadata: ModuleMetadata {
                    name: name.clone(),
                    docstring: None,
                    source_file: None,
                    version: Some("1.0.0".to_string()),
                    compiled_at: std::time::SystemTime::now(),
                },
                ir_node: module_node.clone(),
                exports: RwLock::new(module_exports),
                namespace: module_env,
                dependencies: Vec::new(),
            };

            // Register the module
            self.module_registry.register_module(compiled_module)?;

            Ok(Value::String(format!("Module {} loaded", name)))
        } else {
            Err(RuntimeError::InvalidArgument(
                "Expected Module node".to_string(),
            ))
        }
    }

    /// Execute an import statement
    fn execute_import(&mut self, import_node: &IrNode) -> RuntimeResult<Value> {
        if let IrNode::Import {
            module_name,
            alias,
            imports,
            ..
        } = import_node
        {
            let import_spec = ImportSpec {
                module_name: module_name.clone(),
                alias: alias.clone(),
                symbols: imports.as_ref().map(|syms| {
                    syms.iter()
                        .map(|s| SymbolImport {
                            original_name: s.clone(),
                            local_name: None,
                        })
                        .collect()
                }),
                refer_all: false, // Would need to detect this from IR
            };

            // Import into global environment (simplified)
            let mut global_env = IrEnvironment::new();
            self.module_registry.import_symbols(
                &import_spec,
                &mut global_env,
                &mut self.ir_runtime,
            )?;

            Ok(Value::String(format!("Imported {}", module_name)))
        } else {
            Err(RuntimeError::InvalidArgument(
                "Expected Import node".to_string(),
            ))
        }
    }

    /// Get the module registry
    pub fn module_registry(&self) -> &ModuleRegistry {
        &self.module_registry
    }

    /// Get the module registry (mutable)
    pub fn module_registry_mut(&mut self) -> &mut ModuleRegistry {
        &mut self.module_registry
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    

    #[test]
    fn test_module_registry_creation() {
        let registry = ModuleRegistry::new();
        assert_eq!(registry.loaded_modules().len(), 0);
    }
    #[test]
    fn test_module_loading_from_file() -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = ModuleRegistry::new();
        registry.add_module_path(std::path::PathBuf::from("test_modules"));
        let causal_chain = Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().unwrap()));
        let capability_marketplace = Arc::new(crate::ccos::capability_marketplace::CapabilityMarketplace::new(
            Arc::new(tokio::sync::RwLock::new(crate::ccos::capabilities::registry::CapabilityRegistry::new()))
        ));
        let security_context = crate::runtime::security::RuntimeContext {
            security_level: crate::runtime::security::SecurityLevel::Controlled,
            max_execution_time: Some(30000), // 30 seconds in milliseconds
            max_memory_usage: Some(100 * 1024 * 1024), // 100 MB in bytes
            allowed_capabilities: std::collections::HashSet::new(),
            use_microvm: false,
            log_capability_calls: false,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: std::collections::HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: std::collections::HashSet::new(),
            microvm_config_override: None,
            cross_plan_params: std::collections::HashMap::new(),
        };
        let host = Arc::new(crate::ccos::host::RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        let mut ir_runtime = IrRuntime::new(host, security_context);

        // Test loading the math.utils module
        let module = registry.load_module("math.utils", &mut ir_runtime).unwrap();
        assert_eq!(module.metadata.name, "math.utils");

        // Check that the expected exports are present
        let expected_exports = vec!["add", "multiply", "square"];
        for export in expected_exports {
            assert!(
                module.exports.read().map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))?.contains_key(export),
                "Missing export: {}",
                export
            );
        }
        Ok(())
    }
    #[test]
    fn test_qualified_symbol_resolution() -> Result<(), Box<dyn std::error::Error>> {
        let mut registry = ModuleRegistry::new();
        registry.add_module_path(std::path::PathBuf::from("test_modules"));
        let causal_chain = Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().unwrap()));
        let capability_marketplace = Arc::new(crate::ccos::capability_marketplace::CapabilityMarketplace::new(
            Arc::new(tokio::sync::RwLock::new(crate::ccos::capabilities::registry::CapabilityRegistry::new()))
        ));
        let security_context = crate::runtime::security::RuntimeContext {
            security_level: crate::runtime::security::SecurityLevel::Controlled,
            max_execution_time: Some(30000), // 30 seconds in milliseconds
            max_memory_usage: Some(100 * 1024 * 1024), // 100 MB in bytes
            allowed_capabilities: std::collections::HashSet::new(),
            use_microvm: false,
            log_capability_calls: false,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: std::collections::HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: std::collections::HashSet::new(),
            microvm_config_override: None,
            cross_plan_params: std::collections::HashMap::new(),
        };
        let host = Arc::new(crate::ccos::host::RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        let mut ir_runtime = IrRuntime::new(host, security_context);

        // Load math.utils module from file
        registry.load_module("math.utils", &mut ir_runtime).unwrap();

        // Resolve qualified symbol - should succeed now
    let result = registry.resolve_qualified_symbol("math.utils/add");
    assert!(result.is_ok(), "Should resolve math.utils/add symbol");
    Ok(())
    }

    #[test]
    fn test_circular_dependency_detection() {
        let mut registry = ModuleRegistry::new();
        registry
            .loading_stack
            .write()
            .map_err(|e| RuntimeError::InternalError(format!("RwLock poisoned: {}", e)))
            .unwrap()
            .push("module-a".to_string());
        // Try to load module-a again, which is already in the loading stack
        let causal_chain = Arc::new(std::sync::Mutex::new(crate::ccos::causal_chain::CausalChain::new().unwrap()));
        let capability_marketplace = Arc::new(crate::ccos::capability_marketplace::CapabilityMarketplace::new(
            Arc::new(tokio::sync::RwLock::new(crate::ccos::capabilities::registry::CapabilityRegistry::new()))
        ));
        let security_context = crate::runtime::security::RuntimeContext {
            security_level: crate::runtime::security::SecurityLevel::Controlled,
            max_execution_time: Some(30000), // 30 seconds in milliseconds
            max_memory_usage: Some(100 * 1024 * 1024), // 100 MB in bytes
            allowed_capabilities: std::collections::HashSet::new(),
            use_microvm: false,
            log_capability_calls: false,
            allow_inherit_isolation: true,
            allow_isolated_isolation: true,
            allow_sandboxed_isolation: true,
            expose_readonly_context: false,
            exposed_context_caps: std::collections::HashSet::new(),
            exposed_context_prefixes: Vec::new(),
            exposed_context_tags: std::collections::HashSet::new(),
            microvm_config_override: None,
            cross_plan_params: std::collections::HashMap::new(),
        };
        let host = Arc::new(crate::ccos::host::RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        let mut ir_runtime = IrRuntime::new(host, security_context);
        let result = registry.load_module("module-a", &mut ir_runtime);
        assert!(result.is_ok()); // Should now return a placeholder instead of an error
        let module = result.unwrap();
        assert_eq!(module.metadata.name, "module-a");
    assert!(module.metadata.docstring.as_deref().unwrap_or("").contains("placeholder"));
    }
    #[test]
    fn test_module_aware_runtime() {
        let runtime = ModuleAwareRuntime::new();

        // Test that we can access both IR runtime and module registry
        assert_eq!(runtime.module_registry().loaded_modules().len(), 0);
    }
}
