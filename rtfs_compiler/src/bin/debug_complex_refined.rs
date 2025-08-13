use rtfs_compiler::parser::parse_type_expression;

fn main() {
    let input = "[:and string [:min-length 3] [:max-length 255] [:matches-regex \"^[a-zA-Z]+$\"]]";
    println!("Input: {}", input);
    match parse_type_expression(input) {
        Ok(t) => println!("Parsed OK: {:?}", t),
        Err(e) => println!("Error: {:?}", e),
    }
}
