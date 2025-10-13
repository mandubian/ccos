use std::collections::HashMap;

#[derive(Debug, Clone)]
pub enum ParamType {
    Enum,
    String,
    Integer,
    Float,
    Boolean,
    Unknown,
}

#[derive(Debug, Clone)]
pub struct ParamMeta {
    pub param_type: ParamType,
    pub required: bool,
    pub first_turn: usize,
    pub last_turn: usize,
    pub questions_asked: usize,
    pub enum_values: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct Schema {
    pub params: HashMap<String, ParamMeta>,
}

#[derive(Debug, Clone)]
pub struct Metrics {
    pub coverage: f64,
    pub redundancy: f64,
    pub enum_specificity: f64,
}

pub fn extract_with_metrics(_chain: &crate::ccos::causal_chain::CausalChain) -> (Schema, Metrics) {
    let schema = Schema {
        params: HashMap::new(),
    };
    let metrics = Metrics {
        coverage: 0.0,
        redundancy: 0.0,
        enum_specificity: 0.0,
    };
    (schema, metrics)
}
