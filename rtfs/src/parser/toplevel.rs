use crate::ast::{
    Expression as AstExpression, ImportDefinition, ModuleDefinition, ModuleLevelDefinition,
    Property, ResourceDefinition, Symbol, TopLevel,
};
use crate::parser::common::{build_symbol, next_significant};
use crate::parser::errors::{invalid_input_error, pair_to_source_span, PestParseError};
use crate::parser::expressions::build_expression;
use crate::parser::Rule;
use pest::iterators::{Pair, Pairs};

// --- AST Builder Functions ---

pub fn build_ast(pair: Pair<Rule>) -> Result<TopLevel, PestParseError> {
    let toplevel_result = match pair.as_rule() {
        Rule::expression
        | Rule::literal
        | Rule::symbol
        | Rule::keyword
        | Rule::list
        | Rule::vector
        | Rule::map
        | Rule::let_expr
        | Rule::if_expr
        | Rule::do_expr
        | Rule::fn_expr
        | Rule::def_expr
        | Rule::defn_expr
        | Rule::parallel_expr
        | Rule::with_resource_expr
        | Rule::try_catch_expr
        | Rule::match_expr
        | Rule::log_step_expr
        | Rule::discover_agents_expr
        | Rule::resource_ref
        | Rule::identifier
        | Rule::namespaced_identifier => {
            // Build the expression first. If it's a (resource ...) top-level form,
            // convert it into a TopLevel::Resource with parsed properties.
            let expr = build_expression(pair.clone())?;
            // Match FunctionCall(resource <name> <property>...)
            if let AstExpression::FunctionCall { callee, arguments } = &expr {
                if let AstExpression::Symbol(sym) = &**callee {
                    if sym.0 == "resource" {
                        // First arg should be the name (symbol possibly with @version)
                        if arguments.is_empty() {
                            return Err(super::errors::PestParseError::InvalidInput {
                                message: "resource requires a name identifier".to_string(),
                                span: Some(super::errors::pair_to_source_span(&pair)),
                            });
                        }
                        // Extract name symbol and strip any @version suffix
                        let name_expr = &arguments[0];
                        let name_sym = match name_expr {
                            AstExpression::Symbol(s) => s.0.clone(),
                            _ => {
                                return Err(super::errors::PestParseError::InvalidInput {
                                    message: "resource name must be a symbol".to_string(),
                                    span: Some(super::errors::pair_to_source_span(&pair)),
                                });
                            }
                        };
                        let base_name = if let Some(idx) = name_sym.find('@') {
                            name_sym[..idx].to_string()
                        } else {
                            name_sym
                        };

                        // Parse property entries from remaining arguments
                        let mut properties: Vec<Property> = Vec::new();
                        for arg in arguments.iter().skip(1) {
                            // Expect a FunctionCall (property :key value)
                            if let AstExpression::FunctionCall {
                                callee: prop_callee,
                                arguments: prop_args,
                            } = arg
                            {
                                if let AstExpression::Symbol(prop_sym) = &**prop_callee {
                                    if prop_sym.0 == "property" {
                                        if prop_args.len() < 2 {
                                            return Err(
                                                super::errors::PestParseError::InvalidInput {
                                                    message: "property requires a key and a value"
                                                        .to_string(),
                                                    span: Some(super::errors::pair_to_source_span(
                                                        &pair,
                                                    )),
                                                },
                                            );
                                        }
                                        // key should be a Literal::Keyword
                                        match &prop_args[0] {
                                            AstExpression::Literal(lit) => match lit {
                                                crate::ast::Literal::Keyword(k) => {
                                                    let value_expr = prop_args[1].clone();
                                                    properties.push(Property {
                                                        key: k.clone(),
                                                        value: value_expr,
                                                    });
                                                    continue;
                                                }
                                                _ => {}
                                            },
                                            _ => {}
                                        }
                                        return Err(super::errors::PestParseError::InvalidInput {
                                            message: "property key must be a keyword".to_string(),
                                            span: Some(super::errors::pair_to_source_span(&pair)),
                                        });
                                    }
                                }
                            }
                            // Non-property args are ignored for now (could be errors)
                        }

                        let res_def = ResourceDefinition {
                            name: Symbol(base_name),
                            properties,
                        };
                        return Ok(TopLevel::Resource(res_def));
                    } else if sym.0 == "capability" {
                        // Parse (capability "name" :property1 value1 :property2 value2 ...)
                        if arguments.is_empty() {
                            return Err(super::errors::PestParseError::InvalidInput {
                                message: "capability requires a name".to_string(),
                                span: Some(super::errors::pair_to_source_span(&pair)),
                            });
                        }
                        // First arg should be the name (string)
                        let name_expr = &arguments[0];
                        let name_str = match name_expr {
                            AstExpression::Literal(crate::ast::Literal::String(s)) => s.clone(),
                            _ => {
                                return Err(super::errors::PestParseError::InvalidInput {
                                    message: "capability name must be a string".to_string(),
                                    span: Some(super::errors::pair_to_source_span(&pair)),
                                });
                            }
                        };

                        // Parse property entries from remaining arguments
                        let mut properties: Vec<Property> = Vec::new();
                        let mut i = 1;
                        while i < arguments.len() {
                            // Expect keyword followed by value
                            match &arguments[i] {
                                AstExpression::Literal(crate::ast::Literal::Keyword(k)) => {
                                    if i + 1 >= arguments.len() {
                                        return Err(super::errors::PestParseError::InvalidInput {
                                            message: format!(
                                                "capability property '{}' requires a value",
                                                k.0
                                            ),
                                            span: Some(super::errors::pair_to_source_span(&pair)),
                                        });
                                    }
                                    let value_expr = arguments[i + 1].clone();
                                    properties.push(Property {
                                        key: k.clone(),
                                        value: value_expr,
                                    });
                                    i += 2;
                                }
                                _ => {
                                    return Err(super::errors::PestParseError::InvalidInput {
                                        message:
                                            "capability properties must be keyword-value pairs"
                                                .to_string(),
                                        span: Some(super::errors::pair_to_source_span(&pair)),
                                    });
                                }
                            }
                        }

                        let cap_def = crate::ast::CapabilityDefinition {
                            name: Symbol(name_str),
                            properties,
                        };
                        return Ok(TopLevel::Capability(cap_def));
                    } else if sym.0 == "plan" {
                        // Parse (plan "name" :property1 value1 :property2 value2 ...)
                        if arguments.is_empty() {
                            return Err(super::errors::PestParseError::InvalidInput {
                                message: "plan requires a name".to_string(),
                                span: Some(super::errors::pair_to_source_span(&pair)),
                            });
                        }
                        // First arg should be the name (string)
                        let name_expr = &arguments[0];
                        let name_str = match name_expr {
                            AstExpression::Literal(crate::ast::Literal::String(s)) => s.clone(),
                            _ => {
                                return Err(super::errors::PestParseError::InvalidInput {
                                    message: "plan name must be a string".to_string(),
                                    span: Some(super::errors::pair_to_source_span(&pair)),
                                });
                            }
                        };

                        // Parse property entries from remaining arguments
                        let mut properties: Vec<Property> = Vec::new();
                        let mut i = 1;
                        while i < arguments.len() {
                            // Expect keyword followed by value
                            match &arguments[i] {
                                AstExpression::Literal(crate::ast::Literal::Keyword(k)) => {
                                    if i + 1 >= arguments.len() {
                                        return Err(super::errors::PestParseError::InvalidInput {
                                            message: format!(
                                                "plan property '{}' requires a value",
                                                k.0
                                            ),
                                            span: Some(super::errors::pair_to_source_span(&pair)),
                                        });
                                    }
                                    let value_expr = arguments[i + 1].clone();
                                    properties.push(Property {
                                        key: k.clone(),
                                        value: value_expr,
                                    });
                                    i += 2;
                                }
                                _ => {
                                    return Err(super::errors::PestParseError::InvalidInput {
                                        message: "plan properties must be keyword-value pairs"
                                            .to_string(),
                                        span: Some(super::errors::pair_to_source_span(&pair)),
                                    });
                                }
                            }
                        }

                        let plan_def = crate::ast::PlanDefinition {
                            name: Symbol(name_str),
                            properties,
                        };
                        return Ok(TopLevel::Plan(plan_def));
                    }
                }
            }

            Ok(TopLevel::Expression(expr))
        }
        Rule::module_definition => build_module_definition(pair).map(TopLevel::Module),
        Rule::import_definition => Err(PestParseError::CustomError {
            message: "Import definition found outside of a module context".to_string(),
            span: Some(pair_to_source_span(&pair)),
        }),
        rule => Err(PestParseError::CustomError {
            message: format!(
                "build_ast encountered unexpected top-level rule: {:?}, content: '{}'",
                rule,
                pair.as_str()
            ),
            span: Some(pair_to_source_span(&pair)),
        }),
    };

    return toplevel_result;
}

// --- Top-Level Builders ---

fn build_export_option(
    parent_pair: &Pair<Rule>,
    mut pairs: Pairs<Rule>,
) -> Result<Vec<Symbol>, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair);
    let exports_keyword_pair =
        next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
            message: "Expected :exports keyword in export_option".to_string(),
            span: Some(parent_span.clone()),
        })?;
    if exports_keyword_pair.as_rule() != Rule::exports_keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: ":exports keyword".to_string(),
            found: format!("{:?}", exports_keyword_pair.as_rule()),
            rule_text: exports_keyword_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&exports_keyword_pair)),
        });
    }

    let symbols_vec_pair =
        next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
            message: "Expected symbols vector in export_option".to_string(),
            span: Some(pair_to_source_span(&exports_keyword_pair).end_as_start()),
        })?;
    if symbols_vec_pair.as_rule() != Rule::export_symbols_vec {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbols vector (export_symbols_vec)".to_string(),
            found: format!("{:?}", symbols_vec_pair.as_rule()),
            rule_text: symbols_vec_pair.as_str().to_string(),
            span: Some(pair_to_source_span(&symbols_vec_pair)),
        });
    }

    symbols_vec_pair
        .into_inner()
        .filter(|p| p.as_rule() == Rule::symbol)
        .map(|p| build_symbol(p.clone()))
        .collect::<Result<Vec<Symbol>, PestParseError>>()
}

fn build_module_definition(pair: Pair<Rule>) -> Result<ModuleDefinition, PestParseError> {
    let mut inner = pair.clone().into_inner();
    let _ = next_significant(&mut inner); // Skip "module" keyword
    let name_pair = next_significant(&mut inner)
        .ok_or_else(|| invalid_input_error("Missing module name", &pair))?;
    let name = build_symbol(name_pair)?;

    // Parse exports if present
    let mut exports = None;
    if let Some(next_pair) = inner.peek() {
        if next_pair.as_rule() == Rule::export_option {
            let export_pair = inner.next().unwrap();
            let export_symbols =
                build_export_option(&export_pair, export_pair.clone().into_inner())?;
            exports = Some(export_symbols);
        }
    }

    // Parse definitions
    let mut definitions = Vec::new();
    for def_pair in inner {
        match def_pair.as_rule() {
            Rule::def_expr => {
                let def_expr = crate::parser::special_forms::build_def_expr(def_pair)?;
                definitions.push(ModuleLevelDefinition::Def(def_expr));
            }
            Rule::defn_expr => {
                let defn_expr = crate::parser::special_forms::build_defn_expr(def_pair)?;
                definitions.push(ModuleLevelDefinition::Defn(defn_expr));
            }
            Rule::import_definition => {
                let pairs = def_pair.clone().into_inner();
                let import_expr = build_import_definition(&def_pair, pairs)?;
                definitions.push(ModuleLevelDefinition::Import(import_expr));
            }
            _ => {
                // Skip whitespace and other non-definition rules
                continue;
            }
        }
    }

    Ok(ModuleDefinition {
        name,
        docstring: None, // TODO: Parse docstring if present
        exports,
        definitions,
    })
}

// import_definition =  { "(" ~ import_keyword ~ namespaced_identifier ~ import_options? ~ ")" }
// fn build_import_definition(pair: Pair<Rule>) -> ImportDefinition { // Old signature
fn build_import_definition(
    parent_pair: &Pair<Rule>,
    mut pairs: Pairs<Rule>,
) -> Result<ImportDefinition, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair);

    // Skip the import keyword
    next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
        message: "Expected import keyword in import_definition".to_string(),
        span: Some(parent_span.clone()),
    })?;

    // Parse module name (symbol or namespaced identifier)
    let module_name_pair =
        next_significant(&mut pairs).ok_or_else(|| PestParseError::CustomError {
            message: "Expected module name in import_definition".to_string(),
            span: Some(parent_span.clone()),
        })?;

    let module_name = match module_name_pair.as_rule() {
        Rule::symbol => {
            let symbol_pair = module_name_pair.into_inner().next().unwrap();
            Symbol(symbol_pair.as_str().to_string())
        }
        Rule::namespaced_identifier => Symbol(module_name_pair.as_str().to_string()),
        _ => {
            return Err(PestParseError::CustomError {
                message: "Expected symbol or namespaced identifier for module name".to_string(),
                span: Some(parent_span.clone()),
            });
        }
    };

    // Parse optional import options (:as alias, :only [symbols])
    let mut alias = None;
    let mut only = None;

    while let Some(option_pair) = next_significant(&mut pairs) {
        match option_pair.as_rule() {
            Rule::import_option => {
                let mut option_inner = option_pair.into_inner();
                if let Some(option_type) = option_inner.next() {
                    match option_type.as_str() {
                        ":as" => {
                            if let Some(alias_pair) = option_inner.next() {
                                if alias_pair.as_rule() == Rule::symbol {
                                    let symbol_pair = alias_pair.into_inner().next().unwrap();
                                    alias = Some(Symbol(symbol_pair.as_str().to_string()));
                                }
                            }
                        }
                        ":only" => {
                            if let Some(vector_pair) = option_inner.next() {
                                if vector_pair.as_rule() == Rule::vector {
                                    let mut symbols = Vec::new();
                                    for item in vector_pair.into_inner() {
                                        if item.as_rule() == Rule::symbol {
                                            let symbol_pair = item.into_inner().next().unwrap();
                                            symbols.push(Symbol(symbol_pair.as_str().to_string()));
                                        }
                                    }
                                    only = Some(symbols);
                                }
                            }
                        }
                        _ => {
                            return Err(PestParseError::CustomError {
                                message: format!("Unknown import option: {}", option_type.as_str()),
                                span: Some(parent_span.clone()),
                            });
                        }
                    }
                }
            }
            _ => {
                // Not an import option, might be end of definition
                break;
            }
        }
    }

    Ok(ImportDefinition {
        module_name,
        alias,
        only,
    })
}
