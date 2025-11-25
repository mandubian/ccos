//! Catalog resolution strategy
//!
//! Resolves intents using the CCOS capability catalog (registered capabilities).

use std::sync::Arc;
use async_trait::async_trait;

use super::semantic::{CapabilityCatalog, CapabilityInfo};
use super::{ResolutionContext, ResolutionError, ResolutionStrategy, ResolvedCapability};
use crate::planner::modular_planner::types::{IntentType, SubIntent};

/// Catalog resolution strategy.
/// 
/// Searches the local capability catalog for matching capabilities.
/// This is simpler than SemanticResolution and doesn't use embeddings.
pub struct CatalogResolution {
    catalog: Arc<dyn CapabilityCatalog>,
}

impl CatalogResolution {
    pub fn new(catalog: Arc<dyn CapabilityCatalog>) -> Self {
        Self { catalog }
    }
    
    /// Map intent to built-in capability if applicable
    fn check_builtin(&self, intent: &SubIntent) -> Option<ResolvedCapability> {
        match &intent.intent_type {
            IntentType::UserInput { prompt_topic } => {
                let mut args = std::collections::HashMap::new();
                args.insert("prompt".to_string(), format!("Please provide: {}", prompt_topic));
                
                Some(ResolvedCapability::BuiltIn {
                    capability_id: "ccos.user.ask".to_string(),
                    arguments: args,
                })
            }
            IntentType::Output { format: _ } => {
                let mut args = std::collections::HashMap::new();
                args.insert("message".to_string(), intent.description.clone());
                
                Some(ResolvedCapability::BuiltIn {
                    capability_id: "ccos.io.println".to_string(),
                    arguments: args,
                })
            }
            _ => None,
        }
    }
    
    /// Search catalog for matching capability
    async fn search_catalog(&self, intent: &SubIntent) -> Option<(CapabilityInfo, f64)> {
        let query = &intent.description;
        let candidates = self.catalog.search(query, 5).await;
        
        if candidates.is_empty() {
            return None;
        }
        
        // Simple scoring - first match with basic validation
        for cap in candidates {
            let score = self.score_capability(intent, &cap);
            if score > 0.2 {
                return Some((cap, score));
            }
        }
        
        None
    }
    
    /// Score capability match
    fn score_capability(&self, intent: &SubIntent, cap: &CapabilityInfo) -> f64 {
        let cap_lower = format!("{} {}", cap.name, cap.description).to_lowercase();
        let desc_lower = intent.description.to_lowercase();
        
        let words: Vec<&str> = desc_lower.split_whitespace().collect();
        let mut matches = 0;
        
        for word in &words {
            if word.len() > 2 && cap_lower.contains(word) {
                matches += 1;
            }
        }
        
        if words.is_empty() {
            return 0.0;
        }
        
        matches as f64 / words.len() as f64
    }
}

#[async_trait(?Send)]
impl ResolutionStrategy for CatalogResolution {
    fn name(&self) -> &str {
        "catalog"
    }
    
    fn can_handle(&self, _intent: &SubIntent) -> bool {
        true // Can try to handle any intent
    }
    
    async fn resolve(
        &self,
        intent: &SubIntent,
        _context: &ResolutionContext,
    ) -> Result<ResolvedCapability, ResolutionError> {
        // Check for built-in first
        if let Some(builtin) = self.check_builtin(intent) {
            return Ok(builtin);
        }
        
        // Search catalog
        if let Some((cap, score)) = self.search_catalog(intent).await {
            let arguments = intent.extracted_params
                .iter()
                .filter(|(k, _)| !k.starts_with('_'))
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect();
            
            return Ok(ResolvedCapability::Local {
                capability_id: cap.id,
                arguments,
                confidence: score,
            });
        }
        
        Err(ResolutionError::NotFound(format!(
            "No catalog capability found for: {}",
            intent.description
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    struct MockCatalog;
    
    #[async_trait(?Send)]
    impl CapabilityCatalog for MockCatalog {
        async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
            vec![]
        }
        
        async fn get_capability(&self, _id: &str) -> Option<CapabilityInfo> {
            None
        }
        
        async fn search(&self, _query: &str, _limit: usize) -> Vec<CapabilityInfo> {
            vec![]
        }
    }
    
    #[tokio::test]
    async fn test_builtin_user_input() {
        let catalog = Arc::new(MockCatalog);
        let strategy = CatalogResolution::new(catalog);
        let context = ResolutionContext::new();
        
        let intent = SubIntent::new(
            "Ask user for page size",
            IntentType::UserInput { prompt_topic: "page size".to_string() },
        );
        
        let result = strategy.resolve(&intent, &context).await.expect("Should resolve");
        
        match result {
            ResolvedCapability::BuiltIn { capability_id, arguments } => {
                assert_eq!(capability_id, "ccos.user.ask");
                assert!(arguments.get("prompt").unwrap().contains("page size"));
            }
            _ => panic!("Expected BuiltIn capability"),
        }
    }
}
