use rtfs::ast::{
    ArrayDimension, Keyword, Literal, MapKey, MapTypeEntry, PrimitiveType, Symbol, TypeExpr,
    TypePredicate,
};
use rtfs::runtime::type_validator::{TypeValidator, ValidationError};
use rtfs::runtime::values::Value;
use std::collections::HashMap;

#[test]
fn test_primitive_types() {
    let validator = TypeValidator::new();

    // Test integer type
    assert!(validator
        .validate_value(
            &Value::Integer(42),
            &TypeExpr::Primitive(PrimitiveType::Int)
        )
        .is_ok());

    // Test float type
    assert!(validator
        .validate_value(
            &Value::Float(3.14),
            &TypeExpr::Primitive(PrimitiveType::Float)
        )
        .is_ok());

    // Test string type
    assert!(validator
        .validate_value(
            &Value::String("hello".to_string()),
            &TypeExpr::Primitive(PrimitiveType::String)
        )
        .is_ok());

    // Test boolean type
    assert!(validator
        .validate_value(
            &Value::Boolean(true),
            &TypeExpr::Primitive(PrimitiveType::Bool)
        )
        .is_ok());

    // Test nil type
    assert!(validator
        .validate_value(&Value::Nil, &TypeExpr::Primitive(PrimitiveType::Nil))
        .is_ok());

    // Test keyword type
    assert!(validator
        .validate_value(
            &Value::Keyword(Keyword::new("test")),
            &TypeExpr::Primitive(PrimitiveType::Keyword)
        )
        .is_ok());

    // Test symbol type
    assert!(validator
        .validate_value(
            &Value::Symbol(Symbol::new("test")),
            &TypeExpr::Primitive(PrimitiveType::Symbol)
        )
        .is_ok());
}

#[test]
fn test_type_mismatches() {
    let validator = TypeValidator::new();

    // String provided where integer expected
    assert!(validator
        .validate_value(
            &Value::String("hello".to_string()),
            &TypeExpr::Primitive(PrimitiveType::Int)
        )
        .is_err());

    // Integer provided where string expected
    assert!(validator
        .validate_value(
            &Value::Integer(42),
            &TypeExpr::Primitive(PrimitiveType::String)
        )
        .is_err());
}

#[test]
fn test_vector_types() {
    let validator = TypeValidator::new();

    let int_vector_type = TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::Int)));

    // Valid vector of integers
    let valid_vector = Value::Vector(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);

    assert!(validator
        .validate_value(&valid_vector, &int_vector_type)
        .is_ok());

    // Invalid vector with mixed types
    let invalid_vector = Value::Vector(vec![
        Value::Integer(1),
        Value::String("hello".to_string()),
        Value::Integer(3),
    ]);

    assert!(validator
        .validate_value(&invalid_vector, &int_vector_type)
        .is_err());
}

#[test]
fn test_array_shapes() {
    let validator = TypeValidator::new();

    // Fixed size array [3]
    let fixed_array_type = TypeExpr::Array {
        element_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
        shape: vec![ArrayDimension::Fixed(3)],
    };

    // Valid array with correct size
    let valid_array = Value::Vector(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);

    assert!(validator
        .validate_value(&valid_array, &fixed_array_type)
        .is_ok());

    // Invalid array with wrong size
    let invalid_array = Value::Vector(vec![Value::Integer(1), Value::Integer(2)]);

    assert!(validator
        .validate_value(&invalid_array, &fixed_array_type)
        .is_err());

    // Variable size array [?]
    let variable_array_type = TypeExpr::Array {
        element_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
        shape: vec![ArrayDimension::Variable],
    };

    // Both arrays should be valid for variable size
    assert!(validator
        .validate_value(&valid_array, &variable_array_type)
        .is_ok());
    assert!(validator
        .validate_value(&invalid_array, &variable_array_type)
        .is_ok());
}

#[test]
fn test_refined_types() {
    let validator = TypeValidator::new();

    // Positive integer: [:and :int [:> 0]]
    let positive_int_type = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
        predicates: vec![TypePredicate::GreaterThan(Literal::Integer(0))],
    };

    // Valid positive integer
    assert!(validator
        .validate_value(&Value::Integer(5), &positive_int_type)
        .is_ok());

    // Invalid: zero
    assert!(validator
        .validate_value(&Value::Integer(0), &positive_int_type)
        .is_err());

    // Invalid: negative
    assert!(validator
        .validate_value(&Value::Integer(-1), &positive_int_type)
        .is_err());

    // String with minimum length: [:and :string [:min-length 3]]
    let min_length_string_type = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
        predicates: vec![TypePredicate::MinLength(3)],
    };

    // Valid long string
    assert!(validator
        .validate_value(&Value::String("hello".to_string()), &min_length_string_type)
        .is_ok());

    // Invalid short string
    assert!(validator
        .validate_value(&Value::String("hi".to_string()), &min_length_string_type)
        .is_err());
}

#[test]
fn test_enum_types() {
    let validator = TypeValidator::new();

    // Color enum: [:enum :red :green :blue]
    let color_enum_type = TypeExpr::Enum(vec![
        Literal::Keyword(Keyword::new("red")),
        Literal::Keyword(Keyword::new("green")),
        Literal::Keyword(Keyword::new("blue")),
    ]);

    // Valid color
    assert!(validator
        .validate_value(&Value::Keyword(Keyword::new("red")), &color_enum_type)
        .is_ok());
    assert!(validator
        .validate_value(&Value::Keyword(Keyword::new("green")), &color_enum_type)
        .is_ok());
    assert!(validator
        .validate_value(&Value::Keyword(Keyword::new("blue")), &color_enum_type)
        .is_ok());

    // Invalid color
    assert!(validator
        .validate_value(&Value::Keyword(Keyword::new("yellow")), &color_enum_type)
        .is_err());

    // Wrong type
    assert!(validator
        .validate_value(&Value::String("red".to_string()), &color_enum_type)
        .is_err());
}

#[test]
fn test_union_types() {
    let validator = TypeValidator::new();

    // Union type: [:union :int :string]
    let int_or_string_type = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String),
    ]);

    // Valid integer
    assert!(validator
        .validate_value(&Value::Integer(42), &int_or_string_type)
        .is_ok());

    // Valid string
    assert!(validator
        .validate_value(&Value::String("hello".to_string()), &int_or_string_type)
        .is_ok());

    // Invalid type (boolean)
    assert!(validator
        .validate_value(&Value::Boolean(true), &int_or_string_type)
        .is_err());
}

#[test]
fn test_optional_types() {
    let validator = TypeValidator::new();

    // Optional string: :string?
    let optional_string_type =
        TypeExpr::Optional(Box::new(TypeExpr::Primitive(PrimitiveType::String)));

    // Valid string
    assert!(validator
        .validate_value(&Value::String("hello".to_string()), &optional_string_type)
        .is_ok());

    // Valid nil
    assert!(validator
        .validate_value(&Value::Nil, &optional_string_type)
        .is_ok());

    // Invalid type (integer)
    assert!(validator
        .validate_value(&Value::Integer(42), &optional_string_type)
        .is_err());
}

#[test]
fn test_tuple_types() {
    let validator = TypeValidator::new();

    // Tuple type: [:tuple :int :string :bool]
    let tuple_type = TypeExpr::Tuple(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String),
        TypeExpr::Primitive(PrimitiveType::Bool),
    ]);

    // Valid tuple
    let valid_tuple = Value::Vector(vec![
        Value::Integer(42),
        Value::String("hello".to_string()),
        Value::Boolean(true),
    ]);

    assert!(validator.validate_value(&valid_tuple, &tuple_type).is_ok());

    // Invalid tuple - wrong length
    let wrong_length_tuple =
        Value::Vector(vec![Value::Integer(42), Value::String("hello".to_string())]);

    assert!(validator
        .validate_value(&wrong_length_tuple, &tuple_type)
        .is_err());

    // Invalid tuple - wrong types
    let wrong_type_tuple = Value::Vector(vec![
        Value::String("42".to_string()), // should be int
        Value::String("hello".to_string()),
        Value::Boolean(true),
    ]);

    assert!(validator
        .validate_value(&wrong_type_tuple, &tuple_type)
        .is_err());
}

#[test]
fn test_map_types() {
    let validator = TypeValidator::new();

    // Map type with required fields: [:map [:name :string] [:age :int]]
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

    // Valid person map
    let mut valid_person = HashMap::new();
    valid_person.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Alice".to_string()),
    );
    valid_person.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(30));
    let valid_person_value = Value::Map(valid_person);

    assert!(validator
        .validate_value(&valid_person_value, &person_map_type)
        .is_ok());

    // Invalid person map - missing required field
    let mut invalid_person = HashMap::new();
    invalid_person.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Bob".to_string()),
    );
    // missing age field
    let invalid_person_value = Value::Map(invalid_person);

    assert!(validator
        .validate_value(&invalid_person_value, &person_map_type)
        .is_err());

    // Invalid person map - wrong type for field
    let mut wrong_type_person = HashMap::new();
    wrong_type_person.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Charlie".to_string()),
    );
    wrong_type_person.insert(
        MapKey::Keyword(Keyword::new("age")),
        Value::String("30".to_string()),
    ); // should be int
    let wrong_type_person_value = Value::Map(wrong_type_person);

    assert!(validator
        .validate_value(&wrong_type_person_value, &person_map_type)
        .is_err());
}

#[test]
fn test_regex_predicate() {
    let validator = TypeValidator::new();

    // Email pattern: [:and :string [:matches-regex "^.+@.+\\..+$"]]
    let email_type = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
        predicates: vec![TypePredicate::MatchesRegex("^.+@.+\\..+$".to_string())],
    };

    // Valid email
    assert!(validator
        .validate_value(&Value::String("user@example.com".to_string()), &email_type)
        .is_ok());

    // Invalid email
    assert!(validator
        .validate_value(&Value::String("not-an-email".to_string()), &email_type)
        .is_err());
}

#[test]
fn test_range_predicates() {
    let validator = TypeValidator::new();

    // Number in range: [:and :int [:in-range 1 100]]
    let range_type = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
        predicates: vec![TypePredicate::InRange(
            Literal::Integer(1),
            Literal::Integer(100),
        )],
    };

    // Valid values in range
    assert!(validator
        .validate_value(&Value::Integer(1), &range_type)
        .is_ok());
    assert!(validator
        .validate_value(&Value::Integer(50), &range_type)
        .is_ok());
    assert!(validator
        .validate_value(&Value::Integer(100), &range_type)
        .is_ok());

    // Invalid values out of range
    assert!(validator
        .validate_value(&Value::Integer(0), &range_type)
        .is_err());
    assert!(validator
        .validate_value(&Value::Integer(101), &range_type)
        .is_err());
}

#[test]
fn test_collection_predicates() {
    let validator = TypeValidator::new();

    // Non-empty vector: [:and [:vector :string] [:non-empty]]
    let non_empty_vector_type = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(
            PrimitiveType::String,
        )))),
        predicates: vec![TypePredicate::NonEmpty],
    };

    // Valid non-empty vector
    let non_empty_vector = Value::Vector(vec![Value::String("hello".to_string())]);
    assert!(validator
        .validate_value(&non_empty_vector, &non_empty_vector_type)
        .is_ok());

    // Invalid empty vector
    let empty_vector = Value::Vector(vec![]);
    assert!(validator
        .validate_value(&empty_vector, &non_empty_vector_type)
        .is_err());

    // Min count: [:and [:vector :int] [:min-count 2]]
    let min_count_type = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Vector(Box::new(TypeExpr::Primitive(
            PrimitiveType::Int,
        )))),
        predicates: vec![TypePredicate::MinCount(2)],
    };

    // Valid vector with enough elements
    let enough_elements = Value::Vector(vec![
        Value::Integer(1),
        Value::Integer(2),
        Value::Integer(3),
    ]);
    assert!(validator
        .validate_value(&enough_elements, &min_count_type)
        .is_ok());

    // Invalid vector with too few elements
    let too_few_elements = Value::Vector(vec![Value::Integer(1)]);
    assert!(validator
        .validate_value(&too_few_elements, &min_count_type)
        .is_err());
}

#[test]
fn test_complex_nested_types() {
    let validator = TypeValidator::new();

    // Complex type: Array of person maps with constraints
    // [:array [:map [:name [:and :string [:min-length 1]]] [:age [:and :int [:>= 0]]]] [?]]
    let people_array_type = TypeExpr::Array {
        element_type: Box::new(TypeExpr::Map {
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
                    key: Keyword::new("age"),
                    value_type: Box::new(TypeExpr::Refined {
                        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
                        predicates: vec![TypePredicate::GreaterEqual(Literal::Integer(0))],
                    }),
                    optional: false,
                },
            ],
            wildcard: None,
        }),
        shape: vec![ArrayDimension::Variable],
    };

    // Valid array of people
    let mut person1 = HashMap::new();
    person1.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Alice".to_string()),
    );
    person1.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(30));

    let mut person2 = HashMap::new();
    person2.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Bob".to_string()),
    );
    person2.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(25));

    let valid_people = Value::Vector(vec![Value::Map(person1), Value::Map(person2)]);

    assert!(validator
        .validate_value(&valid_people, &people_array_type)
        .is_ok());

    // Invalid array - person with empty name
    let mut invalid_person = HashMap::new();
    invalid_person.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("".to_string()),
    ); // empty name
    invalid_person.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(30));

    let invalid_people = Value::Vector(vec![Value::Map(invalid_person)]);

    assert!(validator
        .validate_value(&invalid_people, &people_array_type)
        .is_err());

    // Invalid array - person with negative age
    let mut negative_age_person = HashMap::new();
    negative_age_person.insert(
        MapKey::Keyword(Keyword::new("name")),
        Value::String("Charlie".to_string()),
    );
    negative_age_person.insert(MapKey::Keyword(Keyword::new("age")), Value::Integer(-5)); // negative age

    let negative_age_people = Value::Vector(vec![Value::Map(negative_age_person)]);

    assert!(validator
        .validate_value(&negative_age_people, &people_array_type)
        .is_err());
}

#[test]
fn test_any_and_never_types() {
    let validator = TypeValidator::new();

    // Any type accepts everything
    let any_type = TypeExpr::Any;
    assert!(validator
        .validate_value(&Value::Integer(42), &any_type)
        .is_ok());
    assert!(validator
        .validate_value(&Value::String("hello".to_string()), &any_type)
        .is_ok());
    assert!(validator
        .validate_value(&Value::Boolean(true), &any_type)
        .is_ok());
    assert!(validator.validate_value(&Value::Nil, &any_type).is_ok());

    // Never type accepts nothing
    let never_type = TypeExpr::Never;
    assert!(validator
        .validate_value(&Value::Integer(42), &never_type)
        .is_err());
    assert!(validator
        .validate_value(&Value::String("hello".to_string()), &never_type)
        .is_err());
    assert!(validator.validate_value(&Value::Nil, &never_type).is_err());
}

#[test]
fn test_type_parsing() {
    // Test that we can parse type expressions from strings

    // Basic types
    assert!(matches!(
        TypeExpr::from_str(":int"),
        Ok(TypeExpr::Primitive(PrimitiveType::Int))
    ));
    assert!(matches!(
        TypeExpr::from_str(":string"),
        Ok(TypeExpr::Primitive(PrimitiveType::String))
    ));
    assert!(matches!(
        TypeExpr::from_str(":bool"),
        Ok(TypeExpr::Primitive(PrimitiveType::Bool))
    ));

    // Optional types
    assert!(matches!(
        TypeExpr::from_str(":int?"),
        Ok(TypeExpr::Optional(_))
    ));
    assert!(matches!(
        TypeExpr::from_str(":string?"),
        Ok(TypeExpr::Optional(_))
    ));

    // Type aliases
    assert!(matches!(
        TypeExpr::from_str("MyType"),
        Ok(TypeExpr::Alias(_))
    ));
    assert!(matches!(
        TypeExpr::from_str("my.namespace/MyType"),
        Ok(TypeExpr::Alias(_))
    ));
}

#[test]
fn test_error_messages() {
    let validator = TypeValidator::new();

    // Test that validation errors provide useful information
    let result = validator.validate_value(
        &Value::String("hello".to_string()),
        &TypeExpr::Primitive(PrimitiveType::Int),
    );

    assert!(result.is_err());

    if let Err(ValidationError::TypeMismatch {
        expected,
        actual,
        path,
    }) = result
    {
        assert_eq!(path, "");
        assert_eq!(actual, "string");
        assert!(matches!(expected, TypeExpr::Primitive(PrimitiveType::Int)));
    } else {
        panic!("Expected TypeMismatch error");
    }

    // Test predicate violation error
    let positive_int_type = TypeExpr::Refined {
        base_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
        predicates: vec![TypePredicate::GreaterThan(Literal::Integer(0))],
    };

    let result = validator.validate_value(&Value::Integer(-5), &positive_int_type);
    assert!(result.is_err());

    if let Err(ValidationError::PredicateViolation {
        predicate,
        value,
        path,
    }) = result
    {
        assert_eq!(path, "");
        assert_eq!(value, "-5");
        assert_eq!(predicate, "> 0");
    } else {
        panic!("Expected PredicateViolation error");
    }
}

#[test]
fn test_alias_optional_validation() {
    let validator = TypeValidator::new();

    // Alias-style optional (string?) should accept string values
    let alias_optional = TypeExpr::Alias(Symbol("string?".to_string()));
    assert!(validator
        .validate_value(&Value::String("hello".to_string()), &alias_optional)
        .is_ok());

    // Nil is permitted for optional aliases
    assert!(validator
        .validate_value(&Value::Nil, &alias_optional)
        .is_ok());
}

#[test]
fn test_alias_optional_validation_capitalized_and_int() {
    let validator = TypeValidator::new();

    // Capitalized alias (String?) should behave similarly
    let alias_string = TypeExpr::Alias(Symbol("String?".to_string()));
    assert!(validator
        .validate_value(&Value::String("hello".to_string()), &alias_string)
        .is_ok());
    assert!(validator.validate_value(&Value::Nil, &alias_string).is_ok());

    // Integer alias optional
    let alias_int = TypeExpr::Alias(Symbol("Int?".to_string()));
    assert!(validator
        .validate_value(&Value::Integer(42), &alias_int)
        .is_ok());
    assert!(validator.validate_value(&Value::Nil, &alias_int).is_ok());
}
