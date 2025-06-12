// Module Runtime - Comprehensive module system for RTFS
// Handles module loading, dependency resolution, namespacing, and import/export mechanisms

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use crate::ir::*;
use crate::runtime::{Value, RuntimeError, RuntimeResult};
use crate::runtime::ir_runtime::{IrRuntime, IrEnvironment};

/// Module registry that manages all loaded modules
#[derive(Debug)]
pub struct ModuleRegistry {
    /// Map from module name to compiled module
    modules: HashMap<String, Rc<CompiledModule>>,
    /// Map from module name to module namespace environment
    module_environments: HashMap<String, Rc<IrEnvironment>>,
    /// Module loading paths
    module_paths: Vec<PathBuf>,
    /// Currently loading modules (for circular dependency detection)
    loading_stack: Vec<String>,
}

/// A compiled module with its metadata and runtime environment
#[derive(Debug, Clone)]
pub struct CompiledModule {
    /// Module metadata
    pub metadata: ModuleMetadata,
    /// Module's IR representation
    pub ir_node: IrNode,
    /// Module's exported symbols
    pub exports: HashMap<String, ModuleExport>,
    /// Module's private namespace
    pub namespace: Rc<IrEnvironment>,
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
            modules: HashMap::new(),
            module_environments: HashMap::new(),
            module_paths: vec![PathBuf::from(".")],
            loading_stack: Vec::new(),
        }
    }

    /// Add a module search path
    pub fn add_module_path(&mut self, path: PathBuf) {
        if !self.module_paths.contains(&path) {
            self.module_paths.push(path);
        }
    }    /// Register a compiled module
    pub fn register_module(&mut self, module: CompiledModule) -> RuntimeResult<()> {
        let module_name = module.metadata.name.clone();
        
        // Store the module environment
        self.module_environments.insert(module_name.clone(), module.namespace.clone());
        
        // Register the module
        self.modules.insert(module_name, Rc::new(module));
        
        Ok(())
    }    /// Load and compile a module
    pub fn load_module(&mut self, module_name: &str, ir_runtime: &mut IrRuntime) -> RuntimeResult<Rc<CompiledModule>> {
        // Check if already loaded
        if let Some(module) = self.modules.get(module_name) {
            return Ok(module.clone());
        }

        // Check for circular dependency
        if self.loading_stack.contains(&module_name.to_string()) {
            return Err(RuntimeError::ModuleError(format!(
                "Circular dependency detected while loading: {}",
                module_name
            )));
        }

        // Add to loading stack
        self.loading_stack.push(module_name.to_string());

        // Load module from file
        let result = self.load_module_from_file(module_name, ir_runtime);

        // Remove from loading stack
        self.loading_stack.pop();

        result
    }    /// Load and compile a module from a source file
    fn load_module_from_file(&mut self, module_name: &str, ir_runtime: &mut IrRuntime) -> RuntimeResult<Rc<CompiledModule>> {
        // Resolve module path from module name
        let module_path = self.resolve_module_path(module_name)?;
        
        // Read the source file
        let source_content = self.read_module_source(&module_path)?;
        
        // Parse the module source
        let parsed_ast = self.parse_module_source(&source_content, &module_path)?;
        
        // Convert module AST to IR and compile
        let compiled_module = self.compile_module_ast(module_name, parsed_ast, &module_path, ir_runtime)?;
          // Register the compiled module
        self.register_module((*compiled_module).clone())?;
        
        Ok(compiled_module)
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
        &mut self,
        module_name: &str,
        module_def: crate::ast::ModuleDefinition,
        source_path: &std::path::Path,
        _ir_runtime: &mut IrRuntime
    ) -> RuntimeResult<Rc<CompiledModule>> {
        use crate::ir_converter::IrConverter;
        use std::collections::HashMap;
        
        // Create module metadata
        let metadata = ModuleMetadata {
            name: module_name.to_string(),
            docstring: None, // Could extract from module comments
            source_file: Some(source_path.to_path_buf()),
            version: None, // Could extract from module metadata
            compiled_at: std::time::SystemTime::now(),
        };

        // Create module namespace environment
        let mut module_env = IrEnvironment::new();
        
        // Process module dependencies first
        let mut dependencies = Vec::new();
        for definition in &module_def.definitions {
            if let crate::ast::ModuleLevelDefinition::Import(import_def) = definition {
                let dep_module_name = import_def.module_name.0.clone();
                
                // For now, just track dependencies - in full implementation would load them
                dependencies.push(dep_module_name);
                
                // Note: import_symbols_from_module would be implemented to handle actual imports
            }
        }

        // Convert module definitions to IR
        let mut ir_converter = IrConverter::new();
        let mut ir_definitions = Vec::new();
        let mut exports = HashMap::new();
        
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
                    
                    // Check if this definition should be exported
                    let symbol_name = def_expr.symbol.0.clone();
                    if self.should_export_symbol(&symbol_name, &module_def.exports) {
                        self.add_symbol_export(&symbol_name, &def_expr.value, &mut exports, &mut module_env)?;
                    }
                }
                crate::ast::ModuleLevelDefinition::Defn(defn_expr) => {
                    // Convert defn expression to Expression and then to IR
                    let expr = crate::ast::Expression::Defn(Box::new(defn_expr.clone()));
                    let ir_node = ir_converter.convert_expression(expr)
                        .map_err(|e| RuntimeError::ModuleError(format!("IR conversion failed: {:?}", e)))?;
                    ir_definitions.push(ir_node);
                    
                    // Check if this function should be exported
                    let symbol_name = defn_expr.name.0.clone();
                    if self.should_export_symbol(&symbol_name, &module_def.exports) {
                        self.add_function_export(&symbol_name, defn_expr, &mut exports, &mut module_env)?;
                    }
                }
            }
        }

        // Create the module IR node using a simple ID generator
        static mut MODULE_ID_COUNTER: u64 = 1000;
        let module_id = unsafe { 
            MODULE_ID_COUNTER += 1; 
            MODULE_ID_COUNTER 
        };

        let module_ir_node = IrNode::Module {
            id: module_id,
            name: module_name.to_string(),
            exports: exports.keys().cloned().collect(),
            definitions: ir_definitions,
            source_location: None,
        };        let compiled_module = CompiledModule {
            metadata,
            ir_node: module_ir_node,
            exports,
            namespace: Rc::new(module_env),
            dependencies,
        };

        Ok(Rc::new(compiled_module))
    }

    /// Check if a symbol should be exported based on module export specification
    fn should_export_symbol(&self, symbol_name: &str, export_spec: &Option<Vec<crate::ast::Symbol>>) -> bool {
        match export_spec {
            None => false, // No exports specified means nothing is exported
            Some(exports) => exports.iter().any(|sym| sym.0 == symbol_name),
        }
    }    /// Add a variable/constant export to the module
    fn add_symbol_export(
        &self,
        symbol_name: &str,
        _value_expr: &crate::ast::Expression,
        exports: &mut HashMap<String, ModuleExport>,
        env: &mut IrEnvironment
    ) -> RuntimeResult<()> {
        // For now, create a simple export - in a full implementation this would
        // evaluate the expression in the module context
        let placeholder_value = Value::String(format!("exported:{}", symbol_name));
        
        let node_id = (env.binding_count() + 1000) as u64; // Generate unique ID
        env.define(node_id, placeholder_value.clone());
        
        exports.insert(symbol_name.to_string(), ModuleExport {
            original_name: symbol_name.to_string(),
            export_name: symbol_name.to_string(),
            value: placeholder_value,
            ir_type: IrType::Any, // Would determine from expression analysis
            export_type: ExportType::Variable,
        });
        
        Ok(())
    }    /// Add a function export to the module
    fn add_function_export(
        &self,
        symbol_name: &str,
        _defn_expr: &crate::ast::DefnExpr,
        exports: &mut HashMap<String, ModuleExport>,
        env: &mut IrEnvironment
    ) -> RuntimeResult<()> {
        use crate::runtime::values::{Function, Arity};
        
        // Create a placeholder function for now - define it outside to avoid closure issues
        fn placeholder_function(_args: &[Value]) -> Result<Value, RuntimeError> {
            Ok(Value::String("Placeholder function".to_string()))
        }
        
        let placeholder_fn = Value::Function(Function::Builtin {
            name: symbol_name.to_string(),
            func: placeholder_function,
            arity: Arity::Any, // Would determine from defn parameters
        });
        
        let node_id = (env.binding_count() + 1000) as u64; // Generate unique ID
        env.define(node_id, placeholder_fn.clone());
        
        exports.insert(symbol_name.to_string(), ModuleExport {
            original_name: symbol_name.to_string(),
            export_name: symbol_name.to_string(),
            value: placeholder_fn,
            ir_type: IrType::Function {
                param_types: Vec::new(), // Would determine from defn parameters
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            },
            export_type: ExportType::Function,
        });
        
        Ok(())    }    /// Get a loaded module
    pub fn get_module(&self, module_name: &str) -> Option<Rc<CompiledModule>> {
        self.modules.get(module_name).cloned()
    }

    /// Get module environment
    pub fn get_module_environment(&self, module_name: &str) -> Option<Rc<IrEnvironment>> {
        self.module_environments.get(module_name).cloned()
    }

    /// List all loaded modules
    pub fn loaded_modules(&self) -> Vec<String> {
        self.modules.keys().cloned().collect()
    }

    /// Import symbols from a module into an environment
    pub fn import_symbols(
        &mut self,
        import_spec: &ImportSpec,
        target_env: &mut IrEnvironment,
        ir_runtime: &mut IrRuntime,
    ) -> RuntimeResult<()> {
        // Load the module if not already loaded
        let module = self.load_module(&import_spec.module_name, ir_runtime)?;

        match (&import_spec.alias, &import_spec.symbols, import_spec.refer_all) {
            // Import with alias: (import [module :as alias])
            (Some(alias), None, false) => {
                // Create a namespace environment for the alias
                let alias_id = target_env.binding_count() as u64 + 10000;
                
                // For aliased imports, we need to create a way to access module.symbol
                // This is simplified - a full implementation would create namespace objects
                target_env.define(alias_id, Value::String(format!("Module namespace: {}", alias)));
                
                // In a full implementation, you would set up namespace resolution
                // so that alias.symbol_name resolves to the module's exported symbol
            }            // Import specific symbols: (import [module :refer [sym1 sym2]])
            (None, Some(symbols), false) => {
                for symbol_import in symbols {
                    let export_name = &symbol_import.original_name;
                    let _local_name = symbol_import.local_name.as_ref().unwrap_or(export_name);

                    if let Some(export) = module.exports.get(export_name) {
                        let binding_id = target_env.binding_count() as u64 + 20000;
                        target_env.define(binding_id, export.value.clone());
                        // In a full implementation, you would also register the name mapping
                    } else {
                        return Err(RuntimeError::ModuleError(format!(
                            "Symbol '{}' not exported by module '{}'",
                            export_name, import_spec.module_name
                        )));
                    }
                }
            }

            // Import all symbols: (import [module :refer :all])
            (None, None, true) => {
                for (_export_name, export) in &module.exports {
                    let binding_id = target_env.binding_count() as u64 + 30000;
                    target_env.define(binding_id, export.value.clone());
                }
            }

            // Invalid combinations
            _ => {
                return Err(RuntimeError::ModuleError(
                    "Invalid import specification: cannot combine alias with refer".to_string()
                ));
            }
        }

        Ok(())
    }

    /// Resolve a qualified symbol (e.g., "module/symbol")
    pub fn resolve_qualified_symbol(&self, qualified_name: &str) -> RuntimeResult<Value> {
        if let Some(slash_pos) = qualified_name.find('/') {
            let module_name = &qualified_name[..slash_pos];
            let symbol_name = &qualified_name[slash_pos + 1..];

            if let Some(module) = self.get_module(module_name) {
                if let Some(export) = module.exports.get(symbol_name) {
                    Ok(export.value.clone())                } else {
                    Err(RuntimeError::UndefinedSymbol(
                        crate::ast::Symbol(qualified_name.to_string())
                    ))
                }
            } else {
                Err(RuntimeError::ModuleError(format!("Module not found: {}", module_name)))
            }
        } else {
            Err(RuntimeError::InvalidArgument(format!(
                "Not a qualified symbol: {}",
                qualified_name
            )))
        }
    }

    /// Check if a symbol is qualified (contains '/')
    pub fn is_qualified_symbol(symbol: &str) -> bool {
        symbol.contains('/')
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
            _ => self.ir_runtime.execute_program(program),
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
                self.ir_runtime.execute_node(form, &mut env)
            }
        }
    }

    /// Execute a module definition
    fn execute_module_definition(&mut self, module_node: &IrNode) -> RuntimeResult<Value> {
        if let IrNode::Module { name, exports, definitions, .. } = module_node {
            // Create module environment
            let mut module_env = IrEnvironment::new();
            let mut module_exports = HashMap::new();

            // Execute module definitions
            for definition in definitions {
                match definition {
                    IrNode::FunctionDef { name: func_name, lambda, .. } => {
                        let func_value = self.ir_runtime.execute_node(lambda, &mut module_env)?;
                        let binding_id = module_env.binding_count() as u64 + 40000;
                        module_env.define(binding_id, func_value.clone());

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
                        let var_value = self.ir_runtime.execute_node(init_expr, &mut module_env)?;
                        let binding_id = module_env.binding_count() as u64 + 50000;
                        module_env.define(binding_id, var_value.clone());

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
                        self.ir_runtime.execute_node(definition, &mut module_env)?;
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
                exports: module_exports,
                namespace: Rc::new(module_env),
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
            assert!(module.exports.contains_key(export), "Missing export: {}", export);
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
        let mut registry = ModuleRegistry::new();
        registry.loading_stack.push("module-a".to_string());
        registry.loading_stack.push("module-b".to_string());
        
        let mut ir_runtime = IrRuntime::new();
        let result = registry.load_module("module-a", &mut ir_runtime);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Circular dependency"));
    }    #[test]
    fn test_module_aware_runtime() {
        let runtime = ModuleAwareRuntime::new();
        
        // Test that we can access both IR runtime and module registry
        assert_eq!(runtime.module_registry().loaded_modules().len(), 0);
    }
}
