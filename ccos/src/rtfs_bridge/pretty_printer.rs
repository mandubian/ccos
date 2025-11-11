use crate::synthesis::schema_serializer::type_expr_to_rtfs_compact;
use rtfs::ast::{Expression, FnExpr, Literal, MapKey, ParamDef, Pattern};
use std::collections::HashMap;

const DEFAULT_INDENT: &str = "  ";
const DEFAULT_INLINE_THRESHOLD: usize = 80;

/// Render an RTFS expression using a compact, single-line oriented format.
pub fn expression_to_rtfs_string(expr: &Expression) -> String {
    RtfsPrettyPrinter::new(
        DEFAULT_INDENT,
        DEFAULT_INLINE_THRESHOLD,
        PrinterMode::Compact,
    )
    .format(expr, 0)
}

/// Render an RTFS expression with human-friendly indentation and line breaks.
pub fn expression_to_pretty_rtfs_string(expr: &Expression) -> String {
    RtfsPrettyPrinter::new(
        DEFAULT_INDENT,
        DEFAULT_INLINE_THRESHOLD,
        PrinterMode::Pretty,
    )
    .format(expr, 0)
}

/// Render an RTFS expression into a compact inline string useful for diagnostics.
pub fn expression_to_inline_string(expr: &Expression) -> String {
    RtfsPrettyPrinter::new(
        DEFAULT_INDENT,
        DEFAULT_INLINE_THRESHOLD,
        PrinterMode::Inline,
    )
    .format(expr, 0)
}

/// Convert a binding pattern into RTFS syntax.
pub(crate) fn pattern_to_rtfs_string(pat: &Pattern) -> String {
    match pat {
        Pattern::Symbol(s) => s.0.clone(),
        Pattern::Wildcard => "_".to_string(),
        Pattern::VectorDestructuring {
            elements,
            rest,
            as_symbol,
        } => {
            let mut parts: Vec<String> = elements.iter().map(pattern_to_rtfs_string).collect();
            if let Some(r) = rest {
                parts.push(format!("& {}", r.0));
            }
            if let Some(a) = as_symbol {
                parts.push(format!(":as {}", a.0));
            }
            format!("[{}]", parts.join(" "))
        }
        Pattern::MapDestructuring {
            entries,
            rest,
            as_symbol,
        } => {
            let mut parts: Vec<String> = Vec::new();
            for entry in entries {
                match entry {
                    rtfs::ast::MapDestructuringEntry::KeyBinding { key, pattern } => {
                        let key_str = match key {
                            rtfs::ast::MapKey::Keyword(kw) => format!(":{}", kw.0),
                            rtfs::ast::MapKey::String(s) => format!("\"{}\"", s),
                            rtfs::ast::MapKey::Integer(i) => i.to_string(),
                        };
                        parts.push(format!("{} {}", key_str, pattern_to_rtfs_string(pattern)));
                    }
                    rtfs::ast::MapDestructuringEntry::Keys(keys) => {
                        let ks = keys
                            .iter()
                            .map(|k| k.0.clone())
                            .collect::<Vec<_>>()
                            .join(" ");
                        parts.push(format!(":keys [{}]", ks));
                    }
                }
            }
            if let Some(r) = rest {
                parts.push(format!("& {}", r.0));
            }
            if let Some(a) = as_symbol {
                parts.push(format!(":as {}", a.0));
            }
            format!("{{{}}}", parts.join(" "))
        }
    }
}

fn param_def_to_rtfs_string(param: &ParamDef) -> String {
    let pattern_str = pattern_to_rtfs_string(&param.pattern);
    if let Some(ty) = &param.type_annotation {
        format!("{} {}", pattern_str, type_expr_to_rtfs_compact(ty))
    } else {
        pattern_str
    }
}

fn fn_expr_to_rtfs_string(printer: &RtfsPrettyPrinter, fn_expr: &FnExpr, depth: usize) -> String {
    let mut param_parts: Vec<String> = fn_expr
        .params
        .iter()
        .map(param_def_to_rtfs_string)
        .collect();
    if let Some(var_param) = &fn_expr.variadic_param {
        param_parts.push(format!("& {}", param_def_to_rtfs_string(var_param)));
    }
    let params_block = format!("[{}]", param_parts.join(" "));

    let body_str = if fn_expr.body.len() == 1 {
        printer.format(&fn_expr.body[0], depth + 1)
    } else {
        printer.format_block("do", &fn_expr.body, depth + 1)
    };

    let mut result = format!("(fn {}", params_block);
    if printer.should_inline_body(&[body_str.clone()]) && !body_str.contains('\n') {
        result.push(' ');
        result.push_str(&body_str);
        result.push(')');
    } else {
        result.push('\n');
        result.push_str(&printer.indent(depth + 1));
        result.push_str(&body_str);
        result.push('\n');
        result.push_str(&printer.indent(depth));
        result.push(')');
    }
    result
}

#[derive(Clone, Copy)]
enum PrinterMode {
    /// Compact formatting mimicking the previous implementation.
    Compact,
    /// Fully pretty-printed with indentation.
    Pretty,
    /// One-line inline form for diagnostics.
    Inline,
}

struct RtfsPrettyPrinter {
    indent: String,
    inline_threshold: usize,
    mode: PrinterMode,
}

impl RtfsPrettyPrinter {
    fn new(indent: &str, inline_threshold: usize, mode: PrinterMode) -> Self {
        Self {
            indent: indent.to_string(),
            inline_threshold,
            mode,
        }
    }

    fn indent(&self, depth: usize) -> String {
        self.indent.repeat(depth)
    }

    fn format(&self, expr: &Expression, depth: usize) -> String {
        match expr {
            Expression::Literal(literal) => match literal {
                Literal::String(s) => format!("\"{}\"", s),
                Literal::Integer(i) => i.to_string(),
                Literal::Float(f) => f.to_string(),
                Literal::Boolean(b) => b.to_string(),
                Literal::Nil => "nil".to_string(),
                Literal::Keyword(k) => format!(":{}", k.0),
                Literal::Symbol(s) => s.0.clone(),
                _ => self.format_debug(expr),
            },
            Expression::Symbol(s) => s.0.clone(),
            Expression::FunctionCall { callee, arguments } => {
                self.format_function_call(callee, arguments, depth)
            }
            Expression::Do(do_expr) => self.format_block("do", &do_expr.expressions, depth),
            Expression::Vector(vec) => self.format_sequence("[", "]", vec, depth),
            Expression::List(list) => self.format_sequence("(", ")", list, depth),
            Expression::Map(map) => self.format_map(map, depth),
            Expression::Let(let_expr) => {
                self.format_let(let_expr.bindings.as_slice(), &let_expr.body, depth)
            }
            Expression::Fn(fn_expr) => fn_expr_to_rtfs_string(self, fn_expr, depth),
            _ => self.format_debug(expr),
        }
    }

    fn format_debug(&self, expr: &Expression) -> String {
        match self.mode {
            PrinterMode::Pretty | PrinterMode::Compact => format!("{:?}", expr),
            PrinterMode::Inline => format!("{:?}", expr),
        }
    }

    fn format_function_call(
        &self,
        callee: &Expression,
        arguments: &[Expression],
        depth: usize,
    ) -> String {
        let callee_str = self.format(callee, depth + 1);
        if arguments.is_empty() {
            return format!("({})", callee_str);
        }

        let arg_strings: Vec<String> = arguments
            .iter()
            .map(|arg| self.format(arg, depth + 1))
            .collect();

        if self.should_inline_body(&arg_strings) {
            format!("({} {})", callee_str, arg_strings.join(" "))
        } else {
            let mut result = format!("({}", callee_str);
            for arg in arg_strings {
                result.push('\n');
                result.push_str(&self.indent(depth + 1));
                result.push_str(&arg);
            }
            result.push('\n');
            result.push_str(&self.indent(depth));
            result.push(')');
            result
        }
    }

    fn format_block(&self, tag: &str, expressions: &[Expression], depth: usize) -> String {
        if expressions.is_empty() {
            return format!("({})", tag);
        }

        let body_strings: Vec<String> = expressions
            .iter()
            .map(|expr| self.format(expr, depth + 1))
            .collect();

        if self.should_inline_body(&body_strings) {
            let body = body_strings.join(" ");
            format!("({} {})", tag, body)
        } else {
            let mut result = format!("({}", tag);
            for body in body_strings {
                result.push('\n');
                result.push_str(&self.indent(depth + 1));
                result.push_str(&body);
            }
            result.push('\n');
            result.push_str(&self.indent(depth));
            result.push(')');
            result
        }
    }

    fn format_sequence(
        &self,
        open: &str,
        close: &str,
        expressions: &[Expression],
        depth: usize,
    ) -> String {
        if expressions.is_empty() {
            return format!("{}{}", open, close);
        }

        let parts: Vec<String> = expressions
            .iter()
            .map(|expr| self.format(expr, depth + 1))
            .collect();

        if self.should_inline_body(&parts) {
            format!("{}{}{}", open, parts.join(" "), close)
        } else {
            let mut result = String::from(open);
            for part in parts {
                result.push('\n');
                result.push_str(&self.indent(depth + 1));
                result.push_str(&part);
            }
            result.push('\n');
            result.push_str(&self.indent(depth));
            result.push_str(close);
            result
        }
    }

    fn format_map(&self, map: &HashMap<MapKey, Expression>, depth: usize) -> String {
        if map.is_empty() {
            return "{}".to_string();
        }

        let mut entries: Vec<(String, String)> = map
            .iter()
            .map(|(key, value)| {
                let key_str = match key {
                    MapKey::Keyword(kw) => format!(":{}", kw.0),
                    MapKey::String(s) => format!("\"{}\"", s),
                    MapKey::Integer(i) => i.to_string(),
                };
                let value_str = self.format(value, depth + 1);
                (key_str, value_str)
            })
            .collect();

        entries.sort_by(|(ka, _), (kb, _)| ka.cmp(kb));

        if self.should_inline_entries(&entries) {
            let inline = entries
                .iter()
                .map(|(k, v)| format!("{} {}", k, v))
                .collect::<Vec<_>>()
                .join(" ");
            format!("{{{}}}", inline)
        } else {
            let mut result = String::from("{");
            for (key, value) in entries {
                result.push('\n');
                result.push_str(&self.indent(depth + 1));
                result.push_str(&key);
                result.push(' ');
                result.push_str(&value);
            }
            result.push('\n');
            result.push_str(&self.indent(depth));
            result.push('}');
            result
        }
    }

    fn format_let(
        &self,
        bindings: &[rtfs::ast::LetBinding],
        body: &[Expression],
        depth: usize,
    ) -> String {
        let mut binding_parts: Vec<String> = Vec::new();
        for binding in bindings {
            binding_parts.push(format!(
                "{} {}",
                pattern_to_rtfs_string(&binding.pattern),
                self.format(&binding.value, depth + 1)
            ));
        }

        let binding_block = if self.should_inline_body(&binding_parts) {
            format!("[{}]", binding_parts.join(" "))
        } else {
            let mut block = String::from("[");
            for part in binding_parts {
                block.push('\n');
                block.push_str(&self.indent(depth + 1));
                block.push_str(&part);
            }
            block.push('\n');
            block.push_str(&self.indent(depth));
            block.push(']');
            block
        };

        let body_block = if body.len() == 1 {
            self.format(&body[0], depth + 1)
        } else {
            self.format_block("do", body, depth + 1)
        };

        let mut result = format!("(let {}", binding_block);
        if self.should_inline_body(&[body_block.clone()]) && !body_block.contains('\n') {
            result.push(' ');
            result.push_str(&body_block);
            result.push(')');
        } else {
            result.push('\n');
            result.push_str(&self.indent(depth + 1));
            result.push_str(&body_block);
            result.push('\n');
            result.push_str(&self.indent(depth));
            result.push(')');
        }
        result
    }

    fn should_inline_body(&self, parts: &[String]) -> bool {
        match self.mode {
            PrinterMode::Inline => true,
            PrinterMode::Compact => !parts.iter().any(|s| s.contains('\n')),
            PrinterMode::Pretty => {
                if parts.iter().any(|s| s.contains('\n')) {
                    return false;
                }
                let len: usize =
                    parts.iter().map(|s| s.len()).sum::<usize>() + parts.len().saturating_sub(1);
                len <= self.inline_threshold
            }
        }
    }

    fn should_inline_entries(&self, entries: &[(String, String)]) -> bool {
        match self.mode {
            PrinterMode::Inline => true,
            PrinterMode::Compact => entries.iter().all(|(_, v)| !v.contains('\n')),
            PrinterMode::Pretty => {
                if entries.is_empty() {
                    return true;
                }
                if entries.len() > 1 {
                    return false;
                }
                if entries.iter().any(|(_, v)| v.contains('\n')) {
                    return false;
                }
                let len: usize = entries
                    .iter()
                    .map(|(k, v)| k.len() + 1 + v.len())
                    .sum::<usize>();
                len <= self.inline_threshold
            }
        }
    }
}
