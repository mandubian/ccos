use super::{PestParseError, Rule};
use crate::ast::{
    MapTypeEntry, ParamType, PrimitiveType, Symbol, TypeExpr, 
    ArrayDimension, TypePredicate, Literal, Keyword
}; // Enhanced imports
use pest::iterators::Pair;

// Helper function imports from sibling modules
use super::common::{build_keyword, build_symbol, build_literal};

// Build type expression from a parsed pair
pub fn build_type_expr(pair: Pair<Rule>) -> Result<TypeExpr, PestParseError> {
    // Get the actual type pair, handling wrapper rules
    let actual_type_pair = match pair.as_rule() {
        Rule::type_expr => pair
            .into_inner()
            .next()
            .ok_or_else(|| PestParseError::MissingToken { token: "type_expr inner".to_string(), span: None })?,
        _ => pair,
    };

    match actual_type_pair.as_rule() {        Rule::keyword => {
            let keyword_pair = actual_type_pair.clone();
            match keyword_pair.as_str() {
                ":int" => Ok(TypeExpr::Primitive(PrimitiveType::Int)),
                ":float" => Ok(TypeExpr::Primitive(PrimitiveType::Float)),
                ":string" => Ok(TypeExpr::Primitive(PrimitiveType::String)),
                ":bool" => Ok(TypeExpr::Primitive(PrimitiveType::Bool)),
                ":nil" => Ok(TypeExpr::Primitive(PrimitiveType::Nil)),
                ":keyword" => Ok(TypeExpr::Primitive(PrimitiveType::Keyword)),
                ":symbol" => Ok(TypeExpr::Primitive(PrimitiveType::Symbol)),
                ":any" => Ok(TypeExpr::Any),
                ":never" => Ok(TypeExpr::Never),
                _ => {
                    // For other keywords like :ResourceType, treat as type alias
                    let keyword = build_keyword(keyword_pair)?;
                    Ok(TypeExpr::Alias(Symbol(keyword.0)))
                }
            }        }
        Rule::primitive_type => {
            // primitive_type = { symbol } according to grammar
            let symbol_pair = actual_type_pair.into_inner().next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected symbol in primitive_type".to_string(), span: None }
            })?;
            Ok(TypeExpr::Alias(build_symbol(symbol_pair)?))
        }
        Rule::symbol => Ok(TypeExpr::Alias(build_symbol(actual_type_pair)?)),Rule::vector_type => {
            let inner_type_pair = actual_type_pair.into_inner().next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected inner type for vector".to_string(), span: None }
            })?;
            Ok(TypeExpr::Vector(Box::new(build_type_expr(
                inner_type_pair,
            )?)))
        }
        Rule::tuple_type => {
            let type_pairs: Result<Vec<TypeExpr>, PestParseError> = actual_type_pair
                .into_inner()
                .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                .map(build_type_expr)
                .collect();
            Ok(TypeExpr::Tuple(type_pairs?))
        }
        Rule::map_type => {
            let mut inner = actual_type_pair.into_inner();
            let mut entries = Vec::new();
            let mut wildcard = None;            while let Some(map_entry_pair) = inner.next() {
                match map_entry_pair.as_rule() {                    Rule::map_type_entry => {
                        let mut entry_inner = map_entry_pair.into_inner();
                          let key_pair = entry_inner.next().ok_or_else(|| {
                            PestParseError::MissingToken {
                                token: "expected key in map type entry".to_string(),
                                span: None
                            }
                        })?;                        let type_pair = entry_inner.next().ok_or_else(|| {
                            PestParseError::MissingToken {
                                token: "expected type in map type entry".to_string(),
                                span: None
                            }
                        })?;// Check if there's an optional marker "?" after the type
                        let mut optional = false;
                        for remaining_pair in entry_inner {
                            if remaining_pair.as_rule() == Rule::optional_marker {
                                optional = true;
                                break;
                            }
                        }

                        entries.push(MapTypeEntry {
                            key: build_keyword(key_pair)?,
                            value_type: Box::new(build_type_expr(type_pair)?),
                            optional,
                        });
                    }                    Rule::map_type_wildcard => {
                        let wildcard_type_pair =                            map_entry_pair.into_inner().next().ok_or_else(|| {
                                PestParseError::MissingToken {
                                    token: "expected type for map wildcard".to_string(),
                                    span: None
                                }
                            })?;
                        wildcard = Some(Box::new(build_type_expr(wildcard_type_pair)?));
                    }                    _ => {
                        return Err(PestParseError::UnexpectedRule {
                            expected: "map_type_entry or map_type_wildcard".to_string(),
                            found: format!("{:?}", map_entry_pair.as_rule()),
                            rule_text: map_entry_pair.as_str().to_string(),
                            span: None
                        })
                    }
                }
            }
            Ok(TypeExpr::Map { entries, wildcard })
        }        Rule::function_type => {
            let mut inner = actual_type_pair.clone().into_inner();            
            // Parse the function structure
            // Expected: param_type* variadic_param_type? return_type
            let first_part = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected parameter list in function type".to_string(), span: None }
            })?;let mut param_types = Vec::new();
            let mut variadic_param_type = None;            // Parse all tokens - don't consume first_part yet
            let mut tokens: Vec<_> = inner.collect();
            
            // Add the first_part back to the tokens since we already consumed it
            tokens.insert(0, first_part);
              let return_type_token = tokens.pop().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected return type in function type".to_string(), span: None }
            })?;
            
            // Process parameter tokens
            for param_token in tokens.into_iter() {
                match param_token.as_rule() {
                    Rule::param_type => {
                        // param_type contains a type_expr
                        let inner_type = param_token.into_inner().next().ok_or_else(|| {
                            PestParseError::MissingToken { token: "expected type_expr in param_type".to_string(), span: None }
                        })?;
                        param_types.push(ParamType::Simple(Box::new(build_type_expr(inner_type)?)));
                    }
                    Rule::variadic_param_type => {
                        // variadic_param_type = { "&" ~ WHITESPACE* ~ type_expr }
                        let type_pair = param_token.into_inner()
                            .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                            .ok_or_else(|| {
                                PestParseError::MissingToken { token: "expected type in variadic param".to_string(), span: None }
                            })?;
                        variadic_param_type = Some(Box::new(build_type_expr(type_pair)?));
                    }
                    Rule::WHITESPACE | Rule::COMMENT => {
                        // Skip whitespace and comments
                    }                    _ => {
                        return Err(PestParseError::UnexpectedRule {
                            expected: "param_type or variadic_param_type".to_string(),
                            found: format!("{:?}", param_token.as_rule()),
                            rule_text: param_token.as_str().to_string(),
                            span: None
                        })
                    }
                }
            }            Ok(TypeExpr::Function {
                param_types,
                variadic_param_type,
                return_type: Box::new(build_type_expr(return_type_token)?),
            })
        }
        Rule::resource_type => {
            let symbol_pair = actual_type_pair.into_inner().next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected symbol in resource type".to_string(), span: None }
            })?;
            Ok(TypeExpr::Resource(build_symbol(symbol_pair)?))
        }
        Rule::union_type => {
            let type_pairs: Result<Vec<TypeExpr>, PestParseError> = actual_type_pair
                .into_inner()
                .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                .map(build_type_expr)
                .collect();
            Ok(TypeExpr::Union(type_pairs?))
        }
        Rule::intersection_type => {
            let type_pairs: Result<Vec<TypeExpr>, PestParseError> = actual_type_pair
                .into_inner()
                .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                .map(build_type_expr)
                .collect();
            Ok(TypeExpr::Intersection(type_pairs?))
        }        Rule::literal_type => {
            let literal_pair = actual_type_pair.into_inner().next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected literal in literal type".to_string(), span: None }
            })?;
            use super::common::build_literal;
            Ok(TypeExpr::Literal(build_literal(literal_pair)?))
        }
        Rule::literal => {
            // Handle the case where a keyword is parsed as a literal
            let literal = build_literal(actual_type_pair.clone())?;
            match literal {
                crate::ast::Literal::Keyword(keyword) => {
                    // Convert keyword to type alias
                    Ok(TypeExpr::Alias(Symbol(keyword.0)))
                }
                _ => Ok(TypeExpr::Literal(literal))
            }
        }
        Rule::array_type => {
            let mut inner = actual_type_pair.into_inner();
            let element_type_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected element type in array".to_string(), span: None }
            })?;
            let element_type = Box::new(build_type_expr(element_type_pair)?);
            
            // Parse optional shape
            let mut shape = Vec::new();
            if let Some(shape_pair) = inner.next() {
                if shape_pair.as_rule() == Rule::shape {
                    for dimension_pair in shape_pair.into_inner() {
                        if dimension_pair.as_rule() == Rule::dimension {
                            let dimension_inner = dimension_pair.into_inner().next().ok_or_else(|| {
                                PestParseError::MissingToken { token: "expected dimension content".to_string(), span: None }
                            })?;
                            
                            match dimension_inner.as_str() {
                                "?" => shape.push(ArrayDimension::Variable),
                                n => {
                                    let size = n.parse::<usize>().map_err(|_| PestParseError::InvalidInput {
                                        message: format!("Invalid array dimension: {}", n),
                                        span: None,
                                    })?;
                                    shape.push(ArrayDimension::Fixed(size));
                                }
                            }
                        }
                    }
                }
            }
            
            Ok(TypeExpr::Array { element_type, shape })
        }
        Rule::enum_type => {
            let mut literals = Vec::new();
            for literal_pair in actual_type_pair.into_inner() {
                if literal_pair.as_rule() != Rule::WHITESPACE && literal_pair.as_rule() != Rule::COMMENT {
                    literals.push(build_literal(literal_pair)?);
                }
            }
            Ok(TypeExpr::Enum(literals))
        }
        Rule::optional_type => {
            let mut inner = actual_type_pair.into_inner();
            let base_type_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected base type in optional".to_string(), span: None }
            })?;
            
            let base_type = build_type_expr(base_type_pair)?;
            Ok(TypeExpr::Optional(Box::new(base_type)))
        }
        Rule::intersection_type => {
            let mut inner = actual_type_pair.into_inner();
            let mut types = Vec::new();
            let mut predicates = Vec::new();
            
            for pair in inner {
                if pair.as_rule() != Rule::WHITESPACE && pair.as_rule() != Rule::COMMENT {
                    if is_predicate_rule(&pair) {
                        predicates.push(build_predicate_expr(pair)?);
                    } else {
                        types.push(build_type_expr(pair)?);
                    }
                }
            }
            
            // If we have predicates, create a refined type
            if !predicates.is_empty() && types.len() == 1 {
                Ok(TypeExpr::Refined {
                    base_type: Box::new(types.into_iter().next().unwrap()),
                    predicates,
                })
            } else {
                // Otherwise, create an intersection type
                Ok(TypeExpr::Intersection(types))
            }
        }        s => Err(PestParseError::UnexpectedRule {
            expected: "valid type expression".to_string(),
            found: format!("{:?}", s),
            rule_text: actual_type_pair.as_str().to_string(),
            span: None
        }),
    }
}

/// Check if a pair represents a predicate expression
fn is_predicate_rule(pair: &Pair<Rule>) -> bool {
    matches!(pair.as_rule(), 
        Rule::predicate_expr | 
        Rule::comparison_predicate | 
        Rule::length_predicate | 
        Rule::regex_predicate | 
        Rule::range_predicate | 
        Rule::collection_predicate | 
        Rule::map_predicate | 
        Rule::custom_predicate
    )
}

/// Build a predicate expression from a parsed pair
fn build_predicate_expr(pair: Pair<Rule>) -> Result<TypePredicate, PestParseError> {
    let actual_predicate_pair = match pair.as_rule() {
        Rule::predicate_expr => pair
            .into_inner()
            .next()
            .ok_or_else(|| PestParseError::MissingToken { token: "predicate_expr inner".to_string(), span: None })?,
        _ => pair,
    };

    match actual_predicate_pair.as_rule() {
        Rule::comparison_predicate => {
            let mut inner = actual_predicate_pair.into_inner();
            let operator_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected operator in comparison".to_string(), span: None }
            })?;
            let value_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected value in comparison".to_string(), span: None }
            })?;
            
            let value = build_literal(value_pair)?;
            
            match operator_pair.as_str() {
                ":>" => Ok(TypePredicate::GreaterThan(value)),
                ":>=" => Ok(TypePredicate::GreaterEqual(value)),
                ":<" => Ok(TypePredicate::LessThan(value)),
                ":<=" => Ok(TypePredicate::LessEqual(value)),
                ":=" => Ok(TypePredicate::Equal(value)),
                ":!=" => Ok(TypePredicate::NotEqual(value)),
                _ => Err(PestParseError::InvalidInput {
                    message: format!("Unknown comparison operator: {}", operator_pair.as_str()),
                    span: None,
                })
            }
        }
        
        Rule::length_predicate => {
            let mut inner = actual_predicate_pair.into_inner();
            let operator_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected operator in length predicate".to_string(), span: None }
            })?;
            let value_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected value in length predicate".to_string(), span: None }
            })?;
            
            let value_str = value_pair.as_str();
            let length = value_str.parse::<usize>().map_err(|_| PestParseError::InvalidInput {
                message: format!("Invalid length value: {}", value_str),
                span: None,
            })?;
            
            match operator_pair.as_str() {
                ":min-length" => Ok(TypePredicate::MinLength(length)),
                ":max-length" => Ok(TypePredicate::MaxLength(length)),
                ":length" => Ok(TypePredicate::Length(length)),
                _ => Err(PestParseError::InvalidInput {
                    message: format!("Unknown length operator: {}", operator_pair.as_str()),
                    span: None,
                })
            }
        }
        
        Rule::regex_predicate => {
            let mut inner = actual_predicate_pair.into_inner();
            let _keyword = inner.next(); // Skip :matches-regex keyword
            let pattern_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected pattern in regex predicate".to_string(), span: None }
            })?;
            
            if let Literal::String(pattern) = build_literal(pattern_pair)? {
                Ok(TypePredicate::MatchesRegex(pattern))
            } else {
                Err(PestParseError::InvalidInput {
                    message: "Regex pattern must be a string".to_string(),
                    span: None,
                })
            }
        }
        
        Rule::range_predicate => {
            let mut inner = actual_predicate_pair.into_inner();
            let _keyword = inner.next(); // Skip :in-range keyword
            let min_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected min value in range predicate".to_string(), span: None }
            })?;
            let max_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected max value in range predicate".to_string(), span: None }
            })?;
            
            let min_value = build_literal(min_pair)?;
            let max_value = build_literal(max_pair)?;
            
            Ok(TypePredicate::InRange(min_value, max_value))
        }
        
        Rule::collection_predicate => {
            let mut inner = actual_predicate_pair.into_inner();
            let operator_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected operator in collection predicate".to_string(), span: None }
            })?;
            
            match operator_pair.as_str() {
                ":non-empty" => Ok(TypePredicate::NonEmpty),
                ":min-count" | ":max-count" | ":count" => {
                    let value_pair = inner.next().ok_or_else(|| {
                        PestParseError::MissingToken { token: "expected count value".to_string(), span: None }
                    })?;
                    
                    let value_str = value_pair.as_str();
                    let count = value_str.parse::<usize>().map_err(|_| PestParseError::InvalidInput {
                        message: format!("Invalid count value: {}", value_str),
                        span: None,
                    })?;
                    
                    match operator_pair.as_str() {
                        ":min-count" => Ok(TypePredicate::MinCount(count)),
                        ":max-count" => Ok(TypePredicate::MaxCount(count)),
                        ":count" => Ok(TypePredicate::Count(count)),
                        _ => unreachable!(),
                    }
                }
                _ => Err(PestParseError::InvalidInput {
                    message: format!("Unknown collection operator: {}", operator_pair.as_str()),
                    span: None,
                })
            }
        }
        
        Rule::map_predicate => {
            let mut inner = actual_predicate_pair.into_inner();
            let operator_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected operator in map predicate".to_string(), span: None }
            })?;
            
            match operator_pair.as_str() {
                ":has-key" => {
                    let key_pair = inner.next().ok_or_else(|| {
                        PestParseError::MissingToken { token: "expected key in has-key predicate".to_string(), span: None }
                    })?;
                    let key = build_keyword(key_pair)?;
                    Ok(TypePredicate::HasKey(key))
                }
                ":required-keys" => {
                    let keys_list_pair = inner.next().ok_or_else(|| {
                        PestParseError::MissingToken { token: "expected keys list in required-keys predicate".to_string(), span: None }
                    })?;
                    
                    let mut keys = Vec::new();
                    for key_pair in keys_list_pair.into_inner() {
                        if key_pair.as_rule() != Rule::WHITESPACE && key_pair.as_rule() != Rule::COMMENT {
                            keys.push(build_keyword(key_pair)?);
                        }
                    }
                    Ok(TypePredicate::RequiredKeys(keys))
                }
                _ => Err(PestParseError::InvalidInput {
                    message: format!("Unknown map operator: {}", operator_pair.as_str()),
                    span: None,
                })
            }
        }
        
        Rule::custom_predicate => {
            let mut inner = actual_predicate_pair.into_inner();
            let name_pair = inner.next().ok_or_else(|| {
                PestParseError::MissingToken { token: "expected predicate name".to_string(), span: None }
            })?;
            
            let name = match name_pair.as_rule() {
                Rule::keyword => build_keyword(name_pair)?,
                Rule::symbol => {
                    let symbol = build_symbol(name_pair)?;
                    Keyword::new(&symbol.0)
                }
                _ => return Err(PestParseError::InvalidInput {
                    message: "Predicate name must be keyword or symbol".to_string(),
                    span: None,
                })
            };
            
            let mut args = Vec::new();
            for arg_pair in inner {
                if arg_pair.as_rule() != Rule::WHITESPACE && arg_pair.as_rule() != Rule::COMMENT {
                    args.push(build_literal(arg_pair)?);
                }
            }
            
            // Handle built-in predicates without arguments
            match name.0.as_str() {
                "is-url" => Ok(TypePredicate::IsUrl),
                "is-email" => Ok(TypePredicate::IsEmail),
                _ => Ok(TypePredicate::Custom(name, args)),
            }
        }
        
        _ => Err(PestParseError::UnexpectedRule {
            expected: "valid predicate expression".to_string(),
            found: format!("{:?}", actual_predicate_pair.as_rule()),
            rule_text: actual_predicate_pair.as_str().to_string(),
            span: None
        }),
    }
}
