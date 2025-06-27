// Systematic grammar tests for RTFS compiler
// Tests each major grammar rule from rtfs.pest

use crate::parser;
use crate::ast::*;

#[cfg(test)]
mod basic_literals {
    use super::*;

    #[test]
    fn test_integers() {
        let test_cases = vec![
            ("42", Ok(42)),
            ("-42", Ok(-42)),
            ("+42", Ok(42)),
            ("0", Ok(0)),
            ("123456789", Ok(123456789)),
        ];

        for (input, expected) in test_cases {
            let result = parser::parse(input);
            match (result, expected) {
                (Ok(parsed), Ok(expected_val)) => {
                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Integer(val)))) = parsed.first() {
                        assert_eq!(*val, expected_val, "Failed parsing integer: {}", input);
                    } else {
                        panic!("Expected integer literal, got: {:?}", parsed);
                    }
                }
                (Err(e), Ok(_)) => panic!("Failed to parse valid integer '{}': {:?}", input, e),
                _ => panic!("Unexpected result for input: {}", input),
            }
        }
    }

    #[test]
    fn test_floats() {
        let test_cases = vec![
            ("3.14", 3.14),
            ("-3.14", -3.14),
            ("+3.14", 3.14),
            ("0.0", 0.0),
            ("1.23e2", 123.0),
            ("1.23e-2", 0.0123),
            ("1.23E+2", 123.0),
        ];

        for (input, expected) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Float(val)))) = parsed.first() {
                        assert!((val - expected).abs() < f64::EPSILON, 
                               "Float mismatch for '{}': expected {}, got {}", input, expected, val);
                    } else {
                        panic!("Expected float literal, got: {:?}", parsed);
                    }
                }
                Err(e) => panic!("Failed to parse valid float '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_strings() {
        let test_cases = vec![
            (r#""hello""#, "hello"),
            (r#""hello world""#, "hello world"),
            (r#""""#, ""),
            (r#""with \"quotes\"""#, r#"with "quotes""#),
            (r#""with \\backslash""#, r#"with \backslash"#),
            (r#""with \n newline""#, "with \n newline"),
            (r#""with \t tab""#, "with \t tab"),
            (r#""with \r return""#, "with \r return"),
        ];

        for (input, expected) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Literal(Literal::String(val)))) = parsed.first() {
                        assert_eq!(val, expected, "String mismatch for input: {}", input);
                    } else {
                        panic!("Expected string literal, got: {:?}", parsed);
                    }
                }
                Err(e) => panic!("Failed to parse valid string '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_booleans() {
        let test_cases = vec![
            ("true", true),
            ("false", false),
        ];

        for (input, expected) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Boolean(val)))) = parsed.first() {
                        assert_eq!(*val, expected, "Boolean mismatch for input: {}", input);
                    } else {
                        panic!("Expected boolean literal, got: {:?}", parsed);
                    }
                }
                Err(e) => panic!("Failed to parse valid boolean '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_nil() {
        let result = parser::parse("nil");
        match result {
            Ok(parsed) => {
                if let Some(TopLevel::Expression(Expression::Literal(Literal::Nil))) = parsed.first() {
                    // Success
                } else {
                    panic!("Expected nil literal, got: {:?}", parsed);
                }
            }
            Err(e) => panic!("Failed to parse nil: {:?}", e),
        }
    }

    #[test]
    fn test_keywords() {
        let test_cases = vec![
            ":foo",
            ":bar-baz",
            ":my.namespace/keyword",
            ":com.example:v1.0/versioned",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Keyword(_)))) = parsed.first() {
                        // Success - we expect keywords to parse
                    } else {
                        panic!("Expected keyword literal, got: {:?} for input: {}", parsed, input);
                    }
                }
                Err(e) => panic!("Failed to parse valid keyword '{}': {:?}", input, e),
            }
        }
    }
}

#[cfg(test)]
mod symbols_and_identifiers {
    use super::*;

    #[test]
    fn test_simple_symbols() {
        let test_cases = vec![
            "foo",
            "bar-baz",
            "hello_world",
            "test123",
            "+",
            "-",
            "*",
            "/",
            "=",
            "<",
            ">",
            "!",
            "?",
            "symbol?",
            "empty?",
            "nil?",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Symbol(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected symbol, got: {:?} for input: {}", parsed, input);
                    }
                }
                Err(e) => panic!("Failed to parse valid symbol '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_namespaced_symbols() {
        let test_cases = vec![
            "my.namespace/function",
            "std.lib/map",
            "core/reduce",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Symbol(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected namespaced symbol, got: {:?} for input: {}", parsed, input);
                    }
                }
                Err(e) => panic!("Failed to parse valid namespaced symbol '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_versioned_symbols() {
        let test_cases = vec![
            "com.example:v1.0/function",
            "org.lib:v2.1.3/utility",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Symbol(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected versioned symbol, got: {:?} for input: {}", parsed, input);
                    }
                }
                Err(e) => panic!("Failed to parse valid versioned symbol '{}': {:?}", input, e),
            }
        }
    }
}

#[cfg(test)]
mod collections {
    use super::*;

    #[test]
    fn test_empty_collections() {
        let test_cases = vec![
            ("()", "empty list"),
            ("[]", "empty vector"),
            ("{}", "empty map"),
        ];

        for (input, description) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(_)) = parsed.first() {
                        // Success - we expect these to parse
                    } else {
                        panic!("Expected expression for {}, got: {:?}", description, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse {}: {:?}", description, e),
            }
        }
    }

    #[test]
    fn test_simple_collections() {
        let test_cases = vec![
            "(+ 1 2 3)",
            "[1 2 3]",
            r#"{"key" "value" :other 42}"#,
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(_)) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected expression for input '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse collection '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_nested_collections() {
        let test_cases = vec![
            "[[1 2] [3 4]]",
            "[(+ 1 2) (* 3 4)]",
            r#"{"outer" {"inner" "value"}}"#,
            "(list [1 2] {:a 1})",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(_)) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected expression for nested collection '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse nested collection '{}': {:?}", input, e),
            }
        }
    }
}

#[cfg(test)]
mod special_forms {
    use super::*;

    #[test]
    fn test_let_expressions() {
        let test_cases = vec![
            "(let [x 1] x)",
            "(let [x 1 y 2] (+ x y))",
            "(let [] 42)",
            "(let [x (+ 1 2)] (* x 3))",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::LetExpr(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected let expression for '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse let expression '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_if_expressions() {
        let test_cases = vec![
            "(if true 1 2)",
            "(if false 1)",
            "(if (> x 0) x (- x))",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::IfExpr(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected if expression for '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse if expression '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_do_expressions() {
        let test_cases = vec![
            "(do)",
            "(do 1)",
            "(do 1 2 3)",
            "(do (+ 1 2) (* 3 4))",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::DoExpr(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected do expression for '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse do expression '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_function_definitions() {
        let test_cases = vec![
            "(fn [] 42)",
            "(fn [x] x)",
            "(fn [x y] (+ x y))",
            "(fn [x & rest] (cons x rest))",
            "(fn [x:int y:int]:int (+ x y))",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::FnExpr(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected fn expression for '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse fn expression '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_def_expressions() {
        let test_cases = vec![
            "(def x 42)",
            "(def my-var \"hello\")",
            "(def result (+ 1 2 3))",
            "(def x:int 42)",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::DefExpr(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected def expression for '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse def expression '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_defn_expressions() {
        let test_cases = vec![
            "(defn hello [] \"Hello, world!\")",
            "(defn add [x y] (+ x y))",
            "(defn greet [name] (str \"Hello, \" name))",
            "(defn sum [& numbers] (reduce + 0 numbers))",
            "(defn typed-add [x:int y:int]:int (+ x y))",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::DefnExpr(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected defn expression for '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse defn expression '{}': {:?}", input, e),
            }
        }
    }
}

#[cfg(test)]
mod complex_expressions {
    use super::*;

    #[test]
    fn test_nested_function_calls() {
        let test_cases = vec![
            "(+ (- 3 1) (* 2 4))",
            "(map (fn [x] (* x x)) [1 2 3 4])",
            "(reduce + 0 (filter even? [1 2 3 4 5 6]))",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(_)) = parsed.first() {
                        // Success
                    } else {
                        panic!("Expected expression for '{}', got: {:?}", input, parsed);
                    }
                }
                Err(e) => panic!("Failed to parse complex expression '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_mixed_expressions() {
        let test_cases = vec![
            r#"(let [greeting "Hello"] (str greeting ", " "world!"))"#,
            "(if (> (count xs) 0) (first xs) nil)",
            "(do (def x 1) (def y 2) (+ x y))",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(_) => {
                    // Success - just verify it parses
                }
                Err(e) => panic!("Failed to parse mixed expression '{}': {:?}", input, e),
            }
        }
    }
}

#[cfg(test)]
mod edge_cases {
    use super::*;

    #[test]
    fn test_empty_input() {
        let result = parser::parse("");
        match result {
            Ok(parsed) => assert!(parsed.is_empty(), "Expected empty result for empty input"),
            Err(e) => panic!("Empty input should not fail: {:?}", e),
        }
    }

    #[test]
    fn test_whitespace_only() {
        let test_cases = vec![
            "   ",
            "\t\t",
            "\n\n",
            " \t\n\r ",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => assert!(parsed.is_empty(), "Expected empty result for whitespace-only input: '{}'", input),
                Err(e) => panic!("Whitespace-only input '{}' should not fail: {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_comments() {
        let test_cases = vec![
            "; This is a comment",
            "42 ; inline comment",
            "; comment\n42",
            "; comment 1\n; comment 2\n42",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(_) => {
                    // Success - comments should be ignored
                }
                Err(e) => panic!("Failed to parse input with comments '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_malformed_expressions() {
        let test_cases = vec![
            "(",      // Unclosed paren
            ")",      // Extra closing paren
            "[",      // Unclosed bracket
            "]",      // Extra closing bracket
            "{",      // Unclosed brace
            "}",      // Extra closing brace
            "\"",     // Unclosed string
            "42..",   // Invalid float
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(_) => panic!("Expected malformed input '{}' to fail parsing", input),
                Err(_) => {
                    // Success - malformed input should fail
                }
            }
        }
    }
}

#[cfg(test)]
mod type_expressions {
    use super::*;

    #[test]
    fn test_primitive_types() {
        let test_cases = vec![
            "(def x:int 42)",
            "(def y:float 3.14)",
            "(def s:string \"hello\")",
            "(def b:bool true)",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(_) => {
                    // Success - type annotations should parse
                }
                Err(e) => panic!("Failed to parse type annotation in '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_complex_types() {
        let test_cases = vec![
            "(def vec:[:vector int] [1 2 3])",
            "(def mapping:[:map [:key string] [:value int]] {})",
            "(def func:[:=> [int int] int] +)",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(_) => {
                    // Success - complex type annotations should parse
                }
                Err(e) => panic!("Failed to parse complex type annotation in '{}': {:?}", input, e),
            }
        }
    }
}
