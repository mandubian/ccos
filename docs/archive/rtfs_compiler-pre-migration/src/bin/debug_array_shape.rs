use rtfs_compiler::parser::parse_type_expression;

fn main() {
    let inputs = [":int", "[:array string [?]]", "[:array int [5]]"];
    for inp in inputs {
        match parse_type_expression(inp) {
            Ok(t) => println!("OK {inp} => {:?}", t),
            Err(e) => println!("ERR {inp} => {:?}", e),
        }
    }
}
