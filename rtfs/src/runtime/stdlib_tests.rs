#[cfg(test)]
mod test_stdlib_loading {
    use crate::runtime::module_runtime::ModuleRegistry;
    use crate::runtime::stdlib::load_stdlib;

    #[test]
    fn test_load_stdlib_creates_module() {
        // Create a fresh module registry
    let registry = ModuleRegistry::new();

        // Load the stdlib
    let result = load_stdlib(&registry);

        // Verify the operation succeeded
        assert!(result.is_ok(), "Failed to load stdlib: {:?}", result.err());

        // Verify that the stdlib module was created
        let stdlib_module = registry.get_module("stdlib");
        assert!(stdlib_module.is_some(), "stdlib module was not created");

        // Verify that the module has some expected functions
        let module = stdlib_module.unwrap();
        let exports = module.exports.read().expect("RwLock poisoned");

        // Check for some basic arithmetic functions
        assert!(exports.contains_key("+"), "Missing + function");
        assert!(exports.contains_key("-"), "Missing - function");
        assert!(exports.contains_key("*"), "Missing * function");
        assert!(exports.contains_key("/"), "Missing / function");

        // Check for some comparison functions
        assert!(exports.contains_key("="), "Missing = function");
        assert!(exports.contains_key(">"), "Missing > function");
        assert!(exports.contains_key("<"), "Missing < function");

        // Check for some collection functions
        assert!(exports.contains_key("count"), "Missing count function");
        assert!(exports.contains_key("first"), "Missing first function");

        println!(
            "✅ Successfully loaded stdlib with {} functions",
            exports.len()
        );
    }
}
