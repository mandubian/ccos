use rtfs_compiler::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr, TypePredicate};
use rtfs_compiler::ccos::capabilities::registry::CapabilityRegistry;
use rtfs_compiler::ccos::capability_marketplace::CapabilityMarketplace;
use rtfs_compiler::runtime::{RuntimeError, RuntimeResult, Value};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

#[tokio::test]
async fn test_simple_capability_registration() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry.clone());

    // Define a simple number doubling capability
    let input_schema = TypeExpr::Primitive(PrimitiveType::Float);
    let output_schema = TypeExpr::Primitive(PrimitiveType::Float);

    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        match inputs {
            Value::Float(n) => Ok(Value::Float(n * 2.0)),
            Value::Integer(i) => Ok(Value::Float(*i as f64 * 2.0)),
            _ => Err(RuntimeError::new("Expected number input")),
        }
    });

    let result = marketplace
        .register_local_capability_with_schema(
            "double_number".to_string(),
            "Double Number".to_string(),
            "Doubles a number".to_string(),
            handler,
            Some(input_schema),
            Some(output_schema),
        )
        .await;

    assert!(
        result.is_ok(),
        "Failed to register capability: {:?}",
        result.err()
    );

    // Verify the capability was registered
    let capability_manifests = marketplace.list_capabilities().await;
    let capability_ids: Vec<String> = capability_manifests.iter().map(|c| c.id.clone()).collect();
    assert!(
        capability_ids.contains(&"double_number".to_string()),
        "Capability not found in registry"
    );
}

#[tokio::test]
async fn test_capability_execution_with_validation() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Define a string reversal capability
    let input_schema = TypeExpr::Primitive(PrimitiveType::String);
    let output_schema = TypeExpr::Primitive(PrimitiveType::String);

    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        if let Value::String(s) = inputs {
            Ok(Value::String(s.chars().rev().collect()))
        } else {
            Err(RuntimeError::new("Expected string input"))
        }
    });

    // Register the capability
    let result = marketplace
        .register_local_capability_with_schema(
            "reverse_string".to_string(),
            "Reverse String".to_string(),
            "Reverses a string".to_string(),
            handler,
            Some(input_schema),
            Some(output_schema),
        )
        .await;

    assert!(result.is_ok());

    // Test valid input
    let valid_input = Value::String("hello".to_string());
    let result = marketplace
        .execute_capability("reverse_string", &valid_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(reversed)) = result {
        assert_eq!(reversed, "olleh");
    } else {
        panic!("Expected string result, got {:?}", result);
    }

    // Test invalid input (number instead of string)
    let invalid_input = Value::Float(42.0);
    let result = marketplace
        .execute_capability("reverse_string", &invalid_input)
        .await;

    // Should fail due to type validation
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();

    // Check error message format (updated based on actual implementation)
    assert!(
        error_msg.contains("Type mismatch")
            || error_msg.contains("expected")
            || error_msg.contains("validation"),
        "Error message should contain type validation info: {}",
        error_msg
    );
}

#[tokio::test]
async fn test_refined_type_with_predicate() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Define a capability with refined string type (minimum length)
    let input_schema = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
        predicates: vec![TypePredicate::MinLength(3)],
    };

    let output_schema = TypeExpr::Primitive(PrimitiveType::String);

    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        if let Value::String(s) = inputs {
            Ok(Value::String(s.to_uppercase()))
        } else {
            Err(RuntimeError::new("Expected string input"))
        }
    });

    // Register the capability
    let result = marketplace
        .register_local_capability_with_schema(
            "uppercase_min3".to_string(),
            "Uppercase Minimum 3 Chars".to_string(),
            "Converts string to uppercase (min 3 chars)".to_string(),
            handler,
            Some(input_schema),
            Some(output_schema),
        )
        .await;

    assert!(result.is_ok());

    // Test valid input
    let valid_input = Value::String("hello".to_string());
    let result = marketplace
        .execute_capability("uppercase_min3", &valid_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(upper)) = result {
        assert_eq!(upper, "HELLO");
    } else {
        panic!("Expected string result, got {:?}", result);
    }

    // Test invalid input (too short)
    let invalid_input = Value::String("hi".to_string());
    let result = marketplace
        .execute_capability("uppercase_min3", &invalid_input)
        .await;

    // Should fail due to predicate validation
    assert!(result.is_err());
    let error_msg = result.unwrap_err().to_string();
    assert!(
        error_msg.contains("Type validation failed")
            || error_msg.contains("length")
            || error_msg.contains("predicate")
    );
}

#[tokio::test]
async fn test_vector_type_validation() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Define a capability that takes a vector of strings
    let input_schema = TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String)));
    let output_schema = TypeExpr::Primitive(PrimitiveType::String);

    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        if let Value::Vector(vec) = inputs {
            let joined = vec
                .iter()
                .filter_map(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .collect::<Vec<String>>()
                .join(" ");
            Ok(Value::String(joined))
        } else {
            Err(RuntimeError::new("Expected vector input"))
        }
    });

    // Register the capability
    let result = marketplace
        .register_local_capability_with_schema(
            "join_strings".to_string(),
            "Join Strings".to_string(),
            "Joins strings in a vector".to_string(),
            handler,
            Some(input_schema),
            Some(output_schema),
        )
        .await;

    assert!(result.is_ok());

    // Test valid input
    let valid_input = Value::Vector(vec![
        Value::String("hello".to_string()),
        Value::String("world".to_string()),
        Value::String("test".to_string()),
    ]);

    let result = marketplace
        .execute_capability("join_strings", &valid_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(joined)) = result {
        assert_eq!(joined, "hello world test");
    } else {
        panic!("Expected string result, got {:?}", result);
    }
}

#[tokio::test]
async fn test_map_type_validation() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Define a capability with map input type
    let input_schema = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("name"),
                optional: false,
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
            },
            MapTypeEntry {
                key: Keyword::new("count"),
                optional: false,
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
            },
        ],
        wildcard: None,
    };

    let output_schema = TypeExpr::Primitive(PrimitiveType::String);

    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        if let Value::Map(map) = inputs {
            let name = map
                .get(&rtfs_compiler::ast::MapKey::Keyword(Keyword::new("name")))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .ok_or_else(|| RuntimeError::new("Missing name"))?;

            let count = map
                .get(&rtfs_compiler::ast::MapKey::Keyword(Keyword::new("count")))
                .and_then(|v| match v {
                    Value::Integer(i) => Some(*i),
                    _ => None,
                })
                .ok_or_else(|| RuntimeError::new("Missing count"))?;

            Ok(Value::String(format!("{} x{}", name, count)))
        } else {
            Err(RuntimeError::new("Expected map input"))
        }
    });

    // Register the capability
    let result = marketplace
        .register_local_capability_with_schema(
            "format_item".to_string(),
            "Format Item".to_string(),
            "Formats an item with count".to_string(),
            handler,
            Some(input_schema),
            Some(output_schema),
        )
        .await;

    assert!(result.is_ok());

    // Test valid input
    let mut input_map = HashMap::new();
    input_map.insert(
        rtfs_compiler::ast::MapKey::Keyword(Keyword::new("name")),
        Value::String("apple".to_string()),
    );
    input_map.insert(
        rtfs_compiler::ast::MapKey::Keyword(Keyword::new("count")),
        Value::Integer(5),
    );
    let valid_input = Value::Map(input_map);

    let result = marketplace
        .execute_capability("format_item", &valid_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(formatted)) = result {
        assert_eq!(formatted, "apple x5");
    } else {
        panic!("Expected string result, got {:?}", result);
    }
}

#[tokio::test]
async fn test_optional_type_validation() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Define a capability with optional string input
    let input_schema = TypeExpr::Optional(Box::new(TypeExpr::Primitive(PrimitiveType::String)));
    let output_schema = TypeExpr::Primitive(PrimitiveType::String);

    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        match inputs {
            Value::String(s) => Ok(Value::String(format!("Got: {}", s))),
            Value::Nil => Ok(Value::String("Got nothing".to_string())),
            _ => Err(RuntimeError::new("Expected string or nil")),
        }
    });

    // Register the capability
    let result = marketplace
        .register_local_capability_with_schema(
            "optional_string".to_string(),
            "Optional String".to_string(),
            "Handles optional string input".to_string(),
            handler,
            Some(input_schema),
            Some(output_schema),
        )
        .await;

    assert!(result.is_ok());

    // Test with string value
    let string_input = Value::String("hello".to_string());
    let result = marketplace
        .execute_capability("optional_string", &string_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(output)) = result {
        assert_eq!(output, "Got: hello");
    } else {
        panic!("Expected string result, got {:?}", result);
    }

    // Test with nil value
    let nil_input = Value::Nil;
    let result = marketplace
        .execute_capability("optional_string", &nil_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(output)) = result {
        assert_eq!(output, "Got nothing");
    } else {
        panic!("Expected string result, got {:?}", result);
    }
}

#[tokio::test]
async fn test_union_type_validation() {
    let registry = Arc::new(RwLock::new(CapabilityRegistry::new()));
    let marketplace = CapabilityMarketplace::new(registry);

    // Define a capability with union input type (string or integer)
    let input_schema = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::String),
        TypeExpr::Primitive(PrimitiveType::Int),
    ]);
    let output_schema = TypeExpr::Primitive(PrimitiveType::String);

    let handler = Arc::new(|inputs: &Value| -> RuntimeResult<Value> {
        match inputs {
            Value::String(s) => Ok(Value::String(format!("String: {}", s))),
            Value::Integer(i) => Ok(Value::String(format!("Integer: {}", i))),
            _ => Err(RuntimeError::new("Expected string or integer")),
        }
    });

    // Register the capability
    let result = marketplace
        .register_local_capability_with_schema(
            "union_handler".to_string(),
            "Union Handler".to_string(),
            "Handles string or integer input".to_string(),
            handler,
            Some(input_schema),
            Some(output_schema),
        )
        .await;

    assert!(result.is_ok());

    // Test with string
    let string_input = Value::String("test".to_string());
    let result = marketplace
        .execute_capability("union_handler", &string_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(output)) = result {
        assert_eq!(output, "String: test");
    } else {
        panic!("Expected string result, got {:?}", result);
    }

    // Test with integer
    let int_input = Value::Integer(42);
    let result = marketplace
        .execute_capability("union_handler", &int_input)
        .await;
    assert!(result.is_ok());

    if let Ok(Value::String(output)) = result {
        assert_eq!(output, "Integer: 42");
    } else {
        panic!("Expected string result, got {:?}", result);
    }

    // Test with invalid type (float)
    let invalid_input = Value::Float(3.14);
    let result = marketplace
        .execute_capability("union_handler", &invalid_input)
        .await;

    // Should fail due to type validation
    assert!(result.is_err());
}
