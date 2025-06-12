// Module Runtime - Comprehensive module system for RTFS
// Handles module loading, dependency resolution, namespacing, and import/export mechanisms

use std::collections::HashMap;
use std::path::PathBuf;
use std::rc::Rc;
use crate::ir::*;
use crate::runtime::{Value, RuntimeError, RuntimeResult, Environment};
use crate::runtime::ir_runtime::{IrRuntime, IrEnvironment};
use crate::ast::Symbol;

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
    }

    /// Load and compile a module
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

        // For now, create a mock module since we don't have file loading yet
        let result = self.create_mock_module(module_name, ir_runtime);

        // Remove from loading stack
        self.loading_stack.pop();

        result
    }

    /// Create a mock module for demonstration (would be replaced with actual file loading)
    fn create_mock_module(&mut self, module_name: &str, ir_runtime: &mut IrRuntime) -> RuntimeResult<Rc<CompiledModule>> {
        let metadata = ModuleMetadata {
            name: module_name.to_string(),
            docstring: Some(format!("Mock module: {}", module_name)),
            source_file: None,
            version: Some("1.0.0".to_string()),
            compiled_at: std::time::SystemTime::now(),
        };

        // Create module namespace
        let mut module_env = IrEnvironment::new();

        // Add some mock exports based on module name
        let mut exports = HashMap::new();
        
        match module_name {
            "rtfs.core.string" => {
                // Mock string utilities
                self.add_string_module_exports(&mut module_env, &mut exports)?;
            }
            "rtfs.core.math" => {
                // Mock math utilities
                self.add_math_module_exports(&mut module_env, &mut exports)?;
            }
            _ => {
                // Generic mock module
                self.add_generic_module_exports(&mut module_env, &mut exports, module_name)?;
            }
        }

        // Create IR node for the module
        let ir_node = IrNode::Module {
            id: 1, // Would use proper ID generation
            name: module_name.to_string(),
            exports: exports.keys().cloned().collect(),
            definitions: Vec::new(), // Would contain actual definitions
            source_location: None,
        };

        let module = CompiledModule {
            metadata,
            ir_node,
            exports,
            namespace: Rc::new(module_env),
            dependencies: Vec::new(),
        };

        self.register_module(module.clone())?;
        Ok(Rc::new(module))
    }

    /// Add string module exports (mock implementation)
    fn add_string_module_exports(
        &self,
        env: &mut IrEnvironment,
        exports: &mut HashMap<String, ModuleExport>,
    ) -> RuntimeResult<()> {
        use crate::runtime::values::{Function, Arity};

        // length function
        let length_fn = Value::Function(Function::Builtin {
            name: "length".to_string(),
            func: |args| {                if let Some(Value::String(s)) = args.first() {
                    Ok(Value::Integer(s.len() as i64))
                } else {
                    Err(RuntimeError::TypeError {
                        expected: "string".to_string(),
                        actual: "non-string".to_string(),
                        operation: "string length function".to_string(),
                    })
                }
            },
            arity: Arity::Exact(1),
        });

        env.define(1001, length_fn.clone());
        exports.insert("length".to_string(), ModuleExport {
            original_name: "length".to_string(),
            export_name: "length".to_string(),
            value: length_fn,
            ir_type: IrType::Function {
                param_types: vec![IrType::String],
                variadic_param_type: None,
                return_type: Box::new(IrType::Int),
            },
            export_type: ExportType::Function,
        });

        // upper-case function
        let upper_fn = Value::Function(Function::Builtin {
            name: "upper-case".to_string(),
            func: |args| {                if let Some(Value::String(s)) = args.first() {
                    Ok(Value::String(s.to_uppercase()))
                } else {
                    Err(RuntimeError::TypeError {
                        expected: "string".to_string(),
                        actual: "non-string".to_string(),
                        operation: "string upper-case function".to_string(),
                    })
                }
            },
            arity: Arity::Exact(1),
        });

        env.define(1002, upper_fn.clone());
        exports.insert("upper-case".to_string(), ModuleExport {
            original_name: "upper-case".to_string(),
            export_name: "upper-case".to_string(),
            value: upper_fn,
            ir_type: IrType::Function {
                param_types: vec![IrType::String],
                variadic_param_type: None,
                return_type: Box::new(IrType::String),
            },
            export_type: ExportType::Function,
        });

        // substring function
        let substring_fn = Value::Function(Function::Builtin {
            name: "substring".to_string(),
            func: |args| {
                match args {
                    [Value::String(s), Value::Integer(start), Value::Integer(end)] => {
                        let start = *start as usize;
                        let end = (*end as usize).min(s.len());
                        if start <= end && start <= s.len() {
                            Ok(Value::String(s.chars().skip(start).take(end - start).collect()))
                        } else {
                            Err(RuntimeError::InvalidArgument("Invalid substring indices".to_string()))
                        }
                    }
                    _ => Err(RuntimeError::TypeError {
                        expected: "(string, int, int)".to_string(),
                        actual: "other types".to_string(),
                        operation: "substring function".to_string(),
                    })
                }
            },
            arity: Arity::Exact(3),
        });

        env.define(1003, substring_fn.clone());
        exports.insert("substring".to_string(), ModuleExport {
            original_name: "substring".to_string(),
            export_name: "substring".to_string(),
            value: substring_fn,
            ir_type: IrType::Function {
                param_types: vec![IrType::String, IrType::Int, IrType::Int],
                variadic_param_type: None,
                return_type: Box::new(IrType::String),
            },
            export_type: ExportType::Function,
        });

        // concat function
        let concat_fn = Value::Function(Function::Builtin {
            name: "concat".to_string(),
            func: |args| {
                let mut result = String::new();
                for arg in args {
                    match arg {
                        Value::String(s) => result.push_str(s),
                        _ => return Err(RuntimeError::TypeError {
                            expected: "string".to_string(),
                            actual: "non-string".to_string(),
                            operation: "string concat function".to_string(),
                        }),
                    }
                }
                Ok(Value::String(result))
            },
            arity: Arity::AtLeast(0),
        });

        env.define(1004, concat_fn.clone());
        exports.insert("concat".to_string(), ModuleExport {
            original_name: "concat".to_string(),
            export_name: "concat".to_string(),
            value: concat_fn,
            ir_type: IrType::Function {
                param_types: vec![IrType::String],
                variadic_param_type: Some(Box::new(IrType::String)),
                return_type: Box::new(IrType::String),
            },
            export_type: ExportType::Function,
        });

        Ok(())
    }

    /// Add math module exports (mock implementation)
    fn add_math_module_exports(
        &self,
        env: &mut IrEnvironment,
        exports: &mut HashMap<String, ModuleExport>,
    ) -> RuntimeResult<()> {
        use crate::runtime::values::{Function, Arity};

        // abs function
        let abs_fn = Value::Function(Function::Builtin {
            name: "abs".to_string(),
            func: |args| {
                match args.first() {
                    Some(Value::Integer(n)) => Ok(Value::Integer(n.abs())),
                    Some(Value::Float(f)) => Ok(Value::Float(f.abs())),
                    _ => Err(RuntimeError::TypeError {
                        expected: "numeric".to_string(),
                        actual: "non-numeric".to_string(),
                        operation: "abs function".to_string(),
                    }),
                }
            },
            arity: Arity::Exact(1),
        });

        env.define(2001, abs_fn.clone());
        exports.insert("abs".to_string(), ModuleExport {
            original_name: "abs".to_string(),
            export_name: "abs".to_string(),
            value: abs_fn,
            ir_type: IrType::Function {
                param_types: vec![IrType::Any],
                variadic_param_type: None,
                return_type: Box::new(IrType::Any),
            },
            export_type: ExportType::Function,
        });

        // PI constant
        let pi_value = Value::Float(std::f64::consts::PI);
        env.define(2002, pi_value.clone());
        exports.insert("PI".to_string(), ModuleExport {
            original_name: "PI".to_string(),
            export_name: "PI".to_string(),
            value: pi_value,
            ir_type: IrType::Float,
            export_type: ExportType::Variable,
        });

        Ok(())
    }

    /// Add generic module exports (mock implementation)
    fn add_generic_module_exports(
        &self,
        env: &mut IrEnvironment,
        exports: &mut HashMap<String, ModuleExport>,
        module_name: &str,
    ) -> RuntimeResult<()> {
        use crate::runtime::values::{Function, Arity};        // info function that returns module info
        let info_fn = Value::Function(Function::Builtin {
            name: "info".to_string(),
            func: |_args| Ok(Value::String("Generic module info".to_string())),
            arity: Arity::Exact(0),
        });

        env.define(3001, info_fn.clone());
        exports.insert("info".to_string(), ModuleExport {
            original_name: "info".to_string(),
            export_name: "info".to_string(),
            value: info_fn,
            ir_type: IrType::Function {
                param_types: vec![],
                variadic_param_type: None,
                return_type: Box::new(IrType::String),
            },
            export_type: ExportType::Function,
        });

        Ok(())
    }

    /// Get a loaded module
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
            }

            // Import specific symbols: (import [module :refer [sym1 sym2]])
            (None, Some(symbols), false) => {
                for symbol_import in symbols {
                    let export_name = &symbol_import.original_name;
                    let local_name = symbol_import.local_name.as_ref().unwrap_or(export_name);

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
                for (export_name, export) in &module.exports {
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
    }

    #[test]
    fn test_mock_string_module_loading() {
        let mut registry = ModuleRegistry::new();
        let mut ir_runtime = IrRuntime::new();
        
        let module = registry.load_module("rtfs.core.string", &mut ir_runtime).unwrap();
        assert_eq!(module.metadata.name, "rtfs.core.string");
        assert!(module.exports.contains_key("length"));
        assert!(module.exports.contains_key("upper-case"));
    }

    #[test]
    fn test_qualified_symbol_resolution() {
        let mut registry = ModuleRegistry::new();
        let mut ir_runtime = IrRuntime::new();
        
        // Load string module
        registry.load_module("rtfs.core.string", &mut ir_runtime).unwrap();
        
        // Resolve qualified symbol
        let result = registry.resolve_qualified_symbol("rtfs.core.string/length");
        assert!(result.is_ok());
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
    }

    #[test]
    fn test_module_aware_runtime() {
        let mut runtime = ModuleAwareRuntime::new();
        
        // Test that we can access both IR runtime and module registry
        assert_eq!(runtime.module_registry().loaded_modules().len(), 0);
    }
}
