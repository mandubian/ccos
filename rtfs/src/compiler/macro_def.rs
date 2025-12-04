use crate::ast::{Expression, ParamDef, Symbol};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MacroDef {
    pub name: Symbol,
    pub params: Vec<ParamDef>,
    pub variadic_param: Option<ParamDef>,
    pub body: Vec<Expression>,
}
