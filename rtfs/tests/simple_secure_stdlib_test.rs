use rtfs::runtime::secure_stdlib::SecureStandardLibrary;

#[test]
fn test_secure_stdlib_basic() {
    // Test that we can create the secure environment
    let env = SecureStandardLibrary::create_secure_environment();

    // Test that basic arithmetic functions are defined
    let plus_symbol = rtfs::ast::Symbol("+".to_string());
    let plus_func = env.lookup(&plus_symbol);
    assert!(
        plus_func.is_some(),
        "Plus function should be defined in secure stdlib"
    );

    println!("✅ Secure stdlib basic test passed!");
}

#[test]
fn test_secure_stdlib_has_core_functions() {
    let env = SecureStandardLibrary::create_secure_environment();

    // Test core functions are present
    let functions_to_check = vec![
        "+",
        "-",
        "*",
        "/",
        "=",
        ">",
        "<",
        "and",
        "or",
        "not",
        "str",
        "vector",
        "hash-map",
        "count",
        "first",
        "rest",
        "int?",
        "string?",
        "nil?",
        "empty?",
        "inc",
        "dec",
        "factorial",
    ];

    for func_name in functions_to_check {
        let symbol = rtfs::ast::Symbol(func_name.to_string());
        let func = env.lookup(&symbol);
        assert!(
            func.is_some(),
            "Function {} should be defined in secure stdlib",
            func_name
        );
    }

    println!("✅ Secure stdlib core functions test passed!");
}
