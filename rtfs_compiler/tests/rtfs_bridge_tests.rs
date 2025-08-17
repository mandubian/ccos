use rtfs_compiler::ast::{Expression, MapKey, Symbol, Literal};
use rtfs_compiler::ccos::rtfs_bridge::{
    extract_intent_from_rtfs,
    extract_plan_from_rtfs,
    intent_to_rtfs_function_call,
    intent_to_rtfs_map,
    plan_to_rtfs_function_call,
    plan_to_rtfs_map,
    validate_intent,
    validate_plan,
    validate_plan_intent_compatibility,
};
use rtfs_compiler::ccos::types::{Intent, Plan, IntentStatus, PlanStatus, PlanLanguage, PlanBody};
use rtfs_compiler::runtime::values::Value;
use std::collections::HashMap;

#[test]
fn test_intent_function_call_extraction() {
    // Test extracting intent from function call format
    let intent_expr = Expression::FunctionCall {
        callee: Box::new(Expression::Symbol(Symbol("intent".to_string()))), // Test with simpler name
        arguments: vec![
            Expression::Literal(Literal::String("test-intent".to_string())),
            Expression::Map(HashMap::from([
                (MapKey::String(":goal".to_string()), Expression::Literal(Literal::String("Test goal".to_string()))),
                (MapKey::String(":constraints".to_string()), Expression::Map(HashMap::from([
                    (MapKey::String("max_cost".to_string()), Expression::Literal(Literal::Integer(100)))
                ]))),
                (MapKey::String(":preferences".to_string()), Expression::Map(HashMap::from([
                    (MapKey::String("priority".to_string()), Expression::Literal(Literal::String("high".to_string())))
                ]))),
            ]))
        ],
    };

    let intent = extract_intent_from_rtfs(&intent_expr).expect("Failed to extract intent");
    
    assert_eq!(intent.name, Some("test-intent".to_string()));
    assert_eq!(intent.goal, "Test goal");
    assert_eq!(intent.constraints.get("max_cost"), Some(&Value::Integer(100)));
    assert_eq!(intent.preferences.get("priority"), Some(&Value::String("high".to_string())));
    assert_eq!(intent.status, IntentStatus::Active);
}

#[test]
fn test_intent_map_extraction() {
    // Test extracting intent from map format
    let intent_expr = Expression::Map(HashMap::from([
        (MapKey::String(":type".to_string()), Expression::Literal(Literal::String("intent".to_string()))),
        (MapKey::String(":name".to_string()), Expression::Literal(Literal::String("test-intent".to_string()))),
        (MapKey::String(":goal".to_string()), Expression::Literal(Literal::String("Test goal".to_string()))),
        (MapKey::String(":constraints".to_string()), Expression::Map(HashMap::from([
            (MapKey::String("max_cost".to_string()), Expression::Literal(Literal::Integer(100)))
        ]))),
    ]));

    let intent = extract_intent_from_rtfs(&intent_expr).expect("Failed to extract intent");
    
    assert_eq!(intent.name, Some("test-intent".to_string()));
    assert_eq!(intent.goal, "Test goal");
    assert_eq!(intent.constraints.get("max_cost"), Some(&Value::Integer(100)));
    assert_eq!(intent.status, IntentStatus::Active);
}

#[test]
fn test_plan_function_call_extraction() {
    // Test extracting plan from function call format
    let plan_expr = Expression::FunctionCall {
        callee: Box::new(Expression::Symbol(Symbol("plan".to_string()))), // Test with simpler name
        arguments: vec![
            Expression::Literal(Literal::String("test-plan".to_string())),
            Expression::Map(HashMap::from([
                (MapKey::String(":body".to_string()), Expression::Literal(Literal::String("(do (println \"hello\"))".to_string()))),
                (MapKey::String(":intent-ids".to_string()), Expression::Vector(vec![
                    Expression::Literal(Literal::String("intent-1".to_string())),
                    Expression::Literal(Literal::String("intent-2".to_string())),
                ])),
                (MapKey::String(":input-schema".to_string()), Expression::Map(HashMap::from([
                    (MapKey::String("type".to_string()), Expression::Literal(Literal::String("object".to_string()))),
                ]))),
                (MapKey::String(":output-schema".to_string()), Expression::Map(HashMap::from([
                    (MapKey::String("type".to_string()), Expression::Literal(Literal::String("string".to_string()))),
                ]))),
                (MapKey::String(":policies".to_string()), Expression::Map(HashMap::from([
                    (MapKey::String("max_cost".to_string()), Expression::Literal(Literal::Integer(100))),
                    (MapKey::String("timeout".to_string()), Expression::Literal(Literal::Integer(30))),
                ]))),
                (MapKey::String(":capabilities-required".to_string()), Expression::Vector(vec![
                    Expression::Literal(Literal::String("http".to_string())),
                    Expression::Literal(Literal::String("database".to_string())),
                ])),
                (MapKey::String(":annotations".to_string()), Expression::Map(HashMap::from([
                    (MapKey::String("prompt_id".to_string()), Expression::Literal(Literal::String("prompt-123".to_string()))),
                    (MapKey::String("version".to_string()), Expression::Literal(Literal::String("1.0".to_string()))),
                ]))),
            ]))
        ],
    };

    let plan = extract_plan_from_rtfs(&plan_expr).expect("Failed to extract plan");
    
    assert_eq!(plan.name, Some("test-plan".to_string()));
    assert_eq!(plan.intent_ids, vec!["intent-1", "intent-2"]);
    assert_eq!(plan.language, PlanLanguage::Rtfs20);
    assert_eq!(plan.status, PlanStatus::Draft);
    
    if let PlanBody::Rtfs(code) = &plan.body {
        assert_eq!(code, "\"(do (println \"hello\"))\"");
    } else {
        panic!("Expected RTFS body");
    }
    
    assert!(plan.input_schema.is_some());
    assert!(plan.output_schema.is_some());
    assert_eq!(plan.policies.get("max_cost"), Some(&Value::Integer(100)));
    assert_eq!(plan.policies.get("timeout"), Some(&Value::Integer(30)));
    assert_eq!(plan.capabilities_required, vec!["http", "database"]);
    assert_eq!(plan.annotations.get("prompt_id"), Some(&Value::String("prompt-123".to_string())));
    assert_eq!(plan.annotations.get("version"), Some(&Value::String("1.0".to_string())));
}

#[test]
fn test_plan_map_extraction() {
    // Test extracting plan from map format
    let plan_expr = Expression::Map(HashMap::from([
        (MapKey::String(":type".to_string()), Expression::Literal(Literal::String("plan".to_string()))),
        (MapKey::String(":name".to_string()), Expression::Literal(Literal::String("test-plan".to_string()))),
        (MapKey::String(":body".to_string()), Expression::Literal(Literal::String("(do (println \"hello\"))".to_string()))),
        (MapKey::String(":intent-ids".to_string()), Expression::Vector(vec![
            Expression::Literal(Literal::String("intent-1".to_string())),
        ])),
        (MapKey::String(":policies".to_string()), Expression::Map(HashMap::from([
            (MapKey::String("max_cost".to_string()), Expression::Literal(Literal::Integer(100))),
        ]))),
    ]));

    let plan = extract_plan_from_rtfs(&plan_expr).expect("Failed to extract plan");
    
    assert_eq!(plan.name, Some("test-plan".to_string()));
    assert_eq!(plan.intent_ids, vec!["intent-1"]);
    assert_eq!(plan.language, PlanLanguage::Rtfs20);
    assert_eq!(plan.status, PlanStatus::Draft);
    
    if let PlanBody::Rtfs(code) = &plan.body {
        assert_eq!(code, "\"(do (println \"hello\"))\"");
    } else {
        panic!("Expected RTFS body");
    }
    
    assert_eq!(plan.policies.get("max_cost"), Some(&Value::Integer(100)));
}

#[test]
fn test_intent_to_rtfs_conversion() {
    // Create a test intent
    let mut intent = Intent {
        intent_id: "test-intent".to_string(),
        name: Some("test-intent".to_string()),
        original_request: "Test request".to_string(),
        goal: "Test goal".to_string(),
        constraints: HashMap::from([
            ("max_cost".to_string(), Value::Integer(100)),
            ("timeout".to_string(), Value::Integer(30)),
        ]),
        preferences: HashMap::from([
            ("priority".to_string(), Value::String("high".to_string())),
        ]),
        success_criteria: Some(Value::String("success".to_string())),
        status: IntentStatus::Active,
        created_at: 1234567890,
        updated_at: 1234567890,
        metadata: HashMap::new(),
    };

    // Test conversion to function call
    let function_call = intent_to_rtfs_function_call(&intent).expect("Failed to convert to function call");
    
    if let Expression::FunctionCall { callee, arguments } = function_call {
        if let Expression::Symbol(symbol) = *callee {
            assert_eq!(symbol.0, "intent");
        } else {
            panic!("Expected symbol as callee");
        }
        
        assert_eq!(arguments.len(), 2);
        if let Expression::Literal(Literal::String(name)) = &arguments[0] {
            assert_eq!(name, "test-intent");
        } else {
            panic!("Expected string literal as first argument");
        }
    } else {
        panic!("Expected function call expression");
    }

    // Test conversion to map
    let map_expr = intent_to_rtfs_map(&intent).expect("Failed to convert to map");
    
    if let Expression::Map(map) = map_expr {
        assert_eq!(map.get(&MapKey::String(":type".to_string())), Some(&Expression::Literal(Literal::String("intent".to_string()))));
        assert_eq!(map.get(&MapKey::String(":name".to_string())), Some(&Expression::Literal(Literal::String("test-intent".to_string()))));
        assert_eq!(map.get(&MapKey::String(":goal".to_string())), Some(&Expression::Literal(Literal::String("Test goal".to_string()))));
    } else {
        panic!("Expected map expression");
    }
}

#[test]
fn test_plan_to_rtfs_conversion() {
    // Create a test plan
    let mut plan = Plan {
        plan_id: "test-plan".to_string(),
        name: Some("test-plan".to_string()),
        intent_ids: vec!["intent-1".to_string(), "intent-2".to_string()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs("(do (println \"hello\"))".to_string()),
        status: PlanStatus::Draft,
        created_at: 1234567890,
        metadata: HashMap::new(),
        input_schema: Some(Value::Map(HashMap::from([
            (MapKey::String("type".to_string()), Value::String("object".to_string())),
        ]))),
        output_schema: Some(Value::Map(HashMap::from([
            (MapKey::String("type".to_string()), Value::String("string".to_string())),
        ]))),
        policies: HashMap::from([
            ("max_cost".to_string(), Value::Integer(100)),
            ("timeout".to_string(), Value::Integer(30)),
        ]),
        capabilities_required: vec!["http".to_string(), "database".to_string()],
        annotations: HashMap::from([
            ("prompt_id".to_string(), Value::String("prompt-123".to_string())),
            ("version".to_string(), Value::String("1.0".to_string())),
        ]),
    };

    // Test conversion to function call
    let function_call = plan_to_rtfs_function_call(&plan).expect("Failed to convert to function call");
    
    if let Expression::FunctionCall { callee, arguments } = function_call {
        if let Expression::Symbol(symbol) = *callee {
            assert_eq!(symbol.0, "plan");
        } else {
            panic!("Expected symbol as callee");
        }
        
        assert_eq!(arguments.len(), 2);
        if let Expression::Literal(Literal::String(name)) = &arguments[0] {
            assert_eq!(name, "test-plan");
        } else {
            panic!("Expected string literal as first argument");
        }
    } else {
        panic!("Expected function call expression");
    }

    // Test conversion to map
    let map_expr = plan_to_rtfs_map(&plan).expect("Failed to convert to map");
    
    if let Expression::Map(map) = map_expr {
        assert_eq!(map.get(&MapKey::String(":type".to_string())), Some(&Expression::Literal(Literal::String("plan".to_string()))));
        assert_eq!(map.get(&MapKey::String(":name".to_string())), Some(&Expression::Literal(Literal::String("test-plan".to_string()))));
    } else {
        panic!("Expected map expression");
    }
}

#[test]
fn test_intent_validation() {
    // Test valid intent
    let valid_intent = Intent {
        intent_id: "test-intent".to_string(),
        name: Some("test-intent".to_string()),
        original_request: "Test request".to_string(),
        goal: "Test goal".to_string(),
        constraints: HashMap::new(),
        preferences: HashMap::new(),
        success_criteria: None,
        status: IntentStatus::Active,
        created_at: 1234567890,
        updated_at: 1234567890,
        metadata: HashMap::new(),
    };
    
    assert!(validate_intent(&valid_intent).is_ok());

    // Test invalid intent (empty goal)
    let mut invalid_intent = valid_intent.clone();
    invalid_intent.goal = "".to_string();
    assert!(validate_intent(&invalid_intent).is_err());

    // Test invalid intent (empty intent_id)
    let mut invalid_intent = valid_intent.clone();
    invalid_intent.intent_id = "".to_string();
    assert!(validate_intent(&invalid_intent).is_err());
}

#[test]
fn test_plan_validation() {
    // Test valid plan
    let valid_plan = Plan {
        plan_id: "test-plan".to_string(),
        name: Some("test-plan".to_string()),
        intent_ids: vec!["intent-1".to_string()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs("(do (println \"hello\"))".to_string()),
        status: PlanStatus::Draft,
        created_at: 1234567890,
        metadata: HashMap::new(),
        input_schema: None,
        output_schema: None,
        policies: HashMap::new(),
        capabilities_required: vec![],
        annotations: HashMap::new(),
    };
    
    assert!(validate_plan(&valid_plan).is_ok());

    // Test invalid plan (empty plan_id)
    let mut invalid_plan = valid_plan.clone();
    invalid_plan.plan_id = "".to_string();
    assert!(validate_plan(&invalid_plan).is_err());

    // Test invalid plan (empty body)
    let mut invalid_plan = valid_plan.clone();
    invalid_plan.body = PlanBody::Rtfs("".to_string());
    assert!(validate_plan(&invalid_plan).is_err());
}

#[test]
fn test_plan_intent_compatibility() {
    let intent = Intent {
        intent_id: "test-intent".to_string(),
        name: Some("test-intent".to_string()),
        original_request: "Test request".to_string(),
        goal: "Test goal".to_string(),
        constraints: HashMap::new(),
        preferences: HashMap::new(),
        success_criteria: None,
        status: IntentStatus::Active,
        created_at: 1234567890,
        updated_at: 1234567890,
        metadata: HashMap::new(),
    };

    let plan = Plan {
        plan_id: "test-plan".to_string(),
        name: Some("test-plan".to_string()),
        intent_ids: vec!["test-intent".to_string()], // References the intent
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs("(do (println \"hello\"))".to_string()),
        status: PlanStatus::Draft,
        created_at: 1234567890,
        metadata: HashMap::new(),
        input_schema: None,
        output_schema: None,
        policies: HashMap::new(),
        capabilities_required: vec![],
        annotations: HashMap::new(),
    };

    // Test compatible plan and intent
    assert!(validate_plan_intent_compatibility(&plan, &intent).is_ok());

    // Test incompatible plan (doesn't reference intent)
    let mut incompatible_plan = plan.clone();
    incompatible_plan.intent_ids = vec!["different-intent".to_string()];
    assert!(validate_plan_intent_compatibility(&incompatible_plan, &intent).is_err());
}

#[test]
fn test_error_handling() {
    // Test invalid function call (wrong function name)
    let invalid_expr = Expression::FunctionCall {
        callee: Box::new(Expression::Symbol(Symbol("wrong/function".to_string()))),
        arguments: vec![],
    };
    
    assert!(extract_intent_from_rtfs(&invalid_expr).is_err());
    assert!(extract_plan_from_rtfs(&invalid_expr).is_err());

    // Test invalid map (wrong type)
    let invalid_expr = Expression::Map(HashMap::from([
        (MapKey::String(":type".to_string()), Expression::Literal(Literal::String("wrong-type".to_string()))),
    ]));
    
    assert!(extract_intent_from_rtfs(&invalid_expr).is_err());
    assert!(extract_plan_from_rtfs(&invalid_expr).is_err());

    // Test missing required fields
    let invalid_expr = Expression::Map(HashMap::from([
        (MapKey::String(":type".to_string()), Expression::Literal(Literal::String("intent".to_string()))),
        // Missing :name and :goal
    ]));
    
    assert!(extract_intent_from_rtfs(&invalid_expr).is_err());
}

#[test]
fn test_round_trip_conversion() {
    // Test intent round trip: Intent -> RTFS -> Intent
    let original_intent = Intent {
        intent_id: "test-intent".to_string(),
        name: Some("test-intent".to_string()),
        original_request: "Test request".to_string(),
        goal: "Test goal".to_string(),
        constraints: HashMap::from([
            ("max_cost".to_string(), Value::Integer(100)),
        ]),
        preferences: HashMap::from([
            ("priority".to_string(), Value::String("high".to_string())),
        ]),
        success_criteria: Some(Value::String("success".to_string())),
        status: IntentStatus::Active,
        created_at: 1234567890,
        updated_at: 1234567890,
        metadata: HashMap::new(),
    };

    // Convert to RTFS function call
    let rtfs_expr = intent_to_rtfs_function_call(&original_intent).expect("Failed to convert to RTFS");
    
    // Convert back to Intent
    let converted_intent = extract_intent_from_rtfs(&rtfs_expr).expect("Failed to extract from RTFS");
    
    // Compare key fields (ignoring generated IDs and timestamps)
    assert_eq!(converted_intent.name, original_intent.name);
    assert_eq!(converted_intent.goal, original_intent.goal);
    assert_eq!(converted_intent.constraints, original_intent.constraints);
    assert_eq!(converted_intent.preferences, original_intent.preferences);
    assert_eq!(converted_intent.success_criteria, original_intent.success_criteria);
    assert_eq!(converted_intent.status, original_intent.status);

    // Test plan round trip: Plan -> RTFS -> Plan
    let original_plan = Plan {
        plan_id: "test-plan".to_string(),
        name: Some("test-plan".to_string()),
        intent_ids: vec!["intent-1".to_string()],
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs("(do (println \"hello\"))".to_string()),
        status: PlanStatus::Draft,
        created_at: 1234567890,
        metadata: HashMap::new(),
        input_schema: Some(Value::Map(HashMap::from([
            (MapKey::String("type".to_string()), Value::String("object".to_string())),
        ]))),
        output_schema: Some(Value::Map(HashMap::from([
            (MapKey::String("type".to_string()), Value::String("string".to_string())),
        ]))),
        policies: HashMap::from([
            ("max_cost".to_string(), Value::Integer(100)),
        ]),
        capabilities_required: vec!["http".to_string()],
        annotations: HashMap::from([
            ("prompt_id".to_string(), Value::String("prompt-123".to_string())),
        ]),
    };

    // Convert to RTFS function call
    let rtfs_expr = plan_to_rtfs_function_call(&original_plan).expect("Failed to convert to RTFS");
    
    // Convert back to Plan
    let converted_plan = extract_plan_from_rtfs(&rtfs_expr).expect("Failed to extract from RTFS");
    
    // Compare key fields (ignoring generated IDs and timestamps)
    assert_eq!(converted_plan.name, original_plan.name);
    assert_eq!(converted_plan.intent_ids, original_plan.intent_ids);
    assert_eq!(converted_plan.language, original_plan.language);
    assert_eq!(converted_plan.status, original_plan.status);
    assert_eq!(converted_plan.input_schema, original_plan.input_schema);
    assert_eq!(converted_plan.output_schema, original_plan.output_schema);
    assert_eq!(converted_plan.policies, original_plan.policies);
    assert_eq!(converted_plan.capabilities_required, original_plan.capabilities_required);
    assert_eq!(converted_plan.annotations, original_plan.annotations);
    
    // Compare body
    if let (PlanBody::Rtfs(original_code), PlanBody::Rtfs(converted_code)) = (&original_plan.body, &converted_plan.body) {
        assert_eq!(original_code, converted_code);
    } else {
        panic!("Expected RTFS bodies");
    }
}
