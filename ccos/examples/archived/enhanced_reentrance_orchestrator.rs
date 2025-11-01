//! Enhanced Reentrance Orchestrator Demo
//!
//! This example shows how to implement true reentrance in RTFS runtime execution,
//! where the orchestrator can pause execution on host calls and resume with results.

use rtfs::ast::Expression;
use ccos::capability_marketplace::CapabilityMarketplace;
use rtfs::runtime::{
    error::RuntimeError,
    evaluator::Evaluator,
    execution_outcome::{ExecutionOutcome, HostCall},
    security::RuntimeContext,
    values::Value,
};
use std::sync::Arc;

/// Enhanced orchestrator that demonstrates true reentrance
pub struct ReentrantOrchestrator {
    capability_marketplace: Arc<CapabilityMarketplace>,
}

impl ReentrantOrchestrator {
    pub fn new(capability_marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self {
            capability_marketplace,
        }
    }

    /// Execute an RTFS expression with true reentrance
    /// This demonstrates how to pause execution on host calls and resume with results
    pub async fn execute_with_reentrance(
        &self,
        evaluator: &mut Evaluator,
        expr: &Expression,
        _context: &RuntimeContext,
    ) -> Result<Value, RuntimeError> {
        println!("🔄 Starting reentrant execution...");

        let current_expr = expr.clone();
        let mut execution_depth = 0;
        const MAX_DEPTH: usize = 100; // Prevent infinite loops

        loop {
            if execution_depth >= MAX_DEPTH {
                return Err(RuntimeError::Generic(
                    "Maximum execution depth reached".to_string(),
                ));
            }

            execution_depth += 1;
            println!("  📍 Execution depth: {}", execution_depth);

            // Evaluate the current expression
            let result = evaluator.evaluate(&current_expr)?;

            match result {
                ExecutionOutcome::Complete(value) => {
                    println!("  ✅ Execution completed at depth {}", execution_depth);
                    return Ok(value);
                }
                ExecutionOutcome::RequiresHost(host_call) => {
                    println!("  ⏸️  Host call required at depth {}", execution_depth);
                    println!("      Function: {}", host_call.capability_id);
                    println!("      Args: {:?}", host_call.args);

                    // Handle the host call
                    let host_result = self.handle_host_call(&host_call).await?;
                    println!("      Result: {:?}", host_result);

                    // In a true reentrant system, we would substitute the result
                    // back into the expression and continue execution
                    // For this demo, we'll show how that could work

                    // TODO: Implement expression substitution to resume execution
                    // This would involve:
                    // 1. Finding the host call expression in the AST
                    // 2. Substituting it with the result
                    // 3. Continuing evaluation with the modified AST

                    println!("  🔄 Resuming execution after host call...");

                    // For now, return the result directly
                    // In a full implementation, this would continue the loop
                    return Ok(host_result);
                }
                #[cfg(feature = "effect-boundary")]
                ExecutionOutcome::RequiresHost(effect_request) => {
                    println!("  ⏸️  Host effect required at depth {}", execution_depth);
                    println!("      Capability: {}", effect_request.capability_id);

                    // Handle the effect request (treat as host call for demo)
                    let args_value = rtfs_compiler::runtime::values::Value::Vector(
                        effect_request.input_payload.clone(),
                    );
                    let effect_result = self
                        .capability_marketplace
                        .execute_capability(&effect_request.capability_id, &args_value)
                        .await?;
                    println!("      Effect result: {:?}", effect_result);

                    // Similar to host calls, we would substitute and continue
                    return Ok(effect_result);
                }
            }
        }
    }

    async fn handle_host_call(&self, host_call: &HostCall) -> Result<Value, RuntimeError> {
        // Parse the function symbol to determine the type of call
        if host_call.capability_id.starts_with("call:") {
            // Capability call
            let capability_id = host_call
                .capability_id
                .strip_prefix("call:")
                .unwrap_or(&host_call.capability_id);
            let args_value = Value::Vector(host_call.args.clone());
            self.capability_marketplace
                .execute_capability(capability_id, &args_value)
                .await
        } else {
            Err(RuntimeError::Generic(format!(
                "Unknown host call: {}",
                host_call.capability_id
            )))
        }
    }

    #[cfg(feature = "effect-boundary")]
    async fn handle_effect_request(
        &self,
        effect_request: &rtfs_compiler::runtime::execution_outcome::EffectRequest,
    ) -> Result<Value, RuntimeError> {
        // Handle effect requests similar to host calls
        let args_value = Value::Vector(effect_request.input_payload.clone());
        self.capability_marketplace
            .execute_capability(&effect_request.capability_id, &args_value)
            .await
    }
}

/// Demo function showing how to use the reentrant orchestrator
pub async fn demo_reentrant_execution() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Enhanced Reentrance Orchestrator Demo ===\n");

    // Set up the capability marketplace with demo capabilities
    let registry = Arc::new(tokio::sync::RwLock::new(
        rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new(),
    ));
    let capability_marketplace = Arc::new(CapabilityMarketplace::new(registry.clone()));

    // Register default capabilities (including ccos.echo)
    rtfs_compiler::runtime::stdlib::register_default_capabilities(&capability_marketplace).await?;

    // Register a demo capability
    // Note: In a real implementation, we would use the proper registry API
    // For this demo, we'll skip the actual registration

    let orchestrator = ReentrantOrchestrator::new(capability_marketplace.clone());

    // Create a simple RTFS expression that will trigger a host call
    let rtfs_code = r#"(call :ccos.echo "Hello from reentrant execution!")"#;
    let expr = rtfs_compiler::parser::parse_expression(rtfs_code)
        .map_err(|e| format!("Parse error: {:?}", e))?;

    // Set up evaluator and context
    let context = RuntimeContext::full();

    // Create a proper RuntimeHost for this demo
    let causal_chain = Arc::new(std::sync::Mutex::new(
        rtfs_compiler::ccos::causal_chain::CausalChain::new()?,
    ));
    let host = Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
        causal_chain,
        capability_marketplace.clone(),
        context.clone(),
    ));

    let module_registry = Arc::new(rtfs_compiler::runtime::module_runtime::ModuleRegistry::new());
    let mut evaluator = Evaluator::new(module_registry, context.clone(), host);

    // Execute with reentrance
    let result = orchestrator
        .execute_with_reentrance(&mut evaluator, &expr, &context)
        .await?;

    println!("\n✅ Reentrant execution completed!");
    println!("📊 Final result: {:?}", result);

    Ok(())
}

/// Main function to run the enhanced reentrance demo
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    demo_reentrant_execution().await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_reentrant_execution() {
        // This test demonstrates the reentrant execution pattern
        let result = demo_reentrant_execution().await;
        assert!(result.is_ok(), "Reentrant execution should succeed");
    }
}
