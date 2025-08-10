#[cfg(test)]
mod llm_execute_tests {
    use crate::parser;
    use crate::runtime::values::Value;

    #[test]
    fn llm_execute_echo_positional() {
        let code = r#"(llm-execute "echo-model" "Hello")"#;
        let parsed = parser::parse(code).expect("parse");
        let mut last = Value::Nil;
        let evaluator = crate::tests::test_utils::create_llm_test_evaluator();
        for form in parsed {
            if let crate::ast::TopLevel::Expression(expr) = form {
                last = evaluator.evaluate(&expr).expect("eval");
            }
        }
        match last { Value::String(s) => assert!(s.contains("Hello")), _ => panic!("unexpected") }
    }

    #[test]
    fn llm_execute_echo_keyword() {
        let code = r#"(llm-execute :model "echo-model" :prompt "Ping" :system "SYS")"#;
        let parsed = parser::parse(code).expect("parse");
        let mut last = Value::Nil;
        let evaluator = crate::tests::test_utils::create_llm_test_evaluator();
        for form in parsed {
            if let crate::ast::TopLevel::Expression(expr) = form {
                last = evaluator.evaluate(&expr).expect("eval");
            }
        }
        match last { Value::String(s) => assert!(s.contains("Ping")), _ => panic!("unexpected") }
    }

    #[test]
    fn llm_execute_denied_in_pure() {
        let code = r#"(llm-execute "echo-model" "Hello")"#;
        let parsed = parser::parse(code).expect("parse");
        let mut last_err: Option<String> = None;
        // pure context via helper
        let evaluator = crate::tests::test_utils::create_test_evaluator();
        for form in parsed {
            if let crate::ast::TopLevel::Expression(expr) = form {
                last_err = Some(format!("{}", evaluator.evaluate(&expr).unwrap_err()));
            }
        }
        let msg = last_err.expect("expected error");
        assert!(msg.contains("Security violation") || msg.contains("SecurityViolation"));
    }
}
