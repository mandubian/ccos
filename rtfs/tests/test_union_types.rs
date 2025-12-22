// RTFS Union Types Comprehensive Tests
// This file contains comprehensive tests for union type validation

use rtfs::ast::{Keyword, MapTypeEntry, MapKey, PrimitiveType, TypeExpr};
use rtfs::runtime::type_validator::{TypeValidator, ValidationError};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

#[test]
fn test_union_type_validation_success() {
    // [:union :int :string] should accept both int and string
    let union_type = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String),
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept integer
    assert!(validator.validate_value(&Value::Integer(42), &union_type).is_ok());
    
    // Should accept string  
    assert!(validator.validate_value(&Value::String("hello".to_string()), &union_type).is_ok());
}

#[test]
fn test_union_type_validation_failure() {
    // [:union :int :string] should reject boolean
    let union_type = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String),
    ]);
    
    let validator = TypeValidator::new();
    
    // Should reject boolean
    assert!(validator.validate_value(&Value::Boolean(true), &union_type).is_err());
    
    // Should reject float
    assert!(validator.validate_value(&Value::Float(3.14), &union_type).is_err());
    
    // Should reject nil
    assert!(validator.validate_value(&Value::Nil, &union_type).is_err());
}

#[test]
fn test_union_type_with_complex_types() {
    // [:union [:vector :int] [:map [:x :int]]]
    let complex_union = TypeExpr::Union(vec![
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::Int))),
        TypeExpr::Map {
            entries: vec![MapTypeEntry {
                key: Keyword::new("x"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            }],
            wildcard: None,
        },
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept vector of ints
    assert!(validator.validate_value(
        &Value::Vector(vec![Value::Integer(1), Value::Integer(2)]),
        &complex_union
    ).is_ok());
    
    // Should accept map with x key
    let mut test_map: HashMap<MapKey, Value> = HashMap::new();
    test_map.insert(MapKey::Keyword(Keyword::new("x")), Value::Integer(42));
    assert!(validator.validate_value(&Value::Map(test_map), &complex_union).is_ok());
    
    // Should reject vector of strings
    assert!(validator.validate_value(
        &Value::Vector(vec![Value::String("a".to_string()), Value::String("b".to_string())]),
        &complex_union
    ).is_err());
    
    // Should reject map without x key
    let mut bad_map: HashMap<MapKey, Value> = HashMap::new();
    bad_map.insert(MapKey::Keyword(Keyword::new("y")), Value::Integer(42));
    assert!(validator.validate_value(&Value::Map(bad_map), &complex_union).is_err());
}

#[test]
fn test_union_type_with_multiple_primitives() {
    // [:union :int :string :bool :float] - multiple primitive types
    let multi_union = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String),
        TypeExpr::Primitive(PrimitiveType::Bool),
        TypeExpr::Primitive(PrimitiveType::Float),
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept all primitive types
    assert!(validator.validate_value(&Value::Integer(42), &multi_union).is_ok());
    assert!(validator.validate_value(&Value::String("test".to_string()), &multi_union).is_ok());
    assert!(validator.validate_value(&Value::Boolean(true), &multi_union).is_ok());
    assert!(validator.validate_value(&Value::Float(3.14), &multi_union).is_ok());
    
    // Should reject nil
    assert!(validator.validate_value(&Value::Nil, &multi_union).is_err());
}

#[test]
fn test_union_type_nested_in_map() {
    // [:map [:value [:union :int :string]]]
    let map_with_union = TypeExpr::Map {
        entries: vec![MapTypeEntry {
            key: Keyword::new("value"),
            value_type: Box::new(TypeExpr::Union(vec![
                TypeExpr::Primitive(PrimitiveType::Int),
                TypeExpr::Primitive(PrimitiveType::String),
            ])),
            optional: false,
        }],
        wildcard: None,
    };
    
    let validator = TypeValidator::new();
    
    // Should accept map with integer value
    let mut map_with_int: HashMap<MapKey, Value> = HashMap::new();
    map_with_int.insert(MapKey::Keyword(Keyword::new("value")), Value::Integer(42));
    assert!(validator.validate_value(&Value::Map(map_with_int), &map_with_union).is_ok());
    
    // Should accept map with string value
    let mut map_with_string: HashMap<MapKey, Value> = HashMap::new();
    map_with_string.insert(
        MapKey::Keyword(Keyword::new("value")),
        Value::String("test".to_string()),
    );
    assert!(validator.validate_value(&Value::Map(map_with_string), &map_with_union).is_ok());
    
    // Should reject map with boolean value
    let mut map_with_bool: HashMap<MapKey, Value> = HashMap::new();
    map_with_bool.insert(MapKey::Keyword(Keyword::new("value")), Value::Boolean(true));
    assert!(validator.validate_value(&Value::Map(map_with_bool), &map_with_union).is_err());
}

#[test]
fn test_union_type_error_messages() {
    // [:union :int :string] with detailed error checking
    let union_type = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String),
    ]);
    
    let validator = TypeValidator::new();
    
    // Test error message for boolean rejection
    let result = validator.validate_value(&Value::Boolean(true), &union_type);
    assert!(result.is_err());
    
    if let Err(ValidationError::TypeMismatch {
        expected,
        actual,
        path,
    }) = result
    {
        assert!(
            expected.to_string().contains("[:union"),
            "Expected error should mention union type; got expected={}",
            expected
        );
        assert_eq!(actual, "boolean");
        assert!(
            path.is_empty(),
            "Top-level validation path should be empty; got '{}'",
            path
        );
    }
}

#[test]
fn test_union_type_with_nil() {
    // [:union :int :nil] - union with nil type
    let union_with_nil = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::Nil),
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept integer
    assert!(validator.validate_value(&Value::Integer(42), &union_with_nil).is_ok());
    
    // Should accept nil
    assert!(validator.validate_value(&Value::Nil, &union_with_nil).is_ok());
    
    // Should reject string
    assert!(validator.validate_value(&Value::String("test".to_string()), &union_with_nil).is_err());
}