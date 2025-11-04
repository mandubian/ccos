//! Local RTFS synthesizer for simple operations
//!
//! This module detects simple data transformation operations (filter, map, format, display, etc.)
//! and synthesizes them as local RTFS implementations using stdlib functions rather than
//! marking them as incomplete or requiring external services.

use crate::discovery::need_extractor::CapabilityNeed;
use crate::capability_marketplace::types::{CapabilityManifest, LocalCapability};
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use std::sync::Arc;

/// Detects if a capability represents a simple local operation that can be synthesized
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleOperation {
    Filter,
    Map,
    Format,
    Display,
    Sort,
    Reduce,
    Transform,
    Unknown,
}

/// Analyzes a capability need to determine if it's a simple local operation
pub struct LocalSynthesizer;

impl LocalSynthesizer {
    /// Detect if this capability need represents a simple operation
    pub fn detect_simple_operation(need: &CapabilityNeed) -> SimpleOperation {
        let class_lower = need.capability_class.to_lowercase();
        let rationale_lower = need.rationale.to_lowercase();
        
        // Check class name patterns
        if class_lower.contains("filter") || class_lower.contains(".filter") {
            return SimpleOperation::Filter;
        }
        if class_lower.contains("map") || class_lower.contains(".map") {
            return SimpleOperation::Map;
        }
        if class_lower.contains("format") || class_lower.contains(".format") {
            return SimpleOperation::Format;
        }
        if class_lower.contains("display") || class_lower.contains(".display") || 
           class_lower.contains("show") || class_lower.contains(".show") ||
           class_lower.contains("print") || class_lower.contains(".print") {
            return SimpleOperation::Display;
        }
        if class_lower.contains("sort") || class_lower.contains(".sort") {
            return SimpleOperation::Sort;
        }
        if class_lower.contains("reduce") || class_lower.contains(".reduce") {
            return SimpleOperation::Reduce;
        }
        if class_lower.contains("transform") || class_lower.contains(".transform") {
            return SimpleOperation::Transform;
        }
        
        // Check rationale for operation hints
        if rationale_lower.contains("filter") && 
           (rationale_lower.contains("by") || rationale_lower.contains("topic") || rationale_lower.contains("keyword")) {
            return SimpleOperation::Filter;
        }
        if rationale_lower.contains("display") || rationale_lower.contains("present") || 
           rationale_lower.contains("show") || rationale_lower.contains("list") {
            return SimpleOperation::Display;
        }
        if rationale_lower.contains("format") || rationale_lower.contains("convert") {
            return SimpleOperation::Format;
        }
        
        SimpleOperation::Unknown
    }
    
    /// Check if a capability can be synthesized locally
    pub fn can_synthesize_locally(need: &CapabilityNeed) -> bool {
        Self::detect_simple_operation(need) != SimpleOperation::Unknown
    }
    
    /// Synthesize a simple operation as a local RTFS capability
    pub fn synthesize_locally(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        let operation = Self::detect_simple_operation(need);
        
        match operation {
            SimpleOperation::Filter => Self::synthesize_filter(need),
            SimpleOperation::Display => Self::synthesize_display(need),
            SimpleOperation::Format => Self::synthesize_format(need),
            SimpleOperation::Map => Self::synthesize_map(need),
            SimpleOperation::Sort => Self::synthesize_sort(need),
            SimpleOperation::Reduce => Self::synthesize_reduce(need),
            SimpleOperation::Transform => Self::synthesize_transform(need),
            SimpleOperation::Unknown => Err(RuntimeError::Generic(
                format!("Cannot synthesize unknown operation: {}", need.capability_class)
            )),
        }
    }
    
    /// Synthesize a filter operation
    fn synthesize_filter(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        // Extract inputs/outputs
        let collection_input = need.required_inputs.iter()
            .find(|i| i.contains("list") || i.contains("items") || i.contains("collection") || 
                     i.contains("issues") || i.contains("data"))
            .cloned()
            .unwrap_or_else(|| {
                if !need.required_inputs.is_empty() {
                    need.required_inputs[0].clone()
                } else {
                    "items".to_string()
                }
            });
        
        let topic_input = need.required_inputs.iter()
            .find(|i| i.contains("topic") || i.contains("keyword") || i.contains("filter"))
            .cloned()
            .unwrap_or_else(|| "topic".to_string());
        
        let output = need.expected_outputs.first()
            .cloned()
            .unwrap_or_else(|| "filtered".to_string());
        
        // Generate RTFS implementation
        let rtfs_code = format!(
            r#"(fn [input]
  (let [
    {} (get input :{})
    {} (get input :{})
    filtered-items (filter
      (fn [item]
        (let [
          item-str (str item)
          title-str (str (get item :title ""))
          body-str (str (get item :body ""))
          topic-str (str {})
        ]
          (or
            (string-contains? title-str topic-str)
            (string-contains? body-str topic-str)
            (string-contains? item-str topic-str)
          )
        )
      )
      {}
    )
  ]
    {{:{} filtered-items}})
)"#,
            collection_input, collection_input,
            topic_input, topic_input,
            topic_input,
            collection_input,
            output
        );
        
        // Store RTFS code in metadata first (before moving into closure)
        let mut metadata = std::collections::HashMap::new();
        let rtfs_code_for_metadata = rtfs_code.clone();
        metadata.insert("rtfs_implementation".to_string(), rtfs_code_for_metadata);
        
        // Create handler that executes the RTFS
        let handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync> = 
            Arc::new(move |input: &Value| {
                // For now, return a placeholder that indicates this is synthesized
                // TODO: Actually execute RTFS code in runtime
                let mut result = std::collections::HashMap::new();
                result.insert(
                    rtfs::ast::MapKey::String(output.clone()),
                    Value::String(format!("[Synthesized filter operation - requires RTFS execution: {}]", rtfs_code)),
                );
                Ok(Value::Map(result))
            });
        metadata.insert("synthesis_method".to_string(), "local_rtfs".to_string());
        metadata.insert("operation_type".to_string(), "filter".to_string());
        
        let mut manifest = CapabilityManifest::new(
            need.capability_class.clone(),
            format!("Local filter: {}", need.capability_class),
            format!("Synthesized local filter operation: {}", need.rationale),
            crate::capability_marketplace::types::ProviderType::Local(LocalCapability {
                handler,
            }),
            "1.0.0".to_string(),
        );
        manifest.metadata = metadata;
        Ok(manifest)
    }
    
    /// Synthesize a display operation
    fn synthesize_display(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        let input = need.required_inputs.first()
            .cloned()
            .unwrap_or_else(|| "items".to_string());
        
        let output = need.expected_outputs.first()
            .cloned()
            .unwrap_or_else(|| "displayed".to_string());
        
        let rtfs_code = format!(
            r#"(fn [input]
  (let [
    {} (get input :{})
    formatted-items (map
      (fn [item]
        (let [
          title (str (get item :title ""))
          body (str (get item :body ""))
        ]
          (str "• " title " - " body)
        )
      )
      {}
    )
    result (reduce
      (fn [acc item-str]
        (if (empty? acc)
          item-str
          (str acc "\n" item-str)
        )
      )
      formatted-items
      ""
    )
  ]
    (log result)
    {{:{} result}})
)"#,
            input, input,
            input,
            output
        );
        
        // Store RTFS code in metadata first (before moving into closure)
        let mut metadata = std::collections::HashMap::new();
        let rtfs_code_for_metadata = rtfs_code.clone();
        metadata.insert("rtfs_implementation".to_string(), rtfs_code_for_metadata);
        
        let handler: Arc<dyn Fn(&Value) -> RuntimeResult<Value> + Send + Sync> = 
            Arc::new(move |input: &Value| {
                let mut result = std::collections::HashMap::new();
                result.insert(
                    rtfs::ast::MapKey::String(output.clone()),
                    Value::String(format!("[Synthesized display operation - requires RTFS execution: {}]", rtfs_code)),
                );
                Ok(Value::Map(result))
            });
        metadata.insert("synthesis_method".to_string(), "local_rtfs".to_string());
        metadata.insert("operation_type".to_string(), "display".to_string());
        
        let mut manifest = CapabilityManifest::new(
            need.capability_class.clone(),
            format!("Local display: {}", need.capability_class),
            format!("Synthesized local display operation: {}", need.rationale),
            crate::capability_marketplace::types::ProviderType::Local(LocalCapability {
                handler,
            }),
            "1.0.0".to_string(),
        );
        manifest.metadata = metadata;
        Ok(manifest)
    }
    
    /// Synthesize a format operation
    fn synthesize_format(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        // Similar to display but focused on formatting
        Self::synthesize_display(need)
    }
    
    /// Synthesize a map operation
    fn synthesize_map(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        Err(RuntimeError::Generic("Map synthesis not yet implemented".to_string()))
    }
    
    /// Synthesize a sort operation
    fn synthesize_sort(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        Err(RuntimeError::Generic("Sort synthesis not yet implemented".to_string()))
    }
    
    /// Synthesize a reduce operation
    fn synthesize_reduce(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        Err(RuntimeError::Generic("Reduce synthesis not yet implemented".to_string()))
    }
    
    /// Synthesize a transform operation
    fn synthesize_transform(need: &CapabilityNeed) -> RuntimeResult<CapabilityManifest> {
        Err(RuntimeError::Generic("Transform synthesis not yet implemented".to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_detect_filter_operation() {
        let need = CapabilityNeed::new(
            "text.filter.by-topic".to_string(),
            vec!["issues".to_string(), "topic".to_string()],
            vec!["filtered_issues".to_string()],
            "Filter issues by topic".to_string(),
        );
        
        assert_eq!(LocalSynthesizer::detect_simple_operation(&need), SimpleOperation::Filter);
        assert!(LocalSynthesizer::can_synthesize_locally(&need));
    }
    
    #[test]
    fn test_detect_display_operation() {
        let need = CapabilityNeed::new(
            "ui.display.list".to_string(),
            vec!["items".to_string()],
            vec!["displayed".to_string()],
            "Display list of items".to_string(),
        );
        
        assert_eq!(LocalSynthesizer::detect_simple_operation(&need), SimpleOperation::Display);
        assert!(LocalSynthesizer::can_synthesize_locally(&need));
    }
    
    #[test]
    fn test_detect_unknown_operation() {
        let need = CapabilityNeed::new(
            "external.api.call".to_string(),
            vec!["input".to_string()],
            vec!["output".to_string()],
            "Call external API".to_string(),
        );
        
        assert_eq!(LocalSynthesizer::detect_simple_operation(&need), SimpleOperation::Unknown);
        assert!(!LocalSynthesizer::can_synthesize_locally(&need));
    }
}
