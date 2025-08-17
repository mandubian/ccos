use rtfs_compiler::ast::{TypeExpr, PrimitiveType, TypePredicate, Literal, Keyword, MapTypeEntry, MapKey};
use rtfs_compiler::runtime::{Value, RuntimeResult, RuntimeError};
use rtfs_compiler::runtime::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::capability_registry::CapabilityRegistry;
use std::collections::HashMap; // HashMap for capability input param assembly
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_capability_registration_with_type_validation() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    
    // Define a simple calculator capability with type constraints
    let add_handler = Arc::new(|input: &Value| -> RuntimeResult<Value> {
        if let Value::Map(map) = input {
            let a_key = MapKey::Keyword(Keyword::new("a"));
            let b_key = MapKey::Keyword(Keyword::new("b"));
            
            if let (Some(Value::Integer(a)), Some(Value::Integer(b))) = 
                (map.get(&a_key), map.get(&b_key)) {
                Ok(Value::Integer(a + b))
            } else {
                Err(RuntimeError::new("Missing or invalid a/b parameters"))
            }
        } else {
            Err(RuntimeError::new("Input must be a map"))
        }
    });
    
    // Define input schema: [:map [:a :int] [:b :int]]
    let input_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("a"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("b"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
        ],
        wildcard: None,
    };
    
    // Define output schema: :int
    let output_schema = TypeExpr::Primitive(PrimitiveType::Int);
    
    // Register the capability with type validation
    let result = marketplace.register_local_capability_with_schema(
        "math.add".to_string(),
        "Add Two Numbers".to_string(),
        "Adds two integers together".to_string(),
        add_handler,
        Some(input_schema),
        Some(output_schema),
    ).await;
    
    assert!(result.is_ok());
    
    // Test that the capability was registered
    let capability = marketplace.get_capability("math.add").await;
    assert!(capability.is_some());

    let capability = capability.unwrap();
    assert_eq!(capability.name, "Add Two Numbers");
    assert!(capability.input_schema.is_some());
    assert!(capability.output_schema.is_some());
}

#[tokio::test]
async fn test_capability_execution_with_valid_input() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    
    // Register a simple echo capability
    let echo_handler = Arc::new(|input: &Value| -> RuntimeResult<Value> {
        Ok(input.clone())
    });
    
    let string_schema = TypeExpr::Primitive(PrimitiveType::String);
    
    marketplace.register_local_capability_with_schema(
        "util.echo".to_string(),
        "Echo".to_string(),
        "Returns the input unchanged".to_string(),
        echo_handler,
        Some(string_schema.clone()),
        Some(string_schema),
    ).await.unwrap();
    
    // Test execution with valid input
    let valid_input = Value::String("hello world".to_string());
    let result = marketplace.execute_capability("util.echo", &valid_input).await;
    
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), valid_input);
}

#[tokio::test]
async fn test_capability_execution_with_invalid_input() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    
    // Register a capability that expects strings
    let string_handler = Arc::new(|input: &Value| -> RuntimeResult<Value> {
        if let Value::String(s) = input {
            Ok(Value::String(s.to_uppercase()))
        } else {
            Err(RuntimeError::new("Expected string input"))
        }
    });
    
    let string_schema = TypeExpr::Primitive(PrimitiveType::String);
    
    marketplace.register_local_capability_with_schema(
        "string.uppercase".to_string(),
        "Uppercase".to_string(),
        "Converts string to uppercase".to_string(),
        string_handler,
        Some(string_schema.clone()),
        Some(string_schema),
    ).await.unwrap();
    
    // Test execution with invalid input (integer instead of string)
    let invalid_input = Value::Integer(42);
    let mut params = std::collections::HashMap::new();
    params.insert("input".to_string(), invalid_input.clone());
    // Use validation path to trigger schema enforcement
    let result = marketplace.execute_with_validation("string.uppercase", &params).await;
    
    // Should fail due to input validation
    assert!(result.is_err());
    
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Input validation failed"));
}

#[tokio::test]
async fn test_capability_with_refined_types() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    
    // Register a capability that expects positive integers
    let positive_handler = Arc::new(|input: &Value| -> RuntimeResult<Value> {
        if let Value::Integer(n) = input {
            Ok(Value::Integer(n * n)) // Return square
        } else {
            Err(RuntimeError::new("Expected integer input"))
        }
    });
    
    // Input schema: [:and :int [:> 0]] (positive integer)
    let positive_int_schema = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
        predicates: vec![TypePredicate::GreaterThan(Literal::Integer(0))],
    };
    
    let int_schema = TypeExpr::Primitive(PrimitiveType::Int);
    
    marketplace.register_local_capability_with_schema(
        "math.square".to_string(),
        "Square Positive Number".to_string(),
        "Returns the square of a positive integer".to_string(),
        positive_handler,
        Some(positive_int_schema),
        Some(int_schema),
    ).await.unwrap();
    
    // Test with valid positive integer
    let valid_input = Value::Integer(5);
    let result = marketplace.execute_capability("math.square", &valid_input).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::Integer(25));
    
    // Test with invalid zero
    let zero_input = Value::Integer(0);
    let result = marketplace.execute_capability("math.square", &zero_input).await;
    assert!(result.is_err());
    
    // Test with invalid negative
    let negative_input = Value::Integer(-5);
    let result = marketplace.execute_capability("math.square", &negative_input).await;
    assert!(result.is_err());
}

#[tokio::test]
async fn test_capability_with_complex_map_schema() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    
    // Register a user creation capability
    let create_user_handler = Arc::new(|input: &Value| -> RuntimeResult<Value> {
        if let Value::Map(map) = input {
            let name_key = MapKey::Keyword(Keyword::new("name"));
            let email_key = MapKey::Keyword(Keyword::new("email"));
            let age_key = MapKey::Keyword(Keyword::new("age"));
            
            if let (Some(Value::String(name)), Some(Value::String(email)), Some(Value::Integer(age))) = 
                (map.get(&name_key), map.get(&email_key), map.get(&age_key)) {
                
                let mut result = HashMap::new();
                result.insert(MapKey::Keyword(Keyword::new("id")), Value::Integer(12345));
                result.insert(MapKey::Keyword(Keyword::new("name")), Value::String(name.clone()));
                result.insert(MapKey::Keyword(Keyword::new("email")), Value::String(email.clone()));
                result.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(*age));
                result.insert(MapKey::Keyword(Keyword::new("created")), Value::Boolean(true));
                
                Ok(Value::Map(result))
            } else {
                Err(RuntimeError::new("Missing or invalid user data"))
            }
        } else {
            Err(RuntimeError::new("Input must be a map"))
        }
    });
    
    // Input schema: [:map [:name [:and :string [:min-length 1]]] [:email [:and :string [:matches-regex "^.+@.+\\..+$"]]] [:age [:and :int [:>= 18]]]]
    let input_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("name"),
                value_type: Box::new(TypeExpr::Refined {
                    base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                    predicates: vec![TypePredicate::MinLength(1)],
                }),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("email"),
                value_type: Box::new(TypeExpr::Refined {
                    base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                    predicates: vec![TypePredicate::MatchesRegex("^.+@.+\\..+$".to_string())],
                }),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("age"),
                value_type: Box::new(TypeExpr::Refined {
                    base_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                    predicates: vec![TypePredicate::GreaterEqual(Literal::Integer(18))],
                }),
                optional: false,
            },
        ],
        wildcard: None,
    };
    
    // Output schema: [:map [:id :int] [:name :string] [:email :string] [:age :int] [:created :bool]]
    let output_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("id"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("name"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("email"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("age"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("created"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Bool)),
                optional: false,
            },
        ],
        wildcard: None,
    };
    
    marketplace.register_local_capability_with_schema(
        "user.create".to_string(),
        "Create User".to_string(),
        "Creates a new user with validation".to_string(),
        create_user_handler,
        Some(input_schema),
        Some(output_schema),
    ).await.unwrap();
    
    // Test with valid user data
    let mut valid_user_data = HashMap::new();
    valid_user_data.insert(MapKey::Keyword(Keyword::new("name")), Value::String("Alice".to_string()));
    valid_user_data.insert(MapKey::Keyword(Keyword::new("email")), Value::String("alice@example.com".to_string()));
    valid_user_data.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(25));
    
    let valid_input = Value::Map(valid_user_data);
    let result = marketplace.execute_capability("user.create", &valid_input).await;
    
    assert!(result.is_ok());
    
    if let Ok(Value::Map(result_map)) = result {
        assert!(result_map.contains_key(&MapKey::Keyword(Keyword::new("id"))));
        assert!(result_map.contains_key(&MapKey::Keyword(Keyword::new("created"))));
    } else {
        panic!("Expected map result");
    }
    
    // Test with invalid email
    let mut invalid_email_data = HashMap::new();
    invalid_email_data.insert(MapKey::Keyword(Keyword::new("name")), Value::String("Bob".to_string()));
    invalid_email_data.insert(MapKey::Keyword(Keyword::new("email")), Value::String("not-an-email".to_string())); // Invalid email
    invalid_email_data.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(30));
    
    let invalid_input = Value::Map(invalid_email_data);
    let result = marketplace.execute_capability("user.create", &invalid_input).await;
    
    assert!(result.is_err());
    
    // Test with age too young
    let mut too_young_data = HashMap::new();
    too_young_data.insert(MapKey::Keyword(Keyword::new("name")), Value::String("Charlie".to_string()));
    too_young_data.insert(MapKey::Keyword(Keyword::new("email")), Value::String("charlie@example.com".to_string()));
    too_young_data.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(16)); // Too young
    
    let too_young_input = Value::Map(too_young_data);
    let result = marketplace.execute_capability("user.create", &too_young_input).await;
    
    assert!(result.is_err());
    
    // Test with empty name
    let mut empty_name_data = HashMap::new();
    empty_name_data.insert(MapKey::Keyword(Keyword::new("name")), Value::String("".to_string())); // Empty name
    empty_name_data.insert(MapKey::Keyword(Keyword::new("email")), Value::String("david@example.com".to_string()));
    empty_name_data.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(25));
    
    let empty_name_input = Value::Map(empty_name_data);
    let result = marketplace.execute_capability("user.create", &empty_name_input).await;
    
    assert!(result.is_err());
}

#[tokio::test]
async fn test_capability_output_validation() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);
    
    // Register a capability that sometimes returns wrong output type (for testing)
    let bad_output_handler = Arc::new(|input: &Value| -> RuntimeResult<Value> {
        if let Value::Boolean(should_be_correct) = input {
            if *should_be_correct {
                Ok(Value::String("correct output".to_string()))
            } else {
                // Return wrong type - integer instead of string
                Ok(Value::Integer(42))
            }
        } else {
            Err(RuntimeError::new("Expected boolean input"))
        }
    });
    
    let bool_schema = TypeExpr::Primitive(PrimitiveType::Bool);
    let string_schema = TypeExpr::Primitive(PrimitiveType::String);
    
    marketplace.register_local_capability_with_schema(
        "test.output".to_string(),
        "Test Output Validation".to_string(),
        "Tests output type validation".to_string(),
        bad_output_handler,
        Some(bool_schema),
        Some(string_schema),
    ).await.unwrap();
    
    // Test with correct output (validated path)
    let mut ok_params = std::collections::HashMap::new();
    ok_params.insert("input".to_string(), Value::Boolean(true));
    let result = marketplace.execute_with_validation("test.output", &ok_params).await;
    assert!(result.is_ok(), "Expected success for correct output, got {:?}", result);
    
    // Test with incorrect output (handler returns wrong type)
    let mut bad_params = std::collections::HashMap::new();
    bad_params.insert("input".to_string(), Value::Boolean(false));
    let result = marketplace.execute_with_validation("test.output", &bad_params).await;
    
    // Should fail due to output validation
    assert!(result.is_err());
    
    let error_msg = result.unwrap_err().to_string();
    assert!(error_msg.contains("Output validation failed"));
}
