use super::{PestParseError, Rule};
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
use super::common::{build_keyword, build_pattern, build_symbol};
use super::expressions::build_expression;
use super::types::build_type_expr; // For type annotations

// Utility imports (if any) - e.g., for skipping whitespace/comments if not handled by Pest rules
use super::utils::unescape; // For log_step_expr

pub(super) fn build_let_expr(pairs: Pairs<Rule>) -> Result<LetExpr, PestParseError> {

    let mut iter = pairs.peekable();

    if let Some(p) = iter.peek() {
        if p.as_rule() == Rule::let_keyword {

            iter.next(); 
            while let Some(sp) = iter.peek() {
                if sp.as_rule() == Rule::WHITESPACE || sp.as_rule() == Rule::COMMENT {

                    iter.next();
                } else {
                    break;
                }
            }
        }
    }    let mut bindings = Vec::new();
    let mut body_expressions_vec = Vec::new();

    loop {
        while let Some(p) = iter.peek() {
            if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {

                iter.next(); 
            } else {
                break; 
            }
        }

        if iter.peek().is_none() {

            break;
        }
        
        let iter_before_binding_attempt = iter.clone();



        let pattern_pair_candidate = match iter.peek() {
            Some(p) => {

                p.clone()
            }
            None => {

                break; 
            }
        };

        match build_pattern(pattern_pair_candidate.clone()) {
            Ok(pattern) => {

                iter.next(); // Consume the pattern_pair_candidate

                while let Some(p) = iter.peek() {
                    if p.as_rule() == Rule::WHITESPACE || p.as_rule() == Rule::COMMENT {

                        iter.next();
                    } else {
                        break;
                    }
                }

                if iter.peek().is_none() {

                    iter = iter_before_binding_attempt; 
                    break; 
                }

                let value_pair_candidate = match iter.peek() {
                    Some(p) => {

                        p.clone()
                    }
                    None => { // Should be caught by previous check, but defensive.

                        iter = iter_before_binding_attempt; 
                        break; 
                    }
                };
                

                match build_expression(value_pair_candidate.clone()) {
                    Ok(value) => {

                        iter.next(); // Consume the value_pair_candidate

                        bindings.push(LetBinding {
                            pattern,
                            type_annotation: None, 
                            value: Box::new(value),
                        });

                    }
                    Err(e) => {

                        return Err(e); // Return error immediately for malformed binding value
                    }
                }            }
            Err(_e) => {
                break; 
            }
        }
    }


    while let Some(pair) = iter.next() {
        match pair.as_rule() {
            Rule::WHITESPACE | Rule::COMMENT => { 

            }
            _ => {

                match build_expression(pair.clone()) { // Added clone here for safety if build_expression fails and we need pair again (though not in current logic)
                    Ok(expr) => body_expressions_vec.push(expr),
                    Err(e) => {

                        return Err(e);
                    }
                }
            }
        }
    }
    

    if body_expressions_vec.is_empty() {

        return Err(PestParseError::InvalidInput(
            "let expression requires at least one body expression".to_string(),
        ));
    }


    Ok(LetExpr { bindings, body: body_expressions_vec })
}

pub(super) fn build_if_expr(mut pairs: Pairs<Rule>) -> Result<IfExpr, PestParseError> {
    let condition_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::MissingToken("if condition".to_string()))?;
    let then_branch_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::MissingToken("if then_branch".to_string()))?;

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

pub(super) fn build_fn_expr(mut pairs: Pairs<Rule>) -> Result<FnExpr, PestParseError> {
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
        .ok_or_else(|| PestParseError::InvalidInput("fn requires parameters list".to_string()))?;
    if params_pair.as_rule() != Rule::fn_param_list {
        return Err(PestParseError::InvalidInput(format!(
            "Expected fn_param_list, found {:?}",
            params_pair.as_rule()
        )));
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
            }
            let rest_symbol_pair = params_inner
                .next()
                .ok_or_else(|| PestParseError::InvalidInput("& requires a symbol".to_string()))?;
            if rest_symbol_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected symbol after &, found {:?}",
                    rest_symbol_pair.as_rule()
                )));
            }            let rest_symbol = build_symbol(rest_symbol_pair)?;

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
                        }
                    }
                    let type_pair = params_inner.next().ok_or_else(|| {
                        PestParseError::InvalidInput("Expected type_expr after ':' for variadic parameter".to_string())
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
        
        if param_def_pair.as_rule() != Rule::param_def {
            return Err(PestParseError::InvalidInput(format!(
                "Expected param_def, found {:?}",
                param_def_pair.as_rule()
            )));
        }

        // Extract binding_pattern and optional type from param_def
        let mut param_def_inner = param_def_pair.into_inner();
        
        let binding_pattern_pair = param_def_inner.next().ok_or_else(|| {
            PestParseError::InvalidInput("param_def missing binding_pattern".to_string())
        })?;
        let pattern = build_pattern(binding_pattern_pair)?;

        // Check for optional type annotation (COLON ~ type_expr)
        let mut type_annotation = None;
        if let Some(colon_pair) = param_def_inner.next() {
            if colon_pair.as_rule() == Rule::COLON {
                // Get the type_expr after the colon
                let type_pair = param_def_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Expected type_expr after ':' in param_def".to_string(),
                    )
                })?;
                type_annotation = Some(build_type_expr(type_pair)?);
            } else {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected COLON in param_def, found {:?}",
                    colon_pair.as_rule()
                )));
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
                PestParseError::InvalidInput(
                    "Expected type_expr after \':\' for return type".to_string(),
                )
            })?;
            return_type = Some(build_type_expr(return_type_pair)?);
        }
    }

    // Body expressions
    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput(
            "fn requires at least one body expression".to_string(),
        ));
    }    Ok(FnExpr {
        params,
        variadic_param,
        body,
        return_type,
    })
}

pub(super) fn build_def_expr(mut pairs: Pairs<Rule>) -> Result<DefExpr, PestParseError> {
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
        .ok_or_else(|| PestParseError::InvalidInput("def requires a symbol".to_string()))?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput(format!(
            "Expected symbol for def, found {:?}",
            symbol_pair.as_rule()
        )));
    }
    let symbol = build_symbol(symbol_pair)?;

    // Optional type annotation
    let mut type_annotation: Option<TypeExpr> = None;
    if let Some(peeked_colon) = pairs.peek() {
        if peeked_colon.as_rule() == Rule::COLON {
            pairs.next(); // Consume \':\' 
                          // Consume potential whitespace after \':\' 
            while let Some(p_ws) = pairs.peek() {
                if p_ws.as_rule() == Rule::WHITESPACE || p_ws.as_rule() == Rule::COMMENT {
                    pairs.next();
                } else {
                    break;
                }
            }            let type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput("Expected type_expr after \':\' in def".to_string())
            })?;
            type_annotation = Some(build_type_expr(type_pair)?);
        }
    }

    let value_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("def requires a value expression".to_string())
    })?;
    let value = build_expression(value_pair)?;

    Ok(DefExpr {
        symbol,
        type_annotation,
        value: Box::new(value),
    })
}

pub(super) fn build_defn_expr(mut pairs: Pairs<Rule>) -> Result<DefnExpr, PestParseError> {
    // Consume defn_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::defn_keyword {
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

    let symbol_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("defn requires a symbol (function name)".to_string())
    })?;
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput(format!(
            "Expected symbol for defn name, found {:?}",
            symbol_pair.as_rule()
        )));
    }    let name = build_symbol(symbol_pair)?;

    // Parse function components directly since defn combines defn_keyword + symbol + fn_expr
    let params_pair = pairs
        .next()
        .ok_or_else(|| PestParseError::InvalidInput("defn requires parameters list".to_string()))?;

    if params_pair.as_rule() != Rule::fn_param_list {
        return Err(PestParseError::InvalidInput(format!(
            "Expected fn_param_list for defn, found {:?}",
            params_pair.as_rule()
        )));
    }

    let mut params: Vec<ParamDef> = Vec::new();
    let mut variadic_param: Option<ParamDef> = None;
    let mut params_inner = params_pair.into_inner().peekable();

    while let Some(param_item_peek) = params_inner.peek() {
        if param_item_peek.as_rule() == Rule::WHITESPACE
            || param_item_peek.as_rule() == Rule::COMMENT
        {
            params_inner.next();
            continue;
        }        if param_item_peek.as_rule() == Rule::AMPERSAND {
            params_inner.next(); // & 
            while let Some(p) = params_inner.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    params_inner.next();
                } else {
                    break;
                }
            }            let rest_sym_pair = params_inner.next().ok_or_else(|| {
                PestParseError::InvalidInput("defn: & requires symbol".to_string())
            })?;
            if rest_sym_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected symbol after &, found {:?}",
                    rest_sym_pair.as_rule()
                )));
            }
            let rest_sym = build_symbol(rest_sym_pair)?;
            let mut rest_type: Option<TypeExpr> = None;
            if let Some(peek_colon) = params_inner.peek() {
                if peek_colon.as_rule() == Rule::COLON {
                    params_inner.next(); // consume COLON
                    while let Some(p) = params_inner.peek() {
                        if p.as_rule() == Rule::WHITESPACE {
                            params_inner.next();
                        } else {
                            break;
                        }
                    }
                    let type_pair = params_inner.next().ok_or_else(|| {
                        PestParseError::InvalidInput("Expected type_expr after ':' for variadic parameter".to_string())
                    })?;
                    rest_type = Some(build_type_expr(type_pair)?);
                }
            }
            variadic_param = Some(ParamDef {
                pattern: Pattern::Symbol(rest_sym),
                type_annotation: rest_type,
            });
            break;
        }        let param_def_pair = params_inner.next().unwrap();
        
        if param_def_pair.as_rule() != Rule::param_def {
            return Err(PestParseError::InvalidInput(format!(
                "Expected param_def, found {:?}",
                param_def_pair.as_rule()
            )));
        }

        // Extract binding_pattern and optional type from param_def
        let mut param_def_inner = param_def_pair.into_inner();
        
        let binding_pattern_pair = param_def_inner.next().ok_or_else(|| {
            PestParseError::InvalidInput("param_def missing binding_pattern".to_string())
        })?;
        let pattern = build_pattern(binding_pattern_pair)?;

        // Check for optional type annotation (COLON ~ type_expr)
        let mut type_ann = None;
        if let Some(colon_pair) = param_def_inner.next() {
            if colon_pair.as_rule() == Rule::COLON {
                // Get the type_expr after the colon
                let type_pair = param_def_inner.next().ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Expected type_expr after ':' in param_def".to_string(),
                    )
                })?;
                type_ann = Some(build_type_expr(type_pair)?);
            } else {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected COLON in param_def, found {:?}",
                    colon_pair.as_rule()
                )));
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
            pairs.next(); // :
            while let Some(p) = pairs.peek() {
                if p.as_rule() == Rule::WHITESPACE {
                    pairs.next();
                } else {
                    break;
                }
            }
            let ret_type_pair = pairs.next().ok_or_else(|| {
                PestParseError::InvalidInput("defn: expected return type after :".to_string())
            })?;
            return_type = Some(build_type_expr(ret_type_pair)?);
        }
    }

    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput(
            "defn requires at least one body expression".to_string(),
        ));
    }

    Ok(DefnExpr {
        name,
        params,
        variadic_param,
        body,
        return_type,
    })
}

pub(super) fn build_parallel_expr(mut pairs: Pairs<Rule>) -> Result<ParallelExpr, PestParseError> {
    // Consume parallel_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::parallel_keyword {
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

    // Grammar: parallel_expr = { "(" ~ parallel_keyword ~ parallel_binding+ ~ ")" }
    // where parallel_binding = { "[" ~ symbol ~ type_annotation? ~ expression ~ "]" }
    // So we directly get parallel_binding rules, not wrapped in a vector
    
    let mut bindings = Vec::new();
    
    // Process all parallel_binding pairs
    while let Some(binding_pair) = pairs.next() {
        // Skip whitespace/comments
        if binding_pair.as_rule() == Rule::WHITESPACE || binding_pair.as_rule() == Rule::COMMENT {
            continue;
        }
        
        if binding_pair.as_rule() != Rule::parallel_binding {
            return Err(PestParseError::InvalidInput(format!(
                "Expected Rule::parallel_binding for parallel bindings, found {:?}",
                binding_pair.as_rule()
            )));        }
          
        // Parse the parallel_binding: "[" ~ symbol ~ type_annotation? ~ expression ~ "]"
        let all_tokens: Vec<_> = binding_pair.into_inner()
            .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
            .collect();
        
        let mut binding_inner = all_tokens.into_iter();
        
        let symbol_pair = binding_inner.next().ok_or_else(|| {
            PestParseError::InvalidInput("parallel_binding missing symbol".to_string())
        })?;
        if symbol_pair.as_rule() != Rule::symbol {
            return Err(PestParseError::InvalidInput(format!(
                "Expected symbol in parallel_binding, found {:?}",
                symbol_pair.as_rule()
            )));
        }        let symbol = build_symbol(symbol_pair)?;        // Check for optional type annotation
        let mut type_annotation: Option<TypeExpr> = None;
        let mut expr_pair = None;
        
        if let Some(next_pair) = binding_inner.next() {           
            if next_pair.as_rule() == Rule::type_annotation { // Parse type annotation: COLON ~ type_expr
                // Find the type_expr within the type_annotation
                let type_ann_inner = next_pair.into_inner();
                let inner_tokens: Vec<_> = type_ann_inner.collect();
                
                // Find the type_expr - since type_expr is a silent rule, look for its variants                
                for token in inner_tokens {
                    match token.as_rule() {                        Rule::COLON => continue, // Skip the colon
                        Rule::primitive_type | Rule::vector_type | Rule::tuple_type | Rule::map_type | 
                        Rule::function_type | Rule::resource_type | Rule::union_type | 
                        Rule::intersection_type | Rule::literal_type | Rule::symbol => {
                            type_annotation = Some(build_type_expr(token)?);
                            break;
                        }
                        _ => {
                            continue;
                        }
                    } 
                }
                
                // The next token should be the expression
                expr_pair = binding_inner.next();
            } else {
                // No type annotation, this is the expression
                expr_pair = Some(next_pair);
            }
        }
        
        let expr_pair = expr_pair.ok_or_else(|| {
            PestParseError::InvalidInput("parallel_binding missing expression".to_string())
        })?;
        
        let expression = build_expression(expr_pair)?;
        bindings.push(ParallelBinding {
            symbol,
            type_annotation,
            expression: Box::new(expression),
        });
    }
    
    if bindings.is_empty() {
        return Err(PestParseError::InvalidInput(
            "parallel expression requires at least one binding".to_string(),
        ));
    }
    
    Ok(ParallelExpr { bindings })
}

pub(super) fn build_with_resource_expr(
    mut pairs: Pairs<Rule>,
) -> Result<WithResourceExpr, PestParseError> {
    // Consume with_resource_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::with_resource_keyword {
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
    }    // The grammar inlines the binding: "[" ~ symbol ~ type_expr ~ expression ~ "]"
    // So we get the three components directly as separate pairs
    
    // Parse symbol (first component of the binding)
    let symbol_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("with-resource requires a symbol in binding".to_string())
    })?;
    
    // Parse type_expr (second component of the binding)  
    let type_expr_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("with-resource requires a type_expr in binding".to_string())
    })?;
    
    // Parse resource initialization expression (third component of the binding)
    let resource_init_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("with-resource requires an initialization expression in binding".to_string())
    })?;    // Parse symbol (must be a symbol)
    if symbol_pair.as_rule() != Rule::symbol {
        return Err(PestParseError::InvalidInput(format!(
            "Expected symbol for with-resource binding, found {:?}",
            symbol_pair.as_rule()
        )));
    }
    let symbol = build_symbol(symbol_pair)?;
    
    // Parse type - build_type_expr can handle various rule types (symbol, keyword, etc.)
    let resource_type = build_type_expr(type_expr_pair)?;
    
    // Parse resource initialization expression
    let resource_init_expr = build_expression(resource_init_pair)?;

    let body = pairs
        .filter(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    if body.is_empty() {
        return Err(PestParseError::InvalidInput(
            "with-resource requires at least one body expression".to_string(),
        ));
    }    Ok(WithResourceExpr {
        resource_symbol: symbol,
        resource_type,
        resource_init: Box::new(resource_init_expr),
        body,
    })
}

pub(super) fn build_try_catch_expr(mut pairs: Pairs<Rule>) -> Result<TryCatchExpr, PestParseError> {
    // Consume try_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::try_keyword {
            // Assuming try_keyword exists
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
    }    // The first expression is the try_body (grammar uses try_body_expression+)
    // The AST uses try_body: Vec<Expression>
    // Since try_body_expression is a silent rule, we need to look for any rules that are expressions
    // until we hit a catch_clause or finally_clause
    let mut try_body_expressions = Vec::new();
    while let Some(p) = pairs.peek() {
        match p.as_rule() {
            Rule::catch_clause | Rule::finally_clause => {
                break; // End of try_body_expressions
            }
            Rule::WHITESPACE | Rule::COMMENT => {
                pairs.next(); // consume whitespace/comment
            }
            _ => {
                // Any other rule should be treated as an expression in the try body
                try_body_expressions.push(build_expression(pairs.next().unwrap())?);
            }
        }
    }

    if try_body_expressions.is_empty() {
        return Err(PestParseError::InvalidInput(
            "try-catch requires a try block expression".to_string(),
        ));
    }

    // The rest are catch_clauses and an optional finally_clause
    let mut catch_clauses = Vec::new();
    let mut finally_body: Option<Vec<Expression>> = None; // Added

    while let Some(clause_candidate_pair) = pairs.peek() {
        // Skip whitespace/comments
        if clause_candidate_pair.as_rule() == Rule::WHITESPACE
            || clause_candidate_pair.as_rule() == Rule::COMMENT
        {
            pairs.next(); // consume
            continue;
        }

        if clause_candidate_pair.as_rule() == Rule::catch_clause {
            let catch_clause_pair = pairs.next().unwrap(); // Safe due to peek and check
            let mut clause_inner = catch_clause_pair.into_inner();

            // Consume catch_keyword
            let _catch_keyword_pair = clause_inner
                .next()
                .filter(|p| p.as_rule() == Rule::catch_keyword)
                .ok_or_else(|| {
                    PestParseError::InvalidInput("Catch clause missing 'catch' keyword".to_string())
                })?;

            let pattern_pair = clause_inner.next().ok_or_else(|| {
                PestParseError::InvalidInput("Catch clause requires a pattern".to_string())
            })?;
            // catch_pattern = _{ type_expr | keyword | symbol }
            // The AST CatchPattern has variants for these. build_catch_pattern handles this.
            let pattern = build_catch_pattern(pattern_pair)?;

            let binding_symbol_pair = clause_inner.next().ok_or_else(|| {
                PestParseError::InvalidInput("Catch clause requires a binding symbol".to_string())
            })?;
            if binding_symbol_pair.as_rule() != Rule::symbol {
                return Err(PestParseError::InvalidInput(format!(
                    "Expected symbol for catch binding, found {:?}",
                    binding_symbol_pair.as_rule()
                )));
            }
            let binding = build_symbol(binding_symbol_pair)?;

            // Collect all body expressions for the catch clause
            let mut catch_body_expressions = Vec::new();
            while let Some(body_expr_pair) = clause_inner.next() {
                if body_expr_pair.as_rule() == Rule::WHITESPACE
                    || body_expr_pair.as_rule() == Rule::COMMENT
                {
                    continue;
                }
                catch_body_expressions.push(build_expression(body_expr_pair)?);
            }
            if catch_body_expressions.is_empty() {
                return Err(PestParseError::InvalidInput(
                    "Catch clause requires at least one body expression".to_string(),
                ));
            }            catch_clauses.push(CatchClause {
                pattern,
                binding,
                body: catch_body_expressions,
            });
        } else if clause_candidate_pair.as_rule() == Rule::finally_clause {
            if finally_body.is_some() {
                return Err(PestParseError::InvalidInput(
                    "Multiple finally clauses found".to_string(),
                ));
            }
            let finally_clause_pair = pairs.next().unwrap();
            let mut finally_inner = finally_clause_pair.into_inner();
            // Consume finally_keyword
            let _finally_keyword_pair = finally_inner
                .next()
                .filter(|p| p.as_rule() == Rule::finally_keyword)
                .ok_or_else(|| {
                    PestParseError::InvalidInput(
                        "Finally clause missing 'finally' keyword".to_string(),
                    )
                })?;

            let mut finally_expressions = Vec::new();
            while let Some(body_expr_pair) = finally_inner.next() {
                // consume remaining pairs as expressions
                if body_expr_pair.as_rule() == Rule::WHITESPACE
                    || body_expr_pair.as_rule() == Rule::COMMENT
                {
                    continue;
                }
                finally_expressions.push(build_expression(body_expr_pair)?);
            }
            if finally_expressions.is_empty() {
                return Err(PestParseError::InvalidInput(
                    "Finally clause requires at least one body expression".to_string(),
                ));
            }
            finally_body = Some(finally_expressions);
        } else {
            // Should not happen if grammar is correct and all try_body_expressions consumed
            return Err(PestParseError::InvalidInput(format!(
                "Expected catch_clause or finally_clause, found {:?} in try-catch",
                clause_candidate_pair.as_rule()
            )));
        }
    }    // As per grammar, catch_clause* means zero or more.
    // But AST TryCatchExpr implies try_body and catch_clauses are somewhat mandatory.
    // The prompt error "missing structure fields: try_body, finally_body" for TryCatchExpr
    // and "missing structure fields: binding, body" for CatchClause.
    // Let's ensure try_body is passed. finally_body is Option.

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
    match pair.as_rule() {
        Rule::type_expr => Ok(CatchPattern::Type(build_type_expr(pair)?)),
        Rule::keyword => Ok(CatchPattern::Keyword(build_keyword(pair)?)),
        Rule::symbol => Ok(CatchPattern::Symbol(build_symbol(pair)?)),
        unknown_rule => Err(PestParseError::InvalidInput(format!(
            "Invalid rule for catch_pattern: {:?}, content: '{}'",
            unknown_rule,
            pair.as_str()
        ))),
    }
}

pub(super) fn build_match_expr(mut pairs: Pairs<Rule>) -> Result<MatchExpr, PestParseError> {


    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::match_keyword {

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

    let expression_to_match_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput(
            "match expression requires an expression to match against".to_string(),
        )
    })?;

    let matched_expression = Box::new(build_expression(expression_to_match_pair.clone())?);

    let mut all_tokens: Vec<Pair<Rule>> = Vec::new();
    while let Some(pair) = pairs.next() {
        if pair.as_rule() == Rule::WHITESPACE || pair.as_rule() == Rule::COMMENT {
            continue;
        }
        
        if pair.as_rule() == Rule::match_clause_content {
            // Extract inner tokens from match_clause_content
            for inner_pair in pair.into_inner() {
                if inner_pair.as_rule() != Rule::WHITESPACE && inner_pair.as_rule() != Rule::COMMENT {
                    all_tokens.push(inner_pair);
                }
            }
        } else {
            all_tokens.push(pair);
        }
    }



    // Manually group tokens into clauses
    let mut clauses = Vec::new();
    let mut i = 0;    while i < all_tokens.len() {
        // Try to parse the current token as a pattern
        let pattern_result = super::common::build_match_pattern(all_tokens[i].clone());
        if pattern_result.is_err() {
            // If pattern parsing fails, we've reached the end of clauses
            // UNLESS this is a 'when' token which should be handled in guard detection
            if all_tokens[i].as_str() != "when" {
                break;
            }
            // If it's 'when', there might be a parsing issue with the previous pattern,
            // so we should still break to avoid infinite loops
            break;
        }
        
        let pattern = pattern_result.unwrap();

        i += 1;        // Check for optional guard (when keyword)
        let mut guard_expression: Option<Box<Expression>> = None;
        if i < all_tokens.len() && (all_tokens[i].as_str() == "when" || all_tokens[i].as_rule() == Rule::WHEN) {
            i += 1; // Skip 'when'
            if i >= all_tokens.len() {
                return Err(PestParseError::InvalidInput("Guard expression missing after 'when'".to_string()));
            }
            guard_expression = Some(Box::new(build_expression(all_tokens[i].clone())?));
            i += 1;
        }// Collect body expressions until we find the next pattern
        let mut body_expressions = Vec::new();
        
        // Always consume at least one expression as body
        if i < all_tokens.len() {
            let body_expr = build_expression(all_tokens[i].clone())?;

            body_expressions.push(body_expr);
            i += 1;
        }
        
        // Then continue consuming expressions until we find a valid pattern for the next clause
        while i < all_tokens.len() {
            // Try to parse this token as a pattern to see if it starts the next clause
            let next_pattern_result = super::common::build_match_pattern(all_tokens[i].clone());
            if next_pattern_result.is_ok() {
                // Check if there's another expression after this that could also be a pattern
                // If so, this might be a body expression, not a pattern
                let mut should_treat_as_pattern = true;
                if i + 1 < all_tokens.len() {
                    let next_next_result = super::common::build_match_pattern(all_tokens[i + 1].clone());
                    if next_next_result.is_ok() {
                        // Two consecutive patterns suggests this might be the end of current clause
                        should_treat_as_pattern = true;
                    } else {
                        // Pattern followed by non-pattern suggests this might be a pattern-body pair
                        should_treat_as_pattern = true;
                    }
                }
                
                if should_treat_as_pattern {

                    break; // This starts the next clause
                }
            }            // Otherwise, it's a body expression
            // Skip 'when' tokens as they are handled in guard detection
            if all_tokens[i].as_str() == "when" || all_tokens[i].as_rule() == Rule::WHEN {
                break; // This should not happen if guard detection worked correctly
            }
            
            let body_expr = build_expression(all_tokens[i].clone())?;

            body_expressions.push(body_expr);
            i += 1;
        }

        if body_expressions.is_empty() {
            return Err(PestParseError::InvalidInput("Match clause missing body expression".to_string()));
        }

        // Create the body (single expression or DoExpr for multiple)
        let body_expression = if body_expressions.len() == 1 {
            Box::new(body_expressions.into_iter().next().unwrap())
        } else {
            Box::new(Expression::Do(DoExpr { expressions: body_expressions }))
        };

        clauses.push(MatchClause {
            pattern,
            guard: guard_expression,
            body: body_expression,
        });

    }

    if clauses.is_empty() {

        return Err(PestParseError::InvalidInput(
            "match expression requires at least one clause".to_string(),
        ));
    }


    Ok(MatchExpr {
        expression: matched_expression,
        clauses,
    })
}

// Helper function to build MatchPattern from a Pair<Rule>
// This function is now implemented in super::common::build_match_pattern

pub(super) fn build_log_step_expr(mut pairs: Pairs<Rule>) -> Result<LogStepExpr, PestParseError> {
    // Consume log_step_keyword if present
    if let Some(p) = pairs.peek() {
        if p.as_rule() == Rule::log_step_keyword {
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

    let label_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("log-step requires a label (string)".to_string())
    })?;
    if label_pair.as_rule() != Rule::string {
        return Err(PestParseError::InvalidInput(format!(
            "Expected string for log-step label, found {:?}",
            label_pair.as_rule()
        )));
    }

    // Extract string content and unescape it properly
    let raw_str = label_pair.as_str();
    let content = &raw_str[1..raw_str.len() - 1]; // Remove quotes
    let label = unescape(content).map_err(|e| {
        PestParseError::InvalidEscapeSequence(format!(
            "Invalid string literal for log-step label: {:?}",
            e
        ))
    })?;

    let expression_pair = pairs.next().ok_or_else(|| {
        PestParseError::InvalidInput("log-step requires an expression".to_string())
    })?;
    let expression = build_expression(expression_pair)?;

    // Note: LogStepExpr structure doesn't match what's being built here
    // This is a structural issue beyond PestParseError fixes
    Ok(LogStepExpr {
        level: None,
        values: vec![expression],
        location: Some(label),
    })
}
