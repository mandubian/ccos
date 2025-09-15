#[cfg(test)]
mod tests {
    use rtfs_compiler::*;

    #[test]
    fn test_direct_parse_of_exact_string() {
        // Test the exact string that's failing
        let test_string = r#""Special: !@#$%^&*()_+-=[]{}|;':\\n""#;
        println!("Testing string: {:?}", test_string);
        println!("Raw bytes:");
        for (i, byte) in test_string.bytes().enumerate() {
            print!("{:02x} ", byte);
            if (i + 1) % 16 == 0 {
                println!();
            }
        }
        println!();
        
        match parse(test_string) {
            Ok(ast) => {
                println!("Parse successful!");
                println!("AST: {:?}", ast);
            }
            Err(e) => {
                println!("Parse error: {:?}", e);
                panic!("Parse failed");
            }
        }
    }
}
