// Temporary debug file to understand the parsing issue
use pest::Parser;

#[derive(pest_derive::Parser)]
#[grammar = "rtfs.pest"]
struct RTFSParser;

use pest::Rule;
use pest::iterators::Pairs;

fn main() {
    let input = "(match x [a b] when (> a b) (combine a b) _ nil)";
    
    match RTFSParser::parse(Rule::expression, input) {
        Ok(pairs) => {
            for pair in pairs {
                print_pair_recursive(&pair, 0);
            }
        }
        Err(e) => {
            println!("Parse error: {}", e);
        }
    }
}

fn print_pair_recursive(pair: &pest::iterators::Pair<Rule>, depth: usize) {
    let indent = "  ".repeat(depth);
    println!("{}Rule::{:?} -> '{}'", indent, pair.as_rule(), pair.as_str());
    
    for inner_pair in pair.clone().into_inner() {
        print_pair_recursive(&inner_pair, depth + 1);
    }
}
