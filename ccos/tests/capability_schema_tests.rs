use ccos::capabilities::registry::CapabilityRegistry;
use rtfs::ast::{Keyword, MapKey, TypeExpr};
use rtfs::runtime::type_validator::TypeValidator;
use rtfs::runtime::values::Arity;
use rtfs::runtime::Value;

#[test]
fn write_line_accepts_tuple_and_map_inputs() {
    let registry = CapabilityRegistry::default();
    let cap = registry
        .get_capability("ccos.io.write-line")
        .expect("write-line capability should exist");

    assert_eq!(cap.arity, Arity::Range(1, 2));

    let schema = cap
        .input_schema
        .as_ref()
        .expect("write-line should have an input schema");

    match schema {
        TypeExpr::Union(options) => {
            assert!(
                options.iter().any(|t| matches!(t, TypeExpr::Tuple(_))),
                "schema should allow tuple input"
            );
            assert!(
                options.iter().any(|t| matches!(t, TypeExpr::Map { .. })),
                "schema should allow map input"
            );
        }
        _ => panic!("expected union schema for write-line input"),
    }

    let validator = TypeValidator::new();

    // Tuple input: [handle, line]
    let tuple_input = Value::Vector(vec![
        Value::Integer(123),
        Value::String("hello".to_string()),
    ]);
    assert!(
        validator.validate_value(&tuple_input, schema).is_ok(),
        "tuple input should validate"
    );

    // Map input: {:handle <int> :line <string>}
    let mut map = std::collections::HashMap::new();
    map.insert(
        MapKey::Keyword(Keyword("handle".to_string())),
        Value::Integer(123),
    );
    map.insert(
        MapKey::Keyword(Keyword("line".to_string())),
        Value::String("hello".to_string()),
    );
    let map_input = Value::Map(map);

    assert!(
        validator.validate_value(&map_input, schema).is_ok(),
        "map input should validate"
    );
}
