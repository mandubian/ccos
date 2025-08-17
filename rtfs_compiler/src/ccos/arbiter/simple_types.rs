//! Simplified Send+Sync types for LLM Arbiter
//!
//! This module provides simplified versions of CCOS types that are Send+Sync
//! for use in async contexts, avoiding the problematic Function types.

use std::collections::HashMap;
use serde::{Deserialize, Serialize};

/// Simplified Value type that is Send+Sync
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SimpleValue {
    Nil,
    Boolean(bool),
    Integer(i64),
    Float(f64),
    String(String),
    Timestamp(String),
    Uuid(String),
    ResourceHandle(String),
    Vector(Vec<SimpleValue>),
    List(Vec<SimpleValue>),
    Map(HashMap<String, SimpleValue>),
    Error(String),
}

impl From<SimpleValue> for crate::runtime::values::Value {
    fn from(simple: SimpleValue) -> Self {
        match simple {
            SimpleValue::Nil => crate::runtime::values::Value::Nil,
            SimpleValue::Boolean(b) => crate::runtime::values::Value::Boolean(b),
            SimpleValue::Integer(i) => crate::runtime::values::Value::Integer(i),
            SimpleValue::Float(f) => crate::runtime::values::Value::Float(f),
            SimpleValue::String(s) => crate::runtime::values::Value::String(s),
            SimpleValue::Timestamp(t) => crate::runtime::values::Value::Timestamp(t),
            SimpleValue::Uuid(u) => crate::runtime::values::Value::Uuid(u),
            SimpleValue::ResourceHandle(rh) => crate::runtime::values::Value::ResourceHandle(rh),
            SimpleValue::Vector(v) => crate::runtime::values::Value::Vector(v.into_iter().map(|x| x.into()).collect()),
            SimpleValue::List(l) => crate::runtime::values::Value::List(l.into_iter().map(|x| x.into()).collect()),
            SimpleValue::Map(m) => {
                let mut map = std::collections::HashMap::new();
                for (k, v) in m {
                    map.insert(crate::ast::MapKey::String(k), v.into());
                }
                crate::runtime::values::Value::Map(map)
            }
            SimpleValue::Error(e) => crate::runtime::values::Value::Error(crate::runtime::values::ErrorValue {
                message: e,
                stack_trace: None,
            }),
        }
    }
}

impl From<crate::runtime::values::Value> for SimpleValue {
    fn from(value: crate::runtime::values::Value) -> Self {
        match value {
            crate::runtime::values::Value::Nil => SimpleValue::Nil,
            crate::runtime::values::Value::Boolean(b) => SimpleValue::Boolean(b),
            crate::runtime::values::Value::Integer(i) => SimpleValue::Integer(i),
            crate::runtime::values::Value::Float(f) => SimpleValue::Float(f),
            crate::runtime::values::Value::String(s) => SimpleValue::String(s),
            crate::runtime::values::Value::Timestamp(t) => SimpleValue::Timestamp(t),
            crate::runtime::values::Value::Uuid(u) => SimpleValue::Uuid(u),
            crate::runtime::values::Value::ResourceHandle(rh) => SimpleValue::ResourceHandle(rh),
            crate::runtime::values::Value::Vector(v) => SimpleValue::Vector(v.into_iter().map(|x| x.into()).collect()),
            crate::runtime::values::Value::List(l) => SimpleValue::List(l.into_iter().map(|x| x.into()).collect()),
            crate::runtime::values::Value::Map(m) => {
                let mut map = HashMap::new();
                for (k, v) in m {
                    match k {
                        crate::ast::MapKey::String(s) => map.insert(s, v.into()),
                        crate::ast::MapKey::Keyword(k) => map.insert(format!(":{}", k.0), v.into()),
                        crate::ast::MapKey::Symbol(s) => map.insert(s.0, v.into()),
                    };
                }
                SimpleValue::Map(map)
            }
            crate::runtime::values::Value::Error(e) => SimpleValue::Error(e.message),
            // Skip function types as they're not needed for arbiter context
            crate::runtime::values::Value::Function(_) => SimpleValue::String("#<function>".to_string()),
            crate::runtime::values::Value::FunctionPlaceholder(_) => SimpleValue::String("#<function-placeholder>".to_string()),
            crate::runtime::values::Value::Symbol(s) => SimpleValue::String(s.0),
            crate::runtime::values::Value::Keyword(k) => SimpleValue::String(format!(":{}", k.0)),
        }
    }
}

/// Simplified Intent type that is Send+Sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleIntent {
    pub intent_id: String,
    pub name: Option<String>,
    pub goal: String,
    pub original_request: String,
    pub constraints: HashMap<String, SimpleValue>,
    pub preferences: HashMap<String, SimpleValue>,
    pub success_criteria: Option<SimpleValue>,
    pub status: String,
    pub created_at: u64,
    pub updated_at: u64,
    pub metadata: HashMap<String, SimpleValue>,
}

impl From<SimpleIntent> for crate::ccos::types::Intent {
    fn from(simple: SimpleIntent) -> Self {
        Self {
            intent_id: simple.intent_id,
            name: simple.name,
            goal: simple.goal,
            original_request: simple.original_request,
            constraints: simple.constraints.into_iter().map(|(k, v)| (k, v.into())).collect(),
            preferences: simple.preferences.into_iter().map(|(k, v)| (k, v.into())).collect(),
            success_criteria: simple.success_criteria.map(|v| v.into()),
            status: crate::ccos::types::IntentStatus::Active, // Default mapping
            created_at: simple.created_at,
            updated_at: simple.updated_at,
            metadata: simple.metadata.into_iter().map(|(k, v)| (k, v.into())).collect(),
        }
    }
}

impl From<crate::ccos::types::Intent> for SimpleIntent {
    fn from(intent: crate::ccos::types::Intent) -> Self {
        Self {
            intent_id: intent.intent_id,
            name: intent.name,
            goal: intent.goal,
            original_request: intent.original_request,
            constraints: intent.constraints.into_iter().map(|(k, v)| (k, v.into())).collect(),
            preferences: intent.preferences.into_iter().map(|(k, v)| (k, v.into())).collect(),
            success_criteria: intent.success_criteria.map(|v| v.into()),
            status: "active".to_string(), // Simplified mapping
            created_at: intent.created_at,
            updated_at: intent.updated_at,
            metadata: intent.metadata.into_iter().map(|(k, v)| (k, v.into())).collect(),
        }
    }
}

/// Simplified Plan type that is Send+Sync
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimplePlan {
    pub plan_id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub intent_ids: Vec<String>,
    pub language: String,
    pub body: String,
    pub status: String,
    pub created_at: u64,
    pub metadata: HashMap<String, SimpleValue>,
    pub triggered_by: String,
    pub generation_context: SimpleGenerationContext,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleGenerationContext {
    pub arbiter_version: String,
    pub generation_timestamp: u64,
    pub reasoning_trace: Option<String>,
}

impl From<SimplePlan> for crate::ccos::types::Plan {
    fn from(simple: SimplePlan) -> Self {
        Self {
            plan_id: simple.plan_id,
            name: simple.name,
            description: simple.description,
            intent_ids: simple.intent_ids,
            language: crate::ccos::types::PlanLanguage::Rtfs20, // Default mapping
            body: crate::ccos::types::PlanBody::Rtfs(simple.body),
            status: crate::ccos::types::PlanStatus::Draft, // Default mapping
            created_at: simple.created_at,
            metadata: simple.metadata.into_iter().map(|(k, v)| (k, v.into())).collect(),
            triggered_by: crate::ccos::types::TriggerSource::HumanRequest, // Default mapping
            generation_context: crate::ccos::types::GenerationContext {
                arbiter_version: simple.generation_context.arbiter_version,
                generation_timestamp: simple.generation_context.generation_timestamp,
                reasoning_trace: simple.generation_context.reasoning_trace,
            },
        }
    }
}

impl From<crate::ccos::types::Plan> for SimplePlan {
    fn from(plan: crate::ccos::types::Plan) -> Self {
        Self {
            plan_id: plan.plan_id,
            name: plan.name,
            description: plan.description,
            intent_ids: plan.intent_ids,
            language: "rtfs20".to_string(), // Simplified mapping
            body: match plan.body {
                crate::ccos::types::PlanBody::Rtfs(s) => s,
            },
            status: "draft".to_string(), // Simplified mapping
            created_at: plan.created_at,
            metadata: plan.metadata.into_iter().map(|(k, v)| (k, v.into())).collect(),
            triggered_by: "human_request".to_string(), // Simplified mapping
            generation_context: SimpleGenerationContext {
                arbiter_version: plan.generation_context.arbiter_version,
                generation_timestamp: plan.generation_context.generation_timestamp,
                reasoning_trace: plan.generation_context.reasoning_trace,
            },
        }
    }
}

// Ensure these types are Send+Sync
unsafe impl Send for SimpleValue {}
unsafe impl Sync for SimpleValue {}
unsafe impl Send for SimpleIntent {}
unsafe impl Sync for SimpleIntent {}
unsafe impl Send for SimplePlan {}
unsafe impl Sync for SimplePlan {}
unsafe impl Send for SimpleGenerationContext {}
unsafe impl Sync for SimpleGenerationContext {}
