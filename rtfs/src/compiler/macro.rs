
use crate::ast::{Expression, ParamDef, Symbol};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub name: Symbol,
    pub params: Vec<ParamDef>,
    pub body: Vec<Expression>,
}
