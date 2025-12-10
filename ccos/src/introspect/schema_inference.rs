use rtfs::ast::TypeExpr;
use rtfs::ir::converter::IrConverter;
use rtfs::ir::core::IrNode;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};

/// Infer output schema using the RTFS compiler (AST → IR → IR type).
pub fn infer_output_schema(rtfs_src: &str) -> RuntimeResult<Option<TypeExpr>> {
    let parsed = rtfs::parser::parse(rtfs_src)
        .map_err(|e| RuntimeError::Generic(format!("parse error: {:?}", e)))?;
    if parsed.is_empty() {
        return Ok(None);
    }

    let mut converter = IrConverter::new();
    let mut ir_nodes: Vec<IrNode> = Vec::new();
    for top in parsed {
        let ir = match top {
            rtfs::ast::TopLevel::Expression(expr) => converter
                .convert_expression(expr)
                .map_err(|e| RuntimeError::Generic(format!("IR conversion error: {:?}", e)))?,
            rtfs::ast::TopLevel::Plan(p) => {
                let body_expr = p
                    .properties
                    .into_iter()
                    .find(|prop| prop.key.0 == "body")
                    .map(|prop| prop.value)
                    .unwrap_or(rtfs::ast::Expression::Literal(rtfs::ast::Literal::Nil));
                converter
                    .convert_expression(body_expr)
                    .map_err(|e| RuntimeError::Generic(format!("IR conversion error: {:?}", e)))?
            }
            _other => {
                // Skip non-expression top-levels for now
                continue;
            }
        };
        ir_nodes.push(ir);
    }

    let last_ir = match ir_nodes.last() {
        Some(n) => n,
        None => return Ok(None),
    };
    let ir_type = last_ir
        .ir_type()
        .cloned()
        .unwrap_or(rtfs::ir::core::IrType::Any);
    Ok(Some(ir_to_type_expr(&ir_type)))
}

/// Minimal IR → TypeExpr mapper (mirrors runtime/ir_runtime).
fn ir_to_type_expr(ir: &rtfs::ir::core::IrType) -> TypeExpr {
    use rtfs::ast::{ParamType, PrimitiveType};
    use rtfs::ir::core::IrType as IT;
    match ir {
        IT::Int => TypeExpr::Primitive(PrimitiveType::Int),
        IT::Float => TypeExpr::Primitive(PrimitiveType::Float),
        IT::String => TypeExpr::Primitive(PrimitiveType::String),
        IT::Bool => TypeExpr::Primitive(PrimitiveType::Bool),
        IT::Nil => TypeExpr::Primitive(PrimitiveType::Nil),
        IT::Keyword => TypeExpr::Primitive(PrimitiveType::Keyword),
        IT::Symbol => TypeExpr::Primitive(PrimitiveType::Symbol),
        IT::Any => TypeExpr::Any,
        IT::Never => TypeExpr::Never,
        IT::Vector(elem) => TypeExpr::Vector(Box::new(ir_to_type_expr(elem))),
        IT::List(elem) => TypeExpr::Vector(Box::new(ir_to_type_expr(elem))),
        IT::Tuple(types) => TypeExpr::Tuple(types.iter().map(ir_to_type_expr).collect()),
        IT::Map { .. } => TypeExpr::Any, // Extend as needed
        IT::Function {
            param_types,
            variadic_param_type,
            return_type,
        } => TypeExpr::Function {
            param_types: param_types
                .iter()
                .map(|t| ParamType::Simple(Box::new(ir_to_type_expr(t))))
                .collect(),
            variadic_param_type: variadic_param_type
                .as_ref()
                .map(|t| Box::new(ir_to_type_expr(t))),
            return_type: Box::new(ir_to_type_expr(return_type)),
        },
        IT::Union(types) => TypeExpr::Union(types.iter().map(ir_to_type_expr).collect()),
        IT::Intersection(types) => {
            TypeExpr::Intersection(types.iter().map(ir_to_type_expr).collect())
        }
        IT::Resource(sym) => TypeExpr::Resource(rtfs::ast::Symbol(sym.clone())),
        IT::TypeRef(sym) => TypeExpr::Alias(rtfs::ast::Symbol(sym.clone())),
        IT::LiteralValue(lit) => TypeExpr::Literal(lit.clone()),
    }
}

