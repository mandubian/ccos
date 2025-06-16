use rtfs_compiler::*;

fn main() {
    // Test a simple function definition first
    let simple_fn = "(fn [n acc] (+ n acc))";
    
    match parser::parse_expression(simple_fn) {
        Ok(expr) => {
            println!("Simple fn parsed successfully: {:#?}", expr);
            
            if let ast::Expression::Fn(fn_expr) = expr {
                println!("Params count: {}", fn_expr.params.len());
                for (i, param) in fn_expr.params.iter().enumerate() {
                    match &param.pattern {
                        ast::Pattern::Symbol(symbol) => {
                            println!("  Param {}: Symbol({})", i, symbol.0);
                        }
                        other => {
                            println!("  Param {}: {:?}", i, other);
                        }
                    }
                }
            }
        }
        Err(e) => {
            println!("Parse error: {:?}", e);
        }
    }
}
