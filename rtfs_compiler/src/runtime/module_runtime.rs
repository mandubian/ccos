// Module Runtime - Comprehensive module system for RTFS
// Handles module loading, dependency resolution, namespacing, and import/export mechanisms

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;

use crate::ir::*;
use crate::runtime::environment::IrEnvironment;
use crate::runtime::ir_runtime::IrRuntime;
use crate::runtime::{RuntimeError, RuntimeResult, Value};
use crate::ir_converter::{IrConverter, BindingInfo, BindingKind};

/// Module registry that manages all loaded modules
#[derive(Debug)]
pub struct ModuleRegistry {
    /// Map from module name to compiled module
    modules: RefCell<HashMap<String, Rc<CompiledModule>>>,
    /// Map from module name to module namespace environment
    module_environments: RefCell<HashMap<String, Rc<RefCell<IrEnvironment>>>>,

    /// Module loading paths
    module_paths: Vec<PathBuf>,
    /// Currently loading modules (for circular dependency detection)
    loading_stack: RefCell<Vec<String>>,
}

/// A compiled module with its metadata and runtime environment
#[derive(Debug, Clone)]
pub struct CompiledModule {
    /// Module metadata
    pub metadata: ModuleMetadata,
    /// Module's IR representation
    pub ir_node: IrNode,
    /// Module's exported symbols
    pub exports: RefCell<HashMap<String, ModuleExport>>,
    /// Module's private namespace
    pub namespace: Rc<RefCell<IrEnvironment>>,
    /// Module dependencies
    pub dependencies: Vec<String>,
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
            modules: RefCell::new(HashMap::new()),
            module_environments: RefCell::new(HashMap::new()),
            module_paths: vec![PathBuf::from(".")],
            loading_stack: RefCell::new(Vec::new()),
        }
    }

    /// Add a module search path
    pub fn add_module_path(&mut self, path: PathBuf) {
        if !self.module_paths.contains(&path) {
            self.module_paths.push(path);
        }
    }    /// Register a compiled module
    pub fn register_module(&self, module: CompiledModule) -> RuntimeResult<()> {
        let module_name = module.metadata.name.clone();
        
        // Store the module environment
        self.module_environments.borrow_mut().insert(module_name.clone(), module.namespace.clone());
        
        // Register the module
        self.modules.borrow_mut().insert(module_name, Rc::new(module));
        
        Ok(())
    }    /// Load and compile a module
    pub fn load_module(&self, module_name: &str, ir_runtime: &mut IrRuntime) -> RuntimeResult<Rc<CompiledModule>> {
        // If already loaded, return it.
        if let Some(module) = self.modules.borrow().get(module_name) {
            return Ok(module.clone());
        }
        // If module is not found, we need to load it from file.
        // Check for circular dependency.
        if self.loading_stack.borrow().contains(&module_name.to_string()) {
            // It's a cycle. We'll create and register a temporary, empty module to break the cycle.
            let placeholder_metadata = ModuleMetadata {
                name: module_name.to_string(),
                docstring: Some("Circular dependency placeholder".to_string()),
                source_file: None,
                version: None,
                compiled_at: std::time::SystemTime::now(),
            };
            let placeholder_module = Rc::new(CompiledModule {
                metadata: placeholder_metadata,
                ir_node: IrNode::Do { 
                    id: 0, 
                    expressions: vec![], 
                    ir_type: IrType::Nil, 
                    source_location: None 
                },
                exports: RefCell::new(HashMap::new()),
                namespace: Rc::new(RefCell::new(IrEnvironment::new())),
                dependencies: Vec::new(),
            });
            
            // Register the placeholder to allow dependent modules to compile.
            self.modules.borrow_mut().insert(module_name.to_string(), placeholder_module.clone());
            
            return Ok(placeholder_module);
        }

        self.loading_stack.borrow_mut().push(module_name.to_string());

        // Compile the module from source, getting back the module structure and the bindings map.
        let (compiled_module, bindings) = match self.load_module_from_file(module_name, ir_runtime) {
            Ok(result) => result,
            Err(e) => {
                self.loading_stack.borrow_mut().pop();
                return Err(e);
            }
        };

        // Now, execute the module's IR to populate its namespace.
        // The module's environment is internal to the CompiledModule structure.
        ir_runtime.execute_node(&compiled_module.ir_node, &mut compiled_module.namespace.borrow_mut(), false, self).map_err(|e| {
            self.loading_stack.borrow_mut().pop();
            e
        })?;

        // After execution, populate the exports using the bindings map and the populated environment.
        if let IrNode::Module { exports: export_names, .. } = &compiled_module.ir_node {
            let mut exports_map = compiled_module.exports.borrow_mut();
            let module_env_borrow = compiled_module.namespace.borrow();
            
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
                println!("[DEBUG] Environment keys: {:?}", (*module_env_borrow).binding_keys());
            }

            for export_name in export_names {
                if let Some(binding_info) = bindings.get(export_name) {
                    if let Some(value) = module_env_borrow.lookup(binding_info.binding_id) {
                        let export = ModuleExport {
                            original_name: export_name.clone(),
                            export_name: export_name.clone(),
                            value: value.clone(),
                            ir_type: binding_info.ir_type.clone(),
                            export_type: match value {
                                Value::Function(_) => ExportType::Function,
                                _ => ExportType::Variable,
                            },
                        };
                        exports_map.insert(export_name.clone(), export);
                    } else {
                        self.loading_stack.borrow_mut().pop();
                        return Err(RuntimeError::ModuleError(format!(
                            "Exported symbol '{}' not found in module '{}' environment after execution.",
                            export_name, module_name
                        )));
                    }
                }
            }
        }

        self.loading_stack.borrow_mut().pop();

        // Register the definitive, fully-loaded module. This will overwrite any placeholders.
        self.modules.borrow_mut().insert(module_name.to_string(), compiled_module.clone());
        self.module_environments.borrow_mut().insert(module_name.to_string(), compiled_module.namespace.clone());
        
        Ok(compiled_module)
    }

    /// Load and compile a module from a source file
    fn load_module_from_file(&self, module_name: &str, ir_runtime: &mut IrRuntime) -> RuntimeResult<(Rc<CompiledModule>, HashMap<String, BindingInfo>)> {
        // Resolve module path from module name
        let module_path = self.resolve_module_path(module_name)?;
        
        // Read the source file
        let source_content = self.read_module_source(&module_path)?;
        
        // Parse the module source
        let parsed_ast = self.parse_module_source(&source_content, &module_path)?;
        
        // Convert module AST to IR and compile
        let compiled_result = self.compile_module_ast(module_name, parsed_ast, &module_path, ir_runtime)?;
        
        Ok(compiled_result)
    }

    /// Resolve a module name to a file path
    /// Examples:
    /// - "rtfs.core.string" -> "rtfs/core/string.rtfs"
    /// - "my.company/utils" -> "my/company/utils.rtfs"
    fn resolve_module_path(&self, module_name: &str) -> RuntimeResult<std::path::PathBuf> {
        use std::path::PathBuf;
        
        // Convert module name to file path
        // Replace dots and slashes with path separators
        let path_str = module_name
            .replace('.', "/")  // Convert dots to slashes
            .replace("/", std::path::MAIN_SEPARATOR_STR); // Use OS-specific path separator
        
        // Add .rtfs extension
        let filename = format!("{}.rtfs", path_str);
          // Try to find the file in module search paths
        for search_path in &self.module_paths {
            let full_path = search_path.join(&filename);
            if full_path.exists() {
                return Ok(full_path);
            }
        }
        
        // If not found in search paths, try relative to current directory
        let default_path = PathBuf::from(&filename);
        if default_path.exists() {
            return Ok(default_path);
        }
        
        Err(RuntimeError::ModuleError(format!(
            "Module file not found: {} (tried {})",
            module_name, filename
        )))
    }

    /// Read module source from file
    fn read_module_source(&self, path: &std::path::Path) -> RuntimeResult<String> {
        use std::fs;
        
        fs::read_to_string(path).map_err(|err| {
            RuntimeError::ModuleError(format!(
                "Failed to read module file '{}': {}",
                path.display(),
                err
            ))
        })
    }

    /// Parse module source into AST
    fn parse_module_source(&self, source: &str, path: &std::path::Path) -> RuntimeResult<crate::ast::ModuleDefinition> {
        use crate::parser::parse;
        
        // Parse the entire source file
        let top_levels = parse(source).map_err(|err| {
            RuntimeError::ModuleError(format!(
                "Failed to parse module file '{}': {:?}",
                path.display(),
                err
            ))
        })?;
        
        // Find the module definition
        for top_level in top_levels {
            if let crate::ast::TopLevel::Module(module_def) = top_level {
                return Ok(module_def);
            }
        }
        
        Err(RuntimeError::ModuleError(format!(
            "No module definition found in file '{}'",
            path.display()
        )))
    }    /// Compile module AST to a CompiledModule
    fn compile_module_ast(
        &self,
        module_name: &str,
        module_def: crate::ast::ModuleDefinition,
        source_path: &std::path::Path,
        ir_runtime: &mut IrRuntime
    ) -> RuntimeResult<(Rc<CompiledModule>, HashMap<String, BindingInfo>)> {
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
        let stdlib_env = Rc::new(IrEnvironment::with_stdlib());
        let module_env = Rc::new(RefCell::new(IrEnvironment::with_parent(stdlib_env)));
        
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
                        for (export_name, export) in loaded_dep_module.exports.borrow().iter() {
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
                            if let Some(export) = loaded_dep_module.exports.borrow().get(export_name) {
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
                        for (export_name, export) in loaded_dep_module.exports.borrow().iter() {
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
                            "Invalid import specification: cannot combine :as with :only".to_string()
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
                }                crate::ast::ModuleLevelDefinition::Def(def_expr) => {
                    // Convert def expression to Expression and then to IR
                    let expr = crate::ast::Expression::Def(Box::new(def_expr.clone()));
                    let ir_node = ir_converter.convert_expression(expr)
                        .map_err(|e| RuntimeError::ModuleError(format!("IR conversion failed: {:?}", e)))?;
                    ir_definitions.push(ir_node);
                }
                crate::ast::ModuleLevelDefinition::Defn(defn_expr) => {
                    // Convert defn expression to Expression and then to IR
                    let expr = crate::ast::Expression::Defn(Box::new(defn_expr.clone()));
                    let ir_node = ir_converter.convert_expression(expr)
                        .map_err(|e| RuntimeError::ModuleError(format!("IR conversion failed: {:?}", e)))?;
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
                    IrNode::FunctionDef { id, name, ir_type, .. } => {
                        let binding_info = BindingInfo {
                            name: name.clone(),
                            binding_id: *id,
                            ir_type: ir_type.clone(),
                            kind: BindingKind::Function,
                        };
                        bindings.insert(name.clone(), binding_info);
                    }
                    IrNode::VariableDef { id, name, ir_type, .. } => {
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

        let compiled_module = CompiledModule {
            metadata,
            ir_node: module_ir_node,
            exports: RefCell::new(HashMap::new()),
            namespace: module_env,
            dependencies,
        };
        
        Ok((Rc::new(compiled_module), bindings))
    }

    pub fn get_module(&self, module_name: &str) -> Option<Rc<CompiledModule>> {
        self.modules.borrow().get(module_name).cloned()
    }

    pub fn loaded_modules(&self) -> std::cell::Ref<HashMap<String, Rc<CompiledModule>>> {
        self.modules.borrow()
    }    pub fn is_qualified_symbol(name: &str) -> bool {
        if let Some(slash_pos) = name.find('/') {
            // Must have non-empty module name and non-empty symbol name
            slash_pos > 0 && slash_pos < name.len() - 1
        } else {
            false
        }
    }

    pub fn resolve_qualified_symbol(&self, qualified_name: &str) -> RuntimeResult<Value> {
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
            if let Some(export) = module.exports.borrow().get(symbol_name) {
                Ok(export.value.clone())
            } else {
                Err(RuntimeError::SymbolNotFound(format!(
                    "Symbol '{}' not found in module '{}'",
                    symbol_name, module_name
                )))
            }
        } else {
            // Before failing, try to load the module
            let mut ir_runtime = IrRuntime::new(); // Temporary runtime
            match self.load_module(module_name, &mut ir_runtime) {
                Ok(module) => {
                    if let Some(export) = module.exports.borrow().get(symbol_name) {
                        Ok(export.value.clone())
                    } else {
                        Err(RuntimeError::SymbolNotFound(format!(
                            "Symbol '{}' not found in newly loaded module '{}'",
                            symbol_name, module_name
                        )))
                    }
                }
                Err(_) => Err(RuntimeError::ModuleNotFound(module_name.to_string()))
            }
        }
    }

    pub fn import_symbols(
        &self,
        import_spec: &ImportSpec,
        _env: &mut IrEnvironment,
        ir_runtime: &mut IrRuntime,
    ) -> RuntimeResult<()> {
        self.load_module(&import_spec.module_name, ir_runtime)?;
        Ok(())
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
        ModuleAwareRuntime {
            ir_runtime: IrRuntime::new(),
            module_registry: ModuleRegistry::new(),
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
            _ => self.ir_runtime.execute_program(program, &self.module_registry),
        }
    }

    /// Execute a top-level form (module, import, def, etc.)
    fn execute_top_level_form(&mut self, form: &IrNode) -> RuntimeResult<Value> {
        match form {
            IrNode::Module { .. } => {
                self.execute_module_definition(form)
            }
            IrNode::Import { .. } => {
                self.execute_import(form)
            }
            _ => {
                // Regular IR node execution
                let mut env = IrEnvironment::new();
                self.ir_runtime.execute_node(form, &mut env, false, &self.module_registry)
            }
        }
    }

    /// Execute a module definition
    fn execute_module_definition(&mut self, module_node: &IrNode) -> RuntimeResult<Value> {
        if let IrNode::Module { name, exports, definitions, .. } = module_node {
            // Create module environment
            let module_env = Rc::new(RefCell::new(IrEnvironment::new()));
            let mut module_exports = HashMap::new();

            // Execute module definitions
            for definition in definitions {
                match definition {
                    IrNode::Import { module_name, alias, imports, .. } => {
                        let import_spec = ImportSpec {
                            module_name: module_name.clone(),
                            alias: alias.clone(),
                            symbols: imports.as_ref().map(|syms| {
                                syms.iter().map(|s| SymbolImport {
                                    original_name: s.clone(),
                                    local_name: None,
                                }).collect()
                            }),
                            refer_all: false, // This needs to be determined from AST
                        };
                        self.module_registry.import_symbols(&import_spec, &mut module_env.borrow_mut(), &mut self.ir_runtime)?;
                    }
                    IrNode::FunctionDef { name: func_name, lambda, .. } => {
                        let func_value = self.ir_runtime.execute_node(lambda, &mut module_env.borrow_mut(), false, &self.module_registry)?;
                        let binding_id = module_env.borrow().binding_count() as u64 + 40000;
                        module_env.borrow_mut().define(binding_id, func_value.clone());

                        // Add to exports if listed
                        if exports.contains(func_name) {
                            module_exports.insert(func_name.clone(), ModuleExport {
                                original_name: func_name.clone(),
                                export_name: func_name.clone(),
                                value: func_value,
                                ir_type: IrType::Any, // Would infer proper type
                                export_type: ExportType::Function,
                            });
                        }
                    }
                    IrNode::VariableDef { name: var_name, init_expr, .. } => {
                        let var_value = self.ir_runtime.execute_node(init_expr, &mut module_env.borrow_mut(), false, &self.module_registry)?;
                        let binding_id = module_env.borrow().binding_count() as u64 + 50000;
                        module_env.borrow_mut().define(binding_id, var_value.clone());

                        // Add to exports if listed
                        if exports.contains(var_name) {
                            module_exports.insert(var_name.clone(), ModuleExport {
                                original_name: var_name.clone(),
                                export_name: var_name.clone(),
                                value: var_value,
                                ir_type: IrType::Any, // Would infer proper type
                                export_type: ExportType::Variable,
                            });
                        }
                    }
                    _ => {
                        self.ir_runtime.execute_node(definition, &mut module_env.borrow_mut(), false, &self.module_registry)?;
                    }
                }
            }

            // Create compiled module
            let compiled_module = CompiledModule {
                metadata: ModuleMetadata {
                    name: name.clone(),
                    docstring: None,
                    source_file: None,
                    version: Some("1.0.0".to_string()),
                    compiled_at: std::time::SystemTime::now(),
                },
                ir_node: module_node.clone(),
                exports: RefCell::new(module_exports),
                namespace: module_env,
                dependencies: Vec::new(),
            };

            // Register the module
            self.module_registry.register_module(compiled_module)?;

            Ok(Value::String(format!("Module {} loaded", name)))
        } else {
            Err(RuntimeError::InvalidArgument("Expected Module node".to_string()))
        }
    }

    /// Execute an import statement
    fn execute_import(&mut self, import_node: &IrNode) -> RuntimeResult<Value> {
        if let IrNode::Import { module_name, alias, imports, .. } = import_node {
            let import_spec = ImportSpec {
                module_name: module_name.clone(),
                alias: alias.clone(),
                symbols: imports.as_ref().map(|syms| {
                    syms.iter().map(|s| SymbolImport {
                        original_name: s.clone(),
                        local_name: None,
                    }).collect()
                }),
                refer_all: false, // Would need to detect this from IR
            };

            // Import into global environment (simplified)
            let mut global_env = IrEnvironment::new();
            self.module_registry.import_symbols(&import_spec, &mut global_env, &mut self.ir_runtime)?;

            Ok(Value::String(format!("Imported {}", module_name)))
        } else {
            Err(RuntimeError::InvalidArgument("Expected Import node".to_string()))
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
    }    #[test]
    fn test_module_loading_from_file() {
        let mut registry = ModuleRegistry::new();
        registry.add_module_path(std::path::PathBuf::from("test_modules"));
        let mut ir_runtime = IrRuntime::new();
        
        // Test loading the math.utils module
        let module = registry.load_module("math.utils", &mut ir_runtime).unwrap();
        assert_eq!(module.metadata.name, "math.utils");
        
        // Check that the expected exports are present
        let expected_exports = vec!["add", "multiply", "square"];
        for export in expected_exports {
            assert!(module.exports.borrow().contains_key(export), "Missing export: {}", export);
        }
    }    #[test]
    fn test_qualified_symbol_resolution() {
        let mut registry = ModuleRegistry::new();
        registry.add_module_path(std::path::PathBuf::from("test_modules"));
        let mut ir_runtime = IrRuntime::new();
        
        // Load math.utils module from file
        registry.load_module("math.utils", &mut ir_runtime).unwrap();
        
        // Resolve qualified symbol - should succeed now
        let result = registry.resolve_qualified_symbol("math.utils/add");
        assert!(result.is_ok(), "Should resolve math.utils/add symbol");
    }

    #[test]
    fn test_circular_dependency_detection() {
        let registry = ModuleRegistry::new();
        registry.loading_stack.borrow_mut().push("module-a".to_string());
        // Try to load module-a again, which is already in the loading stack
        let mut ir_runtime = IrRuntime::new();
        let result = registry.load_module("module-a", &mut ir_runtime);
        assert!(result.is_ok()); // Should now return a placeholder instead of an error
        let module = result.unwrap();
        assert_eq!(module.metadata.name, "module-a");
        assert!(module.metadata.docstring.as_deref().unwrap_or("").contains("placeholder"));
    }    #[test]
    fn test_module_aware_runtime() {
        let runtime = ModuleAwareRuntime::new();
        
        // Test that we can access both IR runtime and module registry
        assert_eq!(runtime.module_registry().loaded_modules().len(), 0);
    }
}
