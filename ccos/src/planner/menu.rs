use std::collections::{BTreeMap, BTreeSet};

use crate::capability_marketplace::types::{CapabilityManifest, ProviderType};
use crate::synthesis::schema_serializer::type_expr_to_rtfs_compact;
use rtfs::ast::{Literal, MapTypeEntry, PrimitiveType, TypeExpr};
use serde::{Deserialize, Serialize};

/// Provenance of a capability menu entry.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum CapabilityProvenance {
    Catalog,
    MarketplaceOverride,
    AliasResolution,
    Synthetic,
    LlmSuggested,
    Unknown,
}

/// Menu entry exposed to the planner prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityMenuEntry {
    pub id: String,
    pub provider_label: String,
    pub description: String,
    pub required_inputs: Vec<String>,
    pub optional_inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub provenance: CapabilityProvenance,
    pub score: Option<f64>,
    pub metadata: BTreeMap<String, String>,
}

impl CapabilityMenuEntry {
    pub fn new<T: Into<String>>(
        id: T,
        provider_label: T,
        description: T,
        provenance: CapabilityProvenance,
    ) -> Self {
        Self {
            id: id.into(),
            provider_label: provider_label.into(),
            description: description.into(),
            required_inputs: Vec::new(),
            optional_inputs: Vec::new(),
            outputs: Vec::new(),
            provenance,
            score: None,
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_required_inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.required_inputs = inputs.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_optional_inputs<I, S>(mut self, inputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.optional_inputs = inputs.into_iter().map(Into::into).collect();
        self
    }

    pub fn with_outputs<I, S>(mut self, outputs: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.outputs = outputs.into_iter().map(Into::into).collect();
        self
    }

    pub fn is_synthetic(&self) -> bool {
        matches!(self.provenance, CapabilityProvenance::Synthetic)
    }

    /// Extract function parameter names (those marked with "(function - cannot be passed directly)")
    pub fn function_parameters(&self) -> Vec<String> {
        let mut func_params = Vec::new();
        for param in &self.required_inputs {
            if param.contains("(function - cannot be passed directly)") {
                if let Some(name) = param.split(" (function").next() {
                    func_params.push(name.to_string());
                }
            }
        }
        for param in &self.optional_inputs {
            if param.contains("(function - cannot be passed directly)") {
                if let Some(name) = param.split(" (function").next() {
                    func_params.push(name.to_string());
                }
            }
        }
        func_params
    }
}

/// Build `CapabilityMenuEntry` from a manifest.
pub fn menu_entry_from_manifest(
    manifest: &CapabilityManifest,
    score: Option<f64>,
) -> CapabilityMenuEntry {
    let provider_label = provider_to_label(&manifest.provider);
    let mut entry = CapabilityMenuEntry::new(
        manifest.id.clone(),
        provider_label,
        manifest.description.clone(),
        CapabilityProvenance::Catalog,
    );
    let (required_inputs, optional_inputs) = extract_input_fields(manifest.input_schema.as_ref());
    let outputs = extract_output_fields(manifest.output_schema.as_ref());

    entry.required_inputs = required_inputs;
    entry.optional_inputs = optional_inputs;
    entry.outputs = outputs;
    entry.score = score;
    entry.metadata = manifest.metadata.clone().into_iter().collect();
    entry
}

/// Helper to create a synthetic capability menu entry.
pub fn synthetic_entry<T: Into<String>>(
    id: T,
    provider_label: T,
    description: T,
) -> CapabilityMenuEntry {
    CapabilityMenuEntry::new(
        id,
        provider_label,
        description,
        CapabilityProvenance::Synthetic,
    )
}

fn extract_input_fields(schema: Option<&TypeExpr>) -> (Vec<String>, Vec<String>) {
    let mut required = BTreeSet::new();
    let mut optional = BTreeSet::new();

    if let Some(schema) = schema {
        collect_input_keys(schema, &mut required, &mut optional, false);
    }

    (
        required.into_iter().collect(),
        optional.into_iter().collect(),
    )
}

fn collect_input_keys(
    schema: &TypeExpr,
    required: &mut BTreeSet<String>,
    optional: &mut BTreeSet<String>,
    parent_optional: bool,
) {
    match schema {
        TypeExpr::Map { entries, .. } => {
            for entry in entries {
                insert_entry(entry, required, optional, parent_optional);
            }
        }
        TypeExpr::Optional(inner) => {
            collect_input_keys(inner, required, optional, true);
        }
        TypeExpr::Union(options) => {
            for opt in options {
                collect_input_keys(opt, required, optional, parent_optional);
            }
        }
        _ => {}
    }
}

fn insert_entry(
    entry: &MapTypeEntry,
    required: &mut BTreeSet<String>,
    optional: &mut BTreeSet<String>,
    parent_optional: bool,
) {
    let key = entry.key.0.clone();

    // Check if this is a function type (indicated by :fn keyword or Function variant)
    let is_function_type = is_function_type_expr(entry.value_type.as_ref());

    // Annotate function types with a warning suffix
    let annotated_key = if is_function_type {
        format!("{} (function - cannot be passed directly)", key)
    } else {
        key.clone()
    };

    let is_optional =
        entry.optional || parent_optional || type_expr_is_optional(entry.value_type.as_ref());
    // Debug: log MapTypeEntry optionality to help track down why fields like 'after' become required
    #[cfg(debug_assertions)]
    {
        let value_type_str = type_expr_to_rtfs_compact(entry.value_type.as_ref());
        eprintln!(
            "DEBUG: insert_entry key='{}' entry.optional={} parent_optional={} value_type={} computed_is_optional={}",
            key,
            entry.optional,
            parent_optional,
            value_type_str,
            is_optional
        );
        eprintln!("DEBUG: value_type_debug={:?}", entry.value_type);
    }
    if is_optional {
        required.remove(&key);
        optional.insert(annotated_key);
    } else {
        optional.remove(&key);
        required.insert(annotated_key);
    }
}

/// Check if a TypeExpr represents a function type
fn is_function_type_expr(expr: &TypeExpr) -> bool {
    match expr {
        // Check for Function variant
        TypeExpr::Function { .. } => true,
        // Check for :fn keyword type (parsed as Alias or Custom)
        TypeExpr::Alias(sym) => sym.0 == "fn" || sym.0 == ":fn",
        TypeExpr::Primitive(PrimitiveType::Custom(kw)) => {
            kw.0 == "fn" || kw.0 == ":fn" || kw.0.contains("fn")
        }
        // Check inside Optional/Refined
        TypeExpr::Optional(inner)
        | TypeExpr::Refined {
            base_type: inner, ..
        } => is_function_type_expr(inner),
        // Check unions
        TypeExpr::Union(options) => options.iter().any(is_function_type_expr),
        _ => false,
    }
}

fn extract_output_fields(schema: Option<&TypeExpr>) -> Vec<String> {
    let mut outputs = BTreeSet::new();
    if let Some(schema) = schema {
        collect_output_keys(schema, &mut outputs);
    }
    outputs.into_iter().collect()
}

fn collect_output_keys(schema: &TypeExpr, outputs: &mut BTreeSet<String>) {
    match schema {
        TypeExpr::Map { entries, .. } => {
            for entry in entries {
                outputs.insert(entry.key.0.clone());
                collect_output_keys(entry.value_type.as_ref(), outputs);
            }
        }
        TypeExpr::Vector(inner)
        | TypeExpr::Optional(inner)
        | TypeExpr::Refined {
            base_type: inner, ..
        } => collect_output_keys(inner, outputs),
        TypeExpr::Union(options) | TypeExpr::Intersection(options) => {
            for opt in options {
                collect_output_keys(opt, outputs);
            }
        }
        _ => {}
    }
}

fn type_expr_is_optional(expr: &TypeExpr) -> bool {
    match expr {
        TypeExpr::Optional(_) => true,
        TypeExpr::Primitive(PrimitiveType::Nil) => true,
        TypeExpr::Literal(Literal::Nil) => true,
        TypeExpr::Union(options) => options.iter().any(|opt| {
            matches!(
                opt,
                TypeExpr::Primitive(PrimitiveType::Nil) | TypeExpr::Literal(Literal::Nil)
            ) || type_expr_is_optional(opt)
        }),
        TypeExpr::Refined { base_type, .. } => type_expr_is_optional(base_type),
        // Heuristic: alias names that end with '?' (e.g., "string?") should be
        // treated as optional. This covers cases where the parser retained the
        // alias symbol instead of wrapping the type in a TypeExpr::Optional.
        TypeExpr::Alias(sym) => sym.0.ends_with('?'),
        _ => false,
    }
}

fn provider_to_label(provider: &ProviderType) -> String {
    match provider {
        ProviderType::Local(_) => "local".to_string(),
        ProviderType::Http(_) => "http".to_string(),
        ProviderType::MCP(_) => "mcp".to_string(),
        ProviderType::OpenApi(_) => "openapi".to_string(),
        ProviderType::Plugin(_) => "plugin".to_string(),
        ProviderType::RemoteRTFS(_) => "remote-rtfs".to_string(),
        ProviderType::Stream(_) => "stream".to_string(),
        ProviderType::Registry(_) => "registry".to_string(),
        ProviderType::A2A(_) => "agent".to_string(),
        ProviderType::Native(_) => "native".to_string(),
        ProviderType::Sandboxed(_) => "sandboxed".to_string(),
    }
}
