use std::sync::Arc;

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;

use crate::capability_marketplace::CapabilityMarketplace;
use crate::catalog::CatalogService;
use crate::planner::signals::GoalSignals;
use crate::types::Intent;

/// Register planner-focused local capabilities so that the planner's own cognitive
/// pipeline can be invoked via RTFS plans.
pub async fn register_planner_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    catalog: Arc<CatalogService>,
) -> RuntimeResult<()> {
    // planner.extract_goal_signals
    let catalog_for_handler = Arc::clone(&catalog);
    let extract_handler = Arc::new(move |input: &Value| {
        let payload: ExtractGoalSignalsInput =
            parse_payload("planner.extract_goal_signals", input)?;

        let ExtractGoalSignalsInput {
            goal,
            intent,
            apply_catalog_search,
            min_score,
            max_results,
        } = payload;

        let catalog = Arc::clone(&catalog_for_handler);
        let rt_handle = tokio::runtime::Handle::current();

        let signals = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let mut signals = match intent {
                    Some(intent) => GoalSignals::from_goal_and_intent(&goal, &intent),
                    None => GoalSignals::new(goal.clone()),
                };

                if apply_catalog_search.unwrap_or(true) {
                    signals
                        .apply_catalog_search(
                            &catalog,
                            min_score.unwrap_or(0.5),
                            max_results.unwrap_or(10),
                        )
                        .await;
                }

                Ok::<GoalSignals, RuntimeError>(signals)
            })
        })
        .join()
        .map_err(|_| {
            RuntimeError::Generic("Thread join error in planner.extract_goal_signals".to_string())
        })??;

        produce_value(
            "planner.extract_goal_signals",
            ExtractGoalSignalsOutput {
                goal_signals: signals,
            },
        )
    });

    marketplace
        .register_local_capability(
            "planner.extract_goal_signals".to_string(),
            "Planner / Extract Goal Signals".to_string(),
            "Aggregate constraints, preferences, and requirements from raw goal context."
                .to_string(),
            extract_handler,
        )
        .await?;

    Ok(())
}

#[derive(Debug, Deserialize)]
struct ExtractGoalSignalsInput {
    goal: String,
    #[serde(default)]
    intent: Option<Intent>,
    #[serde(default)]
    apply_catalog_search: Option<bool>,
    #[serde(default)]
    min_score: Option<f32>,
    #[serde(default)]
    max_results: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ExtractGoalSignalsOutput {
    goal_signals: GoalSignals,
}

fn parse_payload<T: DeserializeOwned>(capability: &str, value: &Value) -> RuntimeResult<T> {
    let serialized = serde_json::to_value(value).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: failed to convert RTFS value into JSON: {}",
            capability, err
        ))
    })?;

    serde_json::from_value(serialized).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: input payload does not match schema: {}",
            capability, err
        ))
    })
}

fn produce_value<T: Serialize>(capability: &str, data: T) -> RuntimeResult<Value> {
    let json = serde_json::to_value(data).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: failed to serialize response to JSON: {}",
            capability, err
        ))
    })?;

    serde_json::from_value(json).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: failed to convert JSON response into RTFS value: {}",
            capability, err
        ))
    })
}
