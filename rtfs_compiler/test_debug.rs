#[cfg(test)]
mod tests {
    use rtfs_compiler::parser::RTFSParser;
    use rtfs_compiler::parser::Rule;
    use pest::Parser;

    #[test]
    fn debug_fn_parsing() {
        let input = "(fn ^:delegation :local-model \"phi-mini\" [x] (+ x 1))";
        println!("Testing input: {}", input);
        
        match RTFSParser::parse(Rule::fn_expr, input) {
            Ok(mut pairs) => {
                println!("Parsing successful!");
                let pair = pairs.next().unwrap();
                println!("Root rule: {:?}", pair.as_rule());
                
                // Print all inner pairs
                for (i, inner) in pair.clone().into_inner().enumerate() {
                    println!("  {}: {:?} = '{}'", i, inner.as_rule(), inner.as_str());
                    
                    // Print nested pairs
                    for (j, nested) in inner.clone().into_inner().enumerate() {
                        println!("    {}: {:?} = '{}'", j, nested.as_rule(), nested.as_str());
                    }
                }
            }
            Err(e) => {
                println!("Parsing failed: {}", e);
            }
        }
    }
}
