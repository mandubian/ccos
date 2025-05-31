use super::utils::unescape;
use super::PestParseError; // Added for Result return types
use super::Rule;
use crate::ast::{Keyword, Literal, MapDestructuringEntry, MapKey, Pattern, Symbol}; // Fixed: MapPatternEntry -> MapDestructuringEntry
use pest::iterators::Pair;

// --- Helper Builders ---

pub(super) fn build_literal(pair: Pair<Rule>) -> Result<Literal, PestParseError> {
    let inner_pair = pair
        .into_inner()
        .next()
        .ok_or_else(|| PestParseError::MissingToken("literal inner".to_string()))?;
    eprintln!(
        "[build_literal] inner_pair: rule={:?}, str='{}'",
        inner_pair.as_rule(),
        inner_pair.as_str()
    );
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

    while let Some(current_pair) = inner.next() {
        match current_pair.as_rule() {
            Rule::map_destructuring_entry => {
                let mut entry_inner = current_pair.into_inner();
                let key_like_pair = entry_inner.next().ok_or_else(|| {
                    PestParseError::MissingToken("map destructuring key".to_string())
                })?;

                if key_like_pair.as_rule() == Rule::symbol && entry_inner.peek().is_none() {
                    // Shorthand like {x}
                    let sym = build_symbol(key_like_pair.clone())?;
                    entries.push(MapDestructuringEntry::KeyBinding {
                        key: MapKey::Keyword(Keyword(sym.0.clone())),
                        pattern: Box::new(Pattern::Symbol(sym)),
                    });
                } else {
                    // Explicit key pattern like {:key x} or {"key" x}
                    let key_token_pair = key_like_pair;
                    let val_pattern_pair = entry_inner.next().ok_or_else(|| {
                        PestParseError::MissingToken("map destructuring value pattern".to_string())
                    })?;

                    let map_key = match key_token_pair.as_rule() {
                        Rule::keyword => MapKey::Keyword(build_keyword(key_token_pair)?),
                        Rule::string => {
                            let raw_str = key_token_pair.as_str();
                            let content = &raw_str[1..raw_str.len() - 1];
                            MapKey::String(unescape(content)?)
                        }
                        _ => {
                            return Err(PestParseError::UnexpectedRule {
                                expected: "keyword or string for map pattern key".to_string(),
                                found: format!("{:?}", key_token_pair.as_rule()),
                                rule_text: key_token_pair.as_str().to_string(),
                            })
                        }
                    };

                    let pattern_to_bind = build_pattern(val_pattern_pair)?;
                    entries.push(MapDestructuringEntry::KeyBinding {
                        key: map_key,
                        pattern: Box::new(pattern_to_bind),
                    });
                }
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
            Rule::WHITESPACE | Rule::COMMENT => { /* Skip */ }
            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "map destructuring entry, &, or :as".to_string(),
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
            Rule::binding_pattern => {
                elements.push(build_pattern(current_pair)?);
            }
            // Note: These Rule variants don't exist in the grammar, so we handle them differently
            _ if current_pair.as_str() == "&" => {
                // &
                let rest_sym_pair = inner.next().ok_or_else(|| {
                    PestParseError::MissingToken("& rest symbol for vector".to_string())
                })?;
                if rest_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol after & in vector".to_string(),
                        found: format!("{:?}", rest_sym_pair.as_rule()),
                        rule_text: rest_sym_pair.as_str().to_string(),
                    });
                }
                rest_binding = Some(build_symbol(rest_sym_pair)?);
            }
            _ if current_pair.as_str() == ":as" => {
                // :as
                let as_sym_pair = inner.next().ok_or_else(|| {
                    PestParseError::MissingToken(":as alias symbol for vector".to_string())
                })?;
                if as_sym_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::UnexpectedRule {
                        expected: "symbol after :as in vector".to_string(),
                        found: format!("{:?}", as_sym_pair.as_rule()),
                        rule_text: as_sym_pair.as_str().to_string(),
                    });
                }
                as_binding = Some(build_symbol(as_sym_pair)?);
            }
            Rule::WHITESPACE | Rule::COMMENT => { /* Skip */ }
            rule => {
                return Err(PestParseError::UnexpectedRule {
                    expected: "binding pattern, &, or :as in vector".to_string(),
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
