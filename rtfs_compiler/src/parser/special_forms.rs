use super::{PestParseError, Rule, pair_to_source_span};
use pest::iterators::{Pair, Pairs};

// AST Node Imports - Ensure all used AST nodes are listed here
use crate::ast::{
    CatchClause,
    CatchPattern,
    DefExpr,
    DefnExpr,
    DoExpr,
    Expression, // Ensure this is correctly in scope
    FnExpr,    IfExpr,
    LetBinding,
    LetExpr,
    LogStepExpr,
    MatchClause,
    MatchExpr,
    ParallelBinding,
    ParallelExpr,
    ParamDef,
    Pattern,
    TryCatchExpr,
    TypeExpr,
    WithResourceExpr,
};

// Builder function imports from sibling modules
// CORRECTED IMPORT: build_keyword_from_pair -> build_keyword
use super::common::{build_keyword, build_pattern, build_symbol, build_match_pattern};
use super::expressions::build_expression;
use super::types::build_type_expr; // For type annotations

// Utility imports (if any) - e.g., for skipping whitespace/comments if not handled by Pest rules
// use super::utils::unescape; // For log_step_expr

pub(super) fn build_let_expr(pair: Pair<Rule>) -> Result<LetExpr, PestParseError> {
    let span = pair_to_source_span(&pair);
    let mut iter = pair.into_inner().peekable();
    let mut bindings = Vec::new();
    let mut body_expressions = Vec::new();

    // Skip the let_keyword if present
    if let Some(p) = iter.peek() {
        if p.as_rule() == Rule::let_keyword {
            iter.next();
        }
    }

    // Parse let_binding tokens
    for pair in iter {
        match pair.as_rule() {            Rule::let_binding => {
                let pair_clone = pair.clone();
                let binding = build_let_binding(&pair, pair_clone.into_inner())?;
                bindings.push(binding);
            }
            Rule::WHITESPACE | Rule::COMMENT => {
                // Skip whitespace and comments
            }
            _ => {
                // This should be a body expression
                let expr = build_expression(pair)?;
                body_expressions.push(expr);
            }
        }
    }    if body_expressions.is_empty() {        return Err(PestParseError::InvalidInput {
            message: "let expression requires at least one body expression".to_string(),
            span: Some(span),
        });
    }

    Ok(LetExpr { 
        bindings, 
        body: body_expressions 
    })
}

fn build_let_binding(parent_pair: &Pair<Rule>, mut pairs: Pairs<Rule>) -> Result<LetBinding, PestParseError> {
    let parent_span = pair_to_source_span(parent_pair);
    let pattern_pair = pairs.next()
        .ok_or_else(|| PestParseError::MissingToken { token: "let_binding pattern".to_string(), span: Some(parent_span.clone()) })?;
    
    let pattern = build_pattern(pattern_pair)?;
    
    // Check if there's a type annotation
    let mut type_annotation = None;
    let mut value_pair = None;
    
    if let Some(next_pair) = pairs.next() {
        if next_pair.as_rule() == Rule::type_annotation {
            // Parse type annotation
            let type_ann_inner = next_pair.into_inner();
            for token in type_ann_inner {
                match token.as_rule() {
                    Rule::COLON => continue, // Skip the colon
                    Rule::primitive_type | Rule::vector_type | Rule::tuple_type | Rule::map_type | 
                    Rule::function_type | Rule::resource_type | Rule::union_type | 
                    Rule::intersection_type | Rule::literal_type | Rule::symbol => {
                        type_annotation = Some(build_type_expr(token)?);
                        break;
                    }
                    _ => continue,
                }
            }
            // The next token should be the expression
            value_pair = pairs.next();
        } else {
            // No type annotation, this is the expression
            value_pair = Some(next_pair);
        }
    }
      let value_pair = value_pair
        .ok_or_else(|| PestParseError::MissingToken { token: "let_binding value".to_string(), span: Some(parent_span) })?;
    
    let value = Box::new(build_expression(value_pair)?);
    
    Ok(LetBinding {
        pattern,
        type_annotation,
        value,
    })
}

pub(super) fn build_if_expr(pair: Pair<Rule>) -> Result<IfExpr, PestParseError> {
    let parent_span = pair_to_source_span(&pair);
    let mut pairs = pair.into_inner();
    let condition_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::MissingToken { token: "if condition".to_string(), span: Some(parent_span.clone()) })?;
    let then_branch_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::MissingToken { token: "if then_branch".to_string(), span: Some(parent_span.clone()) })?;

    let condition = Box::new(build_expression(condition_pair)?);
    let then_branch = Box::new(build_expression(then_branch_pair)?);
    let else_branch = pairs
        .next()
        .map(|p| build_expression(p).map(Box::new))
        .transpose()?;

    Ok(IfExpr {
        condition,
        then_branch,
        else_branch,
    })
}

pub(super) fn build_do_expr(pairs: Pairs<Rule>) -> Result<DoExpr, PestParseError> {
    let mut significant_pairs = pairs.peekable();

    while let Some(p) = significant_pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            significant_pairs.next();
        } else {
            break;
        }
    }

    if let Some(first_token) = significant_pairs.peek() {
        if first_token.as_rule() == Rule::do_keyword {
            significant_pairs.next();
        }
    }

    let expressions = significant_pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;
    Ok(DoExpr { expressions })
}

pub(super) fn build_fn_expr(pair: Pair<Rule>) -> Result<FnExpr, PestParseError> {
    let parent_span = pair_to_source_span(&pair);
    let mut pairs = pair.into_inner();
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next();
        } else {
            break;
        }
    }

    if let Some(first_token) = pairs.peek() {
        if first_token.as_rule() == Rule::fn_keyword {
            pairs.next();
            while let Some(p) = pairs.peek() {
                if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let params_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::InvalidInput { message: "fn requires parameters list".to_string(), span: Some(parent_span.clone()) })?;
    if params_pair.as_rule() != Rule::fn_param_list {        return Err(PestParseError::InvalidInput { 
            message: format!("Expected fn_param_list, found {:?}", params_pair.as_rule()), 
            span: Some(pair_to_source_span(&params_pair)) 
        });
    }

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param: Option<ParamDef> = None;
    let mut params_inner = params_pair.into_inner().peekable();    while let Some(param_item_peek) = params_inner.peek() {
        if param_item_peek.as_rule() == Rule::WHITESPACE            || param_item_peek.as_rule() == Rule::COMMENT
        {
            params_inner.next();
            continue;
        }        if param_item_peek.as_rule() == Rule::AMPERSAND {
            params_inner.next();
            while let Some(p) = params_inner.peek() {
                if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                    params_inner.next();
                } else {
                    break;
                }
            }            let rest_symbol_pair = params_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput { message: "& requires a symbol".to_string(), span: Some(parent_span.clone()) })?;
            if rest_symbol_pair.as_rule() != Rule::symbol {                return Err(PestParseError::InvalidInput { 
                    message: format!("Expected symbol after &, found {:?}", rest_symbol_pair.as_rule()), 
                    span: Some(pair_to_source_span(&rest_symbol_pair)) 
                });
            }let rest_symbol = build_symbol(rest_symbol_pair)?;

            let mut rest_type_annotation = None;
            if let Some(peeked_colon) = params_inner.peek() {
                if peeked_colon.as_rule() == Rule::COLON {
                    params_inner.next(); // consume COLON
                    // Consume potential whitespace after ':' 
                    while let Some(p_ws) = params_inner.peek() {
                        if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                            params_inner.next();
                        } else {
                            break;
                        }                    }
                    let type_pair = params_inner.next().ok_or_else(|| {
                        PestParseError::InvalidInput { message: "Expected type_expr after ':' for variadic parameter".to_string(), span: Some(parent_span.clone()) }
                    })?;
                    rest_type_annotation = Some(build_type_expr(type_pair)?);
                }
            }
            variadic_param = Some(ParamDef {
                pattern: Pattern::Symbol(rest_symbol),
                type_annotation: rest_type_annotation,
            });            break;
        }        // Regular parameter (param_def contains binding_pattern and optional type)
        let param_def_pair = params_inner.next().unwrap(); // Should be safe due to peek
        let param_def_span = pair_to_source_span(&param_def_pair);
          if param_def_pair.as_rule() != Rule::param_def {
            return Err(PestParseError::InvalidInput { 
                message: format!("Expected param_def, found {:?}", param_def_pair.as_rule()), 
                span: Some(param_def_span.clone()) 
            });
        }

        // Extract binding_pattern and optional type from param_def
        let mut param_def_inner = param_def_pair.into_inner();
        
        let binding_pattern_pair = param_def_inner.next().ok_or_else(|| {
            PestParseError::InvalidInput { message: "param_def missing binding_pattern".to_string(), span: Some(param_def_span.clone()) }
        })?;
        let pattern = build_pattern(binding_pattern_pair)?;

        // Check for optional type annotation (COLON ~ type_expr)
        let mut type_annotation = None;
        if let Some(colon_pair) = param_def_inner.next() {            if colon_pair.as_rule() == Rule::COLON {                // Get the type_expr after the colon
                let type_pair = param_def_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput { 
                        message: "Expected type_expr after ':' in param_def".to_string(), 
                        span: Some(param_def_span.clone()) 
                    }
                })?;                type_annotation = Some(build_type_expr(type_pair)?);
            } else {
                return Err(PestParseError::InvalidInput { 
                    message: format!("Expected COLON in param_def, found {:?}", colon_pair.as_rule()), 
                    span: Some(pair_to_source_span(&colon_pair)) 
                });
            }
        }
        params.push(ParamDef {
            pattern,
            type_annotation,
        });
    }

    // Optional return type
    let mut return_type: Option<TypeExpr> = None;
    if let Some(peeked_ret_colon) = pairs.peek() {
        if peeked_ret_colon.as_rule() == Rule::COLON {
            pairs.next(); // Consume \':\' 
                          // Consume potential whitespace after \':\' 
            while let Some(p_ws) = pairs.peek() {
                if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }            let return_type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput { 
                    message: "Expected type_expr after ':' for return type".to_string(), 
                    span: Some(parent_span.clone()) 
                }
            })?;
            return_type = Some(build_type_expr(return_type_pair)?);
        }
    }

    // Body expressions
    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;    if body.is_empty() {
        return Err(PestParseError::InvalidInput { 
            message: "fn requires at least one body expression".to_string(), 
            span: Some(parent_span) 
        });
    }Ok(FnExpr {
        params,
        variadic_param,
        body,
        return_type,
    })
}

pub(super) fn build_def_expr(def_expr_pair: Pair<Rule>) -> Result<DefExpr, PestParseError> {
    let def_span = pair_to_source_span(&def_expr_pair);
    let mut pairs = def_expr_pair.clone().into_inner();

    // Consume def_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::def_keyword {
            pairs.next();
            // Consume whitespace after keyword
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let symbol_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::InvalidInput { message: "def requires a symbol".to_string(), span: Some(def_span.clone()) })?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput { 
            message: format!("Expected symbol for def, found {:?}", symbol_pair.as_rule()), 
            span: Some(pair_to_source_span(&symbol_pair)) 
        });
    }
    let symbol = build_symbol(symbol_pair.clone())?;

    // Optional type annotation
    let mut type_annotation: Option<TypeExpr> = None;
    if let Some(peeked_colon) = pairs.peek() {
        if peeked_colon.as_rule() == Rule::COLON {
            let colon_pair = pairs.next().unwrap(); 
            while let Some(p_ws) = pairs.peek() {
                if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
            let type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput { 
                    message: "Expected type_expr after ':' in def".to_string(), 
                    span: Some(pair_to_source_span(&colon_pair)) 
                }
            })?;
            type_annotation = Some(build_type_expr(type_pair)?);
        }
    }

    let value_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput { message: "def requires a value expression".to_string(), span: Some(def_span) }
    })?;
    let value = build_expression(value_pair)?;

    Ok(DefExpr {
        symbol,
        type_annotation,
        value: Box::new(value),
    })
}

pub(super) fn build_defn_expr(defn_expr_pair: Pair<Rule>) -> Result<DefnExpr, PestParseError> {
    let defn_span = pair_to_source_span(&defn_expr_pair);
    let mut pairs = defn_expr_pair.clone().into_inner();

    // Consume defn_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::defn_keyword {
            pairs.next();
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }
    let symbol_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput { message: "defn requires a symbol (function name)".to_string(), span: Some(defn_span.clone()) }
    })?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput { 
            message: format!("Expected symbol for defn name, found {:?}", symbol_pair.as_rule()), 
            span: Some(pair_to_source_span(&symbol_pair)) 
        });
    }
    let name = build_symbol(symbol_pair.clone())?;

    let params_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::InvalidInput { message: "defn requires parameters list".to_string(), span: Some(defn_span.clone()) })?;
    if params_pair.as_rule() != Rule::fn_param_list {
        return Err(PestParseError::InvalidInput { 
            message: format!("Expected fn_param_list for defn, found {:?}", params_pair.as_rule()), 
            span: Some(pair_to_source_span(&params_pair)) 
        });
    }

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param: Option<ParamDef> = None;
    let mut params_inner = params_pair.clone().into_inner().peekable();

    while let Some(param_item_peek) = params_inner.peek() {
        if param_item_peek.as_rule() == Rule::WHITESPACE
            || param_item_peek.as_rule() == Rule::COMMENT
        {
            params_inner.next();
            continue;
        }
        if param_item_peek.as_rule() == Rule::AMPERSAND {
            let ampersand_pair = params_inner.next().unwrap(); 
            while let Some(p) = params_inner.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    params_inner.next();
                } else {
                    break;
                }
            }
            let rest_sym_pair = params_inner.next().ok_or_else(|| {
                PestParseError::InvalidInput { message: "defn: & requires symbol".to_string(), span: Some(pair_to_source_span(&ampersand_pair)) }
            })?;
            if rest_sym_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput { 
                    message: format!("Expected symbol after &, found {:?}", rest_sym_pair.as_rule()), 
                    span: Some(pair_to_source_span(&rest_sym_pair)) 
                });
            }
            let rest_sym = build_symbol(rest_sym_pair.clone())?;
            let mut rest_type: Option<TypeExpr> = None;
            if let Some(peek_colon) = params_inner.peek() {
                if peek_colon.as_rule() == Rule::COLON {
                    let colon_for_variadic_type_pair = params_inner.next().unwrap();
                    while let Some(p) = params_inner.peek() {
                        if p.as_rule() == Rule::WHITESPACE {
                            params_inner.next();
                        } else {
                            break;
                        }
                    }
                    let type_pair = params_inner.next().ok_or_else(|| {
                        PestParseError::InvalidInput { message: "Expected type_expr after ':' for variadic parameter".to_string(), span: Some(pair_to_source_span(&colon_for_variadic_type_pair)) }
                    })?;
                    rest_type = Some(build_type_expr(type_pair)?);
                }
            }
            variadic_param = Some(ParamDef {
                pattern: Pattern::Symbol(rest_sym),
                type_annotation: rest_type,
            });
            break; 
        }
        let param_def_pair = params_inner.next().unwrap();
        let param_def_span = pair_to_source_span(&param_def_pair);
        
        if param_def_pair.as_rule() != Rule::param_def {
            return Err(PestParseError::InvalidInput { 
                message: format!("Expected param_def, found {:?}", param_def_pair.as_rule()), 
                span: Some(param_def_span.clone()) 
            });
        }

        let mut param_def_inner = param_def_pair.clone().into_inner();
        
        let binding_pattern_pair = param_def_inner.next().ok_or_else(|| {
            PestParseError::InvalidInput { message: "param_def missing binding_pattern".to_string(), span: Some(param_def_span.clone()) }
        })?;
        let pattern = build_pattern(binding_pattern_pair)?;

        let mut type_ann = None;
        if let Some(colon_candidate_pair) = param_def_inner.next() {
            if colon_candidate_pair.as_rule() == Rule::COLON {
                let type_pair = param_def_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput { 
                        message: "Expected type_expr after ':' in param_def".to_string(), 
                        span: Some(pair_to_source_span(&colon_candidate_pair)) 
                    }
                })?;
                type_ann = Some(build_type_expr(type_pair)?);
            } else {
                return Err(PestParseError::InvalidInput { 
                    message: format!("Expected COLON in param_def, found {:?}", colon_candidate_pair.as_rule()), 
                    span: Some(pair_to_source_span(&colon_candidate_pair)) 
                });
            }
        }
        params.push(ParamDef {
            pattern,
            type_annotation: type_ann,
        });
    }

    let mut return_type: Option<TypeExpr> = None;
    if let Some(peek_ret_colon) = pairs.peek() {
        if peek_ret_colon.as_rule() == Rule::COLON {
            let colon_for_return_type_pair = pairs.next().unwrap(); 
            while let Some(p) = pairs.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    pairs.next();
                } else {
                    break;
                }
            }
            let ret_type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput { message: "defn: expected return type after :".to_string(), span: Some(pair_to_source_span(&colon_for_return_type_pair)) }
            })?;
            return_type = Some(build_type_expr(ret_type_pair)?);
        }
    }

    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput { 
            message: "defn requires at least one body expression".to_string(), 
            span: Some(defn_span) 
        });
    }

    Ok(DefnExpr {
        name,
        params,
        variadic_param,
        body,
        return_type,
    })
}

pub(super) fn build_parallel_expr(parallel_expr_pair: Pair<Rule>) -> Result<ParallelExpr, PestParseError> {
    let parallel_span = pair_to_source_span(&parallel_expr_pair);
    // let mut pairs = parallel_expr_pair.clone().into_inner(); // Not needed if we iterate over original children

    // Consume parallel_keyword if present - this logic might be redundant if handled by iteration
    // if let Some(p) = pairs.peek() { ... }

    let mut bindings = Vec::new();
    
    // Process all parallel_binding pairs from the original parallel_expr_pair's children
    for binding_pair_candidate in parallel_expr_pair.clone().into_inner() { // Iterate over original children, clone for safety
        match binding_pair_candidate.as_rule() {
            Rule::parallel_keyword | Rule::WHITESPACE | Rule::COMMENT => continue,
            Rule::parallel_binding => {
                let binding_pair = binding_pair_candidate; // It is a parallel_binding
                let binding_span = pair_to_source_span(&binding_pair);

                let all_tokens: Vec<_> = binding_pair.clone().into_inner()
                    .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
                    .collect();
                
                let mut binding_inner = all_tokens.into_iter();
                
                let symbol_pair = binding_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput { message: "parallel_binding missing symbol".to_string(), span: Some(binding_span.clone()) }
                })?;
                if symbol_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::InvalidInput { 
                        message: format!("Expected symbol in parallel_binding, found {:?}", symbol_pair.as_rule()), 
                        span: Some(pair_to_source_span(&symbol_pair)) 
                    });
                }
                let symbol = build_symbol(symbol_pair.clone())?;
                
                let mut type_annotation: Option<TypeExpr> = None;
                let mut expr_pair_opt = None; 
                
                // Peek at the next significant token to decide if it's a type_annotation or expression
                let mut temp_binding_inner_peekable = binding_inner.clone().peekable();
                
                if let Some(next_significant_token_peek) = temp_binding_inner_peekable.peek() {
                    if next_significant_token_peek.as_rule() == Rule::type_annotation {
                        let type_annotation_pair = binding_inner.next().unwrap(); // Consume the type_annotation pair
                        let type_ann_span = pair_to_source_span(&type_annotation_pair);
                        let mut type_ann_inner_iter = type_annotation_pair.into_inner();
                        
                        let mut found_type_expr = false;
                        // Iterate through the inner parts of type_annotation (e.g. COLON, the actual type_expr rule)
                        while let Some(token) = type_ann_inner_iter.next() {
                            match token.as_rule() {
                                Rule::COLON | Rule::WHITESPACE | Rule::COMMENT => continue,
                                Rule::primitive_type | Rule::vector_type | Rule::tuple_type | Rule::map_type | 
                                Rule::function_type | Rule::resource_type | Rule::union_type | 
                                Rule::intersection_type | Rule::literal_type | Rule::symbol | Rule::type_expr => { // Added Rule::type_expr
                                    type_annotation = Some(build_type_expr(token)?);
                                    found_type_expr = true;
                                    break; 
                                }
                                _ => { // Unexpected token within type_annotation
                                    return Err(PestParseError::InvalidInput { 
                                        message: format!("Unexpected token {:?} in type_annotation of parallel_binding", token.as_rule()), 
                                        span: Some(pair_to_source_span(&token)) 
                                    });
                                }
                            } 
                        }
                        if !found_type_expr {
                             return Err(PestParseError::InvalidInput { 
                                message: "Malformed or empty type_annotation in parallel_binding".to_string(), 
                                span: Some(type_ann_span) 
                            });
                        }
                        expr_pair_opt = binding_inner.next(); // Next token after type_annotation is the expression
                    } else {
                        // No type_annotation, the current token is the expression
                        expr_pair_opt = binding_inner.next(); 
                    }
                } else {
                     // This case means there was a symbol but nothing after it (neither type_annotation nor expression)
                    expr_pair_opt = binding_inner.next(); // Will be None, handled by ok_or_else below
                }
                
                let actual_expr_pair = expr_pair_opt.ok_or_else(|| { 
                    PestParseError::InvalidInput { message: "parallel_binding missing expression".to_string(), span: Some(binding_span.clone()) }
                })?;
                
                let expression = build_expression(actual_expr_pair)?;
                bindings.push(ParallelBinding {
                    symbol,
                    type_annotation,
                    expression: Box::new(expression),
                });
            }
            unknown_rule_type => { 
                return Err(PestParseError::InvalidInput { 
                    message: format!("Expected Rule::parallel_binding or ignorable token, found {:?}", unknown_rule_type), 
                    span: Some(pair_to_source_span(&binding_pair_candidate)) 
                });
            }
        }
    }
    
    if bindings.is_empty() {
        return Err(PestParseError::InvalidInput { 
            message: "parallel expression requires at least one binding".to_string(), 
            span: Some(parallel_span) 
        });
    }
    
    Ok(ParallelExpr { bindings })
}

pub(super) fn build_with_resource_expr(
    with_resource_expr_pair: Pair<Rule>,
) -> Result<WithResourceExpr, PestParseError> {
    let with_resource_span = pair_to_source_span(&with_resource_expr_pair);
    let mut iter = with_resource_expr_pair.clone().into_inner().peekable();

    // Skip keyword and whitespace
    while let Some(p) = iter.peek() {
        match p.as_rule() {
            Rule::with_resource_keyword | Rule::WHITESPACE | Rule::COMMENT => {
                iter.next();
            }
            _ => break,
        }
    }
    
    // Binding: symbol ~ type_expr ~ expression
    let symbol_pair = iter.next().ok_or_else(|| {
        PestParseError::InvalidInput { message: "with-resource requires a symbol in binding".to_string(), span: Some(with_resource_span.clone()) }
    })?;
    let symbol_span = pair_to_source_span(&symbol_pair);
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput { 
            message: format!("Expected symbol for with-resource binding, found {:?}", symbol_pair.as_rule()), 
            span: Some(symbol_span.clone()) 
        });
    }
    let resource_symbol = build_symbol(symbol_pair)?;
    
    let type_expr_pair = iter.next().ok_or_else(|| {
        PestParseError::InvalidInput { message: "with-resource requires a type_expr in binding".to_string(), span: Some(symbol_span) } 
    })?;
    let type_expr_span = pair_to_source_span(&type_expr_pair);
    let resource_type = build_type_expr(type_expr_pair)?;
    
    let resource_init_pair = iter.next().ok_or_else(|| {
        PestParseError::InvalidInput { message: "with-resource requires an initialization expression in binding".to_string(), span: Some(type_expr_span) }
    })?;
    let resource_init = Box::new(build_expression(resource_init_pair)?);

    let mut body = Vec::new();
    for p in iter {
        match p.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => continue,
            _ => body.push(build_expression(p)?),
        }
    }

    if body.is_empty() {
        return Err(PestParseError::InvalidInput { 
            message: "with-resource expression requires a body".to_string(), 
            span: Some(with_resource_span) 
        });
    }

    Ok(WithResourceExpr {
        resource_symbol,
        resource_type,
        resource_init: resource_init, // Corrected field name
        body,
    })
}

pub(super) fn build_try_catch_expr(try_catch_expr_pair: Pair<Rule>) -> Result<TryCatchExpr, PestParseError> {
    let try_catch_span = pair_to_source_span(&try_catch_expr_pair);
    let mut pairs = try_catch_expr_pair.clone().into_inner().peekable();

    // Consume try_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::try_keyword {
            pairs.next();
            while let Some(sp) = pairs.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }
        }
    }

    let mut try_body_expressions = Vec::new();
    let mut last_try_expr_span = try_catch_span.clone(); // Fallback span

    while let Some(p) = pairs.peek() {
        match p.as_rule() {
            Rule::catch_clause | Rule::finally_clause => break,
            Rule::WHITESPACE | Rule::COMMENT => { pairs.next(); }
            _ => {
                let expr_pair = pairs.next().unwrap();
                last_try_expr_span = pair_to_source_span(&expr_pair);
                try_body_expressions.push(build_expression(expr_pair)?);
            }
        }
    }

    if try_body_expressions.is_empty() {
        return Err(PestParseError::InvalidInput { 
            message: "try-catch requires a try block expression".to_string(), 
            span: Some(try_catch_span.clone()) // Use overall span if try block is empty
        });
    }

    let mut catch_clauses = Vec::new();
    let mut finally_body: Option<Vec<Expression>> = None;
    let mut last_clause_end_span = last_try_expr_span.clone();

    while let Some(clause_candidate_peek) = pairs.peek() {
        let current_candidate_span = pair_to_source_span(clause_candidate_peek);
        match clause_candidate_peek.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => { pairs.next(); continue; }
            Rule::catch_clause => {
                let catch_clause_pair = pairs.next().unwrap();
                let catch_clause_span = pair_to_source_span(&catch_clause_pair);
                last_clause_end_span = catch_clause_span.clone();
                let mut clause_inner = catch_clause_pair.into_inner().peekable();

                let _catch_keyword_pair = clause_inner.next()
                    .filter(|p| p.as_rule() == Rule::catch_keyword)
                    .ok_or_else(|| PestParseError::InvalidInput { 
                        message: "Catch clause missing 'catch' keyword".to_string(), 
                        span: Some(catch_clause_span.clone()) 
                    })?;

                let pattern_pair = clause_inner.next().ok_or_else(|| PestParseError::InvalidInput { 
                    message: "Catch clause requires a pattern".to_string(), 
                    span: Some(catch_clause_span.clone()) 
                })?;
                let pattern_span = pair_to_source_span(&pattern_pair);
                let pattern = build_catch_pattern(pattern_pair)?;

                let binding_symbol_pair = clause_inner.next().ok_or_else(|| PestParseError::InvalidInput { 
                    message: "Catch clause requires a binding symbol".to_string(), 
                    span: Some(pattern_span.clone()) // Span of previous element
                })?;
                let binding_symbol_span = pair_to_source_span(&binding_symbol_pair);
                if binding_symbol_pair.as_rule() != Rule::symbol {
                    return Err(PestParseError::InvalidInput { 
                        message: format!("Expected symbol for catch binding, found {:?}", binding_symbol_pair.as_rule()), 
                        span: Some(binding_symbol_span.clone()) 
                    });
                }
                let binding = build_symbol(binding_symbol_pair)?;

                let mut catch_body_expressions = Vec::new();
                let mut last_catch_expr_span = binding_symbol_span.clone();
                while let Some(body_expr_candidate) = clause_inner.next() {
                    match body_expr_candidate.as_rule() {
                        Rule::WHITESPACE | Rule::COMMENT => continue,
                        _ => {
                            last_catch_expr_span = pair_to_source_span(&body_expr_candidate);
                            catch_body_expressions.push(build_expression(body_expr_candidate)?);
                        }
                    }
                }
                if catch_body_expressions.is_empty() {
                    return Err(PestParseError::InvalidInput { 
                        message: "Catch clause requires at least one body expression".to_string(), 
                        span: Some(last_catch_expr_span) // Span of the binding if body is empty
                    });
                }
                catch_clauses.push(CatchClause {
                    pattern,
                    binding,
                    body: catch_body_expressions,
                });
            }
            Rule::finally_clause => {
                if finally_body.is_some() {
                    return Err(PestParseError::InvalidInput { 
                        message: "Multiple finally clauses found".to_string(), 
                        span: Some(current_candidate_span) 
                    });
                }
                let finally_clause_pair = pairs.next().unwrap();
                let finally_clause_span = pair_to_source_span(&finally_clause_pair);
                last_clause_end_span = finally_clause_span.clone();
                let mut finally_inner = finally_clause_pair.into_inner().peekable();

                let _finally_keyword_pair = finally_inner.next()
                    .filter(|p| p.as_rule() == Rule::finally_keyword)
                    .ok_or_else(|| PestParseError::InvalidInput { 
                        message: "Finally clause missing 'finally' keyword".to_string(), 
                        span: Some(finally_clause_span.clone()) 
                    })?;

                let mut finally_expressions = Vec::new();
                let mut last_finally_expr_span = finally_clause_span.clone(); // Fallback
                while let Some(body_expr_candidate) = finally_inner.next() {
                     match body_expr_candidate.as_rule() {
                        Rule::WHITESPACE | Rule::COMMENT => continue,
                        _ => {
                            last_finally_expr_span = pair_to_source_span(&body_expr_candidate);
                            finally_expressions.push(build_expression(body_expr_candidate)?);
                        }
                    }
                }
                if finally_expressions.is_empty() {
                    return Err(PestParseError::InvalidInput { 
                        message: "Finally clause requires at least one body expression".to_string(), 
                        span: Some(last_finally_expr_span) // Span of keyword if body empty
                    });
                }
                finally_body = Some(finally_expressions);
            }
            _ => {
                return Err(PestParseError::InvalidInput { 
                    message: format!("Expected catch_clause or finally_clause, found {:?} in try-catch", clause_candidate_peek.as_rule()), 
                    span: Some(current_candidate_span) 
                });
            }
        }
    }

    if catch_clauses.is_empty() && finally_body.is_none() {
        return Err(PestParseError::InvalidInput {
            message: "try expression must have at least one catch clause or a finally clause".to_string(),
            span: Some(last_clause_end_span), // Span of the last thing in the try block
        });
    }

    Ok(TryCatchExpr {
        try_body: try_body_expressions,
        catch_clauses,
        finally_body,
    })
}

// build_catch_pattern needs to align with AST CatchPattern and Pest catch_pattern rule
// catch_pattern  = _{ type_expr | keyword | symbol }
// AST: enum CatchPattern { Keyword(Keyword), Type(TypeExpr), Symbol(Symbol) }
fn build_catch_pattern(pair: Pair<Rule>) -> Result<CatchPattern, PestParseError> {
    let span = pair_to_source_span(&pair);
    match pair.as_rule() {
        Rule::type_expr => Ok(CatchPattern::Type(build_type_expr(pair.clone())?)),
        Rule::keyword => Ok(CatchPattern::Keyword(build_keyword(pair.clone())?)),
        Rule::symbol => Ok(CatchPattern::Symbol(build_symbol(pair.clone())?)),
        unknown_rule => Err(PestParseError::InvalidInput { 
            message: format!("Invalid rule for catch_pattern: {:?}, content: '{}'", unknown_rule, pair.as_str()), 
            span: Some(span) 
        }),
    }
}

pub(super) fn build_match_expr(match_expr_pair: Pair<Rule>) -> Result<MatchExpr, PestParseError> {
    let match_span = pair_to_source_span(&match_expr_pair);
    let mut pairs = match_expr_pair.clone().into_inner().peekable();

    while let Some(p) = pairs.peek() {
        match p.as_rule() {
            Rule::match_keyword | Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next();
            }
            _ => break,
        }
    }

    let expression_to_match_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput { 
            message: "match expression requires an expression to match against".to_string(), 
            span: Some(match_span.clone()) 
        }
    })?;
    let expression_to_match_span = pair_to_source_span(&expression_to_match_pair);
    let matched_expression = Box::new(build_expression(expression_to_match_pair)?);

    let mut clauses = Vec::new();    while let Some(clause_candidate_pair) = pairs.next() {
        match clause_candidate_pair.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => continue,
            // Corrected to use Rule::match_clause_content, which is the actual rule in the grammar
            Rule::match_clause_content => {
                let clause_pair = clause_candidate_pair;
                let clause_span = pair_to_source_span(&clause_pair);
                let mut clause_inner = clause_pair.into_inner().peekable();

                while let Some(p) = clause_inner.peek() {
                    if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                        clause_inner.next();
                    } else {
                        break;
                    }
                }

                let pattern_pair = clause_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput { 
                        message: "match clause requires a pattern".to_string(), 
                        span: Some(clause_span.clone()) 
                    }
                })?;
                let ast_pattern = build_match_pattern(pattern_pair)?;                // Skip whitespace and comments
                while let Some(p) = clause_inner.peek() {
                    if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                        clause_inner.next();
                    } else {
                        break;
                    }
                }

                // Check for optional WHEN guard
                let mut guard_expr = None;
                if let Some(p) = clause_inner.peek() {
                    if p.as_rule() == Rule::WHEN {
                        clause_inner.next(); // consume WHEN token
                        
                        // Skip whitespace and comments after WHEN
                        while let Some(p) = clause_inner.peek() {
                            if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                                clause_inner.next();
                            } else {
                                break;
                            }
                        }
                        
                        // Parse the guard expression
                        let guard_pair = clause_inner.next().ok_or_else(|| {
                            PestParseError::InvalidInput { 
                                message: "when clause requires a guard expression".to_string(), 
                                span: Some(clause_span.clone()) 
                            }
                        })?;
                        guard_expr = Some(Box::new(build_expression(guard_pair)?));
                        
                        // Skip whitespace and comments after guard
                        while let Some(p) = clause_inner.peek() {
                            if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
                                clause_inner.next();
                            } else {
                                break;
                            }
                        }
                    }
                }                let expression_pair = clause_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput { 
                        message: "match clause requires a body expression".to_string(), 
                        span: Some(clause_span.clone()) 
                    }
                })?;
                let body_expr = Box::new(build_expression(expression_pair)?);

                clauses.push(MatchClause { pattern: ast_pattern, guard: guard_expr, body: body_expr });
            }
            unknown_rule => {                return Err(PestParseError::InvalidInput { 
                    message: format!("Unexpected rule {:?} in match expression, expected Rule::match_clause_content", unknown_rule),
                    span: Some(pair_to_source_span(&clause_candidate_pair)) 
                });
            }
        }
    }

    if clauses.is_empty() {
        return Err(PestParseError::InvalidInput { 
            message: "match expression requires at least one clause".to_string(), 
            span: Some(expression_to_match_span) 
        });
    }

    Ok(MatchExpr {
        expression: matched_expression,
        clauses,
    })
}

// Helper function to build MatchPattern from a Pair<Rule>
// This function is now implemented in super::common::build_match_pattern

pub(super) fn build_log_step_expr(log_step_expr_pair: Pair<Rule>) -> Result<LogStepExpr, PestParseError> {
    let mut pairs = log_step_expr_pair.clone().into_inner().peekable();

    // Consume log_step_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::log_step_keyword {
            pairs.next();
        }
    }

    // Consume whitespace
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next();
        } else {
            break;
        }
    }

    let mut level = None;
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::keyword {
            level = Some(build_keyword(pairs.next().unwrap())?);
        }
    }

    // Consume whitespace
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next();
        } else {
            break;
        }
    }

    // The original implementation was too restrictive. This new implementation
    // allows for a more flexible structure, which seems to be what the failing
    // tests are using. The evaluator seems to handle this structure correctly.
    let values = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(LogStepExpr {
        level,
        values,
        location: None, // Location is not parsed from the arguments anymore.
    })
}

/// Build a discover-agents expression from parsed pairs
/// Syntax: (discover-agents criteria-map options-map?)
pub(super) fn build_discover_agents_expr(discover_agents_expr_pair: Pair<Rule>) -> Result<crate::ast::DiscoverAgentsExpr, PestParseError> {
    let discover_agents_span = pair_to_source_span(&discover_agents_expr_pair);
    let mut pairs = discover_agents_expr_pair.clone().into_inner().peekable();

    while let Some(p) = pairs.peek() {
        match p.as_rule() {
            Rule::discover_agents_keyword | Rule::WHITESPACE | Rule::COMMENT => { pairs.next(); }
            _ => break,
        }
    }
    
    let criteria_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput { 
        message: "discover-agents requires criteria expression".to_string(), 
        span: Some(discover_agents_span.clone()) 
    })?;
    let criteria_span = pair_to_source_span(&criteria_pair);
    let criteria = Box::new(build_expression(criteria_pair)?);
    
    while let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {
            pairs.next();
        } else {
            break;
        }
    }
    
    let options = if let Some(options_pair_peeked) = pairs.peek() {
        // Ensure it's not just leftover from a previous rule or something unexpected
        // A more robust check might involve checking if options_pair_peeked.as_rule() is an expression type
        let options_pair = pairs.next().unwrap(); // consume the peeked pair
        Some(Box::new(build_expression(options_pair)?))
    } else {
        None
    };
    
    Ok(crate::ast::DiscoverAgentsExpr {
        criteria,
        options,
    })
}

pub(super) fn build_letrec_expr(pair: Pair<Rule>) -> Result<LetExpr, PestParseError> {
    // Letrec uses the same structure as let, but with recursive binding semantics
    // The difference is handled during evaluation, not parsing
    let span = pair_to_source_span(&pair);
    let mut iter = pair.into_inner().peekable();
    let mut bindings = Vec::new();
    let mut body_expressions = Vec::new();

    // Skip the letrec_keyword if present
    if let Some(p) = iter.peek() {
        if p.as_rule() == Rule::letrec_keyword {
            iter.next();
        }
    }

    // Parse let_binding tokens (same structure as let)
    for pair in &mut iter {
        match pair.as_rule() {            Rule::let_binding => {
                let pair_clone = pair.clone();
                let binding = build_let_binding(&pair, pair_clone.into_inner())?;
                bindings.push(binding);
            }
            _ => {
                // Must be a body expression
                let expr = build_expression(pair)?;
                body_expressions.push(expr);
            }
        }
    }

    if body_expressions.is_empty() {
        return Err(PestParseError::MissingToken { 
            token: "letrec body expression".to_string(),
            span: Some(span)
        });
    }

    Ok(LetExpr { 
        bindings, 
        body: body_expressions 
    })
}
