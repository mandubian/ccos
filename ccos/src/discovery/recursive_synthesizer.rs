//! Recursive synthesizer for generating missing capabilities

use crate::arbiter::delegating_arbiter::DelegatingArbiter;
use crate::capability_marketplace::types::CapabilityManifest;
use crate::discovery::cycle_detector::CycleDetector;
use crate::discovery::engine::{DiscoveryContext, DiscoveryEngine, DiscoveryResult};
use crate::discovery::intent_transformer::IntentTransformer;
use crate::discovery::need_extractor::CapabilityNeed;
use crate::intent_graph::IntentGraph;
use crate::types::Intent;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::sync::{Arc, Mutex};

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
        
        // TODO: Use delegating arbiter to refine intent and generate plan
        // For now, we'll create a placeholder capability
        
        // Create a simple stub capability
        let orchestrator_rtfs = format!(
            "(plan \"synthesized-{}\"\n  :body (do\n    (log \"Synthesized capability: {}\")\n    {{:status \"stub\" :capability \"{}\"}}\n  )\n)",
            need.capability_class,
            need.capability_class,
            need.capability_class
        );
        
        // TODO: Generate proper RTFS orchestrator once plan generation is integrated
        
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
            sub_intents: vec![], // TODO: Populate with actual sub-intent IDs
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

