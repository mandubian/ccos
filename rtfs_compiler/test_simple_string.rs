#[cfg(test)]
mod tests {
    use rtfs_compiler::*;

    #[test] 
    fn test_simple_strings() {
        // Test a simple string first
        let simple = r#""hello""#;
        match parse(simple) {
            Ok(ast) => println!("Simple string OK: {:?}", ast),
            Err(e) => panic!("Simple string failed: {:?}", e),
        }

        // Test string with single backslash
        let single_backslash = r#""hello\world""#;
        match parse(single_backslash) {
            Ok(ast) => println!("Single backslash OK: {:?}", ast),
            Err(e) => println!("Single backslash failed: {:?}", e),
        }

        // Test string with double backslash
        let double_backslash = r#""hello\\world""#;
        match parse(double_backslash) {
            Ok(ast) => println!("Double backslash OK: {:?}", ast),
            Err(e) => println!("Double backslash failed: {:?}", e),
        }

        // Test string ending with backslash-n
        let backslash_n = r#""hello\\n""#;
        match parse(backslash_n) {
            Ok(ast) => println!("Backslash-n OK: {:?}", ast),
            Err(e) => println!("Backslash-n failed: {:?}", e),
        }
    }
}
