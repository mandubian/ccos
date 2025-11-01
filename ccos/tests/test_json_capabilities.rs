use ccos::capabilities::{CapabilityExecutionPolicy, CapabilityRegistry};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;
use rtfs::ast::MapKey;

#[test]
fn json_capabilities_parse_and_stringify() {
    let mut registry = CapabilityRegistry::new();
    registry.set_execution_policy(CapabilityExecutionPolicy::InlineDev);

    let context = RuntimeContext::controlled(vec![
        "ccos.json.parse".to_string(),
        "ccos.json.stringify".to_string(),
        "ccos.json.stringify-pretty".to_string(),
    ]);

    let parsed = registry
        .execute_capability_with_microvm(
            "ccos.json.parse",
            vec![Value::String("{\"answer\":42}".to_string())],
            Some(&context),
        )
        .expect("parse");

    match parsed {
        Value::Map(map) => {
            assert_eq!(map.get(&MapKey::String("answer".to_string())), Some(&Value::Integer(42)));
        }
        other => panic!("expected map, got {:?}", other),
    }

    let value = Value::Map(vec![
        (MapKey::String("ok".to_string()), Value::Boolean(true)),
        (MapKey::String("items".to_string()), Value::Vector(vec![Value::Integer(1), Value::Integer(2)])),
    ]
    .into_iter()
    .collect());

    let json_string = registry
        .execute_capability_with_microvm(
            "ccos.json.stringify",
            vec![value.clone()],
            Some(&context),
        )
        .expect("stringify");

    match json_string {
        Value::String(s) => {
            assert!(s.contains("\"ok\""), "expected ok key");
            assert!(s.contains("\"items\""), "expected items key");
        }
        other => panic!("expected string, got {:?}", other),
    }

    let pretty = registry
        .execute_capability_with_microvm(
            "ccos.json.stringify-pretty",
            vec![value],
            Some(&context),
        )
        .expect("stringify pretty");

    match pretty {
        Value::String(s) => {
            assert!(s.contains('\n'), "expected pretty string with newline");
        }
        other => panic!("expected string, got {:?}", other),
    }
}

#[test]
fn json_capabilities_legacy_aliases_still_work() {
    let mut registry = CapabilityRegistry::new();
    registry.set_execution_policy(CapabilityExecutionPolicy::InlineDev);

    let context = RuntimeContext::controlled(vec![
        "ccos.data.parse-json".to_string(),
        "ccos.data.serialize-json".to_string(),
    ]);

    let parsed = registry
        .execute_capability_with_microvm(
            "ccos.data.parse-json",
            vec![Value::String("{\"legacy\":true}".to_string())],
            Some(&context),
        )
        .expect("parse");
    assert!(matches!(parsed, Value::Map(_)));

    let serialized = registry
        .execute_capability_with_microvm(
            "ccos.data.serialize-json",
            vec![Value::Vector(vec![Value::Integer(1)])],
            Some(&context),
        )
        .expect("serialize");
    assert!(matches!(serialized, Value::String(_)));
}
