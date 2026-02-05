use crate::types::Action;
use rtfs::ast::Symbol;
use rtfs::compiler::expander::MacroExpander;
use rtfs::runtime::module_runtime::ModuleRegistry;
use rtfs::runtime::pure_host::create_pure_host;
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::{Arity, BuiltinFunction, Function, Value};
use rtfs::runtime::Evaluator;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// Declarative completion predicate for autonomous runs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Predicate {
    /// Satisfied if an action with the specified function name was recorded and was successful.
    ActionSucceeded { function_name: String },
    /// Satisfied if an action with the specified function name was recorded and failed.
    ActionFailed { function_name: String },
    /// Satisfied if an action's metadata matches a specific key/value pair.
    ActionMetadataMatches {
        function_name: String,
        key: String,
        value: String,
    },
    /// Logical AND of multiple predicates.
    And(Vec<Predicate>),
    /// Logical OR of multiple predicates.
    Or(Vec<Predicate>),
    /// Logical NOT of a predicate.
    Not(Box<Predicate>),
    /// Arbitrary RTFS logic.
    Rtfs(String),
}

impl Predicate {
    /// Evaluates the predicate against a list of actions (the run trace).
    pub fn evaluate(&self, actions: &[&Action]) -> bool {
        match self {
            Predicate::ActionSucceeded { function_name } => actions.iter().any(|a| {
                a.function_name.as_deref() == Some(function_name.as_str())
                    && a.result.as_ref().map(|r| r.success).unwrap_or(false)
            }),
            Predicate::ActionFailed { function_name } => actions.iter().any(|a| {
                a.function_name.as_deref() == Some(function_name.as_str())
                    && a.result.as_ref().map(|r| !r.success).unwrap_or(false)
            }),
            Predicate::ActionMetadataMatches {
                function_name,
                key,
                value,
            } => actions.iter().any(|a| {
                a.function_name.as_deref() == Some(function_name.as_str())
                    && a.metadata.get(key).and_then(|v| v.as_string()) == Some(value.as_str())
            }),
            Predicate::And(predicates) => predicates.iter().all(|p| p.evaluate(actions)),
            Predicate::Or(predicates) => predicates.iter().any(|p| p.evaluate(actions)),
            Predicate::Not(predicate) => !predicate.evaluate(actions),
            Predicate::Rtfs(code) => self.evaluate_rtfs(code, actions),
        }
    }

    fn evaluate_rtfs(&self, code: &str, actions: &[&Action]) -> bool {
        let env = rtfs::runtime::stdlib::StandardLibrary::create_global_environment();
        let mut evaluator = Evaluator::new(
            Arc::new(ModuleRegistry::new()),
            RuntimeContext::pure(),
            create_pure_host(),
            MacroExpander::new(),
        );
        evaluator.env = env;

        // Register audit functions
        let actions_owned: Vec<Action> = actions.iter().map(|a| (*a).clone()).collect();
        let actions_arc = Arc::new(actions_owned);

        let actions_clone = actions_arc.clone();
        evaluator.env.define(
            &Symbol("audit.succeeded?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "audit.succeeded?".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(move |args| {
                    let name = args[0].as_string().unwrap_or("");
                    let ok = actions_clone.iter().any(|a| {
                        a.function_name.as_deref() == Some(name)
                            && a.result.as_ref().map(|r| r.success).unwrap_or(false)
                    });
                    Ok(Value::Boolean(ok))
                }),
            })),
        );

        let actions_clone = actions_arc.clone();
        evaluator.env.define(
            &Symbol("audit.failed?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "audit.failed?".to_string(),
                arity: Arity::Fixed(1),
                func: Arc::new(move |args| {
                    let name = args[0].as_string().unwrap_or("");
                    let ok = actions_clone.iter().any(|a| {
                        a.function_name.as_deref() == Some(name)
                            && a.result.as_ref().map(|r| !r.success).unwrap_or(false)
                    });
                    Ok(Value::Boolean(ok))
                }),
            })),
        );

        let actions_clone = actions_arc.clone();
        evaluator.env.define(
            &Symbol("audit.metadata?".to_string()),
            Value::Function(Function::Builtin(BuiltinFunction {
                name: "audit.metadata?".to_string(),
                arity: Arity::Fixed(3),
                func: Arc::new(move |args| {
                    let name = args[0].as_string().unwrap_or("");
                    let key = args[1].as_string().unwrap_or("");
                    let value = args[2].as_string().unwrap_or("");
                    let ok = actions_clone.iter().any(|a| {
                        a.function_name.as_deref() == Some(name)
                            && a.metadata.get(key).and_then(|v| v.as_string()) == Some(value)
                    });
                    Ok(Value::Boolean(ok))
                }),
            })),
        );

        match rtfs::parser::parse_expression(code) {
            Ok(expr) => match evaluator.evaluate(&expr) {
                Ok(outcome) => {
                    let val = match outcome {
                        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(v) => v,
                        _ => Value::Nil,
                    };
                    val.is_truthy()
                }
                Err(e) => {
                    log::error!("RTFS predicate evaluation failed: {:?}", e);
                    false
                }
            },
            Err(e) => {
                log::error!("RTFS predicate parse failed: {:?}", e);
                false
            }
        }
    }
}

impl std::fmt::Display for Predicate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Predicate::ActionSucceeded { function_name } => {
                write!(f, "(audit.succeeded? \"{}\")", function_name)
            }
            Predicate::ActionFailed { function_name } => {
                write!(f, "(audit.failed? \"{}\")", function_name)
            }
            Predicate::ActionMetadataMatches {
                function_name,
                key,
                value,
            } => {
                write!(
                    f,
                    "(audit.metadata? \"{}\" \"{}\" \"{}\")",
                    function_name, key, value
                )
            }
            Predicate::And(predicates) => {
                write!(f, "(and")?;
                for p in predicates {
                    write!(f, " {}", p)?;
                }
                write!(f, ")")
            }
            Predicate::Or(predicates) => {
                write!(f, "(or")?;
                for p in predicates {
                    write!(f, " {}", p)?;
                }
                write!(f, ")")
            }
            Predicate::Not(predicate) => {
                write!(f, "(not {})", predicate)
            }
            Predicate::Rtfs(code) => write!(f, "{}", code),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Action, ActionType, ExecutionResult};
    use rtfs::runtime::values::Value;
    use std::collections::HashMap;

    fn mock_action(name: &str, success: bool, metadata: HashMap<String, Value>) -> Action {
        Action {
            action_id: "test".to_string(),
            parent_action_id: None,
            session_id: Some("session".to_string()),
            plan_id: "plan".to_string(),
            intent_id: "intent".to_string(),
            action_type: ActionType::CapabilityCall,
            function_name: Some(name.to_string()),
            arguments: None,
            result: Some(ExecutionResult {
                success,
                value: Value::Nil,
                metadata: HashMap::new(),
            }),
            cost: None,
            duration_ms: None,
            timestamp: 0,
            metadata,
        }
    }

    #[test]
    fn test_predicate_evaluation() {
        let mut meta = HashMap::new();
        meta.insert("status".to_string(), Value::String("completed".to_string()));

        let a1 = mock_action("github.create_issue", true, meta.clone());
        let a2 = mock_action("slack.notify", false, HashMap::new());
        let actions = vec![&a1, &a2];

        // test succeed
        let p_suc = Predicate::ActionSucceeded {
            function_name: "github.create_issue".to_string(),
        };
        assert!(p_suc.evaluate(&actions));

        // test fail
        let p_fail = Predicate::ActionFailed {
            function_name: "slack.notify".to_string(),
        };
        assert!(p_fail.evaluate(&actions));

        // test metadata
        let p_meta = Predicate::ActionMetadataMatches {
            function_name: "github.create_issue".to_string(),
            key: "status".to_string(),
            value: "completed".to_string(),
        };
        assert!(p_meta.evaluate(&actions));

        // test logic
        let p_and = Predicate::And(vec![p_suc.clone(), p_fail.clone()]);
        assert!(p_and.evaluate(&actions));

        let p_not = Predicate::Not(Box::new(Predicate::ActionSucceeded {
            function_name: "nonexistent".to_string(),
        }));
        assert!(p_not.evaluate(&actions));
    }

    #[test]
    fn test_rtfs_predicate() {
        let a1 = mock_action("github.create_issue", true, HashMap::new());
        let actions = vec![&a1];

        let p_rtfs = Predicate::Rtfs("(audit.succeeded? \"github.create_issue\")".to_string());
        assert!(p_rtfs.evaluate(&actions));

        let p_rtfs_fail = Predicate::Rtfs("(audit.succeeded? \"nonexistent\")".to_string());
        assert!(!p_rtfs_fail.evaluate(&actions));

        let p_rtfs_complex = Predicate::Rtfs("(and (audit.succeeded? \"github.create_issue\") (not (audit.failed? \"github.create_issue\")))".to_string());
        assert!(p_rtfs_complex.evaluate(&actions));
    }
}
