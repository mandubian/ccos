//! Discovery engine for finding and synthesizing capabilities

use crate::arbiter::delegating_arbiter::DelegatingArbiter;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::capability_marketplace::types::CapabilityManifest;
use crate::discovery::need_extractor::CapabilityNeed;
use crate::discovery::recursive_synthesizer::RecursiveSynthesizer;
use crate::intent_graph::IntentGraph;
use rtfs::runtime::error::RuntimeResult;
use std::sync::{Arc, Mutex};

/// Discovery engine that orchestrates the search for capabilities
pub struct DiscoveryEngine {
    marketplace: Arc<CapabilityMarketplace>,
    intent_graph: Arc<Mutex<IntentGraph>>,
    /// Optional delegating arbiter for recursive synthesis
    delegating_arbiter: Option<Arc<DelegatingArbiter>>,
}

impl DiscoveryEngine {
    /// Create a new discovery engine
    pub fn new(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
    ) -> Self {
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter: None,
        }
    }
    
    /// Create a new discovery engine with delegating arbiter for recursive synthesis
    pub fn new_with_arbiter(
        marketplace: Arc<CapabilityMarketplace>,
        intent_graph: Arc<Mutex<IntentGraph>>,
        delegating_arbiter: Option<Arc<DelegatingArbiter>>,
    ) -> Self {
        Self {
            marketplace,
            intent_graph,
            delegating_arbiter,
        }
    }
    
    /// Attempt to find a capability using the discovery priority chain
    pub async fn discover_capability(&self, need: &CapabilityNeed) -> RuntimeResult<DiscoveryResult> {
        // 1. Try local marketplace search first
        if let Some(manifest) = self.search_marketplace(need).await? {
            return Ok(DiscoveryResult::Found(manifest));
        }
        
        // 2. TODO: Try MCP registry search
        
        // 3. TODO: Try OpenAPI introspection
        
        // 4. Try recursive synthesis (if delegating arbiter is available)
        if let Some(ref arbiter) = self.delegating_arbiter {
            eprintln!("\n🔍 Attempting recursive synthesis for: {}", need.capability_class);
            eprintln!("   Rationale: {}", need.rationale);
            eprintln!("   Required inputs: {:?}", need.required_inputs);
            eprintln!("   Expected outputs: {:?}", need.expected_outputs);
            
            let context = DiscoveryContext::new(5); // Default max depth of 5
            let mut synthesizer = RecursiveSynthesizer::new(
                DiscoveryEngine::new(
                    Arc::clone(&self.marketplace),
                    Arc::clone(&self.intent_graph),
                ),
                Some(Arc::clone(arbiter)),
                5, // max depth
            );
            
            match synthesizer.synthesize_as_intent(need, &context).await {
                Ok(synthesized) => {
                    eprintln!("\n✓ Synthesis succeeded for: {}", need.capability_class);
                    // Register the synthesized capability in the marketplace
                    if let Err(e) = self.marketplace.register_capability_manifest(synthesized.manifest.clone()).await {
                        eprintln!(
                            "⚠️  Warning: Failed to register synthesized capability {}: {}",
                            need.capability_class, e
                        );
                    } else {
                        eprintln!("  → Registered as: {}", synthesized.manifest.id);
                    }
                    // Mark as synthesized (not just found)
                    return Ok(DiscoveryResult::Found(synthesized.manifest));
                }
                Err(e) => {
                    eprintln!(
                        "\n✗ Synthesis failed for {}: {}",
                        need.capability_class, e
                    );
                    // Synthesis failed - fall through to NotFound
                }
            }
        }
        
        // 5. Not found
        Ok(DiscoveryResult::NotFound)
    }
    
    /// Search the local marketplace for a matching capability
    async fn search_marketplace(&self, need: &CapabilityNeed) -> RuntimeResult<Option<CapabilityManifest>> {
        // First, try exact class match
        if let Some(manifest) = self.marketplace.get_capability(&need.capability_class).await {
            // Verify inputs/outputs compatibility
            if self.is_compatible(&manifest, need) {
                return Ok(Some(manifest));
            }
        }
        
        // TODO: Implement semantic search for approximate matches
        // For now, just return None if exact match not found
        
        Ok(None)
    }
    
    /// Check if a capability manifest is compatible with the need
    fn is_compatible(&self, _manifest: &CapabilityManifest, _need: &CapabilityNeed) -> bool {
        // For now, just check that it has inputs and outputs
        // TODO: Implement proper schema compatibility checking
        true
    }
    
    /// Get the marketplace (for cloning into recursive synthesizer)
    pub fn get_marketplace(&self) -> Arc<CapabilityMarketplace> {
        Arc::clone(&self.marketplace)
    }
    
    /// Get the intent graph (for cloning into recursive synthesizer)
    pub fn get_intent_graph(&self) -> Arc<Mutex<IntentGraph>> {
        Arc::clone(&self.intent_graph)
    }
    
    /// Find related capabilities in marketplace by namespace/pattern to provide as examples
    /// Returns up to `max_examples` capabilities that share the namespace or related keywords
    pub async fn find_related_capabilities(
        &self,
        capability_class: &str,
        max_examples: usize,
    ) -> Vec<CapabilityManifest> {
        // Extract namespace from capability class (e.g., "restaurant.api.search" -> "restaurant")
        let namespace = capability_class.split('.').next().unwrap_or("");
        
        if namespace.is_empty() {
            return vec![];
        }
        
        // Search for capabilities with the same namespace prefix using glob pattern
        // e.g., "restaurant.*" matches "restaurant.api.search", "restaurant.booking.reserve", etc.
        let pattern = format!("{}.*", namespace);
        self.marketplace.search_by_id(&pattern).await
            .into_iter()
            .take(max_examples)
            .collect()
    }
}

/// Result of a discovery attempt
#[derive(Debug, Clone)]
pub enum DiscoveryResult {
    /// Capability found
    Found(CapabilityManifest),
    /// Capability not found - needs synthesis or user input
    NotFound,
}

/// Discovery context for tracking discovery attempts
#[derive(Debug, Clone)]
pub struct DiscoveryContext {
    pub max_depth: usize,
    pub current_depth: usize,
    pub visited_intents: Vec<String>,
}

impl DiscoveryContext {
    /// Create a new discovery context
    pub fn new(max_depth: usize) -> Self {
        Self {
            max_depth,
            current_depth: 0,
            visited_intents: Vec::new(),
        }
    }
    
    /// Check if we can go deeper (prevent infinite recursion)
    pub fn can_go_deeper(&self) -> bool {
        self.current_depth < self.max_depth
    }
    
    /// Create a new context one level deeper
    pub fn go_deeper(&self) -> Self {
        Self {
            max_depth: self.max_depth,
            current_depth: self.current_depth + 1,
            visited_intents: self.visited_intents.clone(),
        }
    }
}

