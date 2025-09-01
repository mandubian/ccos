use super::common::{build_literal, build_map_key, build_symbol};
use super::errors::{pair_to_source_span, PestParseError};
use super::special_forms::{
    build_def_expr, build_defn_expr, build_defstruct_expr, build_discover_agents_expr, build_do_expr, build_fn_expr,
    build_if_expr, build_let_expr, build_log_step_expr, build_match_expr, build_parallel_expr,
    build_try_catch_expr, build_with_resource_expr,
};
use super::utils::unescape;
use super::Rule;
use crate::ast::{Expression, ForExpr, MapKey, Symbol}; // Symbol now used for task_context_access desugaring
use pest::iterators::Pair;
use std::collections::HashMap;

pub(super) fn build_expression(mut pair: Pair<Rule>) -> Result<Expression, PestParseError> {
    // Drill down through silent rules like \\'expression\\' or \\'special_form\\'
    let original_pair_for_span = pair.clone(); // Clone for potential error reporting at the original level
    loop {
        let rule = pair.as_rule();
        if rule == Rule::expression || rule == Rule::special_form {
            let mut inner = pair.into_inner();
            if let Some(next) = inner.next() {
                pair = next;
            } else {
                return Err(PestParseError::InvalidInput {
                    message: "Expected inner rule for expression/special_form".to_string(),
                    span: Some(pair_to_source_span(&original_pair_for_span)),
                });
            }
        } else {
            break;
        }
    }
    let current_pair_for_span = pair.clone(); // Clone for error reporting at the current, drilled-down level
    match pair.as_rule() {
        Rule::literal => Ok(Expression::Literal(build_literal(pair)?)),
        Rule::symbol => Ok(Expression::Symbol(build_symbol(pair)?)),
        Rule::method_call_expr => {
            // (.method target arg1 arg2 ...) -> (method target arg1 arg2 ...)
            let mut inner = pair.into_inner();
            // First inner pair is the identifier captured by grammar
            let method_ident_pair = inner.next().ok_or_else(|| PestParseError::InvalidInput {
                message: "Method call missing identifier".to_string(),
                span: Some(pair_to_source_span(&current_pair_for_span))
            })?;
            let method_name = method_ident_pair.as_str().to_string();
            let mut elements: Vec<Expression> = Vec::new();
            elements.push(Expression::Symbol(Symbol(method_name)));
            for arg_pair in inner {
                elements.push(build_expression(arg_pair)?);
            }
            Ok(Expression::List(elements))
        }
        Rule::shorthand_fn => build_shorthand_fn(pair),
        Rule::resource_ref => build_resource_ref(pair),
        // Resource context access creates a ResourceRef
        Rule::task_context_access => {
            let raw = pair.as_str(); // e.g. "@plan-id" or "@:context-key"
            let without_at = &raw[1..];
            // If the access is prefixed with ':' (e.g. @:context-key) treat it as a Symbol
            // Otherwise treat it as a ResourceRef (explicit resource path like @plan-id)
            if let Some(rest) = without_at.strip_prefix(':') {
                Ok(Expression::Symbol(Symbol(rest.to_string())))
            } else {
                Ok(Expression::ResourceRef(without_at.to_string()))
            }
        }
        Rule::atom_deref => {
            // @atom-name desugars to (deref atom-name)
            let raw = pair.as_str(); // e.g. "@atom-name"
            let atom_name = &raw[1..]; // Remove the @
            let atom_symbol = Expression::Symbol(Symbol(atom_name.to_string()));
            Ok(Expression::Deref(Box::new(atom_symbol)))
        }

        Rule::vector => Ok(Expression::Vector(
            pair.into_inner()
                .map(build_expression)
                .collect::<Result<Vec<_>, _>>()?,
        )),
        Rule::anon_fn => {
            // Anonymous function shorthand: #( ... )
            let body = pair
                .into_inner()
                .map(build_expression)
                .collect::<Result<Vec<_>, _>>()?;
            return Ok(Expression::Fn(crate::ast::FnExpr {
                params: vec![],
                variadic_param: None,
                return_type: None,
                body,
                delegation_hint: None,
            }));
        }
        Rule::map => Ok(Expression::Map(build_map(pair)?)),
        Rule::let_expr => Ok(Expression::Let(build_let_expr(pair)?)),
        Rule::if_expr => Ok(Expression::If(build_if_expr(pair)?)),
        Rule::do_expr => Ok(Expression::Do(build_do_expr(pair.into_inner())?)),
        Rule::fn_expr => Ok(Expression::Fn(build_fn_expr(pair)?)),
        Rule::def_expr => Ok(Expression::Def(Box::new(build_def_expr(pair)?))),
        Rule::defn_expr => Ok(Expression::Defn(Box::new(build_defn_expr(pair)?))),
        Rule::defstruct_expr => Ok(Expression::Defstruct(Box::new(build_defstruct_expr(pair)?))),
        Rule::parallel_expr => Ok(Expression::Parallel(build_parallel_expr(pair)?)),
        Rule::with_resource_expr => Ok(Expression::WithResource(build_with_resource_expr(pair)?)),
        Rule::try_catch_expr => Ok(Expression::TryCatch(build_try_catch_expr(pair)?)),
        Rule::match_expr => Ok(Expression::Match(build_match_expr(pair)?)),
        Rule::log_step_expr => Ok(Expression::LogStep(Box::new(build_log_step_expr(pair)?))),
        Rule::discover_agents_expr => Ok(Expression::DiscoverAgents(build_discover_agents_expr(pair)?)),
        Rule::for_expr => Ok(Expression::For(Box::new(build_for_expr(pair)?))),
        // Plan is not a core special form; handled as FunctionCall/Map at CCOS layer
        Rule::list => {
            let _list_pair_span = pair_to_source_span(&pair);
            let mut inner_pairs = pair.into_inner().peekable();

            if inner_pairs.peek().is_none() {
                // Empty list: ()
                Ok(Expression::List(vec![]))
            } else {
                // Non-empty list, potentially a function call or a data list
                let first_element_pair = inner_pairs.next().unwrap(); // We know it's not empty

                // Attempt to parse the first element.
                // We need to clone `first_element_pair` if we might need to re-parse all elements later for a data list.
                let callee_ast = build_expression(first_element_pair.clone())?;

                // Heuristic: if the first element is a Symbol, or an Fn expression,
                // or another FunctionCall, treat it as a function call.
                match callee_ast {
                    Expression::Symbol(_) | Expression::Fn(_) | Expression::FunctionCall { .. } => {
                        // It's likely a function call. Parse remaining as arguments.
                        let arguments = inner_pairs
                            .map(build_expression) // build_expression for each subsequent pair
                            .collect::<Result<Vec<_>, _>>()?;
                        Ok(Expression::FunctionCall {
                            callee: Box::new(callee_ast),
                            arguments,
                        })
                    }
                    // If the first element is not a symbol/fn/call, it's a data list.
                    _ => {
                        // Reconstruct the full list of expressions, including the first element.
                        // We already parsed `callee_ast` (the first element).
                        let mut elements = vec![callee_ast];
                        // Parse the rest of the elements.
                        for p in inner_pairs {
                            elements.push(build_expression(p)?);
                        }
                        Ok(Expression::List(elements))
                    }
                }
            }
        }
        Rule::WHEN => Err(PestParseError::InvalidInput {
            message: "'when' keyword found in unexpected context - should only appear in match expressions".to_string(),
            span: Some(pair_to_source_span(&current_pair_for_span))
        }),
        rule => Err(PestParseError::UnsupportedRule {
            rule: format!(
                "build_expression not implemented for rule: {:?} - {}",
                rule,
                current_pair_for_span.as_str()
            ),
            span: Some(pair_to_source_span(&current_pair_for_span))
        }),
    }
}

fn build_resource_ref(pair: Pair<Rule>) -> Result<Expression, PestParseError> {
    let pair_span = pair_to_source_span(&pair);
    let mut inner = pair.into_inner();
    let _keyword_pair = inner.next(); // Skip resource_ref_keyword
    let string_pair = inner.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "Expected a string literal inside resource:ref".to_string(),
        span: Some(pair_span),
    })?; // The string literal includes the quotes, so we need to strip them and unescape.
    let raw_str = string_pair.as_str();
    let content = &raw_str[1..raw_str.len() - 1];
    let unescaped_content = unescape(content).map_err(|e| PestParseError::InvalidLiteral {
        message: format!(
            "Invalid escape sequence in resource reference string: {:?}",
            e
        ),
        span: Some(pair_to_source_span(&string_pair)),
    })?;

    Ok(Expression::ResourceRef(unescaped_content))
}



pub(super) fn build_map(pair: Pair<Rule>) -> Result<HashMap<MapKey, Expression>, PestParseError> {
    if pair.as_rule() != Rule::map {
        return Err(PestParseError::InvalidInput {
            message: format!(
                "Expected Rule::map, found {:?} for build_map",
                pair.as_rule()
            ),
            span: Some(pair_to_source_span(&pair)),
        });
    }
    // let map_span = pair_to_source_span(&pair); // This was unused
    let mut map_data = HashMap::new();
    let mut map_content = pair.into_inner();

    while let Some(entry_pair) = map_content.next() {
        if entry_pair.as_rule() == Rule::WHITESPACE || entry_pair.as_rule() == Rule::COMMENT {
            continue;
        }
        let entry_span = pair_to_source_span(&entry_pair);
        if entry_pair.as_rule() != Rule::map_entry {
            return Err(PestParseError::InvalidInput {
                message: format!(
                    "Expected map_entry inside map, found {:?}",
                    entry_pair.as_rule()
                ),
                span: Some(entry_span),
            });
        }
        let mut entry_inner = entry_pair.into_inner();
        let key_pair = entry_inner
            .next()
            .ok_or_else(|| PestParseError::InvalidInput {
                message: "Map entry missing key".to_string(),
                span: Some(entry_span.clone()),
            })?;
        let value_pair = entry_inner
            .find(|p| p.as_rule() != Rule::WHITESPACE && p.as_rule() != Rule::COMMENT)
            .ok_or_else(|| PestParseError::InvalidInput {
                message: "Map entry missing value".to_string(),
                span: Some(entry_span),
            })?;
        let key = build_map_key(key_pair)?;
        let value = build_expression(value_pair)?;
        map_data.insert(key, value);
    }
    Ok(map_data)
}

fn build_shorthand_fn(pair: Pair<Rule>) -> Result<Expression, PestParseError> {
    // Collect body expressions first
    let span = pair_to_source_span(&pair);
    let mut body_exprs = Vec::new();
    let mut max_index: usize = 0; // Track highest %n encountered
    let mut uses_plain_percent = false;

    for inner in pair.into_inner() {
        if inner.as_rule() == Rule::WHITESPACE || inner.as_rule() == Rule::COMMENT { continue; }
        let expr = build_expression(inner.clone())?;
        // Scan expression tree for % symbols
        scan_for_placeholders(&expr, &mut max_index, &mut uses_plain_percent);
        body_exprs.push(expr);
    }

    // Determine parameter list
    let param_count = if max_index > 0 { max_index } else if uses_plain_percent { 1 } else { 0 };
    let mut params = Vec::new();
    for i in 1..=param_count {
        let name = if i == 1 && uses_plain_percent { "%".to_string() } else { format!("%{}", i) };
        params.push(crate::ast::ParamDef { pattern: crate::ast::Pattern::Symbol(Symbol(name)), type_annotation: None });
    }

    // Rewrite placeholders in body to generated param symbols
    let rewritten_body: Vec<Expression> = body_exprs.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect();

    Ok(Expression::Fn(crate::ast::FnExpr {
        params,
        variadic_param: None,
        return_type: None,
        body: rewritten_body,
        delegation_hint: None,
    }))
}

fn scan_for_placeholders(expr: &Expression, max_index: &mut usize, uses_plain_percent: &mut bool) {
    match expr {
        Expression::Symbol(Symbol(s)) => {
            if s == "%" { *uses_plain_percent = true; }
            else if s.starts_with('%') {
                if let Ok(n) = s[1..].parse::<usize>() { if n > *max_index { *max_index = n; } }
            }
        }
        Expression::List(items) | Expression::Vector(items) => {
            for e in items { scan_for_placeholders(e, max_index, uses_plain_percent); }
        }
        Expression::Map(m) => {
            for v in m.values() { scan_for_placeholders(v, max_index, uses_plain_percent); }
        }
        Expression::FunctionCall { callee, arguments } => {
            scan_for_placeholders(callee, max_index, uses_plain_percent);
            for a in arguments { scan_for_placeholders(a, max_index, uses_plain_percent); }
        }
        Expression::If(if_expr) => {
            scan_for_placeholders(&if_expr.condition, max_index, uses_plain_percent);
            scan_for_placeholders(&if_expr.then_branch, max_index, uses_plain_percent);
            if let Some(e) = &if_expr.else_branch { scan_for_placeholders(e, max_index, uses_plain_percent); }
        }
        Expression::Let(let_expr) => {
            for b in &let_expr.bindings { scan_for_placeholders(&b.value, max_index, uses_plain_percent); }
            for b in &let_expr.body { scan_for_placeholders(b, max_index, uses_plain_percent); }
        }
        Expression::Do(do_expr) => {
            for e in &do_expr.expressions { scan_for_placeholders(e, max_index, uses_plain_percent); }
        }
        Expression::Fn(fn_expr) => {
            for e in &fn_expr.body { scan_for_placeholders(e, max_index, uses_plain_percent); }
        }
        Expression::Def(def_expr) => {
            scan_for_placeholders(&def_expr.value, max_index, uses_plain_percent);
        }
        Expression::Defn(defn_expr) => {
            for e in &defn_expr.body { scan_for_placeholders(e, max_index, uses_plain_percent); }
        }
        Expression::TryCatch(tc) => {
            for e in &tc.try_body { scan_for_placeholders(e, max_index, uses_plain_percent); }
            for c in &tc.catch_clauses { for e in &c.body { scan_for_placeholders(e, max_index, uses_plain_percent); } }
        }
        Expression::Parallel(p) => {
            for b in &p.bindings { scan_for_placeholders(&b.expression, max_index, uses_plain_percent); }
        }
        Expression::WithResource(w) => {
            scan_for_placeholders(&w.resource_init, max_index, uses_plain_percent);
            for e in &w.body { scan_for_placeholders(e, max_index, uses_plain_percent); }
        }
        Expression::Match(mexpr) => {
            scan_for_placeholders(&mexpr.expression, max_index, uses_plain_percent);
            for c in &mexpr.clauses { if let Some(g) = &c.guard { scan_for_placeholders(g, max_index, uses_plain_percent); } scan_for_placeholders(&c.body, max_index, uses_plain_percent); }
        }
        Expression::LogStep(log) => {
            for v in &log.values { scan_for_placeholders(v, max_index, uses_plain_percent); }
        }
        _ => {}
    }
}

fn rewrite_placeholders(expr: Expression, uses_plain_percent: bool) -> Expression {
    match expr {
        Expression::Symbol(Symbol(s)) => Expression::Symbol(Symbol(s)), // No renaming needed now
        Expression::List(items) => Expression::List(items.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect()),
        Expression::Vector(items) => Expression::Vector(items.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect()),
        Expression::Map(m) => {
            let out = m.into_iter().map(|(k,v)| (k, rewrite_placeholders(v, uses_plain_percent))).collect();
            Expression::Map(out)
        }
        Expression::FunctionCall { callee, arguments } => Expression::FunctionCall {
            callee: Box::new(rewrite_placeholders(*callee, uses_plain_percent)),
            arguments: arguments.into_iter().map(|a| rewrite_placeholders(a, uses_plain_percent)).collect(),
        },
        Expression::If(mut ife) => {
            ife.condition = Box::new(rewrite_placeholders(*ife.condition, uses_plain_percent));
            ife.then_branch = Box::new(rewrite_placeholders(*ife.then_branch, uses_plain_percent));
            if let Some(e)=ife.else_branch { ife.else_branch = Some(Box::new(rewrite_placeholders(*e, uses_plain_percent))); }
            Expression::If(ife)
        }
        Expression::Let(mut le) => {
            for b in &mut le.bindings { let new_v = rewrite_placeholders(*b.value.clone(), uses_plain_percent); b.value = Box::new(new_v); }
            le.body = le.body.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect();
            Expression::Let(le)
        }
        Expression::Do(mut de) => { de.expressions = de.expressions.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); Expression::Do(de) }
        Expression::Fn(mut fe) => { fe.body = fe.body.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); Expression::Fn(fe) }
        Expression::Def(mut de) => { de.value = Box::new(rewrite_placeholders(*de.value, uses_plain_percent)); Expression::Def(de) }
        Expression::Defn(mut dn) => { dn.body = dn.body.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); Expression::Defn(dn) }
        Expression::TryCatch(mut tc) => { tc.try_body = tc.try_body.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); for c in &mut tc.catch_clauses { c.body = c.body.iter().cloned().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); } Expression::TryCatch(tc) }
        Expression::Parallel(mut p) => { for b in &mut p.bindings { let new_e = rewrite_placeholders(*b.expression.clone(), uses_plain_percent); b.expression = Box::new(new_e); } Expression::Parallel(p) }
        Expression::WithResource(mut w) => { w.resource_init = Box::new(rewrite_placeholders(*w.resource_init, uses_plain_percent)); w.body = w.body.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); Expression::WithResource(w) }
        Expression::Match(mut me) => { me.expression = Box::new(rewrite_placeholders(*me.expression, uses_plain_percent)); for c in &mut me.clauses { c.body = Box::new(rewrite_placeholders(*c.body.clone(), uses_plain_percent)); if let Some(g)=c.guard.clone() { c.guard = Some(Box::new(rewrite_placeholders(*g, uses_plain_percent))); } } Expression::Match(me) }
        Expression::LogStep(mut lg) => { lg.values = lg.values.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); Expression::LogStep(lg) }
        Expression::For(mut fe) => { fe.bindings = fe.bindings.into_iter().map(|e| rewrite_placeholders(e, uses_plain_percent)).collect(); fe.body = Box::new(rewrite_placeholders(*fe.body, uses_plain_percent)); Expression::For(fe) }
        Expression::Deref(expr) => Expression::Deref(Box::new(rewrite_placeholders(*expr, uses_plain_percent))),
        other => other,
    }
}

pub(super) fn build_for_expr(pair: Pair<Rule>) -> Result<ForExpr, PestParseError> {
    let parent_span = pair_to_source_span(&pair);
    let mut pairs = pair.into_inner();

    // Skip the 'for' keyword
    let _for_keyword = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "for expression missing keyword".to_string(),
        span: Some(parent_span.clone())
    })?;

    // Parse the bindings vector
    let bindings_vec_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "for expression missing bindings vector".to_string(),
        span: Some(parent_span.clone())
    })?;

    if bindings_vec_pair.as_rule() != Rule::vector {
        return Err(PestParseError::InvalidInput {
            message: format!("Expected vector for for bindings, found {:?}", bindings_vec_pair.as_rule()),
            span: Some(pair_to_source_span(&bindings_vec_pair))
        });
    }

    let bindings = bindings_vec_pair.into_inner()
        .map(build_expression)
        .collect::<Result<Vec<_>, _>>()?;

    // Parse the body expression
    let body_pair = pairs.next().ok_or_else(|| PestParseError::InvalidInput {
        message: "for expression missing body".to_string(),
        span: Some(parent_span)
    })?;

    let body = Box::new(build_expression(body_pair)?);

    Ok(ForExpr {
        bindings,
        body,
    })
}
