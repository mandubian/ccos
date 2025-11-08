use rtfs::ast::{
    Keyword, MapKey, MapTypeEntry, PrimitiveType, TypeExpr,
};
use rtfs::runtime::type_validator::TypeValidator;
use rtfs::runtime::values::Value;
use std::collections::HashMap;

/// Test complex structural types for RTFS language type system
/// This test validates the type system for complex nested structures like
/// vectors of maps with specific field requirements and wildcards

#[test]
fn test_complex_structural_types_github_issue_like() {
    let validator = TypeValidator::new();

    // Define the complex type from the user's example:
    // [:vector [:map [:id :int] [:number :int] [:title :string] [:state :string]
    //                [:labels [:vector :string]] [:estimate :int] [:html_url :string]
    //                [:comment_id :int] [:issue_id :int] [:author :string] [:body :string]
    //                [:* :any]]]

    // First, define the map type for individual issue objects
    let issue_map_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("id"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("number"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("title"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("state"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("labels"),
                value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String)))),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("estimate"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("html_url"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("comment_id"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("issue_id"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("author"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("body"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: Some(Box::new(TypeExpr::Any)), // [:* :any] allows additional fields
    };

    // The full type: vector of issue maps
    let issues_vector_type = TypeExpr::Vector(Box::new(issue_map_type));

    // Create valid test data - a vector containing two valid issue maps
    let mut issue1 = HashMap::new();
    issue1.insert(MapKey::Keyword(Keyword::new("id")), Value::Integer(1));
    issue1.insert(MapKey::Keyword(Keyword::new("number")), Value::Integer(123));
    issue1.insert(MapKey::Keyword(Keyword::new("title")), Value::String("Fix bug in parser".to_string()));
    issue1.insert(MapKey::Keyword(Keyword::new("state")), Value::String("open".to_string()));
    issue1.insert(MapKey::Keyword(Keyword::new("labels")), Value::Vector(vec![
        Value::String("bug".to_string()),
        Value::String("parser".to_string()),
    ]));
    issue1.insert(MapKey::Keyword(Keyword::new("estimate")), Value::Integer(5));
    issue1.insert(MapKey::Keyword(Keyword::new("html_url")), Value::String("https://github.com/org/repo/issues/123".to_string()));
    issue1.insert(MapKey::Keyword(Keyword::new("comment_id")), Value::Integer(456));
    issue1.insert(MapKey::Keyword(Keyword::new("issue_id")), Value::Integer(123));
    issue1.insert(MapKey::Keyword(Keyword::new("author")), Value::String("developer1".to_string()));
    issue1.insert(MapKey::Keyword(Keyword::new("body")), Value::String("This is a bug that needs fixing".to_string()));
    // Additional field allowed by wildcard
    issue1.insert(MapKey::Keyword(Keyword::new("created_at")), Value::String("2024-01-01".to_string()));

    let mut issue2 = HashMap::new();
    issue2.insert(MapKey::Keyword(Keyword::new("id")), Value::Integer(2));
    issue2.insert(MapKey::Keyword(Keyword::new("number")), Value::Integer(124));
    issue2.insert(MapKey::Keyword(Keyword::new("title")), Value::String("Add new feature".to_string()));
    issue2.insert(MapKey::Keyword(Keyword::new("state")), Value::String("closed".to_string()));
    issue2.insert(MapKey::Keyword(Keyword::new("labels")), Value::Vector(vec![
        Value::String("enhancement".to_string()),
    ]));
    issue2.insert(MapKey::Keyword(Keyword::new("estimate")), Value::Integer(8));
    issue2.insert(MapKey::Keyword(Keyword::new("html_url")), Value::String("https://github.com/org/repo/issues/124".to_string()));
    issue2.insert(MapKey::Keyword(Keyword::new("comment_id")), Value::Integer(789));
    issue2.insert(MapKey::Keyword(Keyword::new("issue_id")), Value::Integer(124));
    issue2.insert(MapKey::Keyword(Keyword::new("author")), Value::String("developer2".to_string()));
    issue2.insert(MapKey::Keyword(Keyword::new("body")), Value::String("We need this new feature".to_string()));

    let valid_issues = Value::Vector(vec![Value::Map(issue1), Value::Map(issue2)]);

    // This should validate successfully
    assert!(validator
        .validate_value(&valid_issues, &issues_vector_type)
        .is_ok());
}

#[test]
fn test_complex_structural_types_missing_required_fields() {
    let validator = TypeValidator::new();

    // Same type definition as above
    let issue_map_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("id"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("title"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("state"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    };

    let issues_vector_type = TypeExpr::Vector(Box::new(issue_map_type));

    // Create invalid test data - missing required "title" field
    let mut invalid_issue = HashMap::new();
    invalid_issue.insert(MapKey::Keyword(Keyword::new("id")), Value::Integer(1));
    // missing title
    invalid_issue.insert(MapKey::Keyword(Keyword::new("state")), Value::String("open".to_string()));

    let invalid_issues = Value::Vector(vec![Value::Map(invalid_issue)]);

    // This should fail validation
    assert!(validator
        .validate_value(&invalid_issues, &issues_vector_type)
        .is_err());
}

#[test]
fn test_complex_structural_types_wrong_field_types() {
    let validator = TypeValidator::new();

    // Same type definition
    let issue_map_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("id"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("title"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    };

    let issues_vector_type = TypeExpr::Vector(Box::new(issue_map_type));

    // Create invalid test data - wrong type for "id" field (string instead of int)
    let mut invalid_issue = HashMap::new();
    invalid_issue.insert(MapKey::Keyword(Keyword::new("id")), Value::String("not-an-int".to_string()));
    invalid_issue.insert(MapKey::Keyword(Keyword::new("title")), Value::String("Valid title".to_string()));

    let invalid_issues = Value::Vector(vec![Value::Map(invalid_issue)]);

    // This should fail validation
    assert!(validator
        .validate_value(&invalid_issues, &issues_vector_type)
        .is_err());
}

#[test]
fn test_complex_structural_types_nested_vector_wrong_type() {
    let validator = TypeValidator::new();

    // Type with nested vector that should contain strings
    let issue_map_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("labels"),
                value_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String)))),
                optional: false,
            },
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    };

    let issues_vector_type = TypeExpr::Vector(Box::new(issue_map_type));

    // Create invalid test data - labels vector contains integers instead of strings
    let mut invalid_issue = HashMap::new();
    invalid_issue.insert(MapKey::Keyword(Keyword::new("labels")), Value::Vector(vec![
        Value::Integer(1),  // should be string
        Value::Integer(2),  // should be string
    ]));

    let invalid_issues = Value::Vector(vec![Value::Map(invalid_issue)]);

    // This should fail validation
    assert!(validator
        .validate_value(&invalid_issues, &issues_vector_type)
        .is_err());
}

#[test]
fn test_complex_structural_types_comparison_different_map_structures() {
    let validator = TypeValidator::new();

    // Type 1: Issue-like structure
    let issue_map_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("id"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("title"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    };

    // Type 2: Different structure - person with name and age
    let person_map_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("name"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("age"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
        ],
        wildcard: None,
    };

    let issues_vector_type = TypeExpr::Vector(Box::new(issue_map_type));
    let people_vector_type = TypeExpr::Vector(Box::new(person_map_type));

    // Create issue data
    let mut issue = HashMap::new();
    issue.insert(MapKey::Keyword(Keyword::new("id")), Value::Integer(1));
    issue.insert(MapKey::Keyword(Keyword::new("title")), Value::String("Test issue".to_string()));

    // Create person data
    let mut person = HashMap::new();
    person.insert(MapKey::Keyword(Keyword::new("name")), Value::String("John".to_string()));
    person.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(30));

    let issues_data = Value::Vector(vec![Value::Map(issue)]);
    let people_data = Value::Vector(vec![Value::Map(person)]);

    // Issue data should validate against issue type but not person type
    assert!(validator.validate_value(&issues_data, &issues_vector_type).is_ok());
    assert!(validator.validate_value(&issues_data, &people_vector_type).is_err());

    // Person data should validate against person type but not issue type
    assert!(validator.validate_value(&people_data, &people_vector_type).is_ok());
    assert!(validator.validate_value(&people_data, &issues_vector_type).is_err());
}

#[test]
fn test_map_type_subset_relationships() {
    let validator = TypeValidator::new();

    // Type A: Minimal person with just name (subset - fewer required fields)
    let minimal_person_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("name"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: None,
    };

    // Type B: Full person with name and age (superset - more required fields)
    let full_person_type = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("name"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("age"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                optional: false,
            },
        ],
        wildcard: None,
    };

    // Create data that matches the minimal type (only has name)
    let mut minimal_person = HashMap::new();
    minimal_person.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Alice".to_string()),
    );
    let minimal_person_value = Value::Map(minimal_person);

    // Create data that matches the full type (has both name and age)
    let mut full_person = HashMap::new();
    full_person.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Bob".to_string()),
    );
    full_person.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(30));
    let full_person_value = Value::Map(full_person);

    // Test current behavior: no subtyping relationship
    // Minimal data validates against minimal type
    assert!(validator
        .validate_value(&minimal_person_value, &minimal_person_type)
        .is_ok());

    // Full data validates against full type
    assert!(validator
        .validate_value(&full_person_value, &full_person_type)
        .is_ok());

    // Minimal data does NOT validate against full type (missing required "age" field)
    assert!(validator
        .validate_value(&minimal_person_value, &full_person_type)
        .is_err());

    // Full data validates against minimal type (extra "age" field is allowed - no wildcard checking)
    assert!(validator
        .validate_value(&full_person_value, &minimal_person_type)
        .is_ok());
}