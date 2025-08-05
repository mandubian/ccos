// // Tests for RTFS 2.0 object definitions
// // Tests the object-oriented features introduced in RTFS 2.0

// use crate::ast::*;
// use crate::parser;

// #[cfg(test)]
// mod object_definitions {
//     use super::*;

//     #[test]
//     fn test_intent_definitions() {
//         let test_cases = vec![
//             r#"(intent my-intent)"#,
//             r#"(intent complex-intent :description "A complex intent" :priority :high)"#,
//             r#"(intent data-intent :type "data-processing" :input-schema [:map [:data string]])"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Intent(_)) = parsed.first() {
//                         // Success
//                     } else {
//                         panic!(
//                             "Expected intent definition for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse intent definition '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_plan_definitions() {
//         let test_cases = vec![
//             r#"(plan my-plan)"#,
//             r#"(plan execution-plan :steps [:vector string] :dependencies ["step1" "step2"])"#,
//             r#"(plan workflow :description "A workflow plan" :timeout 300)"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Plan(_)) = parsed.first() {
//                         // Success
//                     } else {
//                         panic!(
//                             "Expected plan definition for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse plan definition '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_action_definitions() {
//         let test_cases = vec![
//             r#"(action my-action)"#,
//             r#"(action perform-task :handler my-handler :timeout 60)"#,
//             r#"(action http-request :method "GET" :url "https://api.example.com")"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Action(_)) = parsed.first() {
//                         // Success
//                     } else {
//                         panic!(
//                             "Expected action definition for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse action definition '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_capability_definitions() {
//         let test_cases = vec![
//             r#"(capability my-capability)"#,
//             r#"(capability data-processing :input-types [:vector string] :output-type string)"#,
//             r#"(capability ai-model :model-type "llm" :version "1.0")"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Capability(_)) = parsed.first() {
//                         // Success
//                     } else {
//                         panic!(
//                             "Expected capability definition for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse capability definition '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_resource_definitions() {
//         let test_cases = vec![
//             r#"(resource my-resource)"#,
//             r#"(resource database :type "postgresql" :connection-string "postgres://localhost")"#,
//             r#"(resource file-system :path "/data" :permissions "read-write")"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Resource(_)) = parsed.first() {
//                         // Success
//                     } else {
//                         panic!(
//                             "Expected resource definition for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse resource definition '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_resource_references() {
//         let test_cases = vec![
//             r#"(resource:ref "my-resource")"#,
//             r#"(resource:ref "database://localhost:5432/mydb")"#,
//             r#"(resource:ref "file:///path/to/file.txt")"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     // Resource references should parse as expressions
//                     if let Some(TopLevel::Expression(_)) = parsed.first() {
//                         // Success
//                     } else {
//                         panic!(
//                             "Expected expression (resource ref) for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse resource reference '{}': {:?}", input, e),
//             }
//         }
//     }
// }

// #[cfg(test)]
// mod advanced_literals {
//     use super::*;

//     #[test]
//     fn test_timestamps() {
//         let test_cases = vec![
//             "2024-01-15T10:30:45Z",
//             "2024-12-31T23:59:59_999Z",
//             "2024-06-15T14:25:30Z",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(Expression::Literal(_))) = parsed.first() {
//                         // Success - timestamps should parse as literals
//                     } else {
//                         panic!(
//                             "Expected timestamp literal for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse timestamp '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_uuids() {
//         let test_cases = vec![
//             "550e8400-e29b-41d4-a716-446655440000",
//             "6ba7b810-9dad-11d1-80b4-00c04fd430c8",
//             "6ba7b811-9dad-11d1-80b4-00c04fd430c8",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(Expression::Literal(_))) = parsed.first() {
//                         // Success - UUIDs should parse as literals
//                     } else {
//                         panic!("Expected UUID literal for '{}', got: {:?}", input, parsed);
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse UUID '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_resource_handles() {
//         let test_cases = vec![
//             "resource://database/users",
//             "resource://file-system/data.txt",
//             "resource://network/api-endpoint",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(Expression::Literal(_))) = parsed.first() {
//                         // Success - resource handles should parse as literals
//                     } else {
//                         panic!(
//                             "Expected resource handle literal for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse resource handle '{}': {:?}", input, e),
//             }
//         }
//     }
// }

// #[cfg(test)]
// mod module_system {
//     use super::*;

//     #[test]
//     fn test_module_definitions() {
//         let test_cases = vec![
//             r#"(module my.module)"#,
//             r#"(module my.lib (:exports [function1 function2]))"#,
//             r#"(module utils.math (:exports [add subtract multiply divide])
//                  (defn add [x y] (+ x y))
//                  (defn subtract [x y] (- x y)))"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Module(_)) = parsed.first() {
//                         // Success
//                     } else {
//                         panic!(
//                             "Expected module definition for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse module definition '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_import_definitions() {
//         let test_cases = vec![
//             r#"(import std.lib)"#,
//             r#"(import my.utils :as utils)"#,
//             r#"(import external.lib :only [function1 function2])"#,
//             r#"(import complex.module :as cm :only [main-function])"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(_parsed) => {
//                     // Import definitions should parse successfully
//                     // They might be parsed as expressions or top-level definitions
//                     // depending on the AST structure
//                 }
//                 Err(e) => panic!("Failed to parse import definition '{}': {:?}", input, e),
//             }
//         }
//     }
// }



// #[cfg(test)]
// mod error_handling {
//     use super::*;

//     #[test]
//     fn test_try_catch_expressions() {
//         let test_cases = vec![
//             "(try (/ 1 0) (catch ArithmeticError e \"Error occurred\"))",
//             "(try (risky-operation) (catch :any e (log e)) (finally (cleanup)))",
//             "(try (read-file \"test.txt\") (catch FileError e nil) (catch :any e \"Unknown error\"))",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(_)) = parsed.first() {
//                         // Success - try-catch should parse as expressions
//                     } else {
//                         panic!(
//                             "Expected try-catch expression for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse try-catch expression '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_match_expressions() {
//         let test_cases = vec![
//             "(match x 1 \"one\" 2 \"two\" _ \"other\")",
//             "(match value :ok \"success\" :error \"failed\")",
//             "(match [a b] [1 2] \"pair\" [x] \"single\" _ \"other\")",
//             "(match data {:type :user} \"user data\" {:type :admin} \"admin data\" _ \"unknown\")",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(_)) = parsed.first() {
//                         // Success - match should parse as expressions
//                     } else {
//                         panic!(
//                             "Expected match expression for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse match expression '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_with_resource_expressions() {
//         let test_cases = vec![
//             "(with-resource [file string \"test.txt\"] (read-line file))",
//             "(with-resource [db Database (connect \"localhost\")] (query db \"SELECT * FROM users\"))",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(_)) = parsed.first() {
//                         // Success - with-resource should parse as expressions
//                     } else {
//                         panic!(
//                             "Expected with-resource expression for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!(
//                     "Failed to parse with-resource expression '{}': {:?}",
//                     input, e
//                 ),
//             }
//         }
//     }
// }

// #[cfg(test)]
// mod agent_system {
//     use super::*;

//     #[test]
//     fn test_discover_agents_expressions() {
//         let test_cases = vec![
//             "(discover-agents {})",
//             r#"(discover-agents {:capability "data-processing"})"#,
//             r#"(discover-agents {:type "ai-agent" :version ">=1.0"} {:timeout 30})"#,
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(_)) = parsed.first() {
//                         // Success - discover-agents should parse as expressions
//                     } else {
//                         panic!(
//                             "Expected discover-agents expression for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!(
//                     "Failed to parse discover-agents expression '{}': {:?}",
//                     input, e
//                 ),
//             }
//         }
//     }

//     #[test]
//     fn test_log_step_expressions() {
//         let test_cases = vec![
//             "(log-step (+ 1 2 3))",
//             "(log-step :info (process-data data))",
//             "(log-step :debug (str \"Processing: \" item))",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(_)) = parsed.first() {
//                         // Success - log-step should parse as expressions
//                     } else {
//                         panic!(
//                             "Expected log-step expression for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse log-step expression '{}': {:?}", input, e),
//             }
//         }
//     }

//     #[test]
//     fn test_parallel_expressions() {
//         let test_cases = vec![
//             "(parallel [task1 Task (background-task1)] [task2 Task (background-task2)])",
//             "(parallel [result1 int (compute-intensive-1)] [result2 string (fetch-data)])",
//         ];

//         for input in test_cases {
//             let result = parser::parse(input);
//             match result {
//                 Ok(parsed) => {
//                     if let Some(TopLevel::Expression(_)) = parsed.first() {
//                         // Success - parallel should parse as expressions
//                     } else {
//                         panic!(
//                             "Expected parallel expression for '{}', got: {:?}",
//                             input, parsed
//                         );
//                     }
//                 }
//                 Err(e) => panic!("Failed to parse parallel expression '{}': {:?}", input, e),
//             }
//         }
//     }
// }

// #[test]
// fn test_schema_validation() {
//     use crate::ast::{
//         Expression, IntentDefinition, Keyword, Literal, PlanDefinition, Property, Symbol, TopLevel,
//     };
//     use crate::parser;
//     use crate::validator::SchemaValidator;

//     // Test valid intent with simple syntax
//     let valid_intent = r#"(intent my-intent)"#;

//     let parsed = parser::parse(valid_intent).unwrap();
//     if let Some(TopLevel::Intent(_)) = parsed.first() {
//         let result = SchemaValidator::validate_object(&parsed[0]);
//         // This should fail validation because it's missing required fields, but parsing should work
//         assert!(
//             result.is_err(),
//             "Simple intent should fail schema validation due to missing required fields"
//         );
//     } else {
//         panic!("Failed to parse valid intent");
//     }

//     // Test valid plan with simple syntax
//     let valid_plan = r#"(plan my-plan)"#;

//     let parsed = parser::parse(valid_plan).unwrap();
//     if let Some(TopLevel::Plan(_)) = parsed.first() {
//         let result = SchemaValidator::validate_object(&parsed[0]);
//         // This should fail validation because it's missing required fields, but parsing should work
//         assert!(
//             result.is_err(),
//             "Simple plan should fail schema validation due to missing required fields"
//         );
//     } else {
//         panic!("Failed to parse valid plan");
//     }

//     // Test that basic validator::Validate still works
//     let simple_expression = "42";
//     let parsed = parser::parse(simple_expression).unwrap();
//     if let Some(TopLevel::Expression(_)) = parsed.first() {
//         let result = SchemaValidator::validate_object(&parsed[0]);
//         assert!(result.is_ok(), "Simple expression should pass validation");
//     } else {
//         panic!("Failed to parse simple expression");
//     }
// }

// #[test]
// fn test_versioned_type_validation() {
//     use crate::validator::SchemaValidator;

//     // Valid versioned types
//     assert!(SchemaValidator::is_valid_versioned_type(
//         ":rtfs.core:v2.0:intent"
//     ));
//     assert!(SchemaValidator::is_valid_versioned_type(
//         ":my.namespace:v1.5:custom-type"
//     ));
//     assert!(SchemaValidator::is_valid_versioned_type(
//         ":test_package:v3.2.1:resource"
//     ));

//     // Invalid versioned types
//     assert!(!SchemaValidator::is_valid_versioned_type(
//         "rtfs.core:v2.0:intent"
//     )); // Missing leading colon
//     assert!(!SchemaValidator::is_valid_versioned_type(":rtfs.core:v2.0")); // Missing type part
//     assert!(!SchemaValidator::is_valid_versioned_type(
//         ":rtfs.core:2.0:intent"
//     )); // Version should start with v
//     assert!(!SchemaValidator::is_valid_versioned_type(
//         ":rtfs@core:v2.0:intent"
//     )); // Invalid character in namespace
//     assert!(!SchemaValidator::is_valid_versioned_type(
//         ":rtfs.core:v2.0:intent@type"
//     )); // Invalid character in type
// }
