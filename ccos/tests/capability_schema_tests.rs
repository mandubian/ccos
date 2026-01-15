use ccos::capabilities::{normalize_args_to_map, registry::CapabilityRegistry};
use rtfs::ast::{Keyword, MapKey, TypeExpr};
use rtfs::runtime::type_validator::TypeValidator;
use rtfs::runtime::values::Arity;
use rtfs::runtime::Value;

#[test]
fn write_line_uses_map_schema_with_normalization() {
    let registry = CapabilityRegistry::default();
    let cap = registry
        .get_capability("ccos.io.write-line")
        .expect("write-line capability should exist");

    assert_eq!(cap.arity, Arity::Range(1, 2));

    let schema = cap
        .input_schema
        .as_ref()
        .expect("write-line should have an input schema");

    // Schema should now be Map, not Union
    match schema {
        TypeExpr::Map { entries, wildcard } => {
            assert_eq!(entries.len(), 2);
            assert_eq!(entries[0].key.0, "handle");
            assert_eq!(entries[1].key.0, "line");
            assert!(wildcard.is_none());
        }
        _ => panic!("expected Map schema for write-line input, got {:?}", schema),
    }

    let validator = TypeValidator::new();

    // Test 1: Positional args → normalized to map → validates
    let positional_args = vec![Value::Integer(123), Value::String("hello".to_string())];
    let normalized = normalize_args_to_map(positional_args, schema)
        .expect("positional normalization should succeed");

    match &normalized {
        Value::Map(map) => {
            assert_eq!(
                map.get(&MapKey::Keyword(Keyword("handle".to_string()))),
                Some(&Value::Integer(123))
            );
            assert_eq!(
                map.get(&MapKey::Keyword(Keyword("line".to_string()))),
                Some(&Value::String("hello".to_string()))
            );
        }
        _ => panic!("expected normalized Map"),
    }

    assert!(
        validator.validate_value(&normalized, schema).is_ok(),
        "normalized positional input should validate"
    );

    // Test 2: Map input → passthrough → validates
    let mut map = std::collections::HashMap::new();
    map.insert(
        MapKey::Keyword(Keyword("handle".to_string())),
        Value::Integer(123),
    );
    map.insert(
        MapKey::Keyword(Keyword("line".to_string())),
        Value::String("hello".to_string()),
    );
    let map_args = vec![Value::Map(map)];
    let passthrough =
        normalize_args_to_map(map_args, schema).expect("map passthrough should succeed");

    assert!(
        validator.validate_value(&passthrough, schema).is_ok(),
        "map input should validate"
    );

    // Test 3: Wrong positional count → error
    let bad_args = vec![Value::Integer(123)];
    let result = normalize_args_to_map(bad_args, schema);
    assert!(result.is_err(), "wrong arg count should error");
}
