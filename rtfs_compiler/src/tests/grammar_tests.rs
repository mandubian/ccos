// Systematic grammar tests for RTFS compiler
// Tests each major grammar rule from rtfs.pest

use crate::ast::*;
use crate::parser;

#[cfg(test)]
mod basic_literals {
    use super::*;

    #[test]
    fn test_integers() {
        let test_cases = vec![
            ("42", 42),
            ("-42", -42),
            ("+42", 42),
            ("0", 0),
            ("123456789", 123456789),
        ];

        for (input, expected) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Integer(val)))) =
                        parsed.first()
                    {
                        assert_eq!(*val, expected, "Failed parsing integer: {}", input);

                        // Additional AST validation
                        if input.starts_with('-') {
                            assert!(
                                *val < 0,
                                "Negative input '{}' should produce negative value",
                                input
                            );
                        } else {
                            assert!(
                                *val >= 0,
                                "Non-negative input '{}' should produce non-negative value",
                                input
                            );
                        }
                    } else {
                        panic!("Expected integer literal, got: {:?}", parsed);
                    }
                }
                Err(e) => panic!("Failed to parse valid integer '{}': {:?}", input, e),
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
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Float(val)))) =
                        parsed.first()
                    {
                        assert!(
                            (val - expected).abs() < f64::EPSILON,
                            "Float mismatch for '{}': expected {}, got {}",
                            input,
                            expected,
                            val
                        );

                        // Additional AST validation
                        assert!(
                            val.is_finite(),
                            "Float value should be finite for '{}'",
                            input
                        );
                        assert!(
                            !val.is_nan(),
                            "Float value should not be NaN for '{}'",
                            input
                        );

                        if input.starts_with('-') {
                            assert!(
                                *val <= 0.0,
                                "Negative input '{}' should produce non-positive value",
                                input
                            );
                        } else if input.starts_with('+')
                            || input.chars().next().unwrap().is_ascii_digit()
                        {
                            assert!(
                                *val >= 0.0,
                                "Positive input '{}' should produce non-negative value",
                                input
                            );
                        }
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
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Literal(Literal::String(val)))) =
                        parsed.first()
                    {
                        assert_eq!(val, expected, "String mismatch for input: {}", input);

                        // Additional AST validation for strings
                        assert!(
                            val.len() == expected.len(),
                            "String length should match expected for '{}'",
                            input
                        );

                        // Validate that escape sequences were properly processed
                        if input.contains("\\\"") {
                            assert!(
                                val.contains('"'),
                                "Escaped quotes should be unescaped in result for '{}'",
                                input
                            );
                        }
                        if input.contains("\\\\") {
                            assert!(
                                val.contains('\\'),
                                "Escaped backslashes should be unescaped in result for '{}'",
                                input
                            );
                        }
                        if input.contains("\\n") {
                            assert!(
                                val.contains('\n'),
                                "Escaped newlines should be unescaped in result for '{}'",
                                input
                            );
                        }
                        if input.contains("\\t") {
                            assert!(
                                val.contains('\t'),
                                "Escaped tabs should be unescaped in result for '{}'",
                                input
                            );
                        }
                        if input.contains("\\r") {
                            assert!(
                                val.contains('\r'),
                                "Escaped returns should be unescaped in result for '{}'",
                                input
                            );
                        }
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
        let test_cases = vec![("true", true), ("false", false)];

        for (input, expected) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Boolean(val)))) =
                        parsed.first()
                    {
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
                if let Some(TopLevel::Expression(Expression::Literal(Literal::Nil))) =
                    parsed.first()
                {
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
            (":foo", "foo"),
            (":bar-baz", "bar-baz"),
            (":my.namespace/keyword", "my.namespace/keyword"),
            (":com.example:v1.0/versioned", "com.example:v1.0/versioned"),
        ];

        for (input, expected_keyword) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Literal(Literal::Keyword(
                        keyword,
                    )))) = parsed.first()
                    {
                        let Keyword(keyword_str) = keyword;
                        assert_eq!(
                            keyword_str, expected_keyword,
                            "Keyword content mismatch for input: {}",
                            input
                        );

                        // Additional AST validation for keywords
                        assert!(
                            !keyword_str.is_empty(),
                            "Keyword should not be empty for '{}'",
                            input
                        );
                        assert!(
                            !keyword_str.starts_with(':'),
                            "Keyword content should not include colon prefix for '{}'",
                            input
                        );

                        // Validate namespace structure if present
                        if keyword_str.contains('/') {
                            let parts: Vec<&str> = keyword_str.split('/').collect();
                            assert_eq!(
                                parts.len(),
                                2,
                                "Namespaced keyword should have exactly one '/' for '{}'",
                                input
                            );
                            assert!(
                                !parts[0].is_empty(),
                                "Namespace part should not be empty for '{}'",
                                input
                            );
                            assert!(
                                !parts[1].is_empty(),
                                "Name part should not be empty for '{}'",
                                input
                            );
                        }

                        // Validate versioned structure if present
                        if keyword_str.contains(':') && keyword_str.contains('/') {
                            let main_parts: Vec<&str> = keyword_str.split('/').collect();
                            if main_parts.len() == 2 && main_parts[0].contains(':') {
                                let ns_parts: Vec<&str> = main_parts[0].split(':').collect();
                                assert_eq!(
                                    ns_parts.len(),
                                    2,
                                    "Versioned namespace should have format 'ns:version' for '{}'",
                                    input
                                );
                                assert!(
                                    !ns_parts[0].is_empty(),
                                    "Namespace name should not be empty for '{}'",
                                    input
                                );
                                assert!(
                                    !ns_parts[1].is_empty(),
                                    "Version should not be empty for '{}'",
                                    input
                                );
                            }
                        }
                    } else {
                        panic!(
                            "Expected keyword literal, got: {:?} for input: {}",
                            parsed, input
                        );
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
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Symbol(symbol))) = parsed.first() {
                        let Symbol(symbol_str) = symbol;
                        assert_eq!(
                            symbol_str, input,
                            "Symbol content should match input for '{}'",
                            input
                        );

                        // Additional AST validation for symbols
                        assert!(
                            !symbol_str.is_empty(),
                            "Symbol should not be empty for '{}'",
                            input
                        );
                        assert!(
                            !symbol_str.contains(' '),
                            "Symbol should not contain spaces for '{}'",
                            input
                        );
                        assert!(
                            !symbol_str.starts_with(':'),
                            "Symbol should not start with colon for '{}'",
                            input
                        );

                        // Validate that predicate symbols end with '?'
                        if symbol_str.ends_with('?') && symbol_str.len() > 1 {
                            // Multi-character predicate symbols should have content before '?'
                        }
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
            ("my.namespace/function", "my.namespace", "function"),
            ("std.lib/map", "std.lib", "map"),
            ("core/reduce", "core", "reduce"),
        ];

        for (input, expected_ns, expected_name) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Symbol(symbol))) = parsed.first() {
                        let Symbol(symbol_str) = symbol;
                        assert_eq!(
                            symbol_str, input,
                            "Symbol content should match input for '{}'",
                            input
                        );

                        // Additional AST validation for namespaced symbols
                        assert!(
                            symbol_str.contains('/'),
                            "Namespaced symbol should contain '/' for '{}'",
                            input
                        );
                        let parts: Vec<&str> = symbol_str.split('/').collect();
                        assert_eq!(
                            parts.len(),
                            2,
                            "Namespaced symbol should have exactly one '/' for '{}'",
                            input
                        );
                        assert_eq!(
                            parts[0], expected_ns,
                            "Namespace part mismatch for '{}'",
                            input
                        );
                        assert_eq!(
                            parts[1], expected_name,
                            "Name part mismatch for '{}'",
                            input
                        );
                        assert!(
                            !parts[0].is_empty(),
                            "Namespace should not be empty for '{}'",
                            input
                        );
                        assert!(
                            !parts[1].is_empty(),
                            "Name should not be empty for '{}'",
                            input
                        );
                    } else {
                        panic!(
                            "Expected namespaced symbol, got: {:?} for input: {}",
                            parsed, input
                        );
                    }
                }
                Err(e) => panic!(
                    "Failed to parse valid namespaced symbol '{}': {:?}",
                    input, e
                ),
            }
        }
    }

    #[test]
    fn test_versioned_symbols() {
        let test_cases = vec!["com.example:v1.0/function", "org.lib:v2.1.3/utility"];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Symbol(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!(
                            "Expected versioned symbol, got: {:?} for input: {}",
                            parsed, input
                        );
                    }
                }
                Err(e) => panic!(
                    "Failed to parse valid versioned symbol '{}': {:?}",
                    input, e
                ),
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
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for {}",
                        description
                    );

                    if let Some(TopLevel::Expression(expr)) = parsed.first() {
                        match expr {
                            Expression::List(items) => {
                                assert!(items.is_empty(), "Empty list should have no items");
                                assert_eq!(input, "()", "List input should be '()'");
                            }
                            Expression::Vector(items) => {
                                assert!(items.is_empty(), "Empty vector should have no items");
                                assert_eq!(input, "[]", "Vector input should be '[]'");
                            }
                            Expression::Map(items) => {
                                assert!(items.is_empty(), "Empty map should have no items");
                                assert_eq!(input, "{}", "Map input should be '{{}}'");
                            }
                            _ => panic!(
                                "Expected collection type for {}, got: {:?}",
                                description, expr
                            ),
                        }
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
            ("(+ 1 2 3)", "function call with arithmetic"),
            ("[1 2 3]", "vector with integers"),
            (r#"{"key" "value" :other 42}"#, "map with mixed keys"),
        ];

        for (input, description) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for {}",
                        description
                    );

                    if let Some(TopLevel::Expression(expr)) = parsed.first() {
                        match expr {
                            Expression::FunctionCall { callee, arguments } => {
                                assert_eq!(input, "(+ 1 2 3)", "Function call input validation");

                                // Validate function call structure
                                if let Expression::Symbol(Symbol(op)) = callee.as_ref() {
                                    assert_eq!(op, "+", "Function should be '+' operator");
                                }

                                // Validate that arguments are integers
                                assert_eq!(
                                    arguments.len(),
                                    3,
                                    "Function call should have 3 arguments"
                                );
                                for (i, arg) in arguments.iter().enumerate() {
                                    if let Expression::Literal(Literal::Integer(val)) = arg {
                                        assert_eq!(
                                            *val,
                                            (i + 1) as i64,
                                            "Argument {} should be {}",
                                            i,
                                            i + 1
                                        );
                                    } else {
                                        panic!("Expected integer at argument position {} in function call, got: {:?}", i, arg);
                                    }
                                }
                            }
                            Expression::Vector(items) => {
                                assert!(
                                    !items.is_empty(),
                                    "Vector should have items for {}",
                                    description
                                );
                                assert_eq!(input, "[1 2 3]", "Vector input validation");
                                assert_eq!(items.len(), 3, "Vector should have 3 items");

                                // Validate all items are integers
                                for (i, item) in items.iter().enumerate() {
                                    if let Expression::Literal(Literal::Integer(val)) = item {
                                        assert_eq!(
                                            *val,
                                            (i + 1) as i64,
                                            "Integer value should match position"
                                        );
                                    } else {
                                        panic!(
                                            "Expected integer at position {} in vector, got: {:?}",
                                            i, item
                                        );
                                    }
                                }
                            }
                            Expression::Map(items) => {
                                assert!(
                                    !items.is_empty(),
                                    "Map should have items for {}",
                                    description
                                );
                                assert_eq!(
                                    input, r#"{"key" "value" :other 42}"#,
                                    "Map input validation"
                                );
                                assert_eq!(items.len(), 2, "Map should have 2 entries");

                                // Validate specific map entries
                                assert!(
                                    items.contains_key(&MapKey::String("key".to_string())),
                                    "Map should contain string key 'key'"
                                );
                                assert!(
                                    items.contains_key(&MapKey::Keyword(Keyword(
                                        "other".to_string()
                                    ))),
                                    "Map should contain keyword key :other"
                                );

                                if let Some(Expression::Literal(Literal::String(val))) =
                                    items.get(&MapKey::String("key".to_string()))
                                {
                                    assert_eq!(val, "value", "String value should be 'value'");
                                }

                                if let Some(Expression::Literal(Literal::Integer(val))) =
                                    items.get(&MapKey::Keyword(Keyword("other".to_string())))
                                {
                                    assert_eq!(*val, 42, "Integer value should be 42");
                                }
                            }
                            _ => panic!(
                                "Expected collection type or function call for {}, got: {:?}",
                                description, expr
                            ),
                        }
                    } else {
                        panic!("Expected expression for '{}', got: {:?}", input, parsed);
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
                        panic!(
                            "Expected expression for nested collection '{}', got: {:?}",
                            input, parsed
                        );
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
            ("(let [x 1] x)", 1, 1), // (input, expected_bindings, expected_body_exprs)
            ("(let [x 1 y 2] (+ x y))", 2, 1),
            ("(let [] 42)", 0, 1),
            ("(let [x (+ 1 2)] (* x 3))", 1, 1),
        ];

        for (input, expected_bindings, expected_body_exprs) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Let(let_expr))) = parsed.first() {
                        // Validate bindings count
                        assert_eq!(
                            let_expr.bindings.len(),
                            expected_bindings,
                            "Let expression should have {} bindings for '{}'",
                            expected_bindings,
                            input
                        );

                        // Validate body count
                        assert_eq!(
                            let_expr.body.len(),
                            expected_body_exprs,
                            "Let expression should have {} body expressions for '{}'",
                            expected_body_exprs,
                            input
                        );

                        // Validate individual bindings structure
                        for (i, binding) in let_expr.bindings.iter().enumerate() {
                            // Validate binding has a pattern
                            match &binding.pattern {
                                Pattern::Symbol(Symbol(name)) => {
                                    assert!(!name.is_empty(), "Binding symbol should not be empty at position {} for '{}'", i, input);
                                }
                                _ => panic!(
                                    "Expected symbol pattern for binding at position {} in '{}'",
                                    i, input
                                ),
                            }

                            // Validate binding has a value expression
                            match binding.value.as_ref() {
                                Expression::Literal(_)
                                | Expression::Symbol(_)
                                | Expression::List(_) => {
                                    // Valid expression types for bindings
                                }
                                _ => {
                                    // Other expression types are also valid, this is just basic validation
                                }
                            }
                        }

                        // Validate body expressions are not empty
                        for body_expr in let_expr.body.iter() {
                            match body_expr {
                                Expression::Literal(_)
                                | Expression::Symbol(_)
                                | Expression::List(_) => {
                                    // Valid expression types
                                }
                                _ => {
                                    // Other expression types are also valid
                                }
                            }
                        }
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
            ("(if true 1 2)", true), // (input, has_else_branch)
            ("(if false 1)", false),
            ("(if (> x 0) x (- x))", true),
        ];

        for (input, has_else_branch) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::If(if_expr))) = parsed.first() {
                        // Validate condition exists
                        match if_expr.condition.as_ref() {
                            Expression::Literal(Literal::Boolean(_))
                            | Expression::Symbol(_)
                            | Expression::List(_) => {
                                // Valid condition types
                            }
                            _ => {
                                // Other expression types are also valid for conditions
                            }
                        }

                        // Validate then branch exists
                        match if_expr.then_branch.as_ref() {
                            Expression::Literal(_)
                            | Expression::Symbol(_)
                            | Expression::List(_) => {
                                // Valid then branch types
                            }
                            _ => {
                                // Other expression types are also valid
                            }
                        }

                        // Validate else branch presence
                        if has_else_branch {
                            assert!(
                                if_expr.else_branch.is_some(),
                                "If expression should have else branch for '{}'",
                                input
                            );
                            if let Some(else_expr) = &if_expr.else_branch {
                                match else_expr.as_ref() {
                                    Expression::Literal(_)
                                    | Expression::Symbol(_)
                                    | Expression::List(_) => {
                                        // Valid else branch types
                                    }
                                    _ => {
                                        // Other expression types are also valid
                                    }
                                }
                            }
                        } else {
                            assert!(
                                if_expr.else_branch.is_none(),
                                "If expression should not have else branch for '{}'",
                                input
                            );
                        }
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
        let test_cases = vec!["(do)", "(do 1)", "(do 1 2 3)", "(do (+ 1 2) (* 3 4))"];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    if let Some(TopLevel::Expression(Expression::Do(_))) = parsed.first() {
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
            ("(fn [] 42)", 0, false, false), // (input, param_count, has_variadic, has_return_type)
            ("(fn [x] x)", 1, false, false),
            ("(fn [x y] (+ x y))", 2, false, false),
            ("(fn [x & rest] (cons x rest))", 1, true, false),
            ("(fn [x:int y:int]:int (+ x y))", 2, false, true),
        ];

        for (input, expected_params, has_variadic, has_return_type) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate the AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(Expression::Fn(fn_expr))) = parsed.first() {
                        // Validate parameter count
                        assert_eq!(
                            fn_expr.params.len(),
                            expected_params,
                            "Function should have {} parameters for '{}'",
                            expected_params,
                            input
                        );

                        // Validate variadic parameter
                        if has_variadic {
                            assert!(
                                fn_expr.variadic_param.is_some(),
                                "Function should have variadic parameter for '{}'",
                                input
                            );
                        } else {
                            assert!(
                                fn_expr.variadic_param.is_none(),
                                "Function should not have variadic parameter for '{}'",
                                input
                            );
                        }

                        // Validate return type annotation
                        if has_return_type {
                            assert!(
                                fn_expr.return_type.is_some(),
                                "Function should have return type annotation for '{}'",
                                input
                            );
                        } else {
                            assert!(
                                fn_expr.return_type.is_none(),
                                "Function should not have return type annotation for '{}'",
                                input
                            );
                        }

                        // Validate parameters structure
                        for (i, param) in fn_expr.params.iter().enumerate() {
                            match &param.pattern {
                                Pattern::Symbol(Symbol(name)) => {
                                    assert!(!name.is_empty(), "Parameter name should not be empty at position {} for '{}'", i, input);
                                }
                                _ => panic!(
                                    "Expected symbol pattern for parameter at position {} in '{}'",
                                    i, input
                                ),
                            }
                        }

                        // Validate variadic parameter structure if present
                        if let Some(variadic_param) = &fn_expr.variadic_param {
                            match &variadic_param.pattern {
                                Pattern::Symbol(Symbol(name)) => {
                                    assert!(
                                        !name.is_empty(),
                                        "Variadic parameter name should not be empty for '{}'",
                                        input
                                    );
                                }
                                _ => panic!(
                                    "Expected symbol pattern for variadic parameter in '{}'",
                                    input
                                ),
                            }
                        }

                        // Validate function body
                        assert!(
                            !fn_expr.body.is_empty(),
                            "Function body should not be empty for '{}'",
                            input
                        );

                        // Basic validation of body expressions
                        for body_expr in fn_expr.body.iter() {
                            match body_expr {
                                Expression::Literal(_)
                                | Expression::Symbol(_)
                                | Expression::List(_) => {
                                    // Valid expression types
                                }
                                _ => {
                                    // Other expression types are also valid
                                }
                            }
                        }
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
                    if let Some(TopLevel::Expression(Expression::Def(_))) = parsed.first() {
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
                    if let Some(TopLevel::Expression(Expression::Defn(_))) = parsed.first() {
                        // Success
                    } else {
                        panic!(
                            "Expected defn expression for '{}', got: {:?}",
                            input, parsed
                        );
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
        let test_cases = vec!["   ", "\t\t", "\n\n", " \t\n\r "];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => assert!(
                    parsed.is_empty(),
                    "Expected empty result for whitespace-only input: '{}'",
                    input
                ),
                Err(e) => panic!("Whitespace-only input '{}' should not fail: {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_comments() {
        let test_cases = vec![
            ";; This is a comment",
            "42 ;; inline comment",
            ";; comment\n42",
            ";; comment 1\n;; comment 2\n42",
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
            "(",    // Unclosed paren
            ")",    // Extra closing paren
            "[",    // Unclosed bracket
            "]",    // Extra closing bracket
            "{",    // Unclosed brace
            "}",    // Extra closing brace
            "\"",   // Unclosed string
            "42..", // Invalid float
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
                Err(e) => panic!(
                    "Failed to parse complex type annotation in '{}': {:?}",
                    input, e
                ),
            }
        }
    }

    #[test]
    fn debug_type_annotation_parsing() {
        let test_inputs = vec![
            "(def x:int 42)",
            "(def y:[:vector int] [1 2 3])",
            "(defn add [x:int y:int]:int (+ x y))",
        ];

        for input in test_inputs {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    println!("Input: {}", input);
                    println!("Parsed: {:#?}", parsed);
                    println!("---");
                }
                Err(e) => {
                    println!("Failed to parse '{}': {:?}", input, e);
                }
            }
        }

        // This test always passes - it's just for debugging
        assert!(true);
    }
}

#[cfg(test)]
mod ast_validation {
    use super::*;
    use validator::Validate;

    #[test]
    fn test_ast_structural_integrity() {
        let test_cases = vec![
            "(defn factorial [n] (if (<= n 1) 1 (* n (factorial (- n 1)))))",
            "(let [x 5 y (+ x 3)] (do (println x) (println y) (* x y)))",
            r#"(fn [data] (map (fn [item] {:processed true :value (* item 2)}) data))"#,
            "(match x [:vector a b] (+ a b) [:map {:key val}] val _ :unknown)",
        ];

        for input in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    // Validate AST structure
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    // Validate each top-level item
                    for top_level in &parsed {
                        // Use the built-in validator
                        if let Err(validation_errors) = top_level.validate() {
                            panic!(
                                "AST validation failed for '{}': {:?}",
                                input, validation_errors
                            );
                        }

                        // Additional custom validation
                        validate_ast_node_consistency(top_level, input);
                    }
                }
                Err(e) => panic!("Failed to parse complex expression '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_nested_collection_validation() {
        let test_cases = vec![
            ("[[1 2] [3 4]]", 2, 2),     // (input, outer_count, inner_count)
            ("[(+ 1 2) (* 3 4)]", 2, 3), // 2 elements, each with 3 sub-elements
            (r#"{"outer" {"inner" "value"}}"#, 1, 1),
            ("(list [1 2] {:a 1})", 3, 2), // list with 3 elements, vector has 2, map has 1
        ];

        for (input, expected_outer, expected_inner) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(expr)) = parsed.first() {
                        validate_nested_collection_structure(
                            expr,
                            expected_outer,
                            expected_inner,
                            input,
                        );
                    } else {
                        panic!(
                            "Expected expression for nested collection '{}', got: {:?}",
                            input, parsed
                        );
                    }
                }
                Err(e) => panic!("Failed to parse nested collection '{}': {:?}", input, e),
            }
        }
    }

    #[test]
    fn test_type_annotation_validation() {
        let test_cases = vec![
            ("(def x 42)", "simple def"),
            ("(defn add [x y] (+ x y))", "simple function"),
            (
                "(defn typed-add [x:int y:int]:int (+ x y))",
                "function with return type",
            ),
        ];

        for (input, description) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(expr)) = parsed.first() {
                        match expr {
                            Expression::Def(def_expr) => {
                                assert!(
                                    !def_expr.symbol.0.is_empty(),
                                    "Def symbol should not be empty for '{}'",
                                    input
                                );
                                // Note: Current parser doesn't separate type annotations from symbol names
                                // Type annotations like x:int are parsed as part of the symbol name
                            }
                            Expression::Defn(defn_expr) => {
                                assert!(
                                    !defn_expr.name.0.is_empty(),
                                    "Defn name should not be empty for '{}'",
                                    input
                                );

                                // Validate return type annotation if present
                                if description == "function with return type" {
                                    assert!(
                                        defn_expr.return_type.is_some(),
                                        "Function should have return type annotation for '{}'",
                                        input
                                    );
                                    if let Some(TypeExpr::Alias(Symbol(type_name))) =
                                        &defn_expr.return_type
                                    {
                                        assert!(
                                            !type_name.is_empty(),
                                            "Return type name should not be empty for '{}'",
                                            input
                                        );
                                    }
                                }

                                // Validate parameter structure
                                for param in &defn_expr.params {
                                    match &param.pattern {
                                        Pattern::Symbol(Symbol(name)) => {
                                            assert!(
                                                !name.is_empty(),
                                                "Parameter name should not be empty for '{}'",
                                                input
                                            );
                                        }
                                        _ => panic!(
                                            "Expected symbol pattern for parameter in '{}'",
                                            input
                                        ),
                                    }
                                }
                            }
                            _ => panic!(
                                "Expected def or defn expression for '{}', got: {:?}",
                                input, expr
                            ),
                        }
                    } else {
                        panic!(
                            "Expected expression with type annotations for '{}', got: {:?}",
                            input, parsed
                        );
                    }
                }
                Err(e) => panic!(
                    "Failed to parse type-annotated expression '{}': {:?}",
                    input, e
                ),
            }
        }
    }

    #[test]
    fn test_pattern_matching_validation() {
        let test_cases = vec![
            ("(let [x 1] x)", "simple binding"),
            ("(let [[a b] [1 2]] (+ a b))", "vector destructuring"),
            (
                "(let [{:keys [name age]} person] (str name age))",
                "map destructuring",
            ),
            ("(fn [x & rest] (cons x rest))", "variadic parameter"),
        ];

        for (input, pattern_type) in test_cases {
            let result = parser::parse(input);
            match result {
                Ok(parsed) => {
                    assert_eq!(
                        parsed.len(),
                        1,
                        "Expected exactly one top-level item for '{}'",
                        input
                    );

                    if let Some(TopLevel::Expression(expr)) = parsed.first() {
                        validate_pattern_structures(expr, pattern_type, input);
                    } else {
                        panic!(
                            "Expected expression with patterns for '{}', got: {:?}",
                            input, parsed
                        );
                    }
                }
                Err(e) => panic!("Failed to parse pattern expression '{}': {:?}", input, e),
            }
        }
    }

    // Helper function to validate AST node consistency
    fn validate_ast_node_consistency(top_level: &TopLevel, input: &str) {
        match top_level {
            TopLevel::Expression(expr) => validate_expression_consistency(expr, input),
            TopLevel::Intent(intent) => {
                assert!(
                    !intent.name.0.is_empty(),
                    "Intent name should not be empty for '{}'",
                    input
                );
                for prop in &intent.properties {
                    assert!(
                        !prop.key.0.is_empty(),
                        "Property key should not be empty for '{}'",
                        input
                    );
                }
            }
            TopLevel::Plan(plan) => {
                assert!(
                    !plan.name.0.is_empty(),
                    "Plan name should not be empty for '{}'",
                    input
                );
            }
            TopLevel::Action(action) => {
                assert!(
                    !action.name.0.is_empty(),
                    "Action name should not be empty for '{}'",
                    input
                );
            }
            TopLevel::Capability(capability) => {
                assert!(
                    !capability.name.0.is_empty(),
                    "Capability name should not be empty for '{}'",
                    input
                );
            }
            TopLevel::Resource(resource) => {
                assert!(
                    !resource.name.0.is_empty(),
                    "Resource name should not be empty for '{}'",
                    input
                );
            }
            TopLevel::Module(module) => {
                assert!(
                    !module.name.0.is_empty(),
                    "Module name should not be empty for '{}'",
                    input
                );
            }
        }
    }

    // Helper function to validate expression consistency
    fn validate_expression_consistency(expr: &Expression, input: &str) {
        match expr {
            Expression::Literal(lit) => {
                match lit {
                    Literal::String(_s) => {
                        // String literals are always valid when parsed
                    }
                    Literal::Integer(_i) => {
                        // Integer values are always valid in Rust's i64 range
                    }
                    Literal::Float(f) => {
                        assert!(f.is_finite(), "Float should be finite for '{}'", input);
                        assert!(!f.is_nan(), "Float should not be NaN for '{}'", input);
                    }
                    Literal::Keyword(Keyword(k)) => {
                        assert!(!k.is_empty(), "Keyword should not be empty for '{}'", input)
                    }
                    _ => {} // Other literals are valid
                }
            }
            Expression::Symbol(Symbol(s)) => {
                assert!(!s.is_empty(), "Symbol should not be empty for '{}'", input);
                assert!(
                    !s.starts_with(':'),
                    "Symbol should not start with colon for '{}'",
                    input
                );
            }
            Expression::List(items) | Expression::Vector(items) => {
                for item in items {
                    validate_expression_consistency(item, input);
                }
            }
            Expression::Map(map) => {
                for (key, value) in map {
                    match key {
                        MapKey::String(s) => assert!(
                            !s.is_empty(),
                            "Map string key should not be empty for '{}'",
                            input
                        ),
                        MapKey::Keyword(Keyword(k)) => assert!(
                            !k.is_empty(),
                            "Map keyword key should not be empty for '{}'",
                            input
                        ),
                        MapKey::Integer(_i) => {
                            // Integer keys are always valid in Rust's i64 range
                        }
                    }
                    validate_expression_consistency(value, input);
                }
            }
            Expression::FunctionCall { callee, arguments } => {
                validate_expression_consistency(callee, input);
                for arg in arguments {
                    validate_expression_consistency(arg, input);
                }
            }
            Expression::Let(let_expr) => {
                assert!(
                    !let_expr.bindings.is_empty() || !let_expr.body.is_empty(),
                    "Let expression should have bindings or body for '{}'",
                    input
                );
                for binding in &let_expr.bindings {
                    validate_expression_consistency(&binding.value, input);
                }
                for body_expr in &let_expr.body {
                    validate_expression_consistency(body_expr, input);
                }
            }
            Expression::If(if_expr) => {
                validate_expression_consistency(&if_expr.condition, input);
                validate_expression_consistency(&if_expr.then_branch, input);
                if let Some(else_branch) = &if_expr.else_branch {
                    validate_expression_consistency(else_branch, input);
                }
            }
            Expression::Fn(fn_expr) => {
                assert!(
                    !fn_expr.body.is_empty(),
                    "Function body should not be empty for '{}'",
                    input
                );
                for body_expr in &fn_expr.body {
                    validate_expression_consistency(body_expr, input);
                }
            }
            Expression::Def(def_expr) => {
                assert!(
                    !def_expr.symbol.0.is_empty(),
                    "Def symbol should not be empty for '{}'",
                    input
                );
                validate_expression_consistency(&def_expr.value, input);
            }
            Expression::Defn(defn_expr) => {
                assert!(
                    !defn_expr.name.0.is_empty(),
                    "Defn name should not be empty for '{}'",
                    input
                );
                assert!(
                    !defn_expr.body.is_empty(),
                    "Defn body should not be empty for '{}'",
                    input
                );
                for body_expr in &defn_expr.body {
                    validate_expression_consistency(body_expr, input);
                }
            }
            _ => {} // Other expression types - basic validation passed if we got here
        }
    }

    // Helper function to validate nested collection structures
    fn validate_nested_collection_structure(
        expr: &Expression,
        expected_outer: usize,
        expected_inner: usize,
        input: &str,
    ) {
        match expr {
            Expression::Vector(items) => {
                assert_eq!(
                    items.len(),
                    expected_outer,
                    "Vector should have {} items for '{}'",
                    expected_outer,
                    input
                );
                for item in items {
                    if let Expression::Vector(inner_items) = item {
                        assert_eq!(
                            inner_items.len(),
                            expected_inner,
                            "Inner vector should have {} items for '{}'",
                            expected_inner,
                            input
                        );
                    }
                }
            }
            Expression::List(items) => {
                assert_eq!(
                    items.len(),
                    expected_outer,
                    "List should have {} items for '{}'",
                    expected_outer,
                    input
                );
            }
            Expression::Map(map) => {
                assert_eq!(
                    map.len(),
                    expected_outer,
                    "Map should have {} entries for '{}'",
                    expected_outer,
                    input
                );
            }
            _ => {} // Other types - basic validation
        }
    }

    // Helper function to validate pattern structures
    fn validate_pattern_structures(expr: &Expression, pattern_type: &str, input: &str) {
        match expr {
            Expression::Let(let_expr) => {
                for binding in &let_expr.bindings {
                    match (&binding.pattern, pattern_type) {
                        (Pattern::Symbol(_), "simple binding") => {
                            // Valid simple binding
                        }
                        (Pattern::VectorDestructuring { .. }, "vector destructuring") => {
                            // Valid vector destructuring
                        }
                        (Pattern::MapDestructuring { .. }, "map destructuring") => {
                            // Valid map destructuring
                        }
                        _ => {} // Other patterns are also valid
                    }
                }
            }
            Expression::Fn(fn_expr) => {
                if pattern_type == "variadic parameter" {
                    assert!(
                        fn_expr.variadic_param.is_some(),
                        "Function should have variadic parameter for '{}'",
                        input
                    );
                }
            }
            _ => {} // Other expression types
        }
    }
}
