use crate::ast::Expression;
use crate::runtime::{RuntimeError, RuntimeResult, Value};
use std::collections::HashMap;

pub type BoundParams = HashMap<String, Value>;

#[derive(Debug)]
pub enum ParamError {
    EvalError(String),
    ValidationError(String),
}

impl From<ParamError> for RuntimeError {
    fn from(e: ParamError) -> RuntimeError {
        RuntimeError::Generic(format!("ParamBinding: {:?}", e))
    }
}

/// Bind params by evaluating each expression using the provided evaluator callback.
/// The evaluator callback is responsible for evaluating a single Expression and
/// returning a RuntimeResult<Value>.
pub fn bind_parameters<F>(
    params: &HashMap<String, Expression>,
    mut eval_cb: F,
) -> Result<BoundParams, ParamError>
where
    F: FnMut(&Expression) -> RuntimeResult<Value>,
{
    let mut out: BoundParams = HashMap::new();
    for (k, expr) in params.iter() {
        match eval_cb(expr) {
            Ok(v) => {
                out.insert(k.clone(), v);
            }
            Err(e) => {
                return Err(ParamError::EvalError(e.to_string()));
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ast::Expression;
    use crate::ast::Literal;
    use crate::runtime::Value;

    #[test]
    fn bind_simple_literals() {
        let mut params: HashMap<String, Expression> = HashMap::new();
        params.insert("a".to_string(), Expression::Literal(Literal::Integer(42)));
        params.insert(
            "b".to_string(),
            Expression::Literal(Literal::String("x".to_string())),
        );

        let res = bind_parameters(&params, |e| {
            // very small fake evaluator supporting literals only
            match e {
                Expression::Literal(Literal::Integer(n)) => Ok(Value::Integer(*n)),
                Expression::Literal(Literal::String(s)) => Ok(Value::String(s.clone())),
                _ => Err(RuntimeError::Generic("unsupported".to_string())),
            }
        });

        assert!(res.is_ok());
        let map = res.unwrap();
        assert_eq!(map.get("a"), Some(&Value::Integer(42)));
        assert_eq!(map.get("b"), Some(&Value::String("x".to_string())));
    }
}
