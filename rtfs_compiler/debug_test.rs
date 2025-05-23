use rtfs_compiler::parser::RTFSParser;
use pest::Parser;

fn main() {
    let input = "(log-step :id \"step-1\" (do-something))";
    
    // Try to parse as expression
    match RTFSParser::parse(rtfs_compiler::parser::Rule::expression, input) {
        Ok(mut pairs) => {
            let pair = pairs.next().unwrap();
            println!("Top-level rule: {:?}", pair.as_rule());
            
            // Drill down to see the actual rule
            let mut current = pair;
            loop {
                let rule = current.as_rule();
                println!("Current rule: {:?}", rule);
                
                if rule == rtfs_compiler::parser::Rule::expression || rule == rtfs_compiler::parser::Rule::special_form {
                    let mut inner = current.into_inner();
                    if let Some(next) = inner.next() {
                        current = next;
                    } else {
                        break;
                    }
                } else {
                    break;
                }
            }
            
            println!("Final rule: {:?}", current.as_rule());
        }
        Err(e) => {
            println!("Parse error: {}", e);
        }
    }
}
