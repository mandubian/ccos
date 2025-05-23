use super::common::{build_keyword, build_literal, build_symbol}; // Added imports
use super::Rule;
// Removed unused Keyword, Literal, Symbol
use crate::ast::{PrimitiveType, TypeExpr};
use pest::iterators::{Pair, Pairs}; // Added Pairs

// Helper to skip whitespace and comments in a Pairs iterator
fn next_significant<'a>(pairs: &mut Pairs<'a, Rule>) -> Option<Pair<'a, Rule>> {
    pairs.find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
}

pub(super) fn build_type_expr(pair: Pair<Rule>) -> TypeExpr {
    // Allow direct calls with concrete rules or via the main type_expr rule
    let actual_pair = match pair.as_rule() {
        Rule::type_expr | Rule::param_type => pair
            .into_inner()
            .next()
            .expect("type_expr/param_type should have inner rule"),
        _ => pair, // Assume it's already the concrete rule
    };

    match actual_pair.as_rule() {
        // --- Primitive Types ---
        Rule::primitive_type => {
            match actual_pair.as_str() {
                ":int" => TypeExpr::Primitive(PrimitiveType::Int),
                ":float" => TypeExpr::Primitive(PrimitiveType::Float),
                ":string" => TypeExpr::Primitive(PrimitiveType::String),
                ":bool" => TypeExpr::Primitive(PrimitiveType::Bool),
                ":nil" => TypeExpr::Primitive(PrimitiveType::Nil),
                ":keyword" => TypeExpr::Primitive(PrimitiveType::Keyword),
                ":symbol" => TypeExpr::Primitive(PrimitiveType::Symbol),
                ":any" => TypeExpr::Any,
                ":never" => TypeExpr::Never,
                s if s.starts_with(":") => {
                    // Treat unknown :Type as an alias (strip leading ':')
                    let name = &s[1..];
                    TypeExpr::Alias(crate::ast::Symbol(name.to_string()))
                }
                _ => unimplemented!("Unknown primitive type string: {}", actual_pair.as_str()),
            }
        },
        // --- Type Alias ---
        Rule::symbol => TypeExpr::Alias(build_symbol(actual_pair)),

        // --- Complex Types ---
        Rule::vector_type => {
            // vector_type = { "[" ~ ":vector" ~ WHITESPACE* ~ type_expr ~ WHITESPACE* ~ "]" }
            let inner_type_pair = actual_pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::type_expr)
                .expect("vector_type requires an inner type_expr");
            TypeExpr::Vector(Box::new(build_type_expr(inner_type_pair)))
        }
        Rule::map_type => {
            // map_type = { "[" ~ ":map" ~ (WHITESPACE* ~ map_type_entry)* ~ (WHITESPACE* ~ map_type_wildcard)? ~ WHITESPACE* ~ "]" }
            let mut inner = actual_pair.into_inner();
            let mut entries = Vec::new();
            let mut wildcard = None;

            while let Some(pair) = inner.peek() {
                match pair.as_rule() {
                    Rule::map_type_entry => {
                        // map_type_entry = { "[" ~ keyword ~ WHITESPACE* ~ type_expr ~ (WHITESPACE* ~ "?")? ~ WHITESPACE* ~ "]" }
                        let mut entry_inner = inner.next().unwrap().into_inner();
                        let key = build_keyword(
                            next_significant(&mut entry_inner).expect("Map entry needs keyword"),
                        );
                        let type_expr = build_type_expr(
                            next_significant(&mut entry_inner).expect("Map entry needs type"),
                        );
                        // Check if '?' exists
                        let is_optional = entry_inner.any(|p| p.as_str() == "?");
                        entries.push((key, type_expr, is_optional));
                    }
                    Rule::map_type_wildcard => {
                        // map_type_wildcard = { "[" ~ ":*" ~ WHITESPACE* ~ type_expr ~ WHITESPACE* ~ "]" }
                        let mut wildcard_inner = inner.next().unwrap().into_inner(); // Made mutable
                        let wildcard_type_pair = wildcard_inner
                            .find(|p| p.as_rule() == Rule::type_expr)
                            .expect("map_type_wildcard requires an inner type_expr");
                        wildcard = Some(Box::new(build_type_expr(wildcard_type_pair)));
                    }
                    Rule::WHITESPACE | Rule::COMMENT => {
                        inner.next(); // Skip
                    }
                    // Skip the initial ":map" keyword
                    Rule::keyword if pair.as_str() == ":map" => {
                        inner.next();
                    }
                    rule => panic!("Unexpected rule inside map_type: {:?}", rule),
                }
            }
            TypeExpr::Map { entries, wildcard }
        }
        Rule::function_type => {
            // function_type = { "[" ~ ":=>" ~ WHITESPACE* ~ "[" ~ (WHITESPACE* ~ param_type)* ~ (WHITESPACE* ~ variadic_param_type)? ~ WHITESPACE* ~ "]" ~ WHITESPACE* ~ type_expr ~ WHITESPACE* ~ "]" }
            let mut inner = actual_pair.into_inner();
            let mut param_types = Vec::new();
            let mut variadic_param_type = None;
            // Removed unused initial assignment: let mut return_type = None;

            // Find the parameters vector '[' ... ']'
            let params_vector = inner
                .find(|p| p.as_rule() == Rule::vector) // Assuming grammar uses vector for params
                .expect("Function type needs parameter vector []");

            let mut params_inner = params_vector.into_inner();
            while let Some(pair) = params_inner.peek() {
                match pair.as_rule() {
                    Rule::param_type => {
                        param_types.push(build_type_expr(params_inner.next().unwrap()));
                    }
                    Rule::variadic_param_type => {
                        // variadic_param_type = { "&" ~ WHITESPACE* ~ type_expr }
                        let mut var_inner = params_inner.next().unwrap().into_inner(); // Made mutable
                        let var_type_pair = var_inner
                            .find(|p| p.as_rule() == Rule::type_expr)
                            .expect("Variadic param type needs inner type_expr");
                        variadic_param_type = Some(Box::new(build_type_expr(var_type_pair)));
                        break; // Only one variadic param allowed
                    }
                    Rule::WHITESPACE | Rule::COMMENT => {
                        params_inner.next(); // Skip
                    }
                    rule => panic!("Unexpected rule inside function params vector: {:?}", rule),
                }
            }

            // Find the return type (the last type_expr in the main list)
            let return_type = inner
                .find(|p| p.as_rule() == Rule::type_expr)
                .map(|p| Box::new(build_type_expr(p)))
                .expect("Function type needs return type");

            TypeExpr::Function {
                param_types,
                variadic_param_type,
                return_type, // Directly use the parsed value
            }
        }
        Rule::resource_type => {
            // resource_type = { "[" ~ ":resource" ~ WHITESPACE* ~ symbol ~ WHITESPACE* ~ "]" }
            let symbol_pair = actual_pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::symbol)
                .expect("resource_type requires a symbol");
            TypeExpr::Resource(build_symbol(symbol_pair))
        }
        Rule::union_type => {
            // union_type = { "[" ~ ":or" ~ (WHITESPACE* ~ type_expr)+ ~ WHITESPACE* ~ "]" }
            let types = actual_pair
                .into_inner()
                .filter(|p| p.as_rule() == Rule::type_expr)
                .map(build_type_expr)
                .collect();
            TypeExpr::Union(types)
        }
        Rule::intersection_type => {
            // intersection_type = { "[" ~ ":and" ~ (WHITESPACE* ~ type_expr)+ ~ WHITESPACE* ~ "]" }
            let types = actual_pair
                .into_inner()
                .filter(|p| p.as_rule() == Rule::type_expr)
                .map(build_type_expr)
                .collect();
            TypeExpr::Intersection(types)
        }
        Rule::literal_type => {
            // literal_type = { "[" ~ ":val" ~ WHITESPACE* ~ literal ~ WHITESPACE* ~ "]" }
            let literal_pair = actual_pair
                .into_inner()
                .find(|p| p.as_rule() == Rule::literal)
                .expect("literal_type requires a literal");
            TypeExpr::Literal(build_literal(literal_pair))
        }

        rule => unimplemented!(
            "build_type_expr not implemented for rule: {:?}, content: {}",
            rule,
            actual_pair.as_str()
        ),
    }
}
