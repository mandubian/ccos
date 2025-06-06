use super::{PestParseError, Rule};
use crate::ast::{MapTypeEntry, ParamType, PrimitiveType, Symbol, TypeExpr}; // Added Symbol
use pest::iterators::Pair;

// Helper function imports from sibling modules
use super::common::{build_keyword, build_symbol};

// Build type expression from a parsed pair
pub fn build_type_expr(pair: Pair<Rule>) -> Result<TypeExpr, PestParseError> {
    // Get the actual type pair, handling wrapper rules
    let actual_type_pair = match pair.as_rule() {
        Rule::type_expr => pair
            .into_inner()
            .next()
            .ok_or_else(|| PestParseError::MissingToken("type_expr inner".to_string()))?,
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
                PestParseError::MissingToken("expected symbol in primitive_type".to_string())
            })?;
            Ok(TypeExpr::Alias(build_symbol(symbol_pair)?))
        }
        Rule::symbol => Ok(TypeExpr::Alias(build_symbol(actual_type_pair)?)),Rule::vector_type => {
            let inner_type_pair = actual_type_pair.into_inner().next().ok_or_else(|| {
                PestParseError::MissingToken("expected inner type for vector".to_string())
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
                            PestParseError::MissingToken(
                                "expected key in map type entry".to_string(),
                            )
                        })?;
                        let type_pair = entry_inner.next().ok_or_else(|| {
                            PestParseError::MissingToken(
                                "expected type in map type entry".to_string(),
                            )
                        })?;                        // Check if there's an optional marker "?" after the type
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
                        let wildcard_type_pair =
                            map_entry_pair.into_inner().next().ok_or_else(|| {
                                PestParseError::MissingToken(
                                    "expected type for map wildcard".to_string(),
                                )                            })?;
                        wildcard = Some(Box::new(build_type_expr(wildcard_type_pair)?));
                    }
                    _ => {
                        return Err(PestParseError::UnexpectedRule {
                            expected: "map_type_entry or map_type_wildcard".to_string(),
                            found: format!("{:?}", map_entry_pair.as_rule()),
                            rule_text: map_entry_pair.as_str().to_string(),
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
                PestParseError::MissingToken("expected parameter list in function type".to_string())
            })?;let mut param_types = Vec::new();
            let mut variadic_param_type = None;            // Parse all tokens - don't consume first_part yet
            let mut tokens: Vec<_> = inner.collect();
            
            // Add the first_part back to the tokens since we already consumed it
            tokens.insert(0, first_part);
              let return_type_token = tokens.pop().ok_or_else(|| {
                PestParseError::MissingToken("expected return type in function type".to_string())
            })?;
            
            // Process parameter tokens
            for param_token in tokens.into_iter() {
                match param_token.as_rule() {
                    Rule::param_type => {
                        // param_type contains a type_expr
                        let inner_type = param_token.into_inner().next().ok_or_else(|| {
                            PestParseError::MissingToken("expected type_expr in param_type".to_string())
                        })?;
                        param_types.push(ParamType::Simple(Box::new(build_type_expr(inner_type)?)));
                    }
                    Rule::variadic_param_type => {
                        // variadic_param_type = { "&" ~ WHITESPACE* ~ type_expr }
                        let type_pair = param_token.into_inner()
                            .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                            .ok_or_else(|| {
                                PestParseError::MissingToken("expected type in variadic param".to_string())
                            })?;
                        variadic_param_type = Some(Box::new(build_type_expr(type_pair)?));
                    }
                    Rule::WHITESPACE | Rule::COMMENT => {
                        // Skip whitespace and comments
                    }
                    _ => {
                        return Err(PestParseError::UnexpectedRule {
                            expected: "param_type or variadic_param_type".to_string(),
                            found: format!("{:?}", param_token.as_rule()),
                            rule_text: param_token.as_str().to_string(),
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
                PestParseError::MissingToken("expected symbol in resource type".to_string())
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
                PestParseError::MissingToken("expected literal in literal type".to_string())
            })?;
            use super::common::build_literal;
            Ok(TypeExpr::Literal(build_literal(literal_pair)?))
        }
        Rule::literal => {
            // Handle the case where a keyword is parsed as a literal
            use super::common::build_literal;
            let literal = build_literal(actual_type_pair.clone())?;
            match literal {
                crate::ast::Literal::Keyword(keyword) => {
                    // Convert keyword to type alias
                    Ok(TypeExpr::Alias(Symbol(keyword.0)))
                }
                _ => Ok(TypeExpr::Literal(literal))
            }
        }
        s => Err(PestParseError::UnexpectedRule {
            expected: "valid type expression".to_string(),
            found: format!("{:?}", s),
            rule_text: actual_type_pair.as_str().to_string(),
        }),
    }
}
