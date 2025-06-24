use super::errors::{PestParseError, pair_to_source_span};
use super::Rule;
use super::utils::unescape;
use crate::ast::{
    Symbol, Keyword, Literal, MapKey, Pattern, MatchPattern, 
    MapDestructuringEntry, MapMatchEntry
};
use pest::iterators::{Pair, Pairs};

// --- Helper Builders ---

pub(super) fn build_literal(pair: Pair<Rule>) -> Result<Literal, PestParseError> {
    let literal_span = pair_to_source_span(&pair); // Get span from original pair
    let inner_pair = pair
        .into_inner()
        .next()
        .ok_or_else(|| PestParseError::MissingToken { 
            token: "literal inner".to_string(), 
            span: Some(literal_span.clone()) // Use cloned span
        })?;
    let inner_span = pair_to_source_span(&inner_pair);
    match inner_pair.as_rule() {
        Rule::timestamp => Ok(Literal::Timestamp(inner_pair.as_str().to_string())),
        Rule::uuid => Ok(Literal::Uuid(inner_pair.as_str().to_string())),
        Rule::resource_handle => Ok(Literal::ResourceHandle(inner_pair.as_str().to_string())),
        Rule::integer => Ok(Literal::Integer(inner_pair.as_str().parse().map_err(
            |_| PestParseError::InvalidLiteral { message: format!("Invalid integer: {}", inner_pair.as_str()), span: Some(inner_span.clone()) },
        )?)),
        Rule::float => Ok(Literal::Float(inner_pair.as_str().parse().map_err(
            |_| PestParseError::InvalidLiteral { message: format!("Invalid float: {}", inner_pair.as_str()), span: Some(inner_span.clone()) },
        )?)),
        Rule::string => {
            let raw_str = inner_pair.as_str();
            let content = &raw_str[1..raw_str.len() - 1];
            Ok(Literal::String(unescape(content).map_err(|_| PestParseError::InvalidEscapeSequence {
                sequence: content.to_string(), // Use the content that failed to unescape
                span: Some(inner_span.clone())
            })?))
        }
        Rule::boolean => Ok(Literal::Boolean(inner_pair.as_str().parse().map_err(
            |_| PestParseError::InvalidLiteral { message: format!("Invalid boolean: {}", inner_pair.as_str()), span: Some(inner_span.clone()) },
        )?)),
        Rule::nil => Ok(Literal::Nil),
        Rule::keyword => Ok(Literal::Keyword(build_keyword(inner_pair.clone())?)), // Clone inner_pair as build_keyword might need its span
        rule => Err(PestParseError::UnexpectedRule {
            expected: "valid literal type".to_string(),
            found: format!("{:?}", rule),
            rule_text: inner_pair.as_str().to_string(),
            span: Some(inner_span.clone())
        }),
    }
}

pub(super) fn build_symbol(pair: Pair<Rule>) -> Result<Symbol, PestParseError> {
    let pair_span = pair_to_source_span(&pair);
    if pair.as_rule() != Rule::symbol {        
        return Err(PestParseError::UnexpectedRule {
            expected: "symbol".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
            span: Some(pair_span)
        });
    }
    Ok(Symbol(pair.as_str().to_string()))
}

pub(super) fn build_keyword(pair: Pair<Rule>) -> Result<Keyword, PestParseError> {
    let pair_span = pair_to_source_span(&pair);
    if pair.as_rule() != Rule::keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: "keyword".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
            span: Some(pair_span)
        });
    }
    Ok(Keyword(pair.as_str()[1..].to_string()))
}

pub(super) fn build_map_key(pair: Pair<Rule>) -> Result<MapKey, PestParseError> {
    let map_key_span = pair_to_source_span(&pair);
    if pair.as_rule() != Rule::map_key {
        return Err(PestParseError::UnexpectedRule {
            expected: "map_key".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
            span: Some(map_key_span.clone())
        });
    }

    let inner_pair = pair
        .into_inner()
        .next()
        .ok_or_else(|| PestParseError::MissingToken { 
            token: "map_key inner".to_string(), 
            span: Some(map_key_span.clone()) 
        })?;
    let inner_span = pair_to_source_span(&inner_pair);
    match inner_pair.as_rule() {
        Rule::keyword => Ok(MapKey::Keyword(build_keyword(inner_pair.clone())?)),
        Rule::string => {
            let raw_str = inner_pair.as_str();
            let content = &raw_str[1..raw_str.len() - 1];
            Ok(MapKey::String(unescape(content).map_err(|_| PestParseError::InvalidEscapeSequence {
                sequence: content.to_string(), // Use the content that failed to unescape
                span: Some(inner_span.clone())
            })?))
        }
        Rule::integer => Ok(MapKey::Integer(inner_pair.as_str().parse().map_err(
            |_| PestParseError::InvalidLiteral { message: format!("Invalid integer map key: {}", inner_pair.as_str()), span: Some(inner_span.clone()) },
        )?)),
        rule => Err(PestParseError::UnexpectedRule {
            expected: "keyword, string, or integer for map key".to_string(),
            found: format!("{:?}", rule),
            rule_text: inner_pair.as_str().to_string(),
            span: Some(inner_span.clone())
        }),
    }
}

// Helper for map destructuring, returns (entries, rest_binding, as_binding)
fn build_map_destructuring_parts(
    pair: Pair<Rule>,
) -> Result<(Vec<MapDestructuringEntry>, Option<Symbol>, Option<Symbol>), PestParseError> {
    let map_dest_span = pair_to_source_span(&pair);
    if pair.as_rule() != Rule::map_destructuring_pattern {
        return Err(PestParseError::UnexpectedRule {
            expected: "map_destructuring_pattern".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
            span: Some(map_dest_span.clone())
        });
    }
    
    let mut inner = pair.clone().into_inner().peekable(); // Clone pair to keep map_dest_span valid
    let mut entries = Vec::new();
    let mut rest_binding = None;
    let mut as_binding = None;

    while let Some(current_pair) = inner.next() {
        let current_entry_span = pair_to_source_span(&current_pair);
        match current_pair.as_rule() {            
            Rule::map_destructuring_entry => {
                let mut entry_inner = current_pair.clone().into_inner(); // Clone for current_entry_span
                
                let first_token_in_entry = entry_inner.peek().ok_or_else(|| {
                    PestParseError::MissingToken { token: "first token in map_destructuring_entry".to_string(), span: Some(current_entry_span.clone()) }
                })?;
                
                if first_token_in_entry.as_rule() == Rule::keys_entry {
                    let keys_entry_pair = entry_inner.next().unwrap(); 
                    let keys_inner = keys_entry_pair.into_inner();
                    let mut symbols = Vec::new();
                    for token in keys_inner {
                        if token.as_rule() == Rule::symbol {
                            symbols.push(build_symbol(token.clone())?);
                        }
                    }
                    entries.push(MapDestructuringEntry::Keys(symbols));
                } else {
                    let key_token_pair = entry_inner.next().ok_or_else(|| {
                        PestParseError::MissingToken { token: "map_key in map_destructuring_entry".to_string(), span: Some(current_entry_span.clone()) }
                    })?;
                    let key_token_span = pair_to_source_span(&key_token_pair);
                    
                    if key_token_pair.as_rule() != Rule::map_key {
                         return Err(PestParseError::UnexpectedRule {
                            expected: "map_key".to_string(),
                            found: format!("{:?}", key_token_pair.as_rule()),
                            rule_text: key_token_pair.as_str().to_string(),
                            span: Some(key_token_span.clone())
                        });
                    }

                    let val_pattern_pair = entry_inner.next().ok_or_else(|| {
                        PestParseError::MissingToken { 
                            token: "binding_pattern in map_destructuring_entry".to_string(), 
                            span: Some(key_token_span.end_as_start()) 
                        }
                    })?;

                    let map_key_val = build_map_key(key_token_pair.clone())?;
                    let pattern_to_bind = build_pattern(val_pattern_pair.clone())?;

                    entries.push(MapDestructuringEntry::KeyBinding {
                        key: map_key_val,
                        pattern: Box::new(pattern_to_bind),
                    });
                }
            }
            Rule::map_rest_binding => {
                let mut rest_inner = current_pair.clone().into_inner();
                let rest_sym_pair = rest_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken { token: "symbol in map_rest_binding".to_string(), span: Some(current_entry_span.clone()) }
                })?;
                let rest_sym_span = pair_to_source_span(&rest_sym_pair);
                if rest_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in map_rest_binding".to_string(),
                        found: format!("{:?}", rest_sym_pair.as_rule()),
                        rule_text: rest_sym_pair.as_str().to_string(),
                        span: Some(rest_sym_span.clone())
                    });
                }
                rest_binding = Some(build_symbol(rest_sym_pair.clone())?);
            }
            Rule::map_as_binding => {
                let mut as_inner = current_pair.clone().into_inner();
                let as_sym_pair = as_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken { token: "symbol in map_as_binding".to_string(), span: Some(current_entry_span.clone()) }
                })?;
                let as_sym_span = pair_to_source_span(&as_sym_pair);
                if as_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in map_as_binding".to_string(),
                        found: format!("{:?}", as_sym_pair.as_rule()),
                        rule_text: as_sym_pair.as_str().to_string(),
                        span: Some(as_sym_span.clone())
                    });
                }
                as_binding = Some(build_symbol(as_sym_pair.clone())?);
            }
            // The grammar defines map_rest_binding = { "&" ~ symbol } and map_as_binding = { ":as" ~ symbol }.
            // Pest will match these as Rule::map_rest_binding and Rule::map_as_binding directly.
            // The cases for matching current_pair.as_str() == "&" or ":as" are redundant if the grammar is correctly defined and used.
            // If these string checks were necessary, it implies the grammar might not be creating distinct rules for these, 
            // or the iteration logic was different. Assuming the grammar is correct, these are removed.
            Rule::WHITESPACE | Rule::COMMENT => { /* Skip */ }            
            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "map destructuring entry, map_rest_binding, or map_as_binding".to_string(),
                    found: format!("{:?}", rule),
                    rule_text: current_pair.as_str().to_string(),
                    span: Some(current_entry_span.clone())
                })
            }
        }
    }
    Ok((entries, rest_binding, as_binding))
}

// Helper for vector destructuring, returns (elements, rest_binding, as_binding)
fn build_vector_destructuring_parts(
    pair: Pair<Rule>,
) -> Result<(Vec<Pattern>, Option<Symbol>, Option<Symbol>), PestParseError> {
    let vec_dest_span = pair_to_source_span(&pair);
    if pair.as_rule() != Rule::vector_destructuring_pattern {
        return Err(PestParseError::UnexpectedRule {
            expected: "vector_destructuring_pattern".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
            span: Some(vec_dest_span.clone()),
        });
    }

    let mut inner = pair.clone().into_inner().peekable();
    let mut elements = Vec::new();
    let mut rest_binding = None;
    let mut as_binding = None;    while let Some(current_pair) = inner.next() {
        let current_element_span = pair_to_source_span(&current_pair);
        match current_pair.as_rule() {
            // Since binding_pattern is a silent rule, we get its constituent rules directly
            Rule::wildcard | Rule::symbol | Rule::map_destructuring_pattern | Rule::vector_destructuring_pattern => {
                elements.push(build_pattern(current_pair.clone())?);
            }
            Rule::vector_rest_binding => {
                let mut rest_inner = current_pair.clone().into_inner();
                let rest_sym_pair = rest_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken { token: "symbol in vector_rest_binding".to_string(), span: Some(current_element_span.clone()) }
                })?;
                let rest_sym_span = pair_to_source_span(&rest_sym_pair);
                if rest_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in vector_rest_binding".to_string(),
                        found: format!("{:?}", rest_sym_pair.as_rule()),
                        rule_text: rest_sym_pair.as_str().to_string(),
                        span: Some(rest_sym_span.clone()),
                    });
                }
                rest_binding = Some(build_symbol(rest_sym_pair.clone())?);
            }
            Rule::vector_as_binding => {
                let mut as_inner = current_pair.clone().into_inner();
                let as_sym_pair = as_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken { token: "symbol in vector_as_binding".to_string(), span: Some(current_element_span.clone()) }
                })?;
                let as_sym_span = pair_to_source_span(&as_sym_pair);
                if as_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in vector_as_binding".to_string(),
                        found: format!("{:?}", as_sym_pair.as_rule()),
                        rule_text: as_sym_pair.as_str().to_string(),
                        span: Some(as_sym_span.clone()),
                    });
                }
                as_binding = Some(build_symbol(as_sym_pair.clone())?);
            }
            Rule::WHITESPACE | Rule::COMMENT => { /* Skip */ }
            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "binding_pattern, vector_rest_binding, or vector_as_binding".to_string(),
                    found: format!("{:?}", rule),
                    rule_text: current_pair.as_str().to_string(),
                    span: Some(current_element_span.clone()),
                });
            }
        }
    }
    Ok((elements, rest_binding, as_binding))
}

pub(super) fn build_pattern(pair: Pair<Rule>) -> Result<Pattern, PestParseError> {
    let pattern_span = pair_to_source_span(&pair);
    match pair.as_rule() {
        Rule::wildcard => Ok(Pattern::Wildcard),
        Rule::symbol => Ok(Pattern::Symbol(build_symbol(pair.clone())?)),
        Rule::map_destructuring_pattern => {
            let (entries, rest, as_sym) = build_map_destructuring_parts(pair.clone())?;
            Ok(Pattern::MapDestructuring {
                entries,
                rest,
                as_symbol: as_sym,
            })
        }
        Rule::vector_destructuring_pattern => {
            let (elements, rest, as_sym) = build_vector_destructuring_parts(pair.clone())?;
            Ok(Pattern::VectorDestructuring {
                elements,
                rest,
                as_symbol: as_sym,
            })
        }
        // Literals can also be patterns
        Rule::literal => {
            let _ = build_literal(pair.clone())?;
            // Literals are not valid patterns - use MatchPattern for matching literals
            Err(PestParseError::InvalidInput {
                message: "Literals cannot be used as binding patterns".to_string(),
                span: Some(pattern_span.clone())
            })
        }
        rule => Err(PestParseError::UnexpectedRule {
            expected: "valid pattern type (wildcard, symbol, map, vector, literal)".to_string(),
            found: format!("{:?}", rule),
            rule_text: pair.as_str().to_string(),
            span: Some(pattern_span.clone()),
        }),
    }
}


// MatchPattern related builders
pub(super) fn build_match_pattern(pair: Pair<Rule>) -> Result<MatchPattern, PestParseError> {
    let match_pattern_span = pair_to_source_span(&pair);
    match pair.as_rule() {
        Rule::literal => Ok(MatchPattern::Literal(build_literal(pair.clone())?)),
        Rule::keyword => Ok(MatchPattern::Keyword(build_keyword(pair.clone())?)),
        Rule::wildcard => Ok(MatchPattern::Wildcard),
        Rule::symbol => Ok(MatchPattern::Symbol(build_symbol(pair.clone())?)),
        Rule::type_expr => {
            // Assuming type_expr is a valid pattern for now, might need specific handling
            // For now, let's treat it as an error or unsupported, as TypeExpr is not directly a MatchPattern variant by default.
            // This depends on how type patterns are defined in AST.
            // If Type(TypeExpr) is a variant of MatchPattern:
            // Ok(MatchPattern::Type(super::types::build_type_expr(pair.clone())?))
            Err(PestParseError::UnsupportedRule {
                rule: "Type matching in patterns not fully implemented yet".to_string(),
                span: Some(match_pattern_span.clone()),
            })
        }
        Rule::as_match_pattern => {
            let mut inner = pair.clone().into_inner();
            // Skip ":as" keyword token if grammar includes it as a token
            // Grammar: as_match_pattern = { "(" ~ ":as" ~ symbol ~ match_pattern ~ ")" }
            // Pest usually gives the literal token if not a sub-rule.
            // Assuming ":as" is consumed as part of the structure or is a specific keyword rule.
            // Let's find the symbol and the actual pattern.
            let _as_keyword = inner.next(); // Consume the ":as" part (or its rule)
            let symbol_pair = inner.next().ok_or_else(|| PestParseError::MissingToken {
                token: "symbol in :as pattern".to_string(),
                span: Some(match_pattern_span.clone()),
            })?;
            let symbol = build_symbol(symbol_pair.clone())?;

            let actual_pattern_pair = inner.next().ok_or_else(|| PestParseError::MissingToken {
                token: "pattern in :as pattern".to_string(),
                span: Some(pair_to_source_span(&symbol_pair).end_as_start()),
            })?;
            let pattern = Box::new(build_match_pattern(actual_pattern_pair.clone())?);
            Ok(MatchPattern::As(symbol, pattern))
        }        Rule::vector_match_pattern => {
            let mut inner = pair.clone().into_inner();
            let mut elements = Vec::new();
            let mut rest_symbol = None;
            // Process each element inside the vector match pattern
            while let Some(p) = inner.peek() {
                match p.as_rule() {
                    // Since match_pattern is a silent rule, we get its constituent rules directly
                    Rule::literal | Rule::keyword | Rule::wildcard | Rule::symbol | 
                    Rule::type_expr | Rule::as_match_pattern | Rule::vector_match_pattern | 
                    Rule::map_match_pattern => {
                        elements.push(build_match_pattern(inner.next().unwrap().clone())?);
                    }
                    Rule::AMPERSAND => { // Assuming AMPERSAND is a rule for '&'
                        inner.next(); // Consume '&'
                        let sym_pair = inner.next().ok_or_else(|| PestParseError::MissingToken {
                            token: "symbol after & in vector match pattern".to_string(),
                            span: Some(pair_to_source_span(&p).end_as_start()), // span after '&'
                        })?;
                        if sym_pair.as_rule() == Rule::symbol {
                            rest_symbol = Some(build_symbol(sym_pair.clone())?);
                        } else {
                            return Err(PestParseError::UnexpectedRule {
                                expected: "symbol after &".to_string(),
                                found: format!("{:?}", sym_pair.as_rule()),
                                rule_text: sym_pair.as_str().to_string(),
                                span: Some(pair_to_source_span(&sym_pair)),
                            });
                        }
                        break; // No more elements after rest symbol
                    }
                    Rule::WHITESPACE | Rule::COMMENT => {inner.next();}
                    _ => break, // End of elements or unexpected token
                }
            }
            Ok(MatchPattern::Vector { elements, rest: rest_symbol })
        }
        Rule::map_match_pattern => {
            let mut inner = pair.clone().into_inner();
            let mut entries = Vec::new();
            let mut rest_symbol = None;
            // Skip "{" and process map_match_pattern_entry* and optional "&" symbol
            while let Some(p) = inner.peek() {
                match p.as_rule() {
                    Rule::map_match_pattern_entry => {
                        let entry_pair = inner.next().unwrap();
                        let entry_span = pair_to_source_span(&entry_pair);
                        let mut entry_inner = entry_pair.clone().into_inner();
                        let key_pair = entry_inner.next().ok_or_else(|| PestParseError::MissingToken {
                            token: "key in map match pattern entry".to_string(),
                            span: Some(entry_span.clone()),
                        })?;
                        let key = build_map_key(key_pair.clone())?;

                        let pattern_pair = entry_inner.next().ok_or_else(|| PestParseError::MissingToken {
                            token: "pattern in map match pattern entry".to_string(),
                            span: Some(pair_to_source_span(&key_pair).end_as_start()),
                        })?;
                        let pattern = build_match_pattern(pattern_pair.clone())?;
                        entries.push(MapMatchEntry { key, pattern: Box::new(pattern) });
                    }
                    Rule::AMPERSAND => { // Assuming AMPERSAND is a rule for '&'
                        inner.next(); // Consume '&'
                        let sym_pair = inner.next().ok_or_else(|| PestParseError::MissingToken {
                            token: "symbol after & in map match pattern".to_string(),
                            span: Some(pair_to_source_span(&p).end_as_start()),
                        })?;
                        if sym_pair.as_rule() == Rule::symbol {
                            rest_symbol = Some(build_symbol(sym_pair.clone())?);
                        } else {
                            return Err(PestParseError::UnexpectedRule {
                                expected: "symbol after &".to_string(),
                                found: format!("{:?}", sym_pair.as_rule()),
                                rule_text: sym_pair.as_str().to_string(),
                                span: Some(pair_to_source_span(&sym_pair)),
                            });
                        }
                        break; 
                    }
                    Rule::WHITESPACE | Rule::COMMENT => {inner.next();}
                    _ => break, 
                }
            }
            Ok(MatchPattern::Map { entries, rest: rest_symbol })
        }
        rule => Err(PestParseError::UnexpectedRule {
            expected: "valid match pattern type".to_string(),
            found: format!("{:?}", rule),
            rule_text: pair.as_str().to_string(),
            span: Some(match_pattern_span.clone()),
        }),
    }
}

// Helper function to skip whitespace and comments in a Pairs iterator
pub fn next_significant<'a>(pairs: &mut Pairs<'a, super::Rule>) -> Option<Pair<'a, super::Rule>> {
    pairs.find(|p| p.as_rule() != super::Rule::WHITESPACE && p.as_rule() != super::Rule::COMMENT)
}
