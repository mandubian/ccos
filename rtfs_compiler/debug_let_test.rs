fn main() -> Result<(), Box<dyn std::error::Error>> {
    use rtfs_compiler::parser;
    use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
    use rtfs_compiler::runtime;
    use std::rc::Rc;

    // Set up the runtime similar to integration tests
    let mut module_registry = ModuleRegistry::new();
    rtfs_compiler::runtime::stdlib::load_stdlib(&mut module_registry).unwrap();
    let module_registry = Rc::new(module_registry);

    // Create AST runtime (tree walking strategy)
    let mut runtime = runtime::Runtime::new_with_tree_walking_strategy(module_registry.clone());

    // Test each let expression case individually
    let test_cases = vec![
        ("let_expressions[0]", "(let [x 42] x)"),
        ("let_expressions[1]", "(let [x 10 y 20] (+ x y))"),
        ("let_expressions[2]", "(let [x 5] (let [y (* x 2)] (+ x y)))"),
        ("let_expressions[3]", "(let [x:int 42 y:string \"hello\"] [x y])"),
    ];

    for (name, code) in test_cases {
        println!("\n=== Testing {} ===", name);
        match parser::parse_expression(code) {
            Ok(ast) => {
                println!("AST: {:#?}", ast);
                match runtime.run(&ast) {
                    Ok(value) => println!("{}: SUCCESS -> {}", name, value),
                    Err(e) => println!("{}: ERROR -> {:?}", name, e),
                }
            }
            Err(e) => println!("{}: PARSE ERROR -> {:?}", name, e),
        }
    }
    
    Ok(())
}
