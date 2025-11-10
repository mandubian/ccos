//! Primitive synthesis framework.
//!
//! This module introduces a schema-aware primitive templating system that can
//! synthesize RTFS capabilities (e.g. filter/map/reduce) without mutating the
//! legacy `local_synthesizer`.  Demos and future planners can opt-in to this
//! new pipeline while the original smart assistant demo remains untouched.

use std::collections::HashMap;

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

use crate::capability_marketplace::types::CapabilityManifest;
use crate::discovery::need_extractor::CapabilityNeed;
use rtfs::ast::{Keyword, MapTypeEntry, TypeExpr};

pub mod executor;
mod templates;

pub use executor::RestrictedRtfsExecutor;
pub use templates::{
    FilterPrimitiveTemplate, GroupByPrimitiveTemplate, JoinPrimitiveTemplate, MapPrimitiveTemplate,
    PrimitiveTemplateId, ProjectPrimitiveTemplate, ReducePrimitiveTemplate, SortPrimitiveTemplate,
};

/// Context describing the capability need plus rich type information that a
/// primitive template can consult.
#[derive(Debug, Clone)]
pub struct PrimitiveContext<'a> {
    pub need: &'a CapabilityNeed,
    pub input_schemas: HashMap<String, TypeExpr>,
    pub output_schemas: HashMap<String, TypeExpr>,
    pub annotations: JsonValue,
}

impl<'a> PrimitiveContext<'a> {
    pub fn new(
        need: &'a CapabilityNeed,
        input_schemas: HashMap<String, TypeExpr>,
        output_schemas: HashMap<String, TypeExpr>,
        annotations: JsonValue,
    ) -> Self {
        Self {
            need,
            input_schemas,
            output_schemas,
            annotations,
        }
    }

    pub fn from_type_schemas(
        need: &'a CapabilityNeed,
        input_schema: Option<&TypeExpr>,
        output_schema: Option<&TypeExpr>,
        annotations: JsonValue,
    ) -> Self {
        let input_map = input_schema
            .map(|schema| binding_map_from_type_expr(schema, &need.required_inputs))
            .unwrap_or_else(HashMap::new);
        let output_map = output_schema
            .map(|schema| binding_map_from_type_expr(schema, &need.expected_outputs))
            .unwrap_or_else(HashMap::new);

        Self {
            need,
            input_schemas: input_map,
            output_schemas: output_map,
            annotations,
        }
    }

    pub fn annotation_path<'b>(&'b self, path: &[&str]) -> Option<&'b JsonValue> {
        let mut value = &self.annotations;
        for segment in path {
            match value {
                JsonValue::Object(map) => {
                    value = map.get(*segment)?;
                }
                _ => return None,
            }
        }
        Some(value)
    }

    pub fn from_manifest(
        need: &'a CapabilityNeed,
        manifest: &CapabilityManifest,
        annotations: JsonValue,
    ) -> Self {
        Self::from_type_schemas(
            need,
            manifest.input_schema.as_ref(),
            manifest.output_schema.as_ref(),
            annotations,
        )
    }

    pub fn input_schema_for(&self, binding: &str) -> Option<&TypeExpr> {
        lookup_binding(&self.input_schemas, binding)
    }

    pub fn output_schema_for(&self, binding: &str) -> Option<&TypeExpr> {
        lookup_binding(&self.output_schemas, binding)
    }

    pub fn aggregated_input_schema(&self) -> TypeExpr {
        map_type_from(&self.input_schemas)
    }

    pub fn aggregated_output_schema(&self) -> TypeExpr {
        map_type_from(&self.output_schemas)
    }
}

/// Result of synthesizing a primitive capability.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SynthesizedPrimitive {
    pub capability_id: String,
    pub primitive_id: PrimitiveTemplateId,
    pub rtfs_code: String,
    pub input_schema: TypeExpr,
    pub output_schema: TypeExpr,
    pub metadata: JsonValue,
}

/// Trait implemented by each primitive template (filter/map/reduce/etc).
pub trait PrimitiveTemplate: Send + Sync {
    fn id(&self) -> PrimitiveTemplateId;
    fn matches(&self, ctx: &PrimitiveContext) -> Result<bool>;
    fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive>;
}

/// Registry that holds all available primitive templates.
pub struct PrimitiveRegistry {
    templates: Vec<Box<dyn PrimitiveTemplate>>,
}

impl PrimitiveRegistry {
    pub fn new() -> Self {
        let mut registry = Self {
            templates: Vec::new(),
        };
        registry.register(Box::new(FilterPrimitiveTemplate::default()));
        registry.register(Box::new(MapPrimitiveTemplate::default()));
        registry.register(Box::new(ProjectPrimitiveTemplate::default()));
        registry.register(Box::new(ReducePrimitiveTemplate::default()));
        registry.register(Box::new(SortPrimitiveTemplate::default()));
        registry.register(Box::new(GroupByPrimitiveTemplate::default()));
        registry.register(Box::new(JoinPrimitiveTemplate::default()));
        registry
    }

    pub fn register(&mut self, template: Box<dyn PrimitiveTemplate>) {
        self.templates.push(template);
    }

    /// Find the first primitive template that matches the context and synthesize
    /// a capability from it.
    pub fn synthesize(&self, ctx: &PrimitiveContext) -> Result<SynthesizedPrimitive> {
        for template in &self.templates {
            if template.matches(ctx)? {
                return template.synthesize(ctx);
            }
        }
        Err(anyhow!(
            "No primitive template matched capability '{}'",
            ctx.need.capability_class
        ))
    }
}

/// Helper to fetch a string value from annotations.
pub(crate) fn annotation_string(ctx: &PrimitiveContext, path: &[&str]) -> Option<String> {
    ctx.annotation_path(path)
        .and_then(|value| value.as_str().map(|s| s.to_string()))
}

/// Helper to fetch a string vector from annotations.
pub(crate) fn annotation_string_vec(ctx: &PrimitiveContext, path: &[&str]) -> Option<Vec<String>> {
    ctx.annotation_path(path).and_then(|value| {
        value.as_array().map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
    })
}

/// Helper to fetch a string->string mapping from annotations.
pub(crate) fn annotation_string_map(
    ctx: &PrimitiveContext,
    path: &[&str],
) -> Option<HashMap<String, String>> {
    ctx.annotation_path(path).and_then(|value| {
        value.as_object().map(|obj| {
            obj.iter()
                .filter_map(|(k, v)| v.as_str().map(|val| (k.clone(), val.to_string())))
                .collect()
        })
    })
}

fn lookup_binding<'a>(
    bindings: &'a HashMap<String, TypeExpr>,
    binding: &str,
) -> Option<&'a TypeExpr> {
    bindings
        .get(binding)
        .or_else(|| bindings.get(binding.trim_start_matches(':')))
}

fn keyword_for(binding: &str) -> Keyword {
    Keyword(binding.trim_start_matches(':').to_string())
}

fn map_type_from(bindings: &HashMap<String, TypeExpr>) -> TypeExpr {
    if bindings.is_empty() {
        return TypeExpr::Any;
    }

    let mut entries: Vec<MapTypeEntry> = bindings
        .iter()
        .map(|(name, ty)| MapTypeEntry {
            key: keyword_for(name),
            value_type: Box::new(ty.clone()),
            optional: false,
        })
        .collect();
    entries.sort_by(|a, b| a.key.0.cmp(&b.key.0));

    TypeExpr::Map {
        entries,
        wildcard: None,
    }
}

fn binding_map_from_type_expr(
    schema: &TypeExpr,
    fallback_names: &[String],
) -> HashMap<String, TypeExpr> {
    match schema {
        TypeExpr::Map { entries, .. } => {
            let mut map = HashMap::new();
            for entry in entries {
                let mut ty = (*entry.value_type).clone();
                if entry.optional {
                    ty = TypeExpr::Optional(Box::new(ty));
                }
                map.insert(format!(":{}", entry.key.0), ty);
            }
            map
        }
        other => {
            let mut map = HashMap::new();
            if let Some(name) = fallback_names.first() {
                map.insert(name.clone(), other.clone());
            }
            map
        }
    }
}
