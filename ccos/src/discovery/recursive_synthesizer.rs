//! Recursive synthesizer for generating missing capabilities

use crate::arbiter::arbiter_engine::ArbiterEngine;
use crate::arbiter::delegating_arbiter::DelegatingArbiter;
use crate::capability_marketplace::types::CapabilityManifest;
use crate::discovery::cycle_detector::CycleDetector;
use crate::discovery::engine::{DiscoveryContext, DiscoveryEngine};
use crate::discovery::intent_transformer::IntentTransformer;
use crate::discovery::need_extractor::{CapabilityNeed, CapabilityNeedExtractor};
use crate::types::Plan;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::sync::Arc;

/// Result of a recursive synthesis attempt
#[derive(Debug, Clone)]
pub struct SynthesizedCapability {
    pub manifest: CapabilityManifest,
    pub orchestrator_rtfs: String,
    pub sub_intents: Vec<String>, // Intent IDs of synthesized sub-capabilities
    pub depth: usize,
}

/// Recursive synthesizer that treats missing capabilities as new intents
pub struct RecursiveSynthesizer {
    discovery_engine: DiscoveryEngine,
    delegating_arbiter: Option<Arc<DelegatingArbiter>>,
    cycle_detector: CycleDetector,
    default_max_depth: usize,
}

impl RecursiveSynthesizer {
    /// Create a new recursive synthesizer
    pub fn new(
        discovery_engine: DiscoveryEngine,
        delegating_arbiter: Option<Arc<DelegatingArbiter>>,
        max_depth: usize,
    ) -> Self {
        Self {
            discovery_engine,
            delegating_arbiter,
            cycle_detector: CycleDetector::new(max_depth),
            default_max_depth: max_depth,
        }
    }
    
    /// Synthesize a capability by treating the need as a new intent
    /// 
    /// This is the core recursive synthesis logic:
    /// 1. Transform capability need → Intent
    /// 2. Refine intent via clarifying questions (if arbiter available)
    /// 3. Decompose into sub-steps
    /// 4. For each sub-step, recursively discover or synthesize
    /// 5. Build orchestrator RTFS once all dependencies resolved
    /// 6. Register as new capability
    pub async fn synthesize_as_intent(
        &mut self,
        need: &CapabilityNeed,
        context: &DiscoveryContext,
    ) -> RuntimeResult<SynthesizedCapability> {
        // Check cycle detection
        if self.cycle_detector.has_cycle(&need.capability_class) {
            return Err(RuntimeError::Generic(format!(
                "Cycle detected: capability {} already being synthesized",
                need.capability_class
            )));
        }
        
        // Check depth limit
        if !self.cycle_detector.can_go_deeper() {
            return Err(RuntimeError::Generic(format!(
                "Maximum depth {} reached while synthesizing {}",
                self.default_max_depth, need.capability_class
            )));
        }
        
        // Mark as visited
        self.cycle_detector.visit(&need.capability_class);
        
        // Transform capability need into intent
        let parent_intent_id = context.visited_intents.last().map(|s| s.as_str());
        let intent = IntentTransformer::need_to_intent(need, parent_intent_id);
        
        // Store intent in intent graph
        {
            let intent_graph = self.discovery_engine.get_intent_graph();
            let mut ig = intent_graph.lock()
                .map_err(|e| RuntimeError::Generic(format!("Failed to lock intent graph: {}", e)))?;
            
            let storable_intent = crate::types::StorableIntent {
                intent_id: intent.intent_id.clone(),
                name: intent.name.clone(),
                original_request: intent.original_request.clone(),
                rtfs_intent_source: "".to_string(),
                goal: intent.goal.clone(),
                constraints: intent.constraints.iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                preferences: intent.preferences.iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
                success_criteria: intent.success_criteria.as_ref().map(|v| v.to_string()),
                parent_intent: parent_intent_id.map(|s| s.to_string()),
                child_intents: vec![],
                triggered_by: crate::types::TriggerSource::ArbiterInference,
                generation_context: IntentTransformer::create_synthesis_context(&need.rationale),
                status: intent.status.clone(),
                priority: 0,
                created_at: intent.created_at,
                updated_at: intent.updated_at,
                metadata: intent.metadata.iter()
                    .map(|(k, v)| (k.clone(), v.to_string()))
                    .collect(),
            };
            ig.store_intent(storable_intent)?;
        }
        
        // Generate plan using delegating arbiter if available
        let plan = if let Some(ref arbiter) = self.delegating_arbiter {
            // Use arbiter to generate plan from intent
            arbiter.intent_to_plan(&intent).await
                .map_err(|e| RuntimeError::Generic(format!(
                    "Failed to generate plan for synthesized intent {}: {}",
                    intent.intent_id, e
                )))?
        } else {
            // No arbiter available - create minimal stub plan
            Plan::new_rtfs(
                format!("(plan \"synthesized-{}\" :body (do (log \"Stub capability: {}\") {{:status \"stub\"}}))",
                    need.capability_class, need.capability_class),
                vec![intent.intent_id.clone()],
            )
        };
        
        // Extract capability needs from the generated plan
        let sub_needs = CapabilityNeedExtractor::extract_from_plan(&plan);
        
        // Recursively discover or synthesize sub-capabilities
        let sub_intents = Vec::new();
        let mut deeper_context = context.go_deeper();
        deeper_context.visited_intents.push(intent.intent_id.clone());
        
        for sub_need in &sub_needs {
            // Check if we can go deeper
            if !deeper_context.can_go_deeper() {
                // Max depth reached - mark as incomplete
                break;
            }
            
            // Try to discover the sub-capability in marketplace first
            let marketplace = self.discovery_engine.get_marketplace();
            if marketplace.get_capability(&sub_need.capability_class).await.is_some() {
                // Found in marketplace - no need to synthesize
                continue;
            }
            
            // Not found - recursively synthesize
            // TODO: Implement proper async recursion - currently limited by Send trait constraints
            // For now, we mark as needing synthesis but don't recursively call
            // This will be enhanced in a future iteration with async_recursion or loop-based approach
            eprintln!(
                "Warning: Sub-capability {} needs synthesis but recursive synthesis is not yet fully implemented",
                sub_need.capability_class
            );
            // Note: In a full implementation, we would:
            // 1. Create deeper synthesizer with go_deeper()
            // 2. Recursively call synthesize_as_intent
            // 3. Register the synthesized sub-capability
            // This requires resolving Send/Sync trait bounds for IntentEventSink
        }
        
        // Extract RTFS orchestrator from plan
        let orchestrator_rtfs = match &plan.body {
            crate::types::PlanBody::Rtfs(rtfs) => rtfs.clone(),
            crate::types::PlanBody::Wasm(_) => {
                // Fallback for WASM plans
                format!("(plan \"synthesized-{}\" :body (do (log \"WASM plan not yet supported\")))",
                    need.capability_class)
            }
        };
        
        // Create a minimal manifest using the constructor
        // Use a stub handler - the actual implementation will be registered later
        let capability_id = need.capability_class.clone();
        let stub_handler: Arc<dyn Fn(&rtfs::runtime::values::Value) -> RuntimeResult<rtfs::runtime::values::Value> + Send + Sync> = 
            Arc::new(move |_input: &rtfs::runtime::values::Value| -> RuntimeResult<rtfs::runtime::values::Value> {
                Err(RuntimeError::Generic(
                    format!("Synthesized capability {} not yet implemented", capability_id)
                ))
            });
        
        let manifest = CapabilityManifest::new(
            need.capability_class.clone(),
            format!("Synthesized {}", need.capability_class),
            format!("Recursively synthesized capability: {}", need.rationale),
            crate::capability_marketplace::types::ProviderType::Local(
                crate::capability_marketplace::types::LocalCapability {
                    handler: stub_handler,
                }
            ),
            "1.0.0".to_string(),
        );
        
        Ok(SynthesizedCapability {
            manifest,
            orchestrator_rtfs,
            sub_intents,
            depth: self.cycle_detector.current_depth(),
        })
    }
    
    /// Create a new synthesizer for a deeper recursion level
    pub fn go_deeper(&self) -> Self {
        Self {
            discovery_engine: DiscoveryEngine::new(
                self.discovery_engine.get_marketplace(),
                self.discovery_engine.get_intent_graph(),
            ),
            delegating_arbiter: self.delegating_arbiter.as_ref().map(Arc::clone),
            cycle_detector: self.cycle_detector.go_deeper(),
            default_max_depth: self.default_max_depth,
        }
    }
}

