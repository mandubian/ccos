use super::utils::unescape;
use super::PestParseError; // Added for Result return types
use super::Rule;
use crate::ast::{Keyword, Literal, MapDestructuringEntry, MapKey, MapMatchEntry, MatchPattern, Pattern, Symbol}; // Added match-related types
use pest::iterators::Pair;

// --- Helper Builders ---

pub(super) fn build_literal(pair: Pair<Rule>) -> Result<Literal, PestParseError> {
    let inner_pair = pair
        .into_inner()
        .next()        .ok_or_else(|| PestParseError::MissingToken("literal inner".to_string()))?;
    match inner_pair.as_rule() {
        Rule::integer => Ok(Literal::Integer(inner_pair.as_str().parse().map_err(
            |e| PestParseError::InvalidLiteral(format!("Invalid integer: {}", e)),
        )?)),
        Rule::float => Ok(Literal::Float(inner_pair.as_str().parse().map_err(
            |e| PestParseError::InvalidLiteral(format!("Invalid float: {}", e)),
        )?)),
        Rule::string => {
            let raw_str = inner_pair.as_str();
            let content = &raw_str[1..raw_str.len() - 1];
            Ok(Literal::String(unescape(content)?))
        }
        Rule::boolean => Ok(Literal::Boolean(inner_pair.as_str().parse().map_err(
            |e| PestParseError::InvalidLiteral(format!("Invalid boolean: {}", e)),
        )?)),
        Rule::nil => Ok(Literal::Nil),
        Rule::keyword => Ok(Literal::Keyword(build_keyword(inner_pair)?)),
        rule => Err(PestParseError::UnexpectedRule {
            expected: "valid literal type".to_string(),
            found: format!("{:?}", rule),
            rule_text: inner_pair.as_str().to_string(),
        }),
    }
}

pub(super) fn build_symbol(pair: Pair<Rule>) -> Result<Symbol, PestParseError> {
    if pair.as_rule() != Rule::symbol {
        return Err(PestParseError::UnexpectedRule {
            expected: "symbol".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
        });
    }
    Ok(Symbol(pair.as_str().to_string()))
}

pub(super) fn build_keyword(pair: Pair<Rule>) -> Result<Keyword, PestParseError> {
    if pair.as_rule() != Rule::keyword {
        return Err(PestParseError::UnexpectedRule {
            expected: "keyword".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
        });
    }
    Ok(Keyword(pair.as_str()[1..].to_string()))
}

pub(super) fn build_map_key(pair: Pair<Rule>) -> Result<MapKey, PestParseError> {
    if pair.as_rule() != Rule::map_key {
        return Err(PestParseError::UnexpectedRule {
            expected: "map_key".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
        });
    }

    let inner_pair = pair
        .into_inner()
        .next()
        .ok_or_else(|| PestParseError::MissingToken("map_key inner".to_string()))?;

    match inner_pair.as_rule() {
        Rule::keyword => Ok(MapKey::Keyword(build_keyword(inner_pair)?)),
        Rule::string => {
            let raw_str = inner_pair.as_str();
            let content = &raw_str[1..raw_str.len() - 1];
            Ok(MapKey::String(unescape(content)?))
        }
        Rule::integer => Ok(MapKey::Integer(inner_pair.as_str().parse().map_err(
            |e| PestParseError::InvalidLiteral(format!("Invalid integer map key: {}", e)),
        )?)),
        rule => Err(PestParseError::UnexpectedRule {
            expected: "keyword, string, or integer for map key".to_string(),
            found: format!("{:?}", rule),
            rule_text: inner_pair.as_str().to_string(),
        }),
    }
}

// Helper for map destructuring, returns (entries, rest_binding, as_binding)
fn build_map_destructuring_parts(
    pair: Pair<Rule>,
) -> Result<(Vec<MapDestructuringEntry>, Option<Symbol>, Option<Symbol>), PestParseError> {
    if pair.as_rule() != Rule::map_destructuring_pattern {
        return Err(PestParseError::UnexpectedRule {
            expected: "map_destructuring_pattern".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
        });
    }
    
    let mut inner = pair.into_inner().peekable();
    let mut entries = Vec::new();
    let mut rest_binding = None;
    let mut as_binding = None;

    while let Some(current_pair) = inner.next() {match current_pair.as_rule() {            Rule::map_destructuring_entry => {
                let mut entry_inner = current_pair.into_inner(); // current_pair is the map_destructuring_entry
                
                // Check what type of entry this is
                let first_token = entry_inner.peek().ok_or_else(|| {
                    PestParseError::MissingToken("first token in map_destructuring_entry".to_string())
                })?;
                
                if first_token.as_rule() == Rule::keys_entry {
                    // Handle keys_entry rule
                    let keys_entry_pair = entry_inner.next().unwrap(); // Consume the keys_entry
                    let keys_inner = keys_entry_pair.into_inner();
                    
                    // Skip the ":keys" token and look for symbols
                    let mut symbols = Vec::new();
                    for token in keys_inner {
                        if token.as_rule() == Rule::symbol {
                            symbols.push(build_symbol(token)?);
                        }
                    }
                    
                    entries.push(MapDestructuringEntry::Keys(symbols));
                } else {
                    // Regular map_key ~ binding_pattern
                    let key_token_pair = entry_inner.next().ok_or_else(|| {
                        PestParseError::MissingToken("map_key in map_destructuring_entry".to_string())
                    })?;
                    
                    // Ensure it's actually a map_key rule, as expected by build_map_key
                    if key_token_pair.as_rule() != Rule::map_key {
                         return Err(PestParseError::UnexpectedRule {
                            expected: "map_key".to_string(),
                            found: format!("{:?}", key_token_pair.as_rule()),
                            rule_text: key_token_pair.as_str().to_string(),
                        });
                    }

                    let val_pattern_pair = entry_inner.next().ok_or_else(|| {
                        PestParseError::MissingToken("binding_pattern in map_destructuring_entry".to_string())
                    })?;
                    // val_pattern_pair is expected to be a binding_pattern. build_pattern can handle this.

                    let map_key_val = build_map_key(key_token_pair)?;
                    let pattern_to_bind = build_pattern(val_pattern_pair)?;

                    entries.push(MapDestructuringEntry::KeyBinding {
                        key: map_key_val,
                        pattern: Box::new(pattern_to_bind),
                    });
                }
            }Rule::map_rest_binding => {
                // Extract the symbol from the map_rest_binding rule
                let mut rest_inner = current_pair.into_inner();
                // The AMPERSAND is part of the rule structure, not a separate inner token.
                // The first inner token IS the symbol.
                let rest_sym_pair = rest_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken("symbol in map_rest_binding".to_string())
                })?;
                if rest_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in map_rest_binding".to_string(),
                        found: format!("{:?}", rest_sym_pair.as_rule()),
                        rule_text: rest_sym_pair.as_str().to_string(),
                    });
                }
                rest_binding = Some(build_symbol(rest_sym_pair)?);
            }
            Rule::map_as_binding => {
                // Extract the symbol from the map_as_binding rule
                let mut as_inner = current_pair.into_inner();
                // Skip the ":as" token (which is a literal string, so it's not passed through)
                let as_sym_pair = as_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken("symbol in map_as_binding".to_string())
                })?;
                if as_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in map_as_binding".to_string(),
                        found: format!("{:?}", as_sym_pair.as_rule()),
                        rule_text: as_sym_pair.as_str().to_string(),
                    });
                }
                as_binding = Some(build_symbol(as_sym_pair)?);
            }
            // Note: These Rule variants don't exist in the grammar, so we handle them differently
            // Rule::AMPERSAND and Rule::COLON_AS don't exist - we need to check string content
            _ if current_pair.as_str() == "&" => {
                // &
                let rest_sym_pair = inner
                    .next()
                    .ok_or_else(|| PestParseError::MissingToken("& rest symbol".to_string()))?;
                if rest_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol after &".to_string(),
                        found: format!("{:?}", rest_sym_pair.as_rule()),
                        rule_text: rest_sym_pair.as_str().to_string(),
                    });
                }
                rest_binding = Some(build_symbol(rest_sym_pair)?);
            }
            _ if current_pair.as_str() == ":as" => {
                // :as
                let as_sym_pair = inner
                    .next()
                    .ok_or_else(|| PestParseError::MissingToken(":as alias symbol".to_string()))?;
                if as_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol after :as".to_string(),
                        found: format!("{:?}", as_sym_pair.as_rule()),
                        rule_text: as_sym_pair.as_str().to_string(),
                    });
                }
                as_binding = Some(build_symbol(as_sym_pair)?);
            }
            Rule::WHITESPACE | Rule::COMMENT => { /* Skip */ }            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "map destructuring entry, map_rest_binding, or map_as_binding".to_string(),
                    found: format!("{:?}", rule),
                    rule_text: current_pair.as_str().to_string(),
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
    if pair.as_rule() != Rule::vector_destructuring_pattern {
        return Err(PestParseError::UnexpectedRule {
            expected: "vector_destructuring_pattern".to_string(),
            found: format!("{:?}", pair.as_rule()),
            rule_text: pair.as_str().to_string(),
        });
    }
    
    let mut inner = pair.into_inner().peekable();
    let mut elements = Vec::new();
    let mut rest_binding = None;
    let mut as_binding = None;
    while let Some(current_pair) = inner.next() {
        match current_pair.as_rule() {
            // Since binding_pattern is silent (_), we get the inner patterns directly
            Rule::symbol | Rule::wildcard | Rule::map_destructuring_pattern | Rule::vector_destructuring_pattern => {
                elements.push(build_pattern(current_pair)?);
            }
            Rule::vector_rest_binding => {
                // Extract the symbol from the vector_rest_binding rule
                let mut rest_inner = current_pair.into_inner();
                // The AMPERSAND is part of the rule structure, not a separate inner token.
                // The first inner token IS the symbol.
                let rest_sym_pair = rest_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken("symbol in vector_rest_binding".to_string())
                })?;
                if rest_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in vector_rest_binding".to_string(),
                        found: format!("{:?}", rest_sym_pair.as_rule()),
                        rule_text: rest_sym_pair.as_str().to_string(),
                    });
                }
                rest_binding = Some(build_symbol(rest_sym_pair)?);
            }
            Rule::vector_as_binding => {
                // Extract the symbol from the vector_as_binding rule
                let mut as_inner = current_pair.into_inner();
                // Skip the ":as" token (which is a literal string, so it's not passed through)
                let as_sym_pair = as_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken("symbol in vector_as_binding".to_string())
                })?;
                if as_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol in vector_as_binding".to_string(),
                        found: format!("{:?}", as_sym_pair.as_rule()),
                        rule_text: as_sym_pair.as_str().to_string(),
                    });
                }
                as_binding = Some(build_symbol(as_sym_pair)?);
            }
            Rule::WHITESPACE | Rule::COMMENT => { /* Skip */ }
            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "binding pattern, vector_rest_binding, or vector_as_binding".to_string(),
                    found: format!("{:?}", rule),
                    rule_text: current_pair.as_str().to_string(),
                })
            }
        }
    }
    Ok((elements, rest_binding, as_binding))
}

pub(super) fn build_pattern(pair: Pair<Rule>) -> Result<Pattern, PestParseError> {
    let actual_pair = match pair.as_rule() {
        Rule::binding_pattern | Rule::match_pattern => pair
            .into_inner()
            .next()
            .ok_or_else(|| PestParseError::MissingToken("binding_pattern inner".to_string()))?,
        _ => pair,
    };

    match actual_pair.as_rule() {
        Rule::symbol => Ok(Pattern::Symbol(build_symbol(actual_pair)?)),
        Rule::wildcard => Ok(Pattern::Wildcard),
        Rule::map_destructuring_pattern => {
            let (entries, rest, as_binding) = build_map_destructuring_parts(actual_pair)?;
            Ok(Pattern::MapDestructuring {
                entries,
                rest,
                as_symbol: as_binding,
            })
        }
        Rule::vector_destructuring_pattern => {
            let (elements, rest, as_binding) = build_vector_destructuring_parts(actual_pair)?;
            Ok(Pattern::VectorDestructuring {
                elements,
                rest,
                as_symbol: as_binding,
            })
        }
        rule => Err(PestParseError::UnexpectedRule {
            expected:
                "symbol, wildcard, map_destructuring_pattern, or vector_destructuring_pattern"
                    .to_string(),
            found: format!("{:?}", rule),
            rule_text: actual_pair.as_str().to_string(),
        }),
    }
}

// Build match pattern for match expressions
pub(super) fn build_match_pattern(pair: Pair<Rule>) -> Result<MatchPattern, PestParseError> {
    let actual_pair = match pair.as_rule() {
        Rule::match_pattern => pair
            .into_inner()
            .next()
            .ok_or_else(|| PestParseError::MissingToken("match_pattern inner".to_string()))?,
        _ => pair,
    };

    match actual_pair.as_rule() {
        Rule::literal => Ok(MatchPattern::Literal(build_literal(actual_pair)?)),
        Rule::symbol => Ok(MatchPattern::Symbol(build_symbol(actual_pair)?)),
        Rule::keyword => Ok(MatchPattern::Keyword(build_keyword(actual_pair)?)),
        Rule::wildcard => Ok(MatchPattern::Wildcard),
        Rule::type_expr => {
            // For type expressions, we don't bind to a symbol in basic matching
            // More complex type matching with binding would need different syntax
            Ok(MatchPattern::Type(super::types::build_type_expr(actual_pair)?, None))
        }
        Rule::vector_match_pattern => {
            let mut elements = Vec::new();
            let mut rest: Option<Symbol> = None;
            let mut inner_pairs = actual_pair.into_inner().peekable();
            
            while let Some(p_peek) = inner_pairs.peek() {
                // Skip whitespace and comments
                if p_peek.as_rule() == Rule::WHITESPACE || p_peek.as_rule() == Rule::COMMENT {
                    inner_pairs.next();
                    continue;
                }
                
                // Check for rest pattern "&"
                if p_peek.as_str() == "&" {
                    inner_pairs.next(); // Consume "&"
                    
                    // Skip whitespace after "&"
                    while let Some(ws_peek) = inner_pairs.peek() {
                        if ws_peek.as_rule() == Rule::WHITESPACE || ws_peek.as_rule() == Rule::COMMENT {
                            inner_pairs.next();
                        } else {
                            break;
                        }
                    }
                    
                    let sym_pair = inner_pairs.next().ok_or_else(|| {
                        PestParseError::InvalidInput(
                            "Expected symbol after & in vector match pattern".to_string(),
                        )
                    })?;
                    
                    if sym_pair.as_rule() != Rule::symbol {
                        return Err(PestParseError::UnexpectedRule {
                            expected: "symbol".to_string(),
                            found: format!("{:?}", sym_pair.as_rule()),
                            rule_text: sym_pair.as_str().to_string(),
                        });
                    }
                    
                    rest = Some(build_symbol(sym_pair)?);
                    break;
                }
                
                // Otherwise, it's an element pattern
                let p = inner_pairs.next().unwrap();
                elements.push(build_match_pattern(p)?);
            }
            
            Ok(MatchPattern::Vector { elements, rest })
        }
        Rule::map_match_pattern => {
            let mut entries = Vec::new();
            let mut rest: Option<Symbol> = None;
            let mut inner_pairs = actual_pair.into_inner().peekable();
            
            while let Some(p_peek) = inner_pairs.peek() {
                // Skip whitespace and comments
                if p_peek.as_rule() == Rule::WHITESPACE || p_peek.as_rule() == Rule::COMMENT {
                    inner_pairs.next();
                    continue;
                }
                
                // Check for rest pattern "&"
                if p_peek.as_str() == "&" {
                    inner_pairs.next(); // Consume "&"
                    
                    // Skip whitespace after "&"
                    while let Some(ws_peek) = inner_pairs.peek() {
                        if ws_peek.as_rule() == Rule::WHITESPACE || ws_peek.as_rule() == Rule::COMMENT {
                            inner_pairs.next();
                        } else {
                            break;
                        }
                    }
                    
                    let sym_pair = inner_pairs.next().ok_or_else(|| {
                        PestParseError::InvalidInput(
                            "Expected symbol after & in map match pattern".to_string(),
                        )
                    })?;
                    
                    if sym_pair.as_rule() != Rule::symbol {
                        return Err(PestParseError::UnexpectedRule {
                            expected: "symbol".to_string(),
                            found: format!("{:?}", sym_pair.as_rule()),
                            rule_text: sym_pair.as_str().to_string(),
                        });
                    }
                    
                    rest = Some(build_symbol(sym_pair)?);
                    break;
                }
                
                // Otherwise, it should be a map_match_pattern_entry
                if p_peek.as_rule() == Rule::map_match_pattern_entry {
                    let entry_pair = inner_pairs.next().unwrap();
                    let mut entry_inner = entry_pair.into_inner();
                    
                    let key_pair = entry_inner.next().ok_or_else(|| {
                        PestParseError::MissingToken("map_key in map_match_pattern_entry".to_string())
                    })?;
                    
                    let value_pattern_pair = entry_inner.next().ok_or_else(|| {
                        PestParseError::MissingToken("match_pattern in map_match_pattern_entry".to_string())
                    })?;
                    
                    entries.push(MapMatchEntry {
                        key: build_map_key(key_pair)?,
                        pattern: Box::new(build_match_pattern(value_pattern_pair)?),
                    });
                } else {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "map_match_pattern_entry or & rest".to_string(),
                        found: format!("{:?}", p_peek.as_rule()),
                        rule_text: p_peek.as_str().to_string(),
                    });
                }
            }
            
            Ok(MatchPattern::Map { entries, rest })
        }
        Rule::as_match_pattern => {
            // Parse (:as symbol pattern)
            let mut inner_pairs = actual_pair.into_inner();
            
            // Skip :as keyword
            let _as_keyword = inner_pairs.next(); // Should be ":as"
            
            // Skip whitespace
            while let Some(ws_peek) = inner_pairs.peek() {
                if ws_peek.as_rule() == Rule::WHITESPACE || ws_peek.as_rule() == Rule::COMMENT {
                    inner_pairs.next();
                } else {
                    break;
                }
            }
            
            let symbol_pair = inner_pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput("AS pattern: missing symbol".to_string())
            })?;
            
            if symbol_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::UnexpectedRule {
                    expected: "symbol".to_string(),
                    found: format!("{:?}", symbol_pair.as_rule()),
                    rule_text: symbol_pair.as_str().to_string(),
                });
            }
            
            // Skip whitespace
            while let Some(ws_peek) = inner_pairs.peek() {
                if ws_peek.as_rule() == Rule::WHITESPACE || ws_peek.as_rule() == Rule::COMMENT {
                    inner_pairs.next();
                } else {
                    break;
                }
            }
            
            let pattern_pair = inner_pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput("AS pattern: missing pattern to bind".to_string())
            })?;
            
            Ok(MatchPattern::As(
                build_symbol(symbol_pair)?,
                Box::new(build_match_pattern(pattern_pair)?),
            ))
        }
        unknown_rule => {
            // Handle wildcard by string content if it's not a specific Rule::wildcard
            if actual_pair.as_str() == "_" {
                Ok(MatchPattern::Wildcard)
            } else {
                Err(PestParseError::UnsupportedRule(format!(
                    "build_match_pattern: Unsupported rule: {:?}, content: '{}'",
                    unknown_rule,
                    actual_pair.as_str()
                )))
            }
        }
    }
}
