use rtfs_compiler::*;

fn main() {
    let test_string = r#""Special: !@#$%^&*()_+-=[]{}|;':\\n""#;
    println!("Testing string: {}", test_string);
    
    match parse(test_string) {
        Ok(ast) => {
            println!("Parse successful!");
            println!("AST: {:?}", ast);
        }
        Err(e) => {
            println!("Parse error: {:?}", e);
        }
    }
}
