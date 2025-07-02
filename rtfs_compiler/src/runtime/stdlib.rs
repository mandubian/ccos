// Standard library implementation for RTFS
// Contains all built-in functions and tool interfaces

use crate::ast::{Keyword, MapKey, Symbol};
use crate::ir::core::{IrNode, IrType};
use crate::runtime::environment::Environment;
use crate::runtime::environment::IrEnvironment;
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::evaluator::Evaluator;
use crate::runtime::module_runtime::{
    ExportType, Module, ModuleExport, ModuleMetadata, ModuleRegistry,
};
use crate::runtime::values::{Arity, BuiltinFunction, BuiltinFunctionWithContext, Function};
use crate::runtime::Value;
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;
use std::rc::Rc;
use std::sync::OnceLock;

pub struct StandardLibrary;

/// File handle for managing open files
struct FileHandle {
    file: File,
    reader: Option<BufReader<File>>,
    path: String,
}

impl FileHandle {
    fn new(file: File, path: String) -> Self {
        Self {
            file,
            reader: None,
            path,
        }
    }

    fn get_reader(&mut self) -> &mut BufReader<File> {
        if self.reader.is_none() {
            // Reopen the file for reading
            if let Ok(file) = OpenOptions::new().read(true).open(&self.path) {
                self.reader = Some(BufReader::new(file));
            }
        }
        self.reader.as_mut().unwrap()
    }
}

/// Global file handle registry
static mut FILE_HANDLES: Option<RefCell<HashMap<i64, FileHandle>>> = None;

fn get_file_handles() -> &'static RefCell<HashMap<i64, FileHandle>> {
    unsafe {
        if FILE_HANDLES.is_none() {
            FILE_HANDLES = Some(RefCell::new(HashMap::new()));
        }
        FILE_HANDLES.as_ref().unwrap()
    }
}

static mut NEXT_FILE_HANDLE_ID: i64 = 1;

fn get_next_file_handle_id() -> i64 {
    unsafe {
        let id = NEXT_FILE_HANDLE_ID;
        NEXT_FILE_HANDLE_ID += 1;
        id
    }
}

impl StandardLibrary {
    /// Create a new environment with all standard library functions loaded
    pub fn create_global_environment() -> Environment {
        let mut env = Environment::new();

        // Load all built-in functions
        Self::load_arithmetic_functions(&mut env);
        Self::load_comparison_functions(&mut env);
        Self::load_boolean_functions(&mut env);
        Self::load_string_functions(&mut env);
        Self::load_collection_functions(&mut env);
        Self::load_type_predicate_functions(&mut env);
        Self::load_tool_functions(&mut env);
        Self::load_agent_functions(&mut env);
        // Self::load_task_functions(&mut env);

        env
    }

    /// Load arithmetic functions (+, -, *, /)
    fn load_arithmetic_functions(env: &mut Environment) {
        // Addition (+)
        env.define(
            &Symbol("+".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "+".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::add),
            })),
        );

        // Subtraction (-)
        env.define(
            &Symbol("-".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "-".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::subtract),
            })),
        );

        // Multiplication (*)
        env.define(
            &Symbol("*".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "*".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::multiply),
            })),
        );
        // Division (/)
        env.define(
            &Symbol("/".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "/".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::divide),
            })),
        );

        // Max
        env.define(
            &Symbol("max".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "max".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::max_value),
            })),
        );

        // Min
        env.define(
            &Symbol("min".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "min".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::min_value),
            })),
        );

        // Increment function
        env.define(
            &Symbol("inc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "inc".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| {
                    if args.len() != 1 {
                        return Err(RuntimeError::Generic(
                            "inc expects exactly 1 argument".to_string(),
                        ));
                    }
                    match &args[0] {
                        Value::Integer(n) => Ok(Value::Integer(n + 1)),
                        Value::Float(f) => Ok(Value::Float(f + 1.0)),
                        _ => Err(RuntimeError::Generic("inc expects a number".to_string())),
                    }
                }),
            })),
        );

        // Decrement function
        env.define(
            &Symbol("dec".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "dec".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(|args| {
                    if args.len() != 1 {
                        return Err(RuntimeError::Generic(
                            "dec expects exactly 1 argument".to_string(),
                        ));
                    }
                    match &args[0] {
                        Value::Integer(n) => Ok(Value::Integer(n - 1)),
                        Value::Float(f) => Ok(Value::Float(f - 1.0)),
                        _ => Err(RuntimeError::Generic("dec expects a number".to_string())),
                    }
                }),
            })),
        );
    }

    /// Load comparison functions (=, !=, >, <, >=, <=)
    fn load_comparison_functions(env: &mut Environment) {
        env.define(
            &Symbol("=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "=".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::equal),
            })),
        );

        env.define(
            &Symbol("!=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "!=".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::not_equal),
            })),
        );

        env.define(
            &Symbol(">".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: ">".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::greater_than),
            })),
        );

        env.define(
            &Symbol("<".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "<".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::less_than),
            })),
        );

        env.define(
            &Symbol(">=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: ">=".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::greater_equal),
            })),
        );

        env.define(
            &Symbol("<=".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "<=".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::less_equal),
            })),
        );
    }

    /// Load boolean functions (and, or, not)
    fn load_boolean_functions(env: &mut Environment) {
        env.define(
            &Symbol("and".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "and".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::and),
            })),
        );

        env.define(
            &Symbol("or".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "or".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::or),
            })),
        );

        env.define(
            &Symbol("not".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "not".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::not),
            })),
        );
    }

    /// Load string functions (str, string-length, substring)
    fn load_string_functions(env: &mut Environment) {
        env.define(
            &Symbol("str".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "str".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::str),
            })),
        );

        env.define(
            &Symbol("string-length".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string-length".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::string_length),
            })),
        );

        env.define(
            &Symbol("substring".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "substring".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::substring),
            })),
        );

        env.define(
            &Symbol("string-contains".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string-contains".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::string_contains),
            })),
        );
    }

    /// Load collection functions (map, reduce, filter, etc.)
    fn load_collection_functions(env: &mut Environment) {
        // Map function - now supports user-defined functions with evaluator context
        env.define(
            &Symbol("map".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "map".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::map_with_context),
            })),
        );

        // Filter function - now supports user-defined functions with evaluator context
        env.define(
            &Symbol("filter".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "filter".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::filter_with_context),
            })),
        );

        // Reduce function - now supports user-defined functions with evaluator context
        env.define(
            &Symbol("reduce".to_string()),
            Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
                name: "reduce".to_string(),
                arity: Arity::Range(2, 3),
                func: Rc::new(Self::reduce_with_context),
            })),
        );

        // Empty predicate
        env.define(
            &Symbol("empty?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "empty?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::empty_p),
            })),
        );

        // Cons function
        env.define(
            &Symbol("cons".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "cons".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::cons),
            })),
        );

        // First function
        env.define(
            &Symbol("first".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "first".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::first),
            })),
        );

        // Rest function
        env.define(
            &Symbol("rest".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "rest".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::rest),
            })),
        );

        // Get-in function
        env.define(
            &Symbol("get-in".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "get-in".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::get_in),
            })),
        );

        // Partition function
        env.define(
            &Symbol("partition".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "partition".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::partition),
            })),
        );

        // Conj function
        env.define(
            &Symbol("conj".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "conj".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::conj),
            })),
        );

        // Get function
        env.define(
            &Symbol("get".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "get".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::get),
            })),
        );

        // Assoc function
        env.define(
            &Symbol("assoc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "assoc".to_string(),
                arity: Arity::Variadic(3),
                func: Rc::new(Self::assoc),
            })),
        );

        // Dissoc function
        env.define(
            &Symbol("dissoc".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "dissoc".to_string(),
                arity: Arity::Variadic(2),
                func: Rc::new(Self::dissoc),
            })),
        );

        // Count function
        env.define(
            &Symbol("count".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "count".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::count),
            })),
        );

        // Vector function
        env.define(
            &Symbol("vector".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "vector".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::vector),
            })),
        );

        // Hash-map function
        env.define(
            &Symbol("hash-map".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "hash-map".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::hash_map),
            })),
        );

        env.define(
            &Symbol("range".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "range".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::range),
            })),
        );
    }

    fn range(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "range".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let start = match &args[0] {
            Value::Integer(i) => *i,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "Integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "range".to_string(),
                })
            }
        };
        let end = match &args[1] {
            Value::Integer(i) => *i,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "Integer".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "range".to_string(),
                })
            }
        };
        if end < start {
            return Ok(Value::Vector(vec![]));
        }
        let vec = (start..end).map(Value::Integer).collect();
        Ok(Value::Vector(vec))
    }

    /// Load type predicate functions (int?, float?, string?, etc.)
    fn load_type_predicate_functions(env: &mut Environment) {
        env.define(
            &Symbol("int?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "int?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::int_p),
            })),
        );

        env.define(
            &Symbol("float?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "float?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::float_p),
            })),
        );

        env.define(
            &Symbol("number?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "number?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::number_p),
            })),
        );

        env.define(
            &Symbol("string?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::string_p),
            })),
        );

        env.define(
            &Symbol("string-p".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "string-p".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::string_p),
            })),
        );

        env.define(
            &Symbol("bool?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "bool?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::nil_p),
            })),
        );

        env.define(
            &Symbol("nil?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "nil?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::nil_p),
            })),
        );

        env.define(
            &Symbol("map?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "map?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::map_p),
            })),
        );

        env.define(
            &Symbol("vector?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "vector?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::vector_p),
            })),
        );

        env.define(
            &Symbol("keyword?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "keyword?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::keyword_p),
            })),
        );

        env.define(
            &Symbol("symbol?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "symbol?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::symbol_p),
            })),
        );

        env.define(
            &Symbol("fn?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "fn?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::fn_p),
            })),
        );
    }

    /// Load tool interface functions (placeholder implementations)
    fn load_tool_functions(env: &mut Environment) {
        // For now, we'll create placeholder implementations
        // These would need to be implemented with actual I/O, networking, etc.
        env.define(
            &Symbol("tool:log".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:log".to_string(),
                arity: Arity::Variadic(0), // Changed from Exact(1) to Any to match implementation
                func: Rc::new(Self::tool_log),
            })),
        );

        env.define(
            &Symbol("tool:print".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:print".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::tool_print),
            })),
        );

        env.define(
            &Symbol("tool:current-time".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:current-time".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::tool_current_time),
            })),
        );

        env.define(
            &Symbol("tool:parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_parse_json),
            })),
        );

        env.define(
            &Symbol("tool:serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_serialize_json),
            })),
        );

        // Enhanced tool functions for resource management
        env.define(
            &Symbol("tool:open-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:open-file".to_string(),
                arity: Arity::Range(1, 3),
                func: Rc::new(Self::tool_open_file),
            })),
        );

        env.define(
            &Symbol("tool:read-line".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:read-line".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_read_line),
            })),
        );

        env.define(
            &Symbol("tool:write-line".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:write-line".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::tool_write_line),
            })),
        );

        env.define(
            &Symbol("tool:close-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:close-file".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_close_file),
            })),
        );

        env.define(
            &Symbol("tool:get-env".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:get-env".to_string(),
                arity: Arity::Range(1, 2),
                func: Rc::new(Self::tool_get_env),
            })),
        );

        env.define(
            &Symbol("tool:http-fetch".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:http-fetch".to_string(),
                arity: Arity::Range(1, 4),
                func: Rc::new(Self::tool_http_fetch),
            })),
        );
        env.define(
            &Symbol("tool:file-exists?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:file-exists?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_file_exists_p),
            })),
        );

        // Add convenience aliases without prefixes
        env.define(
            &Symbol("log".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "log".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::tool_log),
            })),
        );

        env.define(
            &Symbol("current-time".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "current-time".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::tool_current_time),
            })),
        );

        env.define(
            &Symbol("parse-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "parse-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_parse_json),
            })),
        );

        env.define(
            &Symbol("serialize-json".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "serialize-json".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_serialize_json),
            })),
        );

        env.define(
            &Symbol("get-env".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "get-env".to_string(),
                arity: Arity::Fixed(1), // Changed from Range(1, 2) to match implementation
                func: Rc::new(Self::tool_get_env),
            })),
        );

        env.define(
            &Symbol("file-exists?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "file-exists?".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_file_exists_p),
            })),
        );

        env.define(
            &Symbol("print".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "print".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::tool_print),
            })),
        );

        env.define(
            &Symbol("println".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "println".to_string(),
                arity: Arity::Variadic(0),
                func: Rc::new(Self::println),
            })),
        );

        // Add convenience aliases for file operations
        env.define(
            &Symbol("open-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "open-file".to_string(),
                arity: Arity::Range(1, 3),
                func: Rc::new(Self::tool_open_file),
            })),
        );

        env.define(
            &Symbol("read-line".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "read-line".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_read_line),
            })),
        );

        env.define(
            &Symbol("write-line".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "write-line".to_string(),
                arity: Arity::Fixed(2),
                func: Rc::new(Self::tool_write_line),
            })),
        );

        env.define(
            &Symbol("close-file".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "close-file".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::tool_close_file),
            })),
        );

        env.define(
            &Symbol("http-fetch".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "http-fetch".to_string(),
                arity: Arity::Range(1, 4),
                func: Rc::new(Self::tool_http_fetch),
            })),
        );
    }

    /// Load agent system functions
    fn load_agent_functions(env: &mut Environment) {
        // Agent discovery function
        env.define(
            &Symbol("discover-agents".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "discover-agents".to_string(),
                arity: Arity::Fixed(0), // Changed to match implementation
                func: Rc::new(Self::discover_agents),
            })),
        ); // Task coordination function
        env.define(
            &Symbol("task-coordination".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "task-coordination".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::task_coordination),
            })),
        );
        // Mathematical functions
        env.define(
            &Symbol("fact".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "fact".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::factorial),
            })),
        );

        // Add factorial alias for convenience
        env.define(
            &Symbol("factorial".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "factorial".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::factorial),
            })),
        );

        env.define(
            &Symbol("max".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "max".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::max_value),
            })),
        );

        env.define(
            &Symbol("min".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "min".to_string(),
                arity: Arity::Variadic(1),
                func: Rc::new(Self::min_value),
            })),
        );

        env.define(
            &Symbol("length".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "length".to_string(),
                arity: Arity::Fixed(1),
                func: Rc::new(Self::length_value),
            })),
        );

        // Advanced agent system functions
        env.define(
            &Symbol("discover-and-assess-agents".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "discover-and-assess-agents".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::discover_and_assess_agents),
            })),
        );

        env.define(
            &Symbol("establish-system-baseline".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "establish-system-baseline".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::establish_system_baseline),
            })),
        );

        // Tool functions for agent coordination
        env.define(
            &Symbol("tool:current-timestamp-ms".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "tool:current-timestamp-ms".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::current_timestamp_ms),
            })),
        );

        // Add alias without prefix for convenience
        env.define(
            &Symbol("current-timestamp-ms".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "current-timestamp-ms".to_string(),
                arity: Arity::Fixed(0),
                func: Rc::new(Self::current_timestamp_ms),
            })),
        );
    }

    /// Load task-related functions
    // fn load_task_functions(env: &mut Environment) {
    //     env.define(&Symbol("rtfs.task/current".to_string()), Value::Function(Function::BuiltinWithEvaluator(BuiltinFunctionWithEvaluator {
    //         name: "rtfs.task/current".to_string(),
    //         arity: Arity::Fixed(0),
    //         func: Rc::new(Self::task_current_with_evaluator),
    //     })));
    // }

    // Helper for converting Value to MapKey
    fn value_to_map_key(value: &Value) -> RuntimeResult<MapKey> {
        match value {
            Value::String(s) => Ok(MapKey::String(s.clone())),
            Value::Keyword(k) => Ok(MapKey::Keyword(k.clone())),
            _ => Err(RuntimeError::TypeError {
                expected: "string or keyword".to_string(),
                actual: value.type_name().to_string(),
                operation: "map key".to_string(),
            }),
        }
    }

    // All other function implementations that were missing
    fn max_value(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Err(RuntimeError::new("max requires at least one argument"));
        }
        let mut max_val = args[0].clone();
        for val in &args[1..] {
            let is_greater = match (val, &max_val) {
                (Value::Integer(a), Value::Integer(b)) => a > b,
                (Value::Float(a), Value::Float(b)) => a > b,
                (Value::Integer(a), Value::Float(b)) => *a as f64 > *b,
                (Value::Float(a), Value::Integer(b)) => *a > *b as f64,
                _ => return Err(RuntimeError::new("max can only compare numbers")),
            };
            if is_greater {
                max_val = val.clone();
            }
        }
        Ok(max_val)
    }

    fn min_value(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Err(RuntimeError::new("min requires at least one argument"));
        }
        let mut min_val = args[0].clone();
        for val in &args[1..] {
            let is_less = match (val, &min_val) {
                (Value::Integer(a), Value::Integer(b)) => a < b,
                (Value::Float(a), Value::Float(b)) => a < b,
                (Value::Integer(a), Value::Float(b)) => (*a as f64) < *b,
                (Value::Float(a), Value::Integer(b)) => *a < (*b as f64),
                _ => return Err(RuntimeError::new("min can only compare numbers")),
            };
            if is_less {
                min_val = val.clone();
            }
        }
        Ok(min_val)
    }

    fn conj(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "conj".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }
        let mut collection = args[0].clone();
        match &mut collection {
            Value::Vector(vec) => {
                for item in &args[1..] {
                    vec.push(item.clone());
                }
                Ok(collection)
            }
            Value::Map(map) => {
                if args.len() != 3 {
                    return Err(RuntimeError::ArityMismatch {
                        function: "conj".to_string(),
                        expected: "3 (for map)".to_string(),
                        actual: args.len(),
                    });
                }
                let key = Self::value_to_map_key(&args[1])?;
                map.insert(key, args[2].clone());
                Ok(collection)
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector or map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "conj".to_string(),
            }),
        }
    }
    // TODO: These functions are disabled as they require evaluator support
    // fn map_with_evaluator(args: &[Expression], evaluator: &crate::runtime::evaluator::Evaluator, env: &mut Environment) -> RuntimeResult<Value> {
    //     // Implementation commented out
    //     Err(RuntimeError::new("map function not implemented"))
    // }

    // fn filter_with_evaluator(args: &[Expression], evaluator: &crate::runtime::evaluator::Evaluator, env: &mut Environment) -> RuntimeResult<Value> {
    //     // Implementation commented out
    //     Err(RuntimeError::new("filter function not implemented"))
    // }
    fn reduce(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::new("reduce requires 2 or 3 arguments"));
        }

        let function = &args[0];

        let collection_arg_index = args.len() - 1;
        let collection_val = &args[collection_arg_index];

        let collection = match collection_val {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::new(
                    "reduce expects a vector as its last argument",
                ))
            }
        };

        if collection.is_empty() {
            return if args.len() == 3 {
                Ok(args[1].clone()) // initial value
            } else {
                Err(RuntimeError::new(
                    "reduce on empty collection with no initial value",
                ))
            };
        }

        let (mut accumulator, rest) = if args.len() == 3 {
            (args[1].clone(), collection.as_slice())
        } else {
            (collection[0].clone(), &collection[1..])
        };

        let func_ptr = match function {
            Value::Function(Function::Builtin(builtin_func)) => builtin_func.func.clone(),
            _ => return Err(RuntimeError::new("reduce requires a builtin function")),
        };

        for value in rest {
            let func_args = vec![accumulator.clone(), value.clone()];
            accumulator = func_ptr(func_args)?;
        }

        Ok(accumulator)
    }

    fn empty_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "empty?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Vector(v) => Ok(Value::Boolean(v.is_empty())),
            Value::Map(m) => Ok(Value::Boolean(m.is_empty())),
            Value::String(s) => Ok(Value::Boolean(s.is_empty())),
            Value::Nil => Ok(Value::Boolean(true)),
            _ => Ok(Value::Boolean(false)),
        }
    }

    fn cons(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "cons".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        match &args[1] {
            Value::Vector(v) => {
                let mut new_vec = vec![args[0].clone()];
                new_vec.extend_from_slice(v);
                Ok(Value::Vector(new_vec))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[1].type_name().to_string(),
                operation: "cons".to_string(),
            }),
        }
    }

    fn first(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "first".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Vector(v) => Ok(v.first().cloned().unwrap_or(Value::Nil)),
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "first".to_string(),
            }),
        }
    }

    fn rest(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "rest".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        match &args[0] {
            Value::Vector(v) => {
                if v.is_empty() {
                    Ok(Value::Vector(vec![]))
                } else {
                    Ok(Value::Vector(v[1..].to_vec()))
                }
            }
            _ => Err(RuntimeError::TypeError {
                expected: "vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "rest".to_string(),
            }),
        }
    }

    fn get_in(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if !(2..=3).contains(&args.len()) {
            return Err(RuntimeError::ArityMismatch {
                function: "get-in".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let collection = &args[0];
        let path = &args[1];
        let default = if args.len() == 3 {
            Some(args[2].clone())
        } else {
            None
        };

        let path_vec = match path {
            Value::Vector(v) => v,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: path.type_name().to_string(),
                    operation: "get-in path".to_string(),
                })
            }
        };

        let mut current = collection.clone();
        for key in path_vec {
            match (&current, key) {
                (Value::Map(m), Value::Keyword(k)) => {
                    if let Some(val) = m.get(&MapKey::Keyword(k.clone())) {
                        current = val.clone();
                    } else {
                        return Ok(default.unwrap_or(Value::Nil));
                    }
                }
                (Value::Map(m), Value::String(s)) => {
                    if let Some(val) = m.get(&MapKey::String(s.clone())) {
                        current = val.clone();
                    } else {
                        return Ok(default.unwrap_or(Value::Nil));
                    }
                }
                (Value::Vector(v), Value::Integer(i)) => {
                    if let Some(val) = v.get(*i as usize) {
                        current = val.clone();
                    } else {
                        return Ok(default.unwrap_or(Value::Nil));
                    }
                }
                _ => return Ok(default.unwrap_or(Value::Nil)),
            }
        }

        Ok(current)
    }

    fn partition(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "partition".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let size = match &args[0] {
            Value::Integer(i) if *i > 0 => *i as usize,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "positive integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "partition size".to_string(),
                })
            }
        };

        let collection = match &args[1] {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "partition collection".to_string(),
                })
            }
        };

        let mut result = Vec::new();
        for chunk in collection.chunks(size) {
            result.push(Value::Vector(chunk.to_vec()));
        }
        Ok(Value::Vector(result))
    }

    fn map_with_context(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let function = &args[0];
        let collection = &args[1];
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "map".to_string(),
                })
            }
        };
        let mut result = Vec::new();
        for item in collection_vec {
            match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    // Fast path for builtin functions
                    let func_args = vec![item];
                    let mapped_value = (builtin_func.func)(func_args)?;
                    result.push(mapped_value);
                }
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    // Handle builtin functions with context
                    let func_args = vec![item];
                    let mapped_value = (builtin_func.func)(func_args, evaluator, env)?;
                    result.push(mapped_value);
                }
                Value::Function(Function::Closure(closure)) => {
                    // Handle user-defined functions with full evaluator access
                    let mut func_env = Environment::with_parent(closure.env.clone());
                    func_env.define(&closure.params[0], item);
                    let mapped_value = evaluator.eval_expr(&closure.body, &mut func_env)?;
                    result.push(mapped_value);
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "map".to_string(),
                    });
                }
            }
        }
        Ok(Value::Vector(result))
    }

    fn map(args: Vec<Value>) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let function = &args[0];
        let collection = &args[1];
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "map".to_string(),
                })
            }
        };
        let mut result = Vec::new();
        for item in collection_vec {
            match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    let func_args = vec![item];
                    let mapped_value = (builtin_func.func)(func_args)?;
                    result.push(mapped_value);
                }
                // For now, only support builtin functions in stdlib map
                // User-defined functions will be handled by the evaluator
                _ => {
                    return Err(RuntimeError::NotImplemented(
                        "map: user-defined functions not yet supported in stdlib".to_string(),
                    ));
                }
            }
        }
        Ok(Value::Vector(result))
    }

    // Arithmetic functions
    fn add(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Ok(Value::Integer(0));
        }

        let mut result_int: Option<i64> = None;
        let mut result_float: Option<f64> = None;

        for arg in args {
            match arg {
                Value::Integer(n) => {
                    if let Some(float_acc) = result_float {
                        result_float = Some(float_acc + *n as f64);
                    } else if let Some(int_acc) = result_int {
                        result_int = Some(int_acc + n);
                    } else {
                        result_int = Some(*n);
                    }
                }
                Value::Float(f) => {
                    let current = result_float.unwrap_or(result_int.unwrap_or(0) as f64);
                    result_float = Some(current + f);
                    result_int = None;
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: arg.type_name().to_string(),
                        operation: "+".to_string(),
                    })
                }
            }
        }

        if let Some(f) = result_float {
            Ok(Value::Float(f))
        } else {
            Ok(Value::Integer(result_int.unwrap_or(0)))
        }
    }

    fn subtract(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "-".to_string(),
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        if args.len() == 1 {
            // Negation
            match &args[0] {
                Value::Integer(n) => Ok(Value::Integer(-n)),
                Value::Float(f) => Ok(Value::Float(-f)),
                _ => Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "-".to_string(),
                }),
            }
        } else {
            // Subtraction
            let mut result = match &args[0] {
                Value::Integer(n) => (*n as f64, false),
                Value::Float(f) => (*f, true),
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: args[0].type_name().to_string(),
                        operation: "-".to_string(),
                    })
                }
            };

            for arg in &args[1..] {
                match arg {
                    Value::Integer(n) => {
                        result.0 -= *n as f64;
                    }
                    Value::Float(f) => {
                        result.0 -= f;
                        result.1 = true;
                    }
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "number".to_string(),
                            actual: arg.type_name().to_string(),
                            operation: "-".to_string(),
                        })
                    }
                }
            }

            if result.1 {
                Ok(Value::Float(result.0))
            } else {
                Ok(Value::Integer(result.0 as i64))
            }
        }
    }

    fn multiply(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Ok(Value::Integer(1));
        }

        let mut result_int: Option<i64> = None;
        let mut result_float: Option<f64> = None;

        for arg in args {
            match arg {
                Value::Integer(n) => {
                    if let Some(float_acc) = result_float {
                        result_float = Some(float_acc * *n as f64);
                    } else if let Some(int_acc) = result_int {
                        result_int = Some(int_acc * n);
                    } else {
                        result_int = Some(*n);
                    }
                }
                Value::Float(f) => {
                    let current = result_float.unwrap_or(result_int.unwrap_or(1) as f64);
                    result_float = Some(current * f);
                    result_int = None;
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: arg.type_name().to_string(),
                        operation: "*".to_string(),
                    })
                }
            }
        }

        if let Some(f) = result_float {
            Ok(Value::Float(f))
        } else {
            Ok(Value::Integer(result_int.unwrap_or(1)))
        }
    }

    fn divide(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "/".to_string(),
                expected: "at least 1".to_string(),
                actual: 0,
            });
        }

        let mut result = match &args[0] {
            Value::Integer(n) => *n as f64,
            Value::Float(f) => *f,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "number".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "/".to_string(),
                })
            }
        };

        for arg in &args[1..] {
            let divisor = match arg {
                Value::Integer(n) => *n as f64,
                Value::Float(f) => *f,
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "number".to_string(),
                        actual: arg.type_name().to_string(),
                        operation: "/".to_string(),
                    })
                }
            };

            if divisor == 0.0 {
                return Err(RuntimeError::DivisionByZero);
            }

            result /= divisor;
        }

        // Check if the result is a whole number and return integer if so
        if result.fract() == 0.0 {
            Ok(Value::Integer(result as i64))
        } else {
            Ok(Value::Float(result))
        }
    }

    // Comparison functions
    fn equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.is_empty() {
            return Ok(Value::Boolean(true));
        }

        let first = &args[0];
        for arg in &args[1..] {
            if first != arg {
                return Ok(Value::Boolean(false));
            }
        }
        Ok(Value::Boolean(true))
    }

    fn not_equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "!=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(args[0] != args[1]))
    }

    fn greater_than(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: ">".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Self::compare_values(&args[0], &args[1], ">", |a, b| a > b)
    }

    fn less_than(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "<".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Self::compare_values(&args[0], &args[1], "<", |a, b| a < b)
    }

    fn greater_equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: ">=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Self::compare_values(&args[0], &args[1], ">=", |a, b| a >= b)
    }

    fn less_equal(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "<=".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        Self::compare_values(&args[0], &args[1], "<=", |a, b| a <= b)
    }

    fn compare_values(
        a: &Value,
        b: &Value,
        op: &str,
        cmp: fn(f64, f64) -> bool,
    ) -> RuntimeResult<Value> {
        let (a_val, b_val) = match (a, b) {
            (Value::Integer(a), Value::Integer(b)) => (*a as f64, *b as f64),
            (Value::Integer(a), Value::Float(b)) => (*a as f64, *b),
            (Value::Float(a), Value::Integer(b)) => (*a, *b as f64),
            (Value::Float(a), Value::Float(b)) => (*a, *b),
            (Value::String(a), Value::String(b)) => {
                return Ok(Value::Boolean(match op {
                    ">" => a > b,
                    "<" => a < b,
                    ">=" => a >= b,
                    "<=" => a <= b,
                    _ => false,
                }));
            }
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "comparable types".to_string(),
                    actual: format!("{} and {}", a.type_name(), b.type_name()),
                    operation: op.to_string(),
                })
            }
        };

        Ok(Value::Boolean(cmp(a_val, b_val)))
    }

    // Boolean functions
    fn and(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        for arg in args {
            if !arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Boolean(true)))
    }

    fn or(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        for arg in args {
            if arg.is_truthy() {
                return Ok(arg.clone());
            }
        }
        Ok(args.last().cloned().unwrap_or(Value::Nil))
    }

    fn not(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "not".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(!args[0].is_truthy()))
    }

    // String functions
    fn str(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        let result = args
            .iter()
            .map(|v| v.to_string())
            .collect::<Vec<String>>()
            .join("");
        Ok(Value::String(result))
    }

    fn string_length(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "string-length".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::String(s) => Ok(Value::Integer(s.chars().count() as i64)),
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "string-length".to_string(),
            }),
        }
    }

    fn substring(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "substring".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let string = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "substring".to_string(),
                })
            }
        };

        let start = match &args[1] {
            Value::Integer(n) => *n as usize,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "substring".to_string(),
                })
            }
        };

        let end = if args.len() == 3 {
            match &args[2] {
                Value::Integer(n) => Some(*n as usize),
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "integer".to_string(),
                        actual: args[2].type_name().to_string(),
                        operation: "substring".to_string(),
                    })
                }
            }
        } else {
            None
        };

        let chars: Vec<char> = string.chars().collect();
        let slice = if let Some(end) = end {
            chars.get(start..end)
        } else {
            chars.get(start..)
        };

        match slice {
            Some(chars) => Ok(Value::String(chars.iter().collect())),
            None => Err(RuntimeError::IndexOutOfBounds {
                index: start as i64,
                length: chars.len(),
            }),
        }
    }

    // Collection functions (simplified implementations)
    fn get(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "get".to_string(),
                expected: "2 or 3".to_string(),
                actual: args.len(),
            });
        }

        let default = args.get(2).cloned().unwrap_or(Value::Nil);

        match (&args[0], &args[1]) {
            (Value::Map(map), key) => {
                let map_key = Self::value_to_map_key(key)?;
                Ok(map.get(&map_key).cloned().unwrap_or(default))
            }
            (Value::Vector(vec), Value::Integer(index)) => {
                let idx = *index as usize;
                Ok(vec.get(idx).cloned().unwrap_or(default))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "map or vector with appropriate key/index".to_string(),
                actual: format!("{} with {}", args[0].type_name(), args[1].type_name()),
                operation: "get".to_string(),
            }),
        }
    }

    fn count(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "count".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Vector(v) => Ok(Value::Integer(v.len() as i64)),
            Value::Map(m) => Ok(Value::Integer(m.len() as i64)),
            Value::String(s) => Ok(Value::Integer(s.chars().count() as i64)),
            _ => Err(RuntimeError::TypeError {
                expected: "vector, map, or string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "count".to_string(),
            }),
        }
    }

    fn vector(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        Ok(Value::Vector(args.to_vec()))
    }

    fn hash_map(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() % 2 != 0 {
            return Err(RuntimeError::ArityMismatch {
                function: "map".to_string(),
                expected: "even number of arguments".to_string(),
                actual: args.len(),
            });
        }

        let mut result = HashMap::new();
        for chunk in args.chunks(2) {
            let key = Self::value_to_map_key(&chunk[0])?;
            let value = chunk[1].clone();
            result.insert(key, value);
        }

        Ok(Value::Map(result))
    }

    // Placeholder implementations for other collection functions
    fn assoc(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "assoc".to_string(),
                expected: "at least 3".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Map(map) => {
                let mut new_map = map.clone();

                // Process key-value pairs
                for chunk in args[1..].chunks(2) {
                    if chunk.len() == 2 {
                        let key = Self::value_to_map_key(&chunk[0])?;
                        let value = chunk[1].clone();
                        new_map.insert(key, value);
                    }
                }

                Ok(Value::Map(new_map))
            }
            Value::Vector(vec) => {
                if args.len() != 3 {
                    return Err(RuntimeError::ArityMismatch {
                        function: "assoc".to_string(),
                        expected: "3 arguments for vector".to_string(),
                        actual: args.len(),
                    });
                }

                let index = match &args[1] {
                    Value::Integer(i) => *i as usize,
                    _ => {
                        return Err(RuntimeError::TypeError {
                            expected: "integer".to_string(),
                            actual: args[1].type_name().to_string(),
                            operation: "assoc".to_string(),
                        })
                    }
                };

                let mut new_vec = vec.clone();
                if index < new_vec.len() {
                    new_vec[index] = args[2].clone();
                    Ok(Value::Vector(new_vec))
                } else {
                    Err(RuntimeError::IndexOutOfBounds {
                        index: index as i64,
                        length: new_vec.len(),
                    })
                }
            }
            _ => Err(RuntimeError::TypeError {
                expected: "map or vector".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "assoc".to_string(),
            }),
        }
    }

    fn dissoc(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "dissoc".to_string(),
                expected: "at least 2".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Map(map) => {
                let mut new_map = map.clone();

                // Remove all specified keys
                for key_val in &args[1..] {
                    let key = Self::value_to_map_key(key_val)?;
                    new_map.remove(&key);
                }

                Ok(Value::Map(new_map))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "dissoc".to_string(),
            }),
        }
    }

    // Type predicate functions
    fn int_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "int?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Integer(_))))
    }

    fn float_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "float?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Float(_))))
    }

    fn number_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "number?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(
            args[0],
            Value::Integer(_) | Value::Float(_)
        )))
    }

    fn string_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "string?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::String(_))))
    }

    fn bool_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "bool?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Boolean(_))))
    }

    fn nil_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "nil?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Nil)))
    }

    fn map_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "map?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Map(_))))
    }

    fn vector_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "vector?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Vector(_))))
    }

    fn keyword_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "keyword?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Keyword(_))))
    }

    fn symbol_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "symbol?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Symbol(_))))
    }

    fn fn_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "fn?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }
        Ok(Value::Boolean(matches!(args[0], Value::Function(_))))
    }

    // Tool functions
    fn tool_log(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        let message = args
            .iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        println!("[LOG] {}", message);
        Ok(Value::Nil)
    }

    fn tool_print(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        let message = args
            .iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        print!("{}", message);
        Ok(Value::Nil)
    }

    fn println(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        let message = args
            .iter()
            .map(|v| format!("{:?}", v))
            .collect::<Vec<_>>()
            .join(" ");
        println!("{}", message);
        Ok(Value::Nil)
    }

    fn tool_current_time(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if !args.is_empty() {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:current-time".to_string(),
                expected: "0".to_string(),
                actual: args.len(),
            });
        }
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Ok(Value::Integer(timestamp as i64))
    }

    fn tool_parse_json(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:parse-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::String(json_str) => match serde_json::from_str::<serde_json::Value>(json_str) {
                Ok(json_value) => Ok(Self::json_value_to_rtfs_value(&json_value)),
                Err(e) => Err(RuntimeError::Generic(format!("JSON parsing error: {}", e))),
            },
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:parse-json".to_string(),
            }),
        }
    }

    fn tool_serialize_json(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:serialize-json".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let json_value = Self::rtfs_value_to_json_value(&args[0])?;
        match serde_json::to_string_pretty(&json_value) {
            Ok(json_str) => Ok(Value::String(json_str)),
            Err(e) => Err(RuntimeError::Generic(format!(
                "JSON serialization error: {}",
                e
            ))),
        }
    }

    /// Convert a serde_json::Value to an RTFS Value
    fn json_value_to_rtfs_value(json_value: &serde_json::Value) -> Value {
        match json_value {
            serde_json::Value::Null => Value::Nil,
            serde_json::Value::Bool(b) => Value::Boolean(*b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    Value::Integer(i)
                } else if let Some(f) = n.as_f64() {
                    Value::Float(f)
                } else {
                    Value::String(n.to_string())
                }
            }
            serde_json::Value::String(s) => {
                // Handle keyword strings (strings starting with ":")
                if s.starts_with(':') {
                    Value::Keyword(Keyword(s[1..].to_string()))
                } else {
                    Value::String(s.clone())
                }
            }
            serde_json::Value::Array(arr) => {
                let values: Vec<Value> = arr.iter().map(Self::json_value_to_rtfs_value).collect();
                Value::Vector(values)
            }
            serde_json::Value::Object(obj) => {
                let mut map = HashMap::new();
                for (key, value) in obj {
                    // Handle keyword keys (keys starting with ":")
                    let map_key = if key.starts_with(':') {
                        MapKey::Keyword(Keyword(key[1..].to_string()))
                    } else {
                        MapKey::String(key.clone())
                    };
                    map.insert(map_key, Self::json_value_to_rtfs_value(value));
                }
                Value::Map(map)
            }
        }
    }

    /// Convert an RTFS Value to a serde_json::Value
    fn rtfs_value_to_json_value(rtfs_value: &Value) -> RuntimeResult<serde_json::Value> {
        match rtfs_value {
            Value::Nil => Ok(serde_json::Value::Null),
            Value::Boolean(b) => Ok(serde_json::Value::Bool(*b)),
            Value::Integer(i) => Ok(serde_json::Value::Number(serde_json::Number::from(*i))),
            Value::Float(f) => serde_json::Number::from_f64(*f)
                .map(serde_json::Value::Number)
                .ok_or_else(|| RuntimeError::Generic("Invalid float value".to_string())),
            Value::String(s) => Ok(serde_json::Value::String(s.clone())),
            Value::Vector(vec) => {
                let json_array: Result<Vec<serde_json::Value>, RuntimeError> =
                    vec.iter().map(Self::rtfs_value_to_json_value).collect();
                Ok(serde_json::Value::Array(json_array?))
            }
            Value::Map(map) => {
                let mut json_obj = serde_json::Map::new();
                for (key, value) in map {
                    let key_str = match key {
                        MapKey::String(s) => s.clone(),
                        MapKey::Keyword(k) => format!(":{}", k.0),
                        MapKey::Integer(i) => i.to_string(),
                    };
                    json_obj.insert(key_str, Self::rtfs_value_to_json_value(value)?);
                }
                Ok(serde_json::Value::Object(json_obj))
            }
            Value::Keyword(k) => Ok(serde_json::Value::String(format!(":{}", k.0))),
            Value::Symbol(s) => Ok(serde_json::Value::String(format!("{}", s.0))),
            Value::Timestamp(ts) => Ok(serde_json::Value::String(format!("@{}", ts))),
            Value::Uuid(uuid) => Ok(serde_json::Value::String(format!("@{}", uuid))),
            Value::ResourceHandle(handle) => Ok(serde_json::Value::String(format!("@{}", handle))),
            Value::Function(_) => Err(RuntimeError::Generic(
                "Cannot serialize functions to JSON".to_string(),
            )),
            Value::FunctionPlaceholder(_) => Err(RuntimeError::Generic(
                "Cannot serialize function placeholders to JSON".to_string(),
            )),
            Value::Error(e) => Err(RuntimeError::Generic(format!(
                "Cannot serialize errors to JSON: {}",
                e.message
            ))),
            Value::List(_) => Err(RuntimeError::Generic(
                "Cannot serialize lists to JSON (use vectors instead)".to_string(),
            )),
        }
    }

    fn tool_open_file(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 1 || args.len() > 3 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:open-file".to_string(),
                expected: "1-3".to_string(),
                actual: args.len(),
            });
        }

        let path = match &args[0] {
            Value::String(p) => p,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool:open-file".to_string(),
                });
            }
        };

        let mode = if args.len() > 1 {
            match &args[1] {
                Value::String(m) => m.as_str(),
                Value::Keyword(k) => &k.0,
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "string or keyword".to_string(),
                        actual: args[1].type_name().to_string(),
                        operation: "tool:open-file".to_string(),
                    });
                }
            }
        } else {
            "r" // Default to read mode
        };

        let mut open_options = OpenOptions::new();
        match mode {
            "r" | "read" => {
                open_options.read(true);
            }
            "w" | "write" => {
                open_options.write(true).create(true).truncate(true);
            }
            "a" | "append" => {
                open_options.write(true).create(true).append(true);
            }
            "r+" | "read-write" => {
                open_options.read(true).write(true).create(true);
            }
            _ => {
                return Err(RuntimeError::InvalidArgument(format!(
                    "Invalid file mode: {}. Use 'r', 'w', 'a', or 'r+'",
                    mode
                )));
            }
        }

        match open_options.open(path) {
            Ok(file) => {
                let handle_id = get_next_file_handle_id();
                let file_handle = FileHandle::new(file, path.clone());
                get_file_handles()
                    .borrow_mut()
                    .insert(handle_id, file_handle);
                Ok(Value::Integer(handle_id))
            }
            Err(e) => Err(RuntimeError::Generic(format!(
                "Failed to open file '{}': {}",
                path, e
            ))),
        }
    }

    fn tool_read_line(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:read-line".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let handle_id = match &args[0] {
            Value::Integer(id) => *id,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool:read-line".to_string(),
                });
            }
        };

        let mut handles = get_file_handles().borrow_mut();
        let file_handle = handles
            .get_mut(&handle_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Invalid file handle: {}", handle_id)))?;

        let reader = file_handle.get_reader();
        let mut line = String::new();
        match reader.read_line(&mut line) {
            Ok(0) => Ok(Value::Nil), // EOF
            Ok(_) => {
                // Remove trailing newline
                if line.ends_with('\n') {
                    line.pop();
                    if line.ends_with('\r') {
                        line.pop();
                    }
                }
                Ok(Value::String(line))
            }
            Err(e) => Err(RuntimeError::Generic(format!(
                "Error reading from file: {}",
                e
            ))),
        }
    }

    fn tool_write_line(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:write-line".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let handle_id = match &args[0] {
            Value::Integer(id) => *id,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool:write-line".to_string(),
                });
            }
        };

        let content = match &args[1] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "tool:write-line".to_string(),
                });
            }
        };

        let mut handles = get_file_handles().borrow_mut();
        let file_handle = handles
            .get_mut(&handle_id)
            .ok_or_else(|| RuntimeError::Generic(format!("Invalid file handle: {}", handle_id)))?;

        let mut line = content.clone();
        line.push('\n'); // Add newline

        match file_handle.file.write_all(line.as_bytes()) {
            Ok(_) => Ok(Value::Boolean(true)),
            Err(e) => Err(RuntimeError::Generic(format!(
                "Error writing to file: {}",
                e
            ))),
        }
    }

    fn tool_close_file(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:close-file".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        let handle_id = match &args[0] {
            Value::Integer(id) => *id,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "integer".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool:close-file".to_string(),
                });
            }
        };

        let mut handles = get_file_handles().borrow_mut();
        if handles.remove(&handle_id).is_some() {
            Ok(Value::Boolean(true))
        } else {
            Err(RuntimeError::Generic(format!(
                "Invalid file handle: {}",
                handle_id
            )))
        }
    }

    fn tool_get_env(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:get-env".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::String(key) => match std::env::var(key) {
                Ok(value) => Ok(Value::String(value)),
                Err(_) => Ok(Value::Nil),
            },
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:get-env".to_string(),
            }),
        }
    }

    fn tool_http_fetch(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() < 1 || args.len() > 4 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:http-fetch".to_string(),
                expected: "1-4".to_string(),
                actual: args.len(),
            });
        }

        let url = match &args[0] {
            Value::String(u) => u,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "tool:http-fetch".to_string(),
                });
            }
        };

        let method = if args.len() > 1 {
            match &args[1] {
                Value::String(m) => m.to_uppercase(),
                Value::Keyword(k) => k.0.to_uppercase(),
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "string or keyword".to_string(),
                        actual: args[1].type_name().to_string(),
                        operation: "tool:http-fetch".to_string(),
                    });
                }
            }
        } else {
            "GET".to_string()
        };

        // Parse custom headers (optional 3rd argument)
        let custom_headers = if args.len() > 2 {
            match &args[2] {
                Value::Map(header_map) => Some(header_map.clone()),
                Value::Nil => None,
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "map or nil".to_string(),
                        actual: args[2].type_name().to_string(),
                        operation: "tool:http-fetch headers".to_string(),
                    });
                }
            }
        } else {
            None
        };

        // Parse request body (optional 4th argument)
        let request_body = if args.len() > 3 {
            match &args[3] {
                Value::String(body) => Some(body.clone()),
                Value::Nil => None,
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "string or nil".to_string(),
                        actual: args[3].type_name().to_string(),
                        operation: "tool:http-fetch body".to_string(),
                    });
                }
            }
        } else {
            None
        };

        // Use a blocking HTTP client for simplicity
        let client = reqwest::blocking::Client::new();

        let mut request = match method.as_str() {
            "GET" => client.get(url),
            "POST" => client.post(url),
            "PUT" => client.put(url),
            "DELETE" => client.delete(url),
            "PATCH" => client.patch(url),
            "HEAD" => client.head(url),
            _ => {
                return Err(RuntimeError::InvalidArgument(format!(
                    "Unsupported HTTP method: {}. Use GET, POST, PUT, DELETE, PATCH, or HEAD",
                    method
                )));
            }
        };

        // Add custom headers if provided
        if let Some(headers) = custom_headers {
            for (key, value) in headers {
                let header_name = match key {
                    MapKey::String(s) => s,
                    MapKey::Keyword(k) => k.0.clone(),
                    _ => continue, // Skip non-string/keyword keys
                };

                let header_value = match value {
                    Value::String(s) => s,
                    Value::Integer(i) => i.to_string(),
                    Value::Float(f) => f.to_string(),
                    Value::Boolean(b) => b.to_string(),
                    _ => continue, // Skip unsupported value types
                };

                request = request.header(header_name, header_value);
            }
        }

        // Add request body if provided
        if let Some(body) = request_body {
            request = request.body(body);
        }

        match request.send() {
            Ok(response) => {
                let status = response.status().as_u16();
                let headers = response.headers();

                // Convert headers to a map
                let mut header_map = HashMap::new();
                for (key, value) in headers {
                    let key_str = key.as_str();
                    if let Ok(value_str) = value.to_str() {
                        header_map.insert(
                            MapKey::String(key_str.to_string()),
                            Value::String(value_str.to_string()),
                        );
                    }
                }

                // Try to get the response body as text
                let body = match response.text() {
                    Ok(text) => Value::String(text),
                    Err(_) => Value::String("".to_string()),
                };

                // Create response map
                let mut response_map = HashMap::new();
                response_map.insert(
                    MapKey::Keyword(Keyword("status".to_string())),
                    Value::Integer(status as i64),
                );
                response_map.insert(
                    MapKey::Keyword(Keyword("headers".to_string())),
                    Value::Map(header_map),
                );
                response_map.insert(MapKey::Keyword(Keyword("body".to_string())), body);

                Ok(Value::Map(response_map))
            }
            Err(e) => Err(RuntimeError::Generic(format!("HTTP request failed: {}", e))),
        }
    }

    fn tool_file_exists_p(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "tool:file-exists?".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::String(path) => Ok(Value::Boolean(std::path::Path::new(path).exists())),
            _ => Err(RuntimeError::TypeError {
                expected: "string".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "tool:file-exists?".to_string(),
            }),
        }
    }

    // Agent functions
    fn discover_agents(_args: Vec<Value>) -> RuntimeResult<Value> {
        // Placeholder - Agent discovery not implemented
        Ok(Value::Vector(vec![]))
    }

    fn task_coordination(_args: Vec<Value>) -> RuntimeResult<Value> {
        // Placeholder - Task coordination not implemented
        Ok(Value::Map(HashMap::new()))
    }

    fn factorial(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "factorial".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Integer(n) => {
                if *n < 0 {
                    return Err(RuntimeError::InvalidArgument(
                        "Factorial is not defined for negative numbers".to_string(),
                    ));
                }
                let mut result = 1i64;
                for i in 1..=*n {
                    result *= i;
                }
                Ok(Value::Integer(result))
            }
            _ => Err(RuntimeError::TypeError {
                expected: "integer".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "factorial".to_string(),
            }),
        }
    }

    fn length_value(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "length".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        match &args[0] {
            Value::Vector(v) => Ok(Value::Integer(v.len() as i64)),
            Value::String(s) => Ok(Value::Integer(s.len() as i64)),
            Value::Map(m) => Ok(Value::Integer(m.len() as i64)),
            _ => Err(RuntimeError::TypeError {
                expected: "vector, string, or map".to_string(),
                actual: args[0].type_name().to_string(),
                operation: "length".to_string(),
            }),
        }
    }

    fn discover_and_assess_agents(_args: Vec<Value>) -> RuntimeResult<Value> {
        // Placeholder - Agent discovery and assessment not implemented
        Ok(Value::Vector(vec![]))
    }

    fn establish_system_baseline(_args: Vec<Value>) -> RuntimeResult<Value> {
        // Placeholder - System baseline establishment not implemented
        Ok(Value::Map(HashMap::new()))
    }

    fn current_timestamp_ms(_args: Vec<Value>) -> RuntimeResult<Value> {
        use std::time::{SystemTime, UNIX_EPOCH};
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis();
        Ok(Value::Integer(timestamp as i64))
    }

    // fn task_current_with_evaluator(
    //     _args: &[Expression],
    //     ir_runtime: &mut IrRuntime,
    //     _env: &mut IrEnvironment
    // ) -> RuntimeResult<Value> {
    //     Ok(ir_runtime.get_task_context().unwrap_or(Value::Nil))
    // }

    fn filter_with_context(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "filter".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }
        let function = &args[0];
        let collection = &args[1];
        let collection_vec = match collection {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "vector".to_string(),
                    actual: collection.type_name().to_string(),
                    operation: "filter".to_string(),
                })
            }
        };
        let mut result = Vec::new();
        for item in collection_vec {
            let keep = match function {
                Value::Function(Function::Builtin(builtin_func)) => {
                    let func_args = vec![item.clone()];
                    let v = (builtin_func.func)(func_args)?;
                    v.is_truthy()
                }
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    let func_args = vec![item.clone()];
                    let v = (builtin_func.func)(func_args, evaluator, env)?;
                    v.is_truthy()
                }
                Value::Function(Function::Closure(closure)) => {
                    let mut func_env = Environment::with_parent(closure.env.clone());
                    func_env.define(&closure.params[0], item.clone());
                    let v = evaluator.eval_expr(&closure.body, &mut func_env)?;
                    v.is_truthy()
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "filter".to_string(),
                    });
                }
            };
            if keep {
                result.push(item);
            }
        }
        Ok(Value::Vector(result))
    }

    fn reduce_with_context(
        args: Vec<Value>,
        evaluator: &Evaluator,
        env: &mut Environment,
    ) -> RuntimeResult<Value> {
        if args.len() < 2 || args.len() > 3 {
            return Err(RuntimeError::new("reduce requires 2 or 3 arguments"));
        }
        let function = &args[0];
        let collection_arg_index = args.len() - 1;
        let collection_val = &args[collection_arg_index];
        let collection = match collection_val {
            Value::Vector(v) => v.clone(),
            _ => {
                return Err(RuntimeError::new(
                    "reduce expects a vector as its last argument",
                ))
            }
        };
        if collection.is_empty() {
            return if args.len() == 3 {
                Ok(args[1].clone()) // initial value
            } else {
                Err(RuntimeError::new(
                    "reduce on empty collection with no initial value",
                ))
            };
        }
        let (mut accumulator, rest) = if args.len() == 3 {
            (args[1].clone(), collection.as_slice())
        } else {
            (collection[0].clone(), &collection[1..])
        };
        for value in rest {
            let func_args = vec![accumulator.clone(), value.clone()];
            accumulator = match function {
                Value::Function(Function::Builtin(builtin_func)) => (builtin_func.func)(func_args)?,
                Value::Function(Function::BuiltinWithContext(builtin_func)) => {
                    (builtin_func.func)(func_args, evaluator, env)?
                }
                Value::Function(Function::Closure(closure)) => {
                    let mut func_env = Environment::with_parent(closure.env.clone());
                    func_env.define(&closure.params[0], accumulator.clone());
                    func_env.define(&closure.params[1], value.clone());
                    evaluator.eval_expr(&closure.body, &mut func_env)?
                }
                _ => {
                    return Err(RuntimeError::TypeError {
                        expected: "function".to_string(),
                        actual: function.type_name().to_string(),
                        operation: "reduce".to_string(),
                    });
                }
            };
        }
        Ok(accumulator)
    }

    fn string_contains(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 2 {
            return Err(RuntimeError::ArityMismatch {
                function: "string-contains".to_string(),
                expected: "2".to_string(),
                actual: args.len(),
            });
        }

        let haystack = match &args[0] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[0].type_name().to_string(),
                    operation: "string-contains".to_string(),
                })
            }
        };

        let needle = match &args[1] {
            Value::String(s) => s,
            _ => {
                return Err(RuntimeError::TypeError {
                    expected: "string".to_string(),
                    actual: args[1].type_name().to_string(),
                    operation: "string-contains".to_string(),
                })
            }
        };

        Ok(Value::Boolean(haystack.contains(needle)))
    }

    fn type_name(args: Vec<Value>) -> RuntimeResult<Value> {
        let args = args.as_slice();
        if args.len() != 1 {
            return Err(RuntimeError::ArityMismatch {
                function: "type-name".to_string(),
                expected: "1".to_string(),
                actual: args.len(),
            });
        }

        Ok(Value::String(args[0].type_name().to_string()))
    }
}

/// Load the standard library into a module registry
/// This creates a "stdlib" module containing all built-in functions
pub fn load_stdlib(module_registry: &mut ModuleRegistry) -> RuntimeResult<()> {
    // Create module metadata
    let metadata = ModuleMetadata {
        name: "stdlib".to_string(),
        docstring: Some("RTFS Standard Library - Built-in functions and tools".to_string()),
        source_file: None,
        version: Some("1.0.0".to_string()),
        compiled_at: std::time::SystemTime::now(),
    };

    // Create module exports by directly creating all stdlib functions
    let mut exports = HashMap::new();

    // Add all stdlib functions directly
    add_stdlib_exports(&mut exports);

    // Create the module
    let module = Module {
        metadata,
        ir_node: IrNode::Do {
            id: 0,
            ir_type: IrType::Any,
            expressions: vec![],
            source_location: None,
        },
        exports: RefCell::new(exports),
        namespace: Rc::new(RefCell::new(IrEnvironment::new())),
        dependencies: vec![],
    };

    // Register the module
    module_registry.register_module(module)?;

    Ok(())
}

/// Helper function to add all stdlib functions as module exports
fn add_stdlib_exports(exports: &mut HashMap<String, ModuleExport>) {
    // Add arithmetic functions
    add_function_export(exports, "+", |args| StandardLibrary::add(args.to_vec()));
    add_function_export(exports, "-", |args| {
        StandardLibrary::subtract(args.to_vec())
    });
    add_function_export(exports, "*", |args| {
        StandardLibrary::multiply(args.to_vec())
    });
    add_function_export(exports, "/", |args| StandardLibrary::divide(args.to_vec()));
    add_function_export(exports, "max", |args| {
        StandardLibrary::max_value(args.to_vec())
    });
    add_function_export(exports, "min", |args| {
        StandardLibrary::min_value(args.to_vec())
    });

    // Add comparison functions
    add_function_export(exports, "=", |args| StandardLibrary::equal(args.to_vec()));
    add_function_export(exports, "!=", |args| {
        StandardLibrary::not_equal(args.to_vec())
    });
    add_function_export(exports, ">", |args| {
        StandardLibrary::greater_than(args.to_vec())
    });
    add_function_export(exports, "<", |args| {
        StandardLibrary::less_than(args.to_vec())
    });
    add_function_export(exports, ">=", |args| {
        StandardLibrary::greater_equal(args.to_vec())
    });
    add_function_export(exports, "<=", |args| {
        StandardLibrary::less_equal(args.to_vec())
    });

    // Add boolean functions
    add_function_export(exports, "and", |args| StandardLibrary::and(args.to_vec()));
    add_function_export(exports, "or", |args| StandardLibrary::or(args.to_vec()));
    add_function_export(exports, "not", |args| StandardLibrary::not(args.to_vec()));

    // Add string functions
    add_function_export(exports, "str", |args| StandardLibrary::str(args.to_vec()));
    add_function_export(exports, "substring", |args| {
        StandardLibrary::substring(args.to_vec())
    });
    add_function_export(exports, "str-length", |args| {
        StandardLibrary::string_length(args.to_vec())
    });
    add_function_export(exports, "string-contains", |args| {
        StandardLibrary::string_contains(args.to_vec())
    });
    add_function_export(exports, "type-name", |args| {
        StandardLibrary::type_name(args.to_vec())
    });

    // Add collection functions
    add_function_export(exports, "count", |args| {
        StandardLibrary::count(args.to_vec())
    });
    add_function_export(exports, "first", |args| {
        StandardLibrary::first(args.to_vec())
    });
    add_function_export(exports, "rest", |args| StandardLibrary::rest(args.to_vec()));
    add_function_export(exports, "cons", |args| StandardLibrary::cons(args.to_vec()));
    add_function_export(exports, "conj", |args| StandardLibrary::conj(args.to_vec()));
    add_function_export(exports, "vector", |args| {
        StandardLibrary::vector(args.to_vec())
    });
    add_function_export(exports, "hash-map", |args| {
        StandardLibrary::hash_map(args.to_vec())
    });
    add_function_export(exports, "reduce", |args| {
        StandardLibrary::reduce(args.to_vec())
    });
    add_function_export(exports, "get", |args| StandardLibrary::get(args.to_vec()));
    add_function_export(exports, "assoc", |args| {
        StandardLibrary::assoc(args.to_vec())
    });
    add_function_export(exports, "dissoc", |args| {
        StandardLibrary::dissoc(args.to_vec())
    });
    add_function_export(exports, "get-in", |args| {
        StandardLibrary::get_in(args.to_vec())
    });
    add_function_export(exports, "partition", |args| {
        StandardLibrary::partition(args.to_vec())
    });
    add_function_export(exports, "map", |args| StandardLibrary::map(args.to_vec()));

    // Add type predicate functions
    add_function_export(exports, "int?", |args| {
        StandardLibrary::int_p(args.to_vec())
    });
    add_function_export(exports, "float?", |args| {
        StandardLibrary::float_p(args.to_vec())
    });
    add_function_export(exports, "number?", |args| {
        StandardLibrary::number_p(args.to_vec())
    });
    add_function_export(exports, "string?", |args| {
        StandardLibrary::string_p(args.to_vec())
    });
    add_function_export(exports, "boolean?", |args| {
        StandardLibrary::bool_p(args.to_vec())
    });
    add_function_export(exports, "nil?", |args| {
        StandardLibrary::nil_p(args.to_vec())
    });
    add_function_export(exports, "map?", |args| {
        StandardLibrary::map_p(args.to_vec())
    });
    add_function_export(exports, "vector?", |args| {
        StandardLibrary::vector_p(args.to_vec())
    });
    add_function_export(exports, "keyword?", |args| {
        StandardLibrary::keyword_p(args.to_vec())
    });
    add_function_export(exports, "symbol?", |args| {
        StandardLibrary::symbol_p(args.to_vec())
    });
    add_function_export(exports, "fn?", |args| StandardLibrary::fn_p(args.to_vec()));
    add_function_export(exports, "empty?", |args| {
        StandardLibrary::empty_p(args.to_vec())
    });

    // Add tool functions
    add_function_export(exports, "tool/log", |args| {
        StandardLibrary::tool_log(args.to_vec())
    });
    add_function_export(exports, "tool:log", |args| {
        StandardLibrary::tool_log(args.to_vec())
    });
    add_function_export(exports, "tool/print", |args| {
        StandardLibrary::tool_print(args.to_vec())
    });
    add_function_export(exports, "tool:print", |args| {
        StandardLibrary::tool_print(args.to_vec())
    });
    add_function_export(exports, "println", |args| {
        StandardLibrary::println(args.to_vec())
    });
    add_function_export(exports, "tool/current-time", |args| {
        StandardLibrary::tool_current_time(args.to_vec())
    });
    add_function_export(exports, "tool:current-time", |args| {
        StandardLibrary::tool_current_time(args.to_vec())
    });
    add_function_export(exports, "tool/parse-json", |args| {
        StandardLibrary::tool_parse_json(args.to_vec())
    });
    add_function_export(exports, "tool:parse-json", |args| {
        StandardLibrary::tool_parse_json(args.to_vec())
    });
    add_function_export(exports, "tool/serialize-json", |args| {
        StandardLibrary::tool_serialize_json(args.to_vec())
    });
    add_function_export(exports, "tool:serialize-json", |args| {
        StandardLibrary::tool_serialize_json(args.to_vec())
    });
    add_function_export(exports, "tool/get-env", |args| {
        StandardLibrary::tool_get_env(args.to_vec())
    });
    add_function_export(exports, "tool:get-env", |args| {
        StandardLibrary::tool_get_env(args.to_vec())
    });
    add_function_export(exports, "tool/file-exists?", |args| {
        StandardLibrary::tool_file_exists_p(args.to_vec())
    });
    add_function_export(exports, "tool:file-exists?", |args| {
        StandardLibrary::tool_file_exists_p(args.to_vec())
    });
    add_function_export(exports, "tool/http-fetch", |args| {
        StandardLibrary::tool_http_fetch(args.to_vec())
    });
    add_function_export(exports, "tool:http-fetch", |args| {
        StandardLibrary::tool_http_fetch(args.to_vec())
    });
    add_function_export(exports, "tool/current-timestamp-ms", |args| {
        StandardLibrary::current_timestamp_ms(args.to_vec())
    });
    add_function_export(exports, "tool:current-timestamp-ms", |args| {
        StandardLibrary::current_timestamp_ms(args.to_vec())
    });
    add_function_export(exports, "tool/open-file", |args| {
        StandardLibrary::tool_open_file(args.to_vec())
    });
    add_function_export(exports, "tool:open-file", |args| {
        StandardLibrary::tool_open_file(args.to_vec())
    });
    add_function_export(exports, "tool/read-line", |args| {
        StandardLibrary::tool_read_line(args.to_vec())
    });
    add_function_export(exports, "tool:read-line", |args| {
        StandardLibrary::tool_read_line(args.to_vec())
    });
    add_function_export(exports, "tool/write-line", |args| {
        StandardLibrary::tool_write_line(args.to_vec())
    });
    add_function_export(exports, "tool:write-line", |args| {
        StandardLibrary::tool_write_line(args.to_vec())
    });
    add_function_export(exports, "tool/close-file", |args| {
        StandardLibrary::tool_close_file(args.to_vec())
    });
    add_function_export(exports, "tool:close-file", |args| {
        StandardLibrary::tool_close_file(args.to_vec())
    });

    // Add agent functions
    add_function_export(exports, "discover-agents", |args| {
        StandardLibrary::discover_agents(args.to_vec())
    });
    add_function_export(exports, "task-coordination", |args| {
        StandardLibrary::task_coordination(args.to_vec())
    });
    add_function_export(exports, "factorial", |args| {
        StandardLibrary::factorial(args.to_vec())
    });
    add_function_export(exports, "length", |args| {
        StandardLibrary::length_value(args.to_vec())
    });
    add_function_export(exports, "discover-and-assess-agents", |args| {
        StandardLibrary::discover_and_assess_agents(args.to_vec())
    });
    add_function_export(exports, "establish-system-baseline", |args| {
        StandardLibrary::establish_system_baseline(args.to_vec())
    });
    add_function_export(exports, "current-timestamp-ms", |args| {
        StandardLibrary::current_timestamp_ms(args.to_vec())
    });
    add_function_export(exports, "range", |args| {
        StandardLibrary::range(args.to_vec())
    });
    add_function_export(exports, "inc", |args| {
        if args.len() != 1 {
            return Err(RuntimeError::Generic(
                "inc expects exactly 1 argument".to_string(),
            ));
        }
        match &args[0] {
            Value::Integer(n) => Ok(Value::Integer(n + 1)),
            Value::Float(f) => Ok(Value::Float(f + 1.0)),
            _ => Err(RuntimeError::Generic("inc expects a number".to_string())),
        }
    });
    add_function_export(exports, "dec", |args| {
        if args.len() != 1 {
            return Err(RuntimeError::Generic(
                "dec expects exactly 1 argument".to_string(),
            ));
        }
        match &args[0] {
            Value::Integer(n) => Ok(Value::Integer(n - 1)),
            Value::Float(f) => Ok(Value::Float(f - 1.0)),
            _ => Err(RuntimeError::Generic("dec expects a number".to_string())),
        }
    });
}

/// Helper function to add a single function export
fn add_function_export<F>(exports: &mut HashMap<String, ModuleExport>, name: &str, func: F)
where
    F: Fn(Vec<Value>) -> RuntimeResult<Value> + 'static,
{
    let arity = match name {
        // Variadic functions
        "+" | "-" | "*" | "/" | "=" | "and" | "or" | "vector" | "hash-map" => Arity::Variadic(1),
        // Fixed arity functions
        "!=" | ">" | "<" | ">=" | "<=" | "cons" | "get" | "assoc" | "substring" | "map" => {
            Arity::Fixed(2)
        }
        "get-in" => Arity::Fixed(2),
        // Most functions are fixed with 1 arg
        _ => Arity::Fixed(1),
    };

    let builtin_func = BuiltinFunction {
        name: name.to_string(),
        arity,
        func: Rc::new(func),
    };

    let export = ModuleExport {
        original_name: name.to_string(),
        export_name: name.to_string(),
        value: Value::Function(Function::Builtin(builtin_func)),
        ir_type: IrType::Any,
        export_type: ExportType::Function,
    };

    exports.insert(name.to_string(), export);
}
