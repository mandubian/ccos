//! CCOS prelude: registers effectful convenience functions into an existing RTFS environment.
//! This lives on the CCOS side to keep RTFS stdlib pure and host-agnostic.

use std::collections::HashMap;
use std::sync::Arc;

use crate::ast::{Keyword, MapKey, Symbol};
use crate::runtime::error::{RuntimeError, RuntimeResult};
use crate::runtime::evaluator::Evaluator;
use crate::runtime::execution_outcome::ExecutionOutcome;
use crate::runtime::values::{Arity, BuiltinFunction, BuiltinFunctionWithContext, Function, Value};
use crate::runtime::Environment;

/// Load CCOS-provided prelude into the given environment.
/// Registers effectful helpers that delegate to host capabilities via evaluator.host.
pub fn load_prelude(env: &mut Environment) {
    // Logging
    env.define(
        &Symbol("tool/log".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "tool/log".to_string(),
            arity: Arity::Variadic(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator.host.execute_capability("ccos.io.log", &args)
                },
            ),
        })),
    );
    env.define(
        &Symbol("tool.log".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "tool.log".to_string(),
            arity: Arity::Variadic(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator.host.execute_capability("ccos.io.log", &args)
                },
            ),
        })),
    );
    env.define(
        &Symbol("log".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "log".to_string(),
            arity: Arity::Variadic(0),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator.host.execute_capability("ccos.io.log", &args)
                },
            ),
        })),
    );

    // Printing
    env.define(
        &Symbol("println".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "println".to_string(),
            arity: Arity::Variadic(0),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator.host.execute_capability("ccos.io.println", &args)
                },
            ),
        })),
    );

    // System time
    env.define(
        &Symbol("tool/time-ms".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "tool/time-ms".to_string(),
            arity: Arity::Fixed(0),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.system.current-timestamp-ms", &args)
                },
            ),
        })),
    );
    env.define(
        &Symbol("tool.time-ms".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "tool.time-ms".to_string(),
            arity: Arity::Fixed(0),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.system.current-timestamp-ms", &args)
                },
            ),
        })),
    );
    env.define(
        &Symbol("current-time-millis".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "current-time-millis".to_string(),
            arity: Arity::Fixed(0),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.system.current-timestamp-ms", &args)
                },
            ),
        })),
    );

    // Env and Files
    env.define(
        &Symbol("get-env".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "get-env".to_string(),
            arity: Arity::Fixed(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.system.get-env", &args)
                },
            ),
        })),
    );
    env.define(
        &Symbol("file-exists?".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "file-exists?".to_string(),
            arity: Arity::Fixed(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.io.file-exists", &args)
                },
            ),
        })),
    );
    env.define(
        &Symbol("tool/open-file".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "tool/open-file".to_string(),
            arity: Arity::Fixed(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.io.open-file", &args)
                },
            ),
        })),
    );

    // HTTP
    env.define(
        &Symbol("tool/http-fetch".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "tool/http-fetch".to_string(),
            arity: Arity::Fixed(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.network.http-fetch", &args)
                },
            ),
        })),
    );
    env.define(
        &Symbol("http-fetch".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "http-fetch".to_string(),
            arity: Arity::Fixed(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.network.http-fetch", &args)
                },
            ),
        })),
    );

    // Thread sleep
    env.define(
        &Symbol("thread/sleep".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "thread/sleep".to_string(),
            arity: Arity::Fixed(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator
                        .host
                        .execute_capability("ccos.system.sleep-ms", &args)
                },
            ),
        })),
    );

    // Step (debug)
    env.define(
        &Symbol("step".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "step".to_string(),
            arity: Arity::Variadic(1),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, _env: &mut Environment| {
                    evaluator.host.execute_capability("ccos.io.println", &args)
                },
            ),
        })),
    );

    // KV convenience helpers (effectful): assoc!/dissoc!/conj!
    env.define(
        &Symbol("kv/assoc!".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "kv/assoc!".to_string(),
            arity: Arity::Variadic(3),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    kv_assoc_bang(args, evaluator, env)
                },
            ),
        })),
    );
    env.define(
        &Symbol("kv/dissoc!".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "kv/dissoc!".to_string(),
            arity: Arity::Variadic(2),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    kv_dissoc_bang(args, evaluator, env)
                },
            ),
        })),
    );
    env.define(
        &Symbol("kv/conj!".to_string()),
        Value::Function(Function::BuiltinWithContext(BuiltinFunctionWithContext {
            name: "kv/conj!".to_string(),
            arity: Arity::Variadic(2),
            func: Arc::new(
                |args: Vec<Value>, evaluator: &Evaluator, env: &mut Environment| {
                    kv_conj_bang(args, evaluator, env)
                },
            ),
        })),
    );
}

/// (kv/assoc! key k v [k v]...) -> get, assoc (pure), put, return new
fn kv_assoc_bang(
    args: Vec<Value>,
    evaluator: &Evaluator,
    env: &mut Environment,
) -> RuntimeResult<Value> {
    if args.len() < 3 || args.len() % 2 == 0 {
        return Err(RuntimeError::ArityMismatch {
            function: "kv/assoc!".into(),
            expected: "(kv/assoc! key k v [k v]...)".into(),
            actual: args.len(),
        });
    }
    let kv_key = args[0].clone();
    let pairs = args[1..].to_vec();

    let current = evaluator
        .host
        .execute_capability("ccos.state.kv.get", &[kv_key.clone()])
        .unwrap_or(Value::Nil);
    let base = match current {
        Value::Nil => Value::Map(HashMap::new()),
        other => other,
    };

    let assoc_sym = Symbol("assoc".to_string());
    let assoc_fn = env
        .lookup(&assoc_sym)
        .ok_or_else(|| RuntimeError::Generic("assoc not found".into()))?;
    let mut assoc_args = Vec::with_capacity(1 + pairs.len());
    assoc_args.push(base);
    assoc_args.extend(pairs);
    let updated = match evaluator.call_function(assoc_fn, &assoc_args, env)? {
        ExecutionOutcome::Complete(v) => v,
        ExecutionOutcome::RequiresHost(hc) => {
            return Err(RuntimeError::Generic(format!(
                "Host call required in assoc: {}",
                hc.capability_id
            )))
        }
    };

    let _ = evaluator
        .host
        .execute_capability("ccos.state.kv.put", &[kv_key, updated.clone()]);
    Ok(updated)
}

/// (kv/dissoc! key k1 k2 ...) -> get, dissoc (pure), put, return new
fn kv_dissoc_bang(
    args: Vec<Value>,
    evaluator: &Evaluator,
    env: &mut Environment,
) -> RuntimeResult<Value> {
    if args.len() < 2 {
        return Err(RuntimeError::ArityMismatch {
            function: "kv/dissoc!".into(),
            expected: "(kv/dissoc! key k1 k2 ...)".into(),
            actual: args.len(),
        });
    }
    let kv_key = args[0].clone();
    let ds_keys = args[1..].to_vec();

    let current = evaluator
        .host
        .execute_capability("ccos.state.kv.get", &[kv_key.clone()])
        .unwrap_or(Value::Nil);
    let base = match current {
        Value::Nil => Value::Map(HashMap::new()),
        other => other,
    };

    let dissoc_sym = Symbol("dissoc".to_string());
    let dissoc_fn = env
        .lookup(&dissoc_sym)
        .ok_or_else(|| RuntimeError::Generic("dissoc not found".into()))?;
    let mut dissoc_args = Vec::with_capacity(1 + ds_keys.len());
    dissoc_args.push(base);
    dissoc_args.extend(ds_keys);
    let updated = match evaluator.call_function(dissoc_fn, &dissoc_args, env)? {
        ExecutionOutcome::Complete(v) => v,
        ExecutionOutcome::RequiresHost(hc) => {
            return Err(RuntimeError::Generic(format!(
                "Host call required in dissoc: {}",
                hc.capability_id
            )))
        }
    };

    let _ = evaluator
        .host
        .execute_capability("ccos.state.kv.put", &[kv_key, updated.clone()]);
    Ok(updated)
}

/// (kv/conj! key x1 x2 ...) -> get, conj (pure), put, return new
fn kv_conj_bang(
    args: Vec<Value>,
    evaluator: &Evaluator,
    env: &mut Environment,
) -> RuntimeResult<Value> {
    if args.len() < 2 {
        return Err(RuntimeError::ArityMismatch {
            function: "kv/conj!".into(),
            expected: "(kv/conj! key x1 x2 ...)".into(),
            actual: args.len(),
        });
    }
    let kv_key = args[0].clone();
    let items = args[1..].to_vec();

    let current = evaluator
        .host
        .execute_capability("ccos.state.kv.get", &[kv_key.clone()])
        .unwrap_or(Value::Nil);
    let base = match current {
        Value::Nil => Value::Vector(Vec::new()),
        other => other,
    };

    let conj_sym = Symbol("conj".to_string());
    let conj_fn = env
        .lookup(&conj_sym)
        .ok_or_else(|| RuntimeError::Generic("conj not found".into()))?;
    let mut conj_args = Vec::with_capacity(1 + items.len());
    conj_args.push(base);
    conj_args.extend(items);
    let updated = match evaluator.call_function(conj_fn, &conj_args, env)? {
        ExecutionOutcome::Complete(v) => v,
        ExecutionOutcome::RequiresHost(hc) => {
            return Err(RuntimeError::Generic(format!(
                "Host call required in conj: {}",
                hc.capability_id
            )))
        }
    };

    let _ = evaluator
        .host
        .execute_capability("ccos.state.kv.put", &[kv_key, updated.clone()]);
    Ok(updated)
}
