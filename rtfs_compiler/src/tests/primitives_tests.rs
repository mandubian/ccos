#[cfg(test)]
mod primitives_tests {
    use crate::{
        ast::{Keyword, TopLevel},
        parser,
        runtime::{RuntimeResult, Value},
    };
    use crate::tests::pure_test_utils::parse_and_evaluate_pure;

    #[test]
    fn test_basic_literals() {
        let test_cases = vec![
            ("42", Value::Integer(42)),
            ("3.14", Value::Float(3.14)),
            ("\"hello\"", Value::String("hello".to_string())),
            ("true", Value::Boolean(true)),
            ("false", Value::Boolean(false)),
            ("nil", Value::Nil),
            (":keyword", Value::Keyword(Keyword("keyword".to_string()))),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    #[test]
    fn test_basic_arithmetic() {
        let test_cases = vec![
            ("(+ 1 2)", Value::Integer(3)),
            ("(- 5 3)", Value::Integer(2)),
            ("(* 4 3)", Value::Integer(12)),
            ("(/ 10 2)", Value::Integer(5)),
            ("(+ 1.5 2.5)", Value::Float(4.0)),
            ("(- 5.5 2.5)", Value::Float(3.0)),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    #[test]
    fn test_basic_comparisons() {
        let test_cases = vec![
            ("(= 1 1)", Value::Boolean(true)),
            ("(= 1 2)", Value::Boolean(false)),
            ("(< 1 2)", Value::Boolean(true)),
            ("(> 2 1)", Value::Boolean(true)),
            ("(<= 1 1)", Value::Boolean(true)),
            ("(>= 2 1)", Value::Boolean(true)),
        ];
        for (input, expected) in test_cases {
            let result = parse_and_evaluate(input);
            assert!(result.is_ok(), "Failed to parse/evaluate: {}", input);
            assert_eq!(result.unwrap(), expected, "Mismatch for: {}", input);
        }
    }

    fn parse_and_evaluate(input: &str) -> RuntimeResult<Value> {
        parse_and_evaluate_pure(input)
    }
}
