use std::sync::Arc;

use rtfs::ast::{Symbol, TypeExpr};
use rtfs::runtime::environment::Environment;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::{Arity, BuiltinFunction, Function, Value};

/// A first-class function representing a CCOS capability.
///
/// It wraps a regular RTFS builtin/closure but guarantees that a `logger`
/// callback is invoked *before* the inner function so that the CCOS
/// runtime can append a `CapabilityCall` `Action` to the Causal-Chain.
#[derive(Clone)]
pub struct Capability {
    /// Stable capability identifier (e.g. "ccos.ask-human")
    pub id: String,
    /// Arity information for the callable
    pub arity: Arity,
    /// Actual implementation
    pub func: Arc<dyn Fn(Vec<Value>) -> RuntimeResult<Value> + Send + Sync>,
    /// Human-readable description of the capability
    pub description: Option<String>,
    /// Schema describing the expected input
    pub input_schema: Option<TypeExpr>,
    /// Schema describing the expected output
    pub output_schema: Option<TypeExpr>,
}

impl Capability {
    /// Helper constructor for Capability (backwards-compatible, no schemas)
    pub fn new(
        id: String,
        arity: Arity,
        func: Arc<dyn Fn(Vec<Value>) -> RuntimeResult<Value> + Send + Sync>,
    ) -> Self {
        Self {
            id,
            arity,
            func,
            description: None,
            input_schema: None,
            output_schema: None,
        }
    }

    /// Constructor with full metadata including schemas
    pub fn with_metadata(
        id: String,
        arity: Arity,
        func: Arc<dyn Fn(Vec<Value>) -> RuntimeResult<Value> + Send + Sync>,
        description: Option<String>,
        input_schema: Option<TypeExpr>,
        output_schema: Option<TypeExpr>,
    ) -> Self {
        Self {
            id,
            arity,
            func,
            description,
            input_schema,
            output_schema,
        }
    }
    /// Convert this capability into a regular RTFS `Value::Function` that
    /// records provenance via the given logger.
    ///
    /// The `logger` receives `(capability_id, args)` and should append an
    /// `Action` to the Causal-Chain or perform any other side-effect. If the
    /// logger returns an error the underlying capability will **not** be
    /// executed.
    pub fn into_value<L>(self, logger: L) -> Value
    where
        L: Fn(&str, &Vec<Value>) -> RuntimeResult<()> + 'static + Send + Sync,
    {
        let id_clone = self.id.clone();
        let inner = self.func.clone();
        let wrapped = move |args: Vec<Value>| {
            // First call the logger – this can mutate global state / ledger
            logger(&id_clone, &args)?;
            // Then run the actual capability implementation
            (inner)(args)
        };

        Value::Function(Function::Builtin(BuiltinFunction {
            name: self.id,
            arity: self.arity,
            func: Arc::new(wrapped),
        }))
    }
}

/// Utility that looks up a symbol in the given environment. If found and it is a
/// `Value::Function`, replaces it with a capability‐wrapped version using the
/// provided metadata and logger.
pub fn inject_capability<L>(
    env: &mut Environment,
    symbol_name: &str,
    capability_id: &str,
    arity: Arity,
    logger: L,
) -> RuntimeResult<()>
where
    L: Fn(&str, &Vec<Value>) -> RuntimeResult<()> + 'static + Send + Sync,
{
    let sym = Symbol(symbol_name.to_string());
    if let Some(orig_val) = env.lookup(&sym) {
        if let Value::Function(func) = orig_val {
            let cap = Capability {
                id: capability_id.to_string(),
                arity,
                func: match func {
                    Function::Builtin(b) => b.func.clone(),
                    _ => Arc::new(move |_args| {
                        Err(RuntimeError::Generic(
                            "Only builtin functions can be wrapped as capabilities".to_string(),
                        ))
                    }),
                },
                description: None,
                input_schema: None,
                output_schema: None,
            };
            env.define(&sym, cap.into_value(logger));
        }
    }
    Ok(())
}
