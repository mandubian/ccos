use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "rtfs.pest"]
struct RTFSParser;

fn main() {
    let input = "[b :SomeType (g 2)]";
    
    match RTFSParser::parse(Rule::parallel_binding, input) {
        Ok(pairs) => {
            for pair in pairs {
                println!("Top level: rule={:?}, text='{}'", pair.as_rule(), pair.as_str());
                for inner in pair.into_inner() {
                    println!("  Inner: rule={:?}, text='{}'", inner.as_rule(), inner.as_str());
                }
            }
        }
        Err(e) => {
            println!("Parse error: {}", e);
        }
    }
}
