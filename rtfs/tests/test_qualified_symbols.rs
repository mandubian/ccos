#[test]
fn test_qualified_symbol_check() {
    use rtfs::runtime::module_runtime::ModuleRegistry;

    // Test that single "/" is not a qualified symbol
    assert!(!ModuleRegistry::is_qualified_symbol("/"));

    // Test that these are qualified symbols
    assert!(ModuleRegistry::is_qualified_symbol("module/symbol"));
    assert!(ModuleRegistry::is_qualified_symbol("my.module/function"));

    // Test edge cases
    assert!(!ModuleRegistry::is_qualified_symbol(""));
    assert!(!ModuleRegistry::is_qualified_symbol("no_slash"));
    assert!(!ModuleRegistry::is_qualified_symbol("/starts_with_slash")); // Empty module name
    assert!(!ModuleRegistry::is_qualified_symbol("ends_with_slash/")); // Empty symbol name

    println!("All qualified symbol tests passed!");
}
