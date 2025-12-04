use crate::ast::{Expression, Literal, Symbol};
use crate::compiler::macro_def::MacroDef;
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct MacroExpander {
    macros: HashMap<Symbol, MacroDef>,
}

impl Default for MacroExpander {
    fn default() -> Self {
        MacroExpander::new()
    }
}

/// Expand a sequence of top-level items using a single persistent MacroExpander.
/// This ensures `defmacro` declarations are registered and available for later
/// top-level items in the same program.
pub fn expand_top_levels(
    items: &[crate::ast::TopLevel],
) -> Result<(Vec<crate::ast::TopLevel>, MacroExpander), String> {
    let mut expander = MacroExpander::default();
    let mut out: Vec<crate::ast::TopLevel> = Vec::new();

    for item in items {
        match item {
            crate::ast::TopLevel::Expression(expr) => {
                let expanded = expander.expand_top_level(expr)?;
                out.push(crate::ast::TopLevel::Expression(expanded));
            }
            other => {
                out.push(other.clone());
            }
        }
    }

    Ok((out, expander))
}

impl MacroExpander {
    pub fn new() -> Self {
        MacroExpander {
            macros: HashMap::new(),
        }
    }

    /// Convenience: implement Default to make global replacement of constructors
    /// simpler and to standardize instantiation sites. Default just delegates to
    /// `MacroExpander::new()` so behavior is unchanged.
    pub fn default() -> Self {
        MacroExpander::new()
    }

    pub fn expand_top_level(&mut self, expression: &Expression) -> Result<Expression, String> {
        self.expand(expression, 0)
    }

    pub fn expand(
        &mut self,
        expression: &Expression,
        quasiquote_level: u32,
    ) -> Result<Expression, String> {
        match expression {
            Expression::Quasiquote(expr) => {
                let expanded_expr = self.expand(expr, quasiquote_level + 1)?;
                Ok(Expression::Quasiquote(Box::new(expanded_expr)))
            }
            Expression::Unquote(expr) => {
                let new_level = if quasiquote_level == 0 {
                    0
                } else {
                    quasiquote_level - 1
                };
                let expanded_expr = self.expand(expr, new_level)?;
                Ok(Expression::Unquote(Box::new(expanded_expr)))
            }
            Expression::UnquoteSplicing(expr) => {
                let new_level = if quasiquote_level == 0 {
                    0
                } else {
                    quasiquote_level - 1
                };
                let expanded_expr = self.expand(expr, new_level)?;
                Ok(Expression::UnquoteSplicing(Box::new(expanded_expr)))
            }
            Expression::Defmacro(defmacro_expr) => {
                if quasiquote_level == 0 {
                    let macro_def = MacroDef {
                        name: defmacro_expr.name.clone(),
                        params: defmacro_expr.params.clone(),
                        variadic_param: defmacro_expr.variadic_param.clone(),
                        body: defmacro_expr.body.clone(),
                    };
                    self.macros.insert(defmacro_expr.name.clone(), macro_def);
                    Ok(Expression::Literal(crate::ast::Literal::Nil))
                } else {
                    Ok(expression.clone())
                }
            }
            Expression::List(expressions) => {
                if quasiquote_level == 0 {
                    if expressions.is_empty() {
                        return Ok(Expression::List(vec![]));
                    }
                    let first = &expressions[0];
                    if let Expression::Symbol(symbol) = first {
                        if self.macros.contains_key(symbol) {
                            // This is a macro call, expand it
                            let macro_def = self.macros.get(symbol).unwrap().clone();
                            let args = &expressions[1..];
                            let mut bindings = HashMap::new();

                            // Bind regular parameters
                            let num_regular_params = macro_def.params.len();
                            for (param, arg) in macro_def.params.iter().zip(args.iter()) {
                                if let crate::ast::Pattern::Symbol(symbol) = &param.pattern {
                                    bindings.insert(symbol.clone(), arg.clone());
                                }
                            }

                            // Bind variadic parameter if present
                            if let Some(variadic_param) = &macro_def.variadic_param {
                                if let crate::ast::Pattern::Symbol(symbol) = &variadic_param.pattern
                                {
                                    let remaining_args = &args[num_regular_params..];
                                    let args_list = Expression::List(remaining_args.to_vec());
                                    bindings.insert(symbol.clone(), args_list);
                                }
                            }

                            let substituted_body =
                                self.substitute(&macro_def.body[0], &bindings, quasiquote_level)?;
                            // If the body is a quasiquote, evaluate it
                            let evaluated_body = match substituted_body {
                                Expression::Quasiquote(expr) => {
                                    self.substitute(&expr, &bindings, 1)?
                                }
                                _ => substituted_body,
                            };
                            // Replace any remaining simple unquote forms that refer to
                            // bound symbols with their bound values. This is a safety
                            // pass to handle edge cases where substitution left
                            // Unquote nodes in the expanded body.
                            let final_body = self.replace_unquotes(&evaluated_body, &bindings)?;
                            return self.expand(&final_body, quasiquote_level);
                        }
                    }
                }
                // Not a macro call, just expand the elements of the list
                let expanded_expressions = expressions
                    .iter()
                    .map(|expr| self.expand(expr, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::List(expanded_expressions))
            }
            Expression::Vector(expressions) => {
                let expanded_expressions = expressions
                    .iter()
                    .map(|expr| self.expand(expr, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Vector(expanded_expressions))
            }
            Expression::Map(map) => {
                let mut expanded_map = HashMap::new();
                for (key, value) in map {
                    let expanded_value = self.expand(value, quasiquote_level)?;
                    expanded_map.insert(key.clone(), expanded_value);
                }
                Ok(Expression::Map(expanded_map))
            }
            Expression::If(if_expr) => {
                let condition = self.expand(&if_expr.condition, quasiquote_level)?;
                let then_branch = self.expand(&if_expr.then_branch, quasiquote_level)?;
                let else_branch = if_expr
                    .else_branch
                    .as_ref()
                    .map(|expr| self.expand(expr, quasiquote_level))
                    .transpose()?;
                Ok(Expression::If(crate::ast::IfExpr {
                    condition: Box::new(condition),
                    then_branch: Box::new(then_branch),
                    else_branch: else_branch.map(Box::new),
                }))
            }
            Expression::Let(let_expr) => {
                let bindings = let_expr
                    .bindings
                    .iter()
                    .map(|binding| {
                        let value = self.expand(&binding.value, quasiquote_level)?;
                        Ok(crate::ast::LetBinding {
                            pattern: binding.pattern.clone(),
                            type_annotation: binding.type_annotation.clone(),
                            value: Box::new(value),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let body = let_expr
                    .body
                    .iter()
                    .map(|expr| self.expand(expr, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Let(crate::ast::LetExpr { bindings, body }))
            }
            Expression::Do(do_expr) => {
                let expressions = do_expr
                    .expressions
                    .iter()
                    .map(|expr| self.expand(expr, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Do(crate::ast::DoExpr { expressions }))
            }
            Expression::Fn(fn_expr) => {
                let body = fn_expr
                    .body
                    .iter()
                    .map(|expr| self.expand(expr, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Fn(crate::ast::FnExpr {
                    params: fn_expr.params.clone(),
                    variadic_param: fn_expr.variadic_param.clone(),
                    return_type: fn_expr.return_type.clone(),
                    body,
                    delegation_hint: fn_expr.delegation_hint.clone(),
                }))
            }
            Expression::FunctionCall { callee, arguments } => {
                if quasiquote_level == 0 {
                    if let Expression::Symbol(symbol) = callee.as_ref() {
                        if self.macros.contains_key(symbol) {
                            // This is a macro call, expand it
                            let macro_def = self.macros.get(symbol).unwrap().clone();
                            let args = arguments;
                            let mut bindings = HashMap::new();
                            for (param, arg) in macro_def.params.iter().zip(args.iter()) {
                                if let crate::ast::Pattern::Symbol(symbol) = &param.pattern {
                                    bindings.insert(symbol.clone(), arg.clone());
                                }
                            }
                            let substituted_body =
                                self.substitute(&macro_def.body[0], &bindings, quasiquote_level)?;
                            // If the body is a quasiquote, evaluate it
                            let evaluated_body = match substituted_body {
                                Expression::Quasiquote(expr) => {
                                    self.substitute(&expr, &bindings, 1)?
                                }
                                _ => substituted_body,
                            };
                            let final_body = self.replace_unquotes(&evaluated_body, &bindings)?;
                            return self.expand(&final_body, quasiquote_level);
                        }
                    }
                }
                // Not a macro call, expand callee and arguments
                let expanded_callee = self.expand(callee, quasiquote_level)?;
                let expanded_arguments = arguments
                    .iter()
                    .map(|expr| self.expand(expr, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::FunctionCall {
                    callee: Box::new(expanded_callee),
                    arguments: expanded_arguments,
                })
            }
            _ => Ok(expression.clone()),
        }
    }

    fn substitute(
        &self,
        expression: &Expression,
        bindings: &HashMap<Symbol, Expression>,
        quasiquote_level: u32,
    ) -> Result<Expression, String> {
        match expression {
            Expression::Quasiquote(expr) => {
                let substituted_expr = self.substitute(expr, bindings, quasiquote_level + 1)?;
                Ok(Expression::Quasiquote(Box::new(substituted_expr)))
            }
            Expression::Unquote(expr) => {
                let new_level = if quasiquote_level == 0 {
                    0
                } else {
                    quasiquote_level - 1
                };
                let substituted_expr = self.substitute(expr, bindings, new_level)?;
                if new_level == 0 {
                    Ok(substituted_expr)
                } else {
                    Ok(Expression::Unquote(Box::new(substituted_expr)))
                }
            }
            Expression::UnquoteSplicing(expr) => {
                let new_level = if quasiquote_level == 0 {
                    0
                } else {
                    quasiquote_level - 1
                };
                let substituted_expr = self.substitute(expr, bindings, new_level)?;
                if new_level == 0 {
                    Ok(substituted_expr)
                } else {
                    Ok(Expression::UnquoteSplicing(Box::new(substituted_expr)))
                }
            }
            Expression::Symbol(symbol) => {
                if quasiquote_level == 0 {
                    if let Some(value) = bindings.get(symbol) {
                        return Ok(value.clone());
                    }
                }
                Ok(Expression::Symbol(symbol.clone()))
            }
            Expression::ResourceRef(name) => {
                if quasiquote_level == 0 {
                    if let Some(value) = bindings.get(&Symbol::new(name)) {
                        return Ok(value.clone());
                    }
                }
                Ok(Expression::ResourceRef(name.clone()))
            }
            Expression::List(expressions) => {
                if quasiquote_level == 0 {
                    if expressions.len() >= 2 {
                        if let Expression::Symbol(s) = &expressions[0] {
                            if s.0.as_str() == "+" {
                                let mut sum = 0i64;
                                let mut all_int = true;
                                for expr in &expressions[1..] {
                                    if let Expression::Literal(Literal::Integer(i)) = expr {
                                        sum += *i;
                                    } else {
                                        all_int = false;
                                        break;
                                    }
                                }
                                if all_int {
                                    return Ok(Expression::Literal(Literal::Integer(sum)));
                                }
                            }
                        }
                    }
                }
                let mut new_list = Vec::new();
                for expr in expressions {
                    if let Expression::UnquoteSplicing(spliced_expr) = expr {
                        if quasiquote_level == 1 {
                            let substituted_expr =
                                self.substitute(spliced_expr, bindings, quasiquote_level - 1)?;
                            if let Expression::List(spliced_list) = substituted_expr {
                                new_list.extend(spliced_list);
                            } else {
                                return Err(
                                    "Unquote-splicing can only be used on a list".to_string()
                                );
                            }
                        } else {
                            new_list.push(self.substitute(expr, bindings, quasiquote_level)?);
                        }
                    } else {
                        new_list.push(self.substitute(expr, bindings, quasiquote_level)?);
                    }
                }
                Ok(Expression::List(new_list))
            }
            Expression::Vector(expressions) => {
                let substituted_expressions = expressions
                    .iter()
                    .map(|expr| self.substitute(expr, bindings, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Vector(substituted_expressions))
            }
            Expression::Map(map) => {
                let mut substituted_map = HashMap::new();
                for (key, value) in map {
                    let substituted_value = self.substitute(value, bindings, quasiquote_level)?;
                    substituted_map.insert(key.clone(), substituted_value);
                }
                Ok(Expression::Map(substituted_map))
            }
            Expression::If(if_expr) => {
                let condition = self.substitute(&if_expr.condition, bindings, quasiquote_level)?;
                let then_branch =
                    self.substitute(&if_expr.then_branch, bindings, quasiquote_level)?;
                let else_branch = if_expr
                    .else_branch
                    .as_ref()
                    .map(|expr| self.substitute(expr, bindings, quasiquote_level))
                    .transpose()?;
                Ok(Expression::If(crate::ast::IfExpr {
                    condition: Box::new(condition),
                    then_branch: Box::new(then_branch),
                    else_branch: else_branch.map(Box::new),
                }))
            }
            Expression::Let(let_expr) => {
                let processed_bindings = let_expr
                    .bindings
                    .iter()
                    .map(|binding| {
                        let value = self.substitute(&binding.value, bindings, quasiquote_level)?;
                        Ok(crate::ast::LetBinding {
                            pattern: binding.pattern.clone(),
                            type_annotation: binding.type_annotation.clone(),
                            value: Box::new(value),
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                let body = let_expr
                    .body
                    .iter()
                    .map(|expr| self.substitute(expr, bindings, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Let(crate::ast::LetExpr {
                    bindings: processed_bindings,
                    body,
                }))
            }
            Expression::Do(do_expr) => {
                let expressions = do_expr
                    .expressions
                    .iter()
                    .map(|expr| self.substitute(expr, bindings, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Do(crate::ast::DoExpr { expressions }))
            }
            Expression::Fn(fn_expr) => {
                let body = fn_expr
                    .body
                    .iter()
                    .map(|expr| self.substitute(expr, bindings, quasiquote_level))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Expression::Fn(crate::ast::FnExpr {
                    params: fn_expr.params.clone(),
                    variadic_param: fn_expr.variadic_param.clone(),
                    return_type: fn_expr.return_type.clone(),
                    body,
                    delegation_hint: fn_expr.delegation_hint.clone(),
                }))
            }
            _ => Ok(expression.clone()),
        }
    }

    // Replace simple unquote/unquote-splicing nodes that directly reference
    // bound symbols with their bound expressions. Returns an error only when
    // unquote-splicing is used on a non-list value.
    fn replace_unquotes(
        &self,
        expression: &Expression,
        bindings: &HashMap<Symbol, Expression>,
    ) -> Result<Expression, String> {
        match expression {
            Expression::Unquote(expr) => {
                match expr.as_ref() {
                    Expression::Symbol(sym) => {
                        if let Some(v) = bindings.get(sym) {
                            return Ok(v.clone());
                        }
                    }
                    _ => {}
                }
                Ok(expression.clone())
            }
            Expression::UnquoteSplicing(expr) => {
                match expr.as_ref() {
                    Expression::Symbol(sym) => {
                        if let Some(v) = bindings.get(sym) {
                            return Ok(v.clone());
                        }
                    }
                    _ => {}
                }
                Ok(expression.clone())
            }
            Expression::List(exprs) => {
                let mut out: Vec<Expression> = Vec::new();
                for e in exprs {
                    if let Expression::UnquoteSplicing(inner) = e {
                        // If the spliced expression resolves to a list, splice it
                        let replaced = self.replace_unquotes(inner, bindings)?;
                        if let Expression::List(items) = replaced {
                            out.extend(items);
                        } else {
                            return Err("Unquote-splicing can only be used on a list".to_string());
                        }
                    } else {
                        out.push(self.replace_unquotes(e, bindings)?);
                    }
                }
                Ok(Expression::List(out))
            }
            Expression::Vector(exprs) => {
                let mut newv = Vec::new();
                for e in exprs {
                    newv.push(self.replace_unquotes(e, bindings)?);
                }
                Ok(Expression::Vector(newv))
            }
            Expression::FunctionCall { callee, arguments } => {
                let new_callee = Box::new(self.replace_unquotes(callee, bindings)?);
                let mut new_args = Vec::new();
                for a in arguments {
                    new_args.push(self.replace_unquotes(a, bindings)?);
                }
                Ok(Expression::FunctionCall {
                    callee: new_callee,
                    arguments: new_args,
                })
            }
            Expression::Map(map) => {
                let mut new_map = HashMap::new();
                for (k, v) in map {
                    new_map.insert(k.clone(), self.replace_unquotes(v, bindings)?);
                }
                Ok(Expression::Map(new_map))
            }
            Expression::If(if_expr) => {
                let else_branch = match &if_expr.else_branch {
                    Some(e) => Some(Box::new(self.replace_unquotes(e, bindings)?)),
                    None => None,
                };
                Ok(Expression::If(crate::ast::IfExpr {
                    condition: Box::new(self.replace_unquotes(&if_expr.condition, bindings)?),
                    then_branch: Box::new(self.replace_unquotes(&if_expr.then_branch, bindings)?),
                    else_branch,
                }))
            }
            Expression::Let(let_expr) => {
                let mut processed_bindings = Vec::new();
                for binding in &let_expr.bindings {
                    processed_bindings.push(crate::ast::LetBinding {
                        pattern: binding.pattern.clone(),
                        type_annotation: binding.type_annotation.clone(),
                        value: Box::new(self.replace_unquotes(&binding.value, bindings)?),
                    });
                }
                let mut body = Vec::new();
                for b in &let_expr.body {
                    body.push(self.replace_unquotes(b, bindings)?);
                }
                Ok(Expression::Let(crate::ast::LetExpr {
                    bindings: processed_bindings,
                    body,
                }))
            }
            Expression::Do(do_expr) => {
                let mut new_exprs = Vec::new();
                for e in &do_expr.expressions {
                    new_exprs.push(self.replace_unquotes(e, bindings)?);
                }
                Ok(Expression::Do(crate::ast::DoExpr {
                    expressions: new_exprs,
                }))
            }
            Expression::Fn(fn_expr) => {
                let mut body = Vec::new();
                for e in &fn_expr.body {
                    body.push(self.replace_unquotes(e, bindings)?);
                }
                Ok(Expression::Fn(crate::ast::FnExpr {
                    params: fn_expr.params.clone(),
                    variadic_param: fn_expr.variadic_param.clone(),
                    return_type: fn_expr.return_type.clone(),
                    body,
                    delegation_hint: fn_expr.delegation_hint.clone(),
                }))
            }
            _ => Ok(expression.clone()),
        }
    }
}
