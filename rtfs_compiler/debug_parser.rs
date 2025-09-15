use rtfs_compiler::parser::RTFSParser;
use rtfs_compiler::parser::Rule;

fn main() {
    let input = "(fn ^:delegation :local-model \"phi-mini\" [x] (+ x 1))";
    println!("Testing input: {}", input);
    
    match rtfs_compiler::parser::parse(input) {
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
                    
                    // Print deeply nested pairs
                    for (k, deep) in nested.clone().into_inner().enumerate() {
                        println!("      {}: {:?} = '{}'", k, deep.as_rule(), deep.as_str());
                    }
                }
            }
        }
        Err(e) => {
            println!("Parsing failed: {}", e);
        }
    }
}
