use rtfs_compiler::ccos::delegation::StaticDelegationEngine;
use rtfs_compiler::parser;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry};
use rtfs_compiler::runtime::stdlib::StandardLibrary;
use rtfs_compiler::ast::TopLevel;
use rtfs_compiler::runtime::security::RuntimeContext;
use rtfs_compiler::runtime::host_interface::HostInterface;
use std::collections::HashMap;
use std::sync::Arc;
use std::env;

/// Run a generated plan file with all capabilities enabled (Full security context)
pub fn run_plan_with_full_security_context(plan_path: &str) -> Result<(), Box<dyn std::error::Error>> {
    // Recursively convert all keyword literals in an expression to string literals
    fn normalize_keywords_to_strings(expr: &rtfs_compiler::Expression) -> rtfs_compiler::Expression {
        use rtfs_compiler::Expression::*;
        match expr {
            Literal(rtfs_compiler::ast::Literal::Keyword(k)) => {
                Literal(rtfs_compiler::ast::Literal::String(k.0.clone()))
            },
            FunctionCall { callee, arguments } => {
                FunctionCall {
                    callee: Box::new(normalize_keywords_to_strings(callee)),
                    arguments: arguments.iter().map(|a| normalize_keywords_to_strings(a)).collect(),
                }
            },
            Vector(vec) => Vector(vec.iter().map(|e| normalize_keywords_to_strings(e)).collect()),
            Map(map) => Map(map.iter().map(|(k, v)| (k.clone(), normalize_keywords_to_strings(v))).collect()),
            Let(let_expr) => {
                let mut new_let = let_expr.clone();
                new_let.bindings = new_let.bindings.iter().map(|b| {
                    let mut nb = b.clone();
                    // Only normalize value, not pattern
                    nb.value = Box::new(normalize_keywords_to_strings(&nb.value));
                    nb
                }).collect();
                new_let.body = new_let.body.iter().map(|b| normalize_keywords_to_strings(b)).collect();
                Let(new_let)
            },
            _ => expr.clone(),
        }
    }
    println!("\n🧪 Running generated plan: {}", plan_path);
    let plan_rtfs = std::fs::read_to_string(plan_path)?;
    let ast = parser::parse(&plan_rtfs)?;
    // Find the top-level plan expression
    let plan_expr = ast.iter().find_map(|top| {
        if let TopLevel::Expression(expr) = top {
            Some(expr)
        } else {
            None
        }
    });
    // Helper to extract plan name and :steps property from plan expression
    fn extract_plan_name_and_steps(expr: &rtfs_compiler::Expression) -> (Option<String>, Option<&Vec<rtfs_compiler::Expression>>) {
        use rtfs_compiler::Expression::*;
        let mut plan_name: Option<String> = None;
        let mut steps: Option<&Vec<rtfs_compiler::Expression>> = None;
        if let FunctionCall { arguments, .. } = expr {
            let mut args_iter = arguments.iter();
            // First argument is plan name
            if let Some(arg) = args_iter.next() {
                match arg {
                    Literal(rtfs_compiler::ast::Literal::Keyword(k)) => {
                        // Strip leading ':' if present
                        let name = if k.0.starts_with(':') {
                            k.0.trim_start_matches(':').to_string()
                        } else {
                            k.0.clone()
                        };
                        plan_name = Some(name);
                    },
                    Literal(rtfs_compiler::ast::Literal::String(s)) => plan_name = Some(s.clone()),
                    _ => {}
                }
            }
            // Search for :steps property
            while let Some(arg) = args_iter.next() {
                if let Literal(rtfs_compiler::ast::Literal::Keyword(k)) = arg {
                    if k.0 == "steps" || k.0 == ":steps" {
                        if let Some(Vector(vec)) = args_iter.next() {
                            steps = Some(vec);
                        }
                    }
                }
            }
        }
        (plan_name, steps)
    }
    if let Some(plan_expr) = plan_expr {
        let (plan_name, steps_vec) = extract_plan_name_and_steps(plan_expr);
        if let Some(steps_vec) = steps_vec {
            println!("\n🚀 Executing plan steps with Full security context:");
            println!("Plan name: {:?}", plan_name);
            // Setup evaluator with Full context
            let delegation = Arc::new(StaticDelegationEngine::new(HashMap::new()));
            let stdlib_env = StandardLibrary::create_global_environment();
            let registry = std::sync::Arc::new(tokio::sync::RwLock::new(rtfs_compiler::runtime::capabilities::registry::CapabilityRegistry::new()));
            let capability_marketplace = std::sync::Arc::new(rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace::new(registry.clone()));
            let host = std::sync::Arc::new(rtfs_compiler::runtime::host::RuntimeHost::new(
                Arc::new(std::sync::Mutex::new(rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap())),
                capability_marketplace,
                rtfs_compiler::runtime::security::RuntimeContext::full(),
            ));
            let evaluator = Evaluator::with_environment(
                Arc::new(ModuleRegistry::new()),
                stdlib_env,
                delegation,
                RuntimeContext::full(),
                host,
            );
            let _exec_plan_name = plan_name.unwrap_or_else(|| "run-plan".to_string());
            for (i, step_expr) in steps_vec.iter().enumerate() {
                // TODO: Set execution context when HostInterface supports it
                let normalized_step = normalize_keywords_to_strings(step_expr);
                let result = evaluator.eval_expr(&normalized_step, &mut evaluator.env.clone());
                match result {
                    Ok(val) => println!("  ✅ Step {} result: {:?}", i+1, val),
                    Err(e) => println!("  ❌ Step {} error: {}", i+1, e),
                }
            }
        } else {
            println!("⚠️  No :steps property found in plan, cannot execute steps.");
        }
    } else {
        println!("⚠️  No top-level plan expression found, cannot execute plan.");
    }
    Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: run_plan <path-to-plan-file>");
        return Ok(());
    }
    let plan_path = &args[1];
    run_plan_with_full_security_context(plan_path)?;
    Ok(())
}
