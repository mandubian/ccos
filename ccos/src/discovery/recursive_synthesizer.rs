//! Recursive synthesizer for generating missing capabilities

use crate::arbiter::arbiter_engine::ArbiterEngine;
use crate::arbiter::delegating_arbiter::DelegatingArbiter;
use crate::capability_marketplace::types::CapabilityManifest;
use crate::discovery::cycle_detector::CycleDetector;
use crate::discovery::engine::{DiscoveryContext, DiscoveryEngine};
use crate::discovery::intent_transformer::IntentTransformer;
use crate::discovery::need_extractor::{CapabilityNeed, CapabilityNeedExtractor};
use crate::types::Plan;
use async_recursion::async_recursion;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::sync::Arc;

/// Result of a recursive synthesis attempt
#[derive(Debug, Clone)]
pub struct SynthesizedCapability {
    pub manifest: CapabilityManifest,
    pub orchestrator_rtfs: String,
    pub plan: Option<Plan>, // Store the generated plan for sub-need extraction
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
    #[async_recursion(?Send)]
    pub async fn synthesize_as_intent(
        &mut self,
        need: &CapabilityNeed,
        context: &DiscoveryContext,
    ) -> RuntimeResult<SynthesizedCapability> {
        let depth = context.current_depth;
        let indent = "  ".repeat(depth);
        
        eprintln!(
            "{}🔄 [Depth {}] Starting synthesis for: {}",
            indent, depth, need.capability_class
        );
        
        // Check depth limit first
        if !self.cycle_detector.can_go_deeper() {
            eprintln!(
                "{}⚠️  [Depth {}] Max depth {} reached for {}",
                indent, depth, self.default_max_depth, need.capability_class
            );
            return Err(RuntimeError::Generic(format!(
                "Maximum depth {} reached while synthesizing {}",
                self.default_max_depth, need.capability_class
            )));
        }
        
        // Check if capability already exists in marketplace (parent may have already synthesized it)
        // This prevents re-synthesizing capabilities that were already created at a parent level
        let marketplace = self.discovery_engine.get_marketplace();
        if let Some(manifest) = marketplace.get_capability(&need.capability_class).await {
            eprintln!(
                "{}  → Capability already exists in marketplace: {}",
                indent, manifest.id
            );
            eprintln!("{}  ✓ Using existing capability (no re-synthesis needed)", indent);
            // Return the existing capability (we'll create a SynthesizedCapability wrapper)
            return Ok(SynthesizedCapability {
                manifest: manifest.clone(),
                orchestrator_rtfs: "".to_string(), // Not available for existing capabilities
                plan: None,
                sub_intents: vec![],
                depth: self.cycle_detector.current_depth(),
            });
        }
        
        // Check cycle detection (only if not already in marketplace)
        // This prevents infinite loops when a capability needs itself
        if self.cycle_detector.has_cycle(&need.capability_class) {
            eprintln!(
                "{}  ✗ Cycle detected: {} is already being synthesized in this path",
                indent, need.capability_class
            );
            return Err(RuntimeError::Generic(format!(
                "Cycle detected: capability {} already being synthesized",
                need.capability_class
            )));
        }
        
        // Mark as visited (tracks capabilities currently in synthesis path)
        self.cycle_detector.visit(&need.capability_class);
        
        // Transform capability need into intent
        let parent_intent_id = context.visited_intents.last().map(|s| s.as_str());
        let intent = IntentTransformer::need_to_intent(need, parent_intent_id);
        
        eprintln!(
            "{}  → Created intent: {} (parent: {:?})",
            indent, intent.intent_id, parent_intent_id
        );
        
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
        eprintln!("{}  → Generating plan via delegating arbiter...", indent);
        let plan = if let Some(ref arbiter) = self.delegating_arbiter {
            // Use arbiter to generate plan from intent
            let generated_plan = arbiter.intent_to_plan(&intent).await
                .map_err(|e| RuntimeError::Generic(format!(
                    "Failed to generate plan for synthesized intent {}: {}",
                    intent.intent_id, e
                )))?;
            eprintln!("{}  ✓ Plan generated successfully", indent);
            // Log the generated plan RTFS
            if let crate::types::PlanBody::Rtfs(rtfs) = &generated_plan.body {
                eprintln!("{}  📄 Generated plan RTFS:", indent);
                for line in rtfs.lines().take(20) { // Show first 20 lines
                    eprintln!("{}    {}", indent, line);
                }
                if rtfs.lines().count() > 20 {
                    eprintln!("{}    ... ({} more lines)", indent, rtfs.lines().count() - 20);
                }
            }
            generated_plan
        } else {
            // No arbiter available - create minimal stub plan
            eprintln!("{}  ⚠️  No arbiter available, creating stub plan", indent);
            Plan::new_rtfs(
                format!("(plan \"synthesized-{}\" :body (do (log \"Stub capability: {}\") {{:status \"stub\"}}))",
                    need.capability_class, need.capability_class),
                vec![intent.intent_id.clone()],
            )
        };
        
        // Extract capability needs from the generated plan
        eprintln!("{}  → Extracting sub-capability needs from plan...", indent);
        let sub_needs = CapabilityNeedExtractor::extract_from_plan(&plan);
        
        // Validate that the plan declares a service (not just asks questions)
        let plan_rtfs = match &plan.body {
            crate::types::PlanBody::Rtfs(rtfs) => rtfs,
            crate::types::PlanBody::Wasm(_) => "",
        };
        let has_user_ask = plan_rtfs.contains(":ccos.user.ask");
        let has_service_call = plan_rtfs.contains("(call :") && 
            (plan_rtfs.contains("api") || plan_rtfs.contains("service") || 
             plan_rtfs.contains("restaurant") || plan_rtfs.contains("hotel") ||
             plan_rtfs.contains("travel") || plan_rtfs.contains("booking"));
        let has_user_ask_only = has_user_ask && !has_service_call;
        
        if sub_needs.is_empty() && has_user_ask_only {
            eprintln!(
                "{}  ⚠️  WARNING: Plan only asks questions but doesn't declare a service capability.",
                indent
            );
            eprintln!(
                "{}  ⚠️  Expected plan to declare and call a specific service (e.g., restaurant.api.search)",
                indent
            );
        }
        
        // Check for self-referencing cycles: if the plan only calls itself, prevent infinite recursion
        let mut sub_needs = sub_needs;
        if sub_needs.len() == 1 && sub_needs[0].capability_class == need.capability_class {
            eprintln!(
                "{}  ⚠️  WARNING: Plan only calls itself ({}) - this would cause infinite recursion.",
                indent, need.capability_class
            );
            eprintln!(
                "{}  ⚠️  Skipping further recursion for this capability.",
                indent
            );
            // Clear sub_needs to prevent recursion
            sub_needs.clear();
        }
        
        eprintln!(
            "{}  ✓ Found {} sub-capability needs",
            indent, sub_needs.len()
        );
        for sub_need in &sub_needs {
            eprintln!(
                "{}    • {} (inputs: {:?}, outputs: {:?})",
                indent, sub_need.capability_class, sub_need.required_inputs, sub_need.expected_outputs
            );
        }
        
        // Use queue-based approach to avoid async recursion issues
        // This processes sub-capabilities iteratively instead of recursively
        let mut sub_intents = Vec::new();
        let mut processing_queue: Vec<(CapabilityNeed, usize, Vec<String>)> = sub_needs
            .into_iter()
            .map(|need| {
                let mut visited = context.visited_intents.clone();
                visited.push(intent.intent_id.clone());
                (need, context.current_depth + 1, visited)
            })
            .collect();
        
        let marketplace = self.discovery_engine.get_marketplace();
        
        // Process queue until empty or max depth reached
        if !processing_queue.is_empty() {
            eprintln!("{}  → Processing {} sub-capabilities in queue...", indent, processing_queue.len());
        }
        
        while let Some((sub_need, depth, visited)) = processing_queue.pop() {
            let sub_indent = "  ".repeat(depth);
            
            // Check depth limit
            if depth > self.default_max_depth {
                eprintln!(
                    "{}⚠️  [Depth {}] Max depth {} reached for {}",
                    sub_indent, depth, self.default_max_depth, sub_need.capability_class
                );
                continue;
            }
            
            // Check cycle detection using a simpler approach
            // Check if we've already synthesized a capability with this class at this depth
            // We could enhance this to track capability classes separately, but for now
            // we rely on depth limits and the cycle detector in synthesize_as_intent
            
            // Try to discover the sub-capability in marketplace first
            eprintln!("{}  → [Depth {}] Checking marketplace for: {}", sub_indent, depth, sub_need.capability_class);
            if marketplace.get_capability(&sub_need.capability_class).await.is_some() {
                // Found in marketplace - no need to synthesize
                eprintln!("{}    ✓ Found in marketplace, skipping synthesis", sub_indent);
                continue;
            }
            
            // Check if this capability is currently being synthesized by a parent
            // by checking the cycle detector (which tracks visited capabilities in this synthesis tree)
            // If so, it's a true cycle (capability needs itself) - skip it gracefully
            // Note: go_deeper() clones the visited set, so we can check what would be in the deeper detector
            if self.cycle_detector.has_cycle(&sub_need.capability_class) {
                eprintln!(
                    "{}    ⚠️  Skipping: {} is already being synthesized in this path",
                    sub_indent, sub_need.capability_class
                );
                eprintln!(
                    "{}      (Cycle detected - capability would depend on itself)",
                    sub_indent
                );
                continue;
            }
            
            // Not found - synthesize it using queue-based approach
            eprintln!("{}    → Not found, synthesizing...", sub_indent);
            // Create a new context for this depth level
            let mut sub_context = DiscoveryContext::new(self.default_max_depth);
            sub_context.current_depth = depth;
            sub_context.visited_intents = visited.clone();
            
            // Synthesize the sub-capability
            let mut deeper_synthesizer = self.go_deeper();
            match deeper_synthesizer.synthesize_as_intent(&sub_need, &sub_context).await {
                Ok(synthesized) => {
                    eprintln!(
                        "{}    ✓ [Depth {}] Successfully synthesized: {}",
                        sub_indent, depth, synthesized.manifest.id
                    );
                    sub_intents.push(synthesized.manifest.id.clone());
                    
                    // Register synthesized sub-capability in marketplace
                    if let Err(e) = marketplace.register_capability_manifest(synthesized.manifest.clone()).await {
                        eprintln!(
                            "{}    ⚠️  Warning: Failed to register synthesized sub-capability {}: {}",
                            sub_indent, sub_need.capability_class, e
                        );
                    } else {
                        eprintln!("{}    ✓ Registered in marketplace", sub_indent);
                    }
                    
                    // Extract sub-needs from the synthesized capability's plan and add to queue
                    // This enables full multi-level recursive synthesis
                    if let Some(ref sub_plan) = synthesized.plan {
                        let sub_sub_needs = CapabilityNeedExtractor::extract_from_plan(sub_plan);
                        
                        if !sub_sub_needs.is_empty() {
                            eprintln!(
                                "{}    → Found {} sub-sub-capabilities, adding to queue...",
                                sub_indent, sub_sub_needs.len()
                            );
                        }
                        
                        // Add new sub-needs to the queue with incremented depth
                        for sub_sub_need in sub_sub_needs {
                            let capability_class = sub_sub_need.capability_class.clone();
                            let mut new_visited = visited.clone();
                            new_visited.push(synthesized.manifest.id.clone());
                            processing_queue.push((sub_sub_need, depth + 1, new_visited));
                            eprintln!(
                                "{}      • Queued: {} (depth {})",
                                sub_indent, capability_class, depth + 1
                            );
                        }
                    }
                }
                Err(e) => {
                    eprintln!(
                        "{}    ✗ [Depth {}] Failed to synthesize {}: {}",
                        sub_indent, depth, sub_need.capability_class, e
                    );
                }
            }
        }
        
        if sub_intents.is_empty() && !processing_queue.is_empty() {
            eprintln!("{}  → All sub-capabilities processed", indent);
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
        
        eprintln!(
            "{}✓ [Depth {}] Synthesis complete for: {} (sub-capabilities: {})",
            indent, depth, need.capability_class, sub_intents.len()
        );
        
        Ok(SynthesizedCapability {
            manifest,
            orchestrator_rtfs,
            plan: Some(plan),
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

