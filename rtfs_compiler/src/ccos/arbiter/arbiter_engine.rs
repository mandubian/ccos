use std::collections::HashMap;

use async_trait::async_trait;

use crate::runtime::error::RuntimeError;
use crate::runtime::values::Value;

use crate::ccos::types::{ExecutionResult, Intent, Plan};

/// High-level interface exposed by all Arbiter implementations.
///
/// 1. `natural_language_to_intent` – parse user text and create a structured [`Intent`].
/// 2. `intent_to_plan` – turn that intent into a concrete RTFS [`Plan`].
/// 3. `execute_plan` – run the plan and return an [`ExecutionResult`].
///
/// A default convenience method `process_natural_language` wires the three
/// stages together.  Implementers may override it for custom behaviour but
/// typically the default is sufficient.
#[async_trait(?Send)]
pub trait ArbiterEngine {
    /// Convert natural-language user input into a structured [`Intent`].
    async fn natural_language_to_intent(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>, // optional additional context
    ) -> Result<Intent, RuntimeError>;

    /// Generate or select an executable plan that fulfils the provided intent.
    async fn intent_to_plan(&self, intent: &Intent) -> Result<Plan, RuntimeError>;

    /// Execute the plan and return the resulting value / metadata.
    async fn execute_plan(&self, plan: &Plan) -> Result<ExecutionResult, RuntimeError>;

    /// Optional learning hook – update internal statistics from an execution.
    /// Default implementation does nothing.
    async fn learn_from_execution(
        &self,
        _intent: &Intent,
        _plan: &Plan,
        _result: &ExecutionResult,
    ) -> Result<(), RuntimeError> {
        Ok(())
    }

    /// Convenience shortcut that chains the three phases together.
    async fn process_natural_language(
        &self,
        natural_language: &str,
        context: Option<HashMap<String, Value>>, // optional ctx
    ) -> Result<ExecutionResult, RuntimeError> {
        let intent = self
            .natural_language_to_intent(natural_language, context)
            .await?;
        let plan = self.intent_to_plan(&intent).await?;
        let result = self.execute_plan(&plan).await?;
        self.learn_from_execution(&intent, &plan, &result).await?;
        Ok(result)
    }
} 