use pest::iterators::Pair; // Removed unused Pairs
                           // Removed unused build_expression, build_type_expr, TypeExpr
use super::utils::unescape;
use super::Rule;
use crate::ast::{Keyword, Literal, MapKey, MapPattern, Pattern, Symbol, VectorPattern}; // Added MapPattern, VectorPattern
use std::collections::HashMap; // Added HashMap
                               // Removed unused build_type_expr, build_expression imports

// --- Helper Builders ---

pub(super) fn build_literal(pair: Pair<Rule>) -> Literal {
    let inner_pair = pair.into_inner().next().unwrap(); // Get the actual literal type rule
    eprintln!("[build_literal] inner_pair: rule={:?}, str='{}'", inner_pair.as_rule(), inner_pair.as_str());
    match inner_pair.as_rule() {
        Rule::integer => Literal::Integer(
            inner_pair
                .as_str()
                .parse()
                .expect("Invalid integer literal"),
        ),
        Rule::float => Literal::Float(inner_pair.as_str().parse().expect("Invalid float literal")),
        Rule::string => {
            let raw_str = inner_pair.as_str();
            // Remove quotes and unescape
            let content = &raw_str[1..raw_str.len() - 1];
            Literal::String(unescape(content).expect("Invalid escape sequence in string"))
        }
        Rule::boolean => Literal::Boolean(
            inner_pair
                .as_str()
                .parse()
                .expect("Invalid boolean literal"),
        ),
        Rule::nil => Literal::Nil,
        rule => unimplemented!("build_literal not implemented for rule: {:?}", rule),
    }
}

pub(super) fn build_symbol(pair: Pair<Rule>) -> Symbol {
    assert_eq!(pair.as_rule(), Rule::symbol);
    Symbol(pair.as_str().to_string())
}

pub(super) fn build_keyword(pair: Pair<Rule>) -> Keyword {
    assert_eq!(pair.as_rule(), Rule::keyword);
    // Remove the leading ':'
    Keyword(pair.as_str()[1..].to_string())
}

// Builds a MapKey from a map_key rule pair
pub(super) fn build_map_key(pair: Pair<Rule>) -> MapKey {
    // Expect the outer rule to be map_key
    assert_eq!(
        pair.as_rule(),
        Rule::map_key,
        "build_map_key expects Rule::map_key"
    );

    // Get the actual inner key rule (keyword, string, or integer)
    let inner_pair = pair
        .into_inner()
        .next()
        .expect("map_key rule should have an inner rule");

    match inner_pair.as_rule() {
        Rule::keyword => MapKey::Keyword(build_keyword(inner_pair)),
        Rule::string => {
            let raw_str = inner_pair.as_str();
            // Remove quotes and unescape
            let content = &raw_str[1..raw_str.len() - 1];
            MapKey::String(unescape(content).expect("Invalid escape sequence in map key string"))
        }
        Rule::integer => MapKey::Integer(
            inner_pair
                .as_str()
                .parse()
                .expect("Invalid integer map key"),
        ),
        // This panic now refers to unexpected rules *inside* map_key
        rule => unimplemented!(
            "build_map_key encountered unexpected rule inside map_key: {:?}",
            rule
        ),
    }
}

// Builds a Pattern::MapPattern from a map_destructuring_pattern rule pair
fn build_map_destructuring_pattern(pair: Pair<Rule>) -> MapPattern {
    assert_eq!(pair.as_rule(), Rule::map_destructuring_pattern);
    let mut inner = pair.into_inner().peekable();

    let mut keys = Vec::new();
    let mut entries = Vec::new();
    let mut or_defaults = None;
    let mut rest = None;
    let mut as_binding = None;

    while let Some(current_pair) = inner.peek() {
        match current_pair.as_rule() {
            // map_destructuring_key_entry = _{ (":keys" ~ "[" ~ binding_pattern+ ~ "]") | (keyword ~ binding_pattern) | (string ~ binding_pattern) }
            Rule::keyword | Rule::string => {
                // It's a (key pattern) entry
                let key_pair = inner.next().unwrap(); // Consume key
                                                      // Consume potential whitespace/comment
                while let Some(peeked) = inner.peek() {
                    if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                        inner.next();
                    } else {
                        break;
                    }
                }
                let pattern_pair = inner
                    .next()
                    .expect("Map pattern entry needs a pattern after key");
                assert_eq!(
                    pattern_pair.as_rule(),
                    Rule::binding_pattern,
                    "Expected binding_pattern in map entry"
                );
                let map_key = build_map_key(key_pair); // Use build_map_key for consistency
                let pattern = build_pattern(pattern_pair);
                entries.push((map_key, pattern));
            }
            Rule::identifier if current_pair.as_str() == ":keys" => {
                inner.next(); // Consume :keys
                              // Consume potential whitespace/comment
                while let Some(peeked) = inner.peek() {
                    if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                        inner.next();
                    } else {
                        break;
                    }
                }
                let keys_vec_pair = inner.next().expect(":keys requires a vector []");
                assert_eq!(keys_vec_pair.as_rule(), Rule::vector); // Assuming grammar uses vector syntax `[...]`
                for key_pattern_pair in keys_vec_pair.into_inner() {
                    if key_pattern_pair.as_rule() == Rule::binding_pattern {
                        // Expect symbols inside :keys [...]
                        if let Pattern::Symbol(s) = build_pattern(key_pattern_pair) {
                            keys.push(s);
                        } else {
                            panic!(":keys expects symbols as patterns");
                        }
                    }
                }
            }
            // map_destructuring_or_entry = { ":or" ~ "{" ~ (symbol ~ literal)+ ~ "}" }
            Rule::identifier if current_pair.as_str() == ":or" => {
                inner.next(); // Consume :or
                              // Consume potential whitespace/comment
                while let Some(peeked) = inner.peek() {
                    if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                        inner.next();
                    } else {
                        break;
                    }
                }
                let or_map_pair = inner.next().expect(":or requires a map {}");
                assert_eq!(or_map_pair.as_rule(), Rule::map); // Assuming grammar uses map syntax `{...}`
                let mut defaults = HashMap::new();
                let mut or_inner = or_map_pair.into_inner();
                while let Some(sym_pair) = or_inner.next() {
                    if sym_pair.as_rule() == Rule::WHITESPACE || sym_pair.as_rule() == Rule::COMMENT
                    {
                        continue;
                    }
                    assert_eq!(sym_pair.as_rule(), Rule::symbol);
                    let sym = build_symbol(sym_pair);
                    // Consume potential whitespace/comment
                    while let Some(peeked) = or_inner.peek() {
                        if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT
                        {
                            or_inner.next();
                        } else {
                            break;
                        }
                    }
                    let lit_pair = or_inner.next().expect(":or entry needs literal value");
                    assert_eq!(lit_pair.as_rule(), Rule::literal);
                    let lit = build_literal(lit_pair);
                    defaults.insert(sym, lit);
                }
                or_defaults = Some(defaults);
            }
            // ("&" ~ symbol)?
            // Changed Rule::identifier to Rule::symbol based on grammar
            Rule::symbol if current_pair.as_str() == "&" => {
                inner.next(); // Consume &
                              // Consume potential whitespace/comment
                while let Some(peeked) = inner.peek() {
                    if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                        inner.next();
                    } else {
                        break;
                    }
                }
                let rest_sym_pair = inner.next().expect("& requires a symbol");
                assert_eq!(rest_sym_pair.as_rule(), Rule::symbol);
                rest = Some(build_symbol(rest_sym_pair));
            }
            // (":as" ~ symbol)?
            // Changed Rule::keyword to Rule::symbol based on grammar
            Rule::symbol if current_pair.as_str() == ":as" => {
                inner.next(); // Consume :as
                              // Consume potential whitespace/comment
                while let Some(peeked) = inner.peek() {
                    if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                        inner.next();
                    } else {
                        break;
                    }
                }
                let as_sym_pair = inner.next().expect(":as requires a symbol");
                assert_eq!(as_sym_pair.as_rule(), Rule::symbol);
                as_binding = Some(build_symbol(as_sym_pair));
            }
            Rule::WHITESPACE | Rule::COMMENT => {
                inner.next(); // Consume whitespace/comment
            }
            rule => panic!(
                "Unexpected rule inside map_destructuring_pattern: {:?}",
                rule
            ),
        }
    }

    MapPattern {
        keys,
        entries,
        or_defaults,
        rest,
        as_binding,
    }
}

// Builds a Pattern::VectorPattern from a vector_destructuring_pattern rule pair
fn build_vector_destructuring_pattern(pair: Pair<Rule>) -> VectorPattern {
    assert_eq!(pair.as_rule(), Rule::vector_destructuring_pattern);
    let mut inner = pair.into_inner().peekable();

    let mut elements = Vec::new();
    let mut rest = None;
    let mut as_binding = None;

    while let Some(current_pair) = inner.peek() {
        match current_pair.as_rule() {
            Rule::binding_pattern => {
                elements.push(build_pattern(inner.next().unwrap()));
            }
            // ("&" ~ symbol)?
            // Changed Rule::identifier to Rule::symbol based on grammar
            Rule::symbol if current_pair.as_str() == "&" => {
                inner.next(); // Consume &
                              // Consume potential whitespace/comment
                while let Some(peeked) = inner.peek() {
                    if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                        inner.next();
                    } else {
                        break;
                    }
                }
                let rest_sym_pair = inner.next().expect("& requires a symbol");
                assert_eq!(rest_sym_pair.as_rule(), Rule::symbol);
                rest = Some(build_symbol(rest_sym_pair));
            }
            // (":as" ~ symbol)?
            // Changed Rule::keyword to Rule::symbol based on grammar
            Rule::symbol if current_pair.as_str() == ":as" => {
                inner.next(); // Consume :as
                              // Consume potential whitespace/comment
                while let Some(peeked) = inner.peek() {
                    if peeked.as_rule() == Rule::WHITESPACE || peeked.as_rule() == Rule::COMMENT {
                        inner.next();
                    } else {
                        break;
                    }
                }
                let as_sym_pair = inner.next().expect(":as requires a symbol");
                assert_eq!(as_sym_pair.as_rule(), Rule::symbol);
                as_binding = Some(build_symbol(as_sym_pair));
            }
            Rule::WHITESPACE | Rule::COMMENT => {
                inner.next(); // Consume whitespace/comment
            }
            rule => panic!(
                "Unexpected rule inside vector_destructuring_pattern: {:?}",
                rule
            ),
        }
    }

    VectorPattern {
        elements,
        rest,
        as_binding,
    }
}

// Updated build_pattern
pub(super) fn build_pattern(pair: Pair<Rule>) -> Pattern {
    // binding_pattern = _{ symbol | map_destructuring_pattern | vector_destructuring_pattern | wildcard }
    // This function might also be called with literal or other rules in match context later
    let actual_pair = match pair.as_rule() {
        Rule::binding_pattern | Rule::match_pattern => pair.into_inner().next().unwrap(), // Drill down
        _ => pair, // Assume it's already the concrete rule
    };

    match actual_pair.as_rule() {
        Rule::symbol => Pattern::Symbol(build_symbol(actual_pair)),
        Rule::literal => Pattern::Literal(build_literal(actual_pair)), // Needed for match
        Rule::wildcard => Pattern::Wildcard,
        Rule::map_destructuring_pattern => {
            Pattern::MapPattern(build_map_destructuring_pattern(actual_pair))
        }
        Rule::vector_destructuring_pattern => {
            Pattern::VectorPattern(build_vector_destructuring_pattern(actual_pair))
        }
        // TODO: Handle match-specific patterns like map_match_pattern, vector_match_pattern, :as, keyword, type_expr if build_pattern is used for match patterns directly.
        // It might be better to have a separate build_match_pattern function.
        rule => unimplemented!(
            "build_pattern not implemented for rule: {:?} - {}",
            rule,
            actual_pair.as_str()
        ),
    }
}
