use rtfs_compiler::runtime::secure_stdlib::SecureStandardLibrary;
use rtfs_compiler::runtime::evaluator::Evaluator;
use rtfs_compiler::runtime::module_runtime::ModuleRegistry;
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::values::Value;
use rtfs_compiler::ast::{MapKey, Keyword};
use std::sync::Arc;
use std::sync::Mutex;
use tokio::sync::RwLock;

/// Test runner for secure standard library end-to-end tests
struct SecureStdlibTestRunner {
    evaluator: Evaluator,
    env: rtfs_compiler::runtime::environment::Environment,
}

impl SecureStdlibTestRunner {
    fn new() -> Self {
        let env = SecureStandardLibrary::create_secure_environment();
        let module_registry = Arc::new(ModuleRegistry::new());
        let registry = Arc::new(RwLock::new(rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry::new()));
        let capability_marketplace = std::sync::Arc::new(
            rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace::new(registry)
        );
        let causal_chain = std::sync::Arc::new(Mutex::new(
            rtfs_compiler::ccos::causal_chain::CausalChain::new().unwrap()
        ));
        let security_context = rtfs_compiler::runtime::security::RuntimeContext::pure();
        let host = std::sync::Arc::new(rtfs_compiler::ccos::host::RuntimeHost::new(
            causal_chain,
            capability_marketplace,
            security_context.clone(),
        ));
        let evaluator = Evaluator::new(
            module_registry,
            security_context,
            host
        );
        
        Self { evaluator, env }
    }

    fn run_test(&mut self, source: &str, expected: Value) -> Result<(), String> {
        let ast = parse_expression(source)
            .map_err(|e| format!("Parse error: {:?}", e))?;
        
        let outcome = self.evaluator.evaluate(&ast)
            .map_err(|e| format!("Evaluation error: {:?}", e))?;
        
        let result = match outcome {
            rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::Complete(value) => value,
            rtfs_compiler::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
                return Err("Host call required in pure test".to_string());
            }
        };
        
        if result == expected {
            Ok(())
        } else {
            Err(format!("Expected {:?}, got {:?}", expected, result))
        }
    }

    fn run_error_test(&mut self, source: &str, expected_error_contains: &str) -> Result<(), String> {
        let ast = parse_expression(source)
            .map_err(|e| format!("Parse error: {:?}", e))?;
        
        match self.evaluator.evaluate(&ast) {
            Ok(result) => Err(format!("Expected error containing '{}', but got success: {:?}", expected_error_contains, result)),
            Err(error) => {
                let error_string = format!("{:?}", error);
                if error_string.contains(expected_error_contains) {
                    Ok(())
                } else {
                    Err(format!("Expected error containing '{}', but got: {}", expected_error_contains, error_string))
                }
            }
        }
    }

    fn run_arithmetic_tests(&mut self) -> Result<(), String> {
        println!("Running arithmetic function tests...");
        
        // Addition tests
        self.run_test("(+ 1 2)", Value::Integer(3))?;
        self.run_test("(+ 1 2 3 4 5)", Value::Integer(15))?;
        self.run_test("(+ 0 0)", Value::Integer(0))?;
        self.run_test("(+ -1 1)", Value::Integer(0))?;
        self.run_test("(+ 1.5 2.5)", Value::Float(4.0))?;
        self.run_test("(+ 1 2.5)", Value::Float(3.5))?;
        
        // Subtraction tests
        self.run_test("(- 5 3)", Value::Integer(2))?;
        self.run_test("(- 10 5 2)", Value::Integer(3))?;
        self.run_test("(- 0 5)", Value::Integer(-5))?;
        self.run_test("(- 1.5 0.5)", Value::Float(1.0))?;
        self.run_test("(- 1 2.5)", Value::Float(-1.5))?;
        
        // Multiplication tests
        self.run_test("(* 2 3)", Value::Integer(6))?;
        self.run_test("(* 2 3 4)", Value::Integer(24))?;
        self.run_test("(* 0 5)", Value::Integer(0))?;
        self.run_test("(* 2.5 3)", Value::Float(7.5))?;
        
        // Division tests
        self.run_test("(/ 6 2)", Value::Integer(3))?;
        self.run_test("(/ 10 2 2)", Value::Float(2.5))?;
        self.run_test("(/ 5.0 2)", Value::Float(2.5))?;
        
        // Max/Min tests
        self.run_test("(max 1 2 3)", Value::Integer(3))?;
        self.run_test("(min 1 2 3)", Value::Integer(1))?;
        self.run_test("(max 1.5 2.5)", Value::Float(2.5))?;
        self.run_test("(min 1.5 2.5)", Value::Float(1.5))?;
        
        // Increment tests
        self.run_test("(inc 5)", Value::Integer(6))?;
        self.run_test("(inc 0)", Value::Integer(1))?;
        self.run_test("(inc -1)", Value::Integer(0))?;
        self.run_test("(inc 1.5)", Value::Float(2.5))?;
        
        // Decrement tests - NEW
        self.run_test("(dec 5)", Value::Integer(4))?;
        self.run_test("(dec 1)", Value::Integer(0))?;
        self.run_test("(dec 0)", Value::Integer(-1))?;
        self.run_test("(dec 1.5)", Value::Float(0.5))?;
        
        // Factorial tests
        self.run_test("(factorial 0)", Value::Integer(1))?;
        self.run_test("(factorial 1)", Value::Integer(1))?;
        self.run_test("(factorial 5)", Value::Integer(120))?;
        
        println!(" All arithmetic function tests passed!");
        Ok(())
    }

    fn run_comparison_tests(&mut self) -> Result<(), String> {
        println!("Running comparison function tests...");
        
        // Equal tests
        self.run_test("(= 1 1)", Value::Boolean(true))?;
        self.run_test("(= 1 2)", Value::Boolean(false))?;
        self.run_test("(= 1.0 1.0)", Value::Boolean(true))?;
        self.run_test("(= 1 1.0)", Value::Boolean(false))?; // Different types
        self.run_test("(= \"hello\" \"hello\")", Value::Boolean(true))?;
        self.run_test("(= \"hello\" \"world\")", Value::Boolean(false))?;
        self.run_test("(= true true)", Value::Boolean(true))?;
        self.run_test("(= false true)", Value::Boolean(false))?;
        self.run_test("(= nil nil)", Value::Boolean(true))?;
        
        // Not equal tests
        self.run_test("(not= 1 2)", Value::Boolean(true))?;
        self.run_test("(not= 1 1)", Value::Boolean(false))?;
        self.run_test("(not= \"hello\" \"world\")", Value::Boolean(true))?;
        self.run_test("(not= \"hello\" \"hello\")", Value::Boolean(false))?;
        
        // Both != and not= should work
        self.run_test("(!= 1 2)", Value::Boolean(true))?;
        self.run_test("(!= 1 1)", Value::Boolean(false))?;
        
        // Greater than tests
        self.run_test("(> 5 3)", Value::Boolean(true))?;
        self.run_test("(> 3 5)", Value::Boolean(false))?;
        self.run_test("(> 3 3)", Value::Boolean(false))?;
        self.run_test("(> 3.5 2.5)", Value::Boolean(true))?;
        
        // Less than tests
        self.run_test("(< 3 5)", Value::Boolean(true))?;
        self.run_test("(< 5 3)", Value::Boolean(false))?;
        self.run_test("(< 3 3)", Value::Boolean(false))?;
        self.run_test("(< 2.5 3.5)", Value::Boolean(true))?;
        
        // Greater equal tests
        self.run_test("(>= 5 3)", Value::Boolean(true))?;
        self.run_test("(>= 3 5)", Value::Boolean(false))?;
        self.run_test("(>= 3 3)", Value::Boolean(true))?;
        self.run_test("(>= 3.5 2.5)", Value::Boolean(true))?;
        
        // Less equal tests
        self.run_test("(<= 3 5)", Value::Boolean(true))?;
        self.run_test("(<= 5 3)", Value::Boolean(false))?;
        self.run_test("(<= 3 3)", Value::Boolean(true))?;
        self.run_test("(<= 2.5 3.5)", Value::Boolean(true))?;
        
        println!(" All comparison function tests passed!");
        Ok(())
    }

    fn run_boolean_tests(&mut self) -> Result<(), String> {
        println!("Running boolean logic function tests...");
        
        // And tests
        self.run_test("(and true true)", Value::Boolean(true))?;
        self.run_test("(and true false)", Value::Boolean(false))?;
        self.run_test("(and false true)", Value::Boolean(false))?;
        self.run_test("(and false false)", Value::Boolean(false))?;
        self.run_test("(and true true true)", Value::Boolean(true))?;
        self.run_test("(and true false true)", Value::Boolean(false))?;
        self.run_test("(and)", Value::Boolean(true))?;
        self.run_test("(and true)", Value::Boolean(true))?;
        self.run_test("(and false)", Value::Boolean(false))?;
        
        // Or tests
        self.run_test("(or true true)", Value::Boolean(true))?;
        self.run_test("(or true false)", Value::Boolean(true))?;
        self.run_test("(or false true)", Value::Boolean(true))?;
        self.run_test("(or false false)", Value::Boolean(false))?;
        self.run_test("(or true true true)", Value::Boolean(true))?;
        self.run_test("(or false false true)", Value::Boolean(true))?;
        self.run_test("(or)", Value::Boolean(false))?;
        self.run_test("(or true)", Value::Boolean(true))?;
        self.run_test("(or false)", Value::Boolean(false))?;
        
        // Not tests
        self.run_test("(not true)", Value::Boolean(false))?;
        self.run_test("(not false)", Value::Boolean(true))?;
        self.run_test("(not 1)", Value::Boolean(false))?; // 1 is truthy
        self.run_test("(not 0)", Value::Boolean(false))?; // 0 is truthy
        self.run_test("(not \"\")", Value::Boolean(false))?; // empty string is truthy
        self.run_test("(not nil)", Value::Boolean(true))?; // nil is falsy
        
        println!(" All boolean logic function tests passed!");
        Ok(())
    }

    fn run_string_tests(&mut self) -> Result<(), String> {
        println!("Running string function tests...");
        
        // Str function tests
        self.run_test("(str 42)", Value::String("42".to_string()))?;
        self.run_test("(str 3.14)", Value::String("3.14".to_string()))?;
        self.run_test("(str true)", Value::String("true".to_string()))?;
        self.run_test("(str false)", Value::String("false".to_string()))?;
        self.run_test("(str nil)", Value::String("nil".to_string()))?;
        self.run_test("(str \"hello\")", Value::String("hello".to_string()))?; // String representation without quotes
        self.run_test("(str)", Value::String("".to_string()))?;
        
        // Substring tests
        self.run_test("(substring \"hello world\" 0 5)", Value::String("hello".to_string()))?;
        self.run_test("(substring \"hello world\" 6 11)", Value::String("world".to_string()))?;
        self.run_test("(substring \"hello\" 0 0)", Value::String("".to_string()))?;
        self.run_test("(substring \"hello\" 0 1)", Value::String("h".to_string()))?;
        
        // String length tests
        self.run_test("(string-length \"\")", Value::Integer(0))?;
        self.run_test("(string-length \"hello\")", Value::Integer(5))?;
        self.run_test("(string-length \"hello world\")", Value::Integer(11))?;
        
        // String contains tests
        self.run_test("(string-contains \"hello world\" \"hello\")", Value::Boolean(true))?;
        self.run_test("(string-contains \"hello world\" \"world\")", Value::Boolean(true))?;
        self.run_test("(string-contains \"hello world\" \"xyz\")", Value::Boolean(false))?;
        self.run_test("(string-contains \"hello\" \"\")", Value::Boolean(true))?;
        
        println!(" All string function tests passed!");
        Ok(())
    }

    fn run_collection_tests(&mut self) -> Result<(), String> {
        println!("Running collection function tests...");
        
        // Vector tests
        self.run_test("(vector)", Value::Vector(vec![]))?;
        self.run_test("(vector 1 2 3)", Value::Vector(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]))?;
        self.run_test("(vector \"a\" \"b\" \"c\")", Value::Vector(vec![Value::String("a".to_string()), Value::String("b".to_string()), Value::String("c".to_string())]))?;
        
        // Hash map tests
        self.run_test("(hash-map)", Value::Map(std::collections::HashMap::new()))?;
        
        // Get tests
        self.run_test("(get [1 2 3] 0)", Value::Integer(1))?;
        self.run_test("(get [1 2 3] 1)", Value::Integer(2))?;
        self.run_test("(get [1 2 3] 5)", Value::Nil)?;
        self.run_test("(get [1 2 3] -1)", Value::Nil)?;
        
        // Count tests
        self.run_test("(count [])", Value::Integer(0))?;
        self.run_test("(count [1 2 3])", Value::Integer(3))?;
        self.run_test("(count \"hello\")", Value::Integer(5))?;
        self.run_test("(count {})", Value::Integer(0))?;
        
        // Length tests - separate from count
        self.run_test("(length [])", Value::Integer(0))?;
        self.run_test("(length [1 2 3])", Value::Integer(3))?;
        self.run_test("(length \"hello\")", Value::Integer(5))?;
        self.run_test("(length {})", Value::Integer(0))?;
        
        // First tests
        self.run_test("(first [1 2 3])", Value::Integer(1))?;
        self.run_test("(first [])", Value::Nil)?;
        
        // Rest tests
        self.run_test("(rest [1 2 3])", Value::Vector(vec![Value::Integer(2), Value::Integer(3)]))?;
        self.run_test("(rest [])", Value::Vector(vec![]))?;
        self.run_test("(rest [1])", Value::Vector(vec![]))?;
        
        // Empty tests
        self.run_test("(empty? [])", Value::Boolean(true))?;
        self.run_test("(empty? [1 2 3])", Value::Boolean(false))?;
        self.run_test("(empty? \"\")", Value::Boolean(true))?;
        self.run_test("(empty? \"hello\")", Value::Boolean(false))?;
        self.run_test("(empty? {})", Value::Boolean(true))?;
        
        // Conj tests
        self.run_test("(conj [1 2] 3)", Value::Vector(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]))?;
        self.run_test("(conj [] 1)", Value::Vector(vec![Value::Integer(1)]))?;
        
        // Cons tests
        self.run_test("(cons 1 [2 3])", Value::Vector(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]))?;
        self.run_test("(cons 1 [])", Value::Vector(vec![Value::Integer(1)]))?;
        
        // Range tests
        self.run_test("(range 0 3)", Value::Vector(vec![Value::Integer(0), Value::Integer(1), Value::Integer(2)]))?;
        self.run_test("(range 1 4)", Value::Vector(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]))?;
        self.run_test("(range 5 5)", Value::Vector(vec![]))?;
        
        println!(" All collection function tests passed!");
        Ok(())
    }

    fn run_advanced_collection_tests(&mut self) -> Result<(), String> {
        println!("Running advanced collection function tests...");
        
        // Get-in tests - comprehensive tests
        self.run_test("(get-in {:a {:b {:c 1}}} [:a :b :c])", Value::Integer(1))?;
        self.run_test("(get-in {:a {:b {:c 1}}} [:a :b :d])", Value::Nil)?;
        self.run_test("(get-in {:a {:b {:c 1}}} [:a :b :d] 42)", Value::Integer(42))?;
        self.run_test("(get-in [[1 2] [3 4]] [0 1])", Value::Integer(2))?;
        
        // Partition tests
        self.run_test("(partition 2 [1 2 3 4])", 
            Value::Vector(vec![
                Value::Vector(vec![Value::Integer(1), Value::Integer(2)]),
                Value::Vector(vec![Value::Integer(3), Value::Integer(4)])
            ]))?;
        self.run_test("(partition 3 [1 2 3 4 5 6])",
            Value::Vector(vec![
                Value::Vector(vec![Value::Integer(1), Value::Integer(2), Value::Integer(3)]),
                Value::Vector(vec![Value::Integer(4), Value::Integer(5), Value::Integer(6)])
            ]))?;
        
        // Assoc tests
        let mut expected_map_assoc = std::collections::HashMap::new();
        expected_map_assoc.insert(MapKey::Keyword(Keyword("a".to_string())), Value::Integer(1));
        expected_map_assoc.insert(MapKey::Keyword(Keyword("b".to_string())), Value::Integer(2));
        self.run_test("(assoc {:a 1} :b 2)", Value::Map(expected_map_assoc))?;
        
        self.run_test("(assoc [1 2 3] 1 42)", Value::Vector(vec![Value::Integer(1), Value::Integer(42), Value::Integer(3)]))?;
        
        // Dissoc tests
        let mut expected_map_dissoc = std::collections::HashMap::new();
        expected_map_dissoc.insert(MapKey::Keyword(Keyword("b".to_string())), Value::Integer(2));
        self.run_test("(dissoc {:a 1 :b 2} :a)", Value::Map(expected_map_dissoc))?;
        
        // Type-name tests
        self.run_test("(type-name 42)", Value::String("integer".to_string()))?;
        self.run_test("(type-name 3.14)", Value::String("float".to_string()))?;
        self.run_test("(type-name \"hello\")", Value::String("string".to_string()))?;
        self.run_test("(type-name true)", Value::String("boolean".to_string()))?;
        self.run_test("(type-name nil)", Value::String("nil".to_string()))?;
        self.run_test("(type-name [])", Value::String("vector".to_string()))?;
        self.run_test("(type-name {})", Value::String("map".to_string()))?;
        
        println!(" All advanced collection function tests passed!");
        Ok(())
    }

    fn run_type_predicate_tests(&mut self) -> Result<(), String> {
        println!("Running type predicate function tests...");
        
        // Integer predicate tests
        self.run_test("(int? 42)", Value::Boolean(true))?;
        self.run_test("(int? 0)", Value::Boolean(true))?;
        self.run_test("(int? -1)", Value::Boolean(true))?;
        self.run_test("(int? 3.14)", Value::Boolean(false))?;
        self.run_test("(int? \"42\")", Value::Boolean(false))?;
        self.run_test("(int? true)", Value::Boolean(false))?;
        self.run_test("(int? nil)", Value::Boolean(false))?;
        self.run_test("(int? [])", Value::Boolean(false))?;
        self.run_test("(int? {})", Value::Boolean(false))?;
        
        // Float predicate tests
        self.run_test("(float? 3.14)", Value::Boolean(true))?;
        self.run_test("(float? 0.0)", Value::Boolean(true))?;
        self.run_test("(float? -1.5)", Value::Boolean(true))?;
        self.run_test("(float? 42)", Value::Boolean(false))?;
        self.run_test("(float? \"3.14\")", Value::Boolean(false))?;
        self.run_test("(float? true)", Value::Boolean(false))?;
        self.run_test("(float? nil)", Value::Boolean(false))?;
        
        // Number predicate tests
        self.run_test("(number? 42)", Value::Boolean(true))?;
        self.run_test("(number? 3.14)", Value::Boolean(true))?;
        self.run_test("(number? 0)", Value::Boolean(true))?;
        self.run_test("(number? 0.0)", Value::Boolean(true))?;
        self.run_test("(number? \"42\")", Value::Boolean(false))?;
        self.run_test("(number? true)", Value::Boolean(false))?;
        self.run_test("(number? nil)", Value::Boolean(false))?;
        
        // String predicate tests
        self.run_test("(string? \"hello\")", Value::Boolean(true))?;
        self.run_test("(string? \"\")", Value::Boolean(true))?;
        self.run_test("(string? 42)", Value::Boolean(false))?;
        self.run_test("(string? 3.14)", Value::Boolean(false))?;
        self.run_test("(string? true)", Value::Boolean(false))?;
        self.run_test("(string? nil)", Value::Boolean(false))?;
        
        // String-p predicate tests (alternative name)
        self.run_test("(string-p \"hello\")", Value::Boolean(true))?;
        self.run_test("(string-p \"\")", Value::Boolean(true))?;
        self.run_test("(string-p 42)", Value::Boolean(false))?;
        
        // Boolean predicate tests
        self.run_test("(bool? true)", Value::Boolean(true))?;
        self.run_test("(bool? false)", Value::Boolean(true))?;
        self.run_test("(bool? 42)", Value::Boolean(false))?;
        self.run_test("(bool? \"true\")", Value::Boolean(false))?;
        self.run_test("(bool? nil)", Value::Boolean(false))?;
        
        // Nil predicate tests
        self.run_test("(nil? nil)", Value::Boolean(true))?;
        self.run_test("(nil? 42)", Value::Boolean(false))?;
        self.run_test("(nil? \"hello\")", Value::Boolean(false))?;
        self.run_test("(nil? true)", Value::Boolean(false))?;
        self.run_test("(nil? [])", Value::Boolean(false))?;
        self.run_test("(nil? {})", Value::Boolean(false))?;
        
        // Map predicate tests
        self.run_test("(map? {})", Value::Boolean(true))?;
        self.run_test("(map? {:a 1})", Value::Boolean(true))?;
        self.run_test("(map? [])", Value::Boolean(false))?;
        self.run_test("(map? 42)", Value::Boolean(false))?;
        self.run_test("(map? \"hello\")", Value::Boolean(false))?;
        self.run_test("(map? true)", Value::Boolean(false))?;
        self.run_test("(map? nil)", Value::Boolean(false))?;
        
        // Vector predicate tests
        self.run_test("(vector? [])", Value::Boolean(true))?;
        self.run_test("(vector? [1 2 3])", Value::Boolean(true))?;
        self.run_test("(vector? {})", Value::Boolean(false))?;
        self.run_test("(vector? 42)", Value::Boolean(false))?;
        self.run_test("(vector? \"hello\")", Value::Boolean(false))?;
        self.run_test("(vector? true)", Value::Boolean(false))?;
        self.run_test("(vector? nil)", Value::Boolean(false))?;
        
        // Keyword predicate tests
        self.run_test("(keyword? :hello)", Value::Boolean(true))?;
        self.run_test("(keyword? :a)", Value::Boolean(true))?;
        self.run_test("(keyword? \"hello\")", Value::Boolean(false))?;
        self.run_test("(keyword? 42)", Value::Boolean(false))?;
        self.run_test("(keyword? nil)", Value::Boolean(false))?;
        
        // Symbol predicate tests - in RTFS, function names resolve to functions, not symbols
        // We test symbol? with keyword symbols and literal values
        self.run_test("(symbol? +)", Value::Boolean(false))?; // + is a function, not a symbol
        self.run_test("(symbol? inc)", Value::Boolean(false))?; // inc is a function, not a symbol
        self.run_test("(symbol? :hello)", Value::Boolean(false))?; // keywords are not symbols
        self.run_test("(symbol? \"hello\")", Value::Boolean(false))?;
        self.run_test("(symbol? 42)", Value::Boolean(false))?;
        
        // Function predicate tests
        self.run_test("(fn? +)", Value::Boolean(true))?;
        self.run_test("(fn? inc)", Value::Boolean(true))?;
        self.run_test("(fn? 42)", Value::Boolean(false))?;
        self.run_test("(fn? \"hello\")", Value::Boolean(false))?;
        
        println!(" All type predicate function tests passed!");
        Ok(())
    }
}

#[test]
fn test_arithmetic_functions() {
    let mut runner = SecureStdlibTestRunner::new();
    runner.run_arithmetic_tests().unwrap();
}

#[test]
fn test_comparison_functions() {
    let mut runner = SecureStdlibTestRunner::new();
    runner.run_comparison_tests().unwrap();
}

#[test]
fn test_boolean_functions() {
    let mut runner = SecureStdlibTestRunner::new();
    runner.run_boolean_tests().unwrap();
}

#[test]
fn test_string_functions() {
    let mut runner = SecureStdlibTestRunner::new();
    runner.run_string_tests().unwrap();
}

#[test]
fn test_collection_functions() {
    let mut runner = SecureStdlibTestRunner::new();
    runner.run_collection_tests().unwrap();
}

#[test]
fn test_advanced_collection_functions() {
    let mut runner = SecureStdlibTestRunner::new();
    runner.run_advanced_collection_tests().unwrap();
}

#[test]
fn test_type_predicate_functions() {
    let mut runner = SecureStdlibTestRunner::new();
    runner.run_type_predicate_tests().unwrap();
}

#[test]
fn test_all_secure_stdlib_functions() {
    println!("Running comprehensive secure standard library end-to-end tests...");
    
    let mut runner = SecureStdlibTestRunner::new();
    
    runner.run_arithmetic_tests().unwrap();
    runner.run_comparison_tests().unwrap();
    runner.run_boolean_tests().unwrap();
    runner.run_string_tests().unwrap();
    runner.run_collection_tests().unwrap();
    runner.run_advanced_collection_tests().unwrap();
    runner.run_type_predicate_tests().unwrap();
    
    println!("<� All secure standard library function tests passed!");
}

#[test]
fn test_error_handling() {
    use rtfs_compiler::runtime::error::RuntimeError;
    
    let mut runner = SecureStdlibTestRunner::new();
    
    // Test division by zero
    let ast = rtfs_compiler::parser::parse_expression("(/ 1 0)").unwrap();
    let result = runner.evaluator.evaluate(&ast);
    assert!(result.is_err());
    if let Err(error) = result {
        assert!(matches!(error, RuntimeError::DivisionByZero));
    }
    
    // Test factorial negative input
    let ast = rtfs_compiler::parser::parse_expression("(factorial -1)").unwrap();
    let result = runner.evaluator.evaluate(&ast);
    assert!(result.is_err());
    
    // Test type errors
    let ast = rtfs_compiler::parser::parse_expression("(+ 1 \"hello\")").unwrap();
    let result = runner.evaluator.evaluate(&ast);
    assert!(result.is_err());
    
    println!("✅ Error handling tests passed!");
}
