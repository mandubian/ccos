# RTFS 2.0 Comprehensive Test Plan

## Executive Summary
**Current Coverage**: 143 passing tests (~85% comprehensive)
**Critical Gaps**: Type system edge cases, pattern matching complexity
**Priority**: Focus on high-impact, production-critical scenarios

## 1. Type System Edge Cases (HIGH PRIORITY)

### Union Types Testing
**File**: `rtfs/tests/test_union_types.rs` (NEW)
**Status**: Not implemented
**Priority**: High

```rust
#[test]
fn test_union_type_validation_success() {
    // [:union :int :string] should accept both int and string
    let union_type = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Primitive(PrimitiveType::String)
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
        TypeExpr::Primitive(PrimitiveType::String)
    ]);
    
    let validator = TypeValidator::new();
    
    // Should reject boolean
    assert!(validator.validate_value(&Value::Boolean(true), &union_type).is_err());
}

#[test]
fn test_union_type_with_complex_types() {
    // [:union [:vector :int] [:map {:x :int}]]
    let complex_union = TypeExpr::Union(vec![
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::Int))),
        TypeExpr::Map(vec![MapTypeEntry {
            key: MapKey::Keyword(Keyword::new("x")),
            value: TypeExpr::Primitive(PrimitiveType::Int),
            required: true
        }])
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept vector of ints
    assert!(validator.validate_value(
        &Value::Vector(vec![Value::Integer(1), Value::Integer(2)]),
        &complex_union
    ).is_ok());
}
```

### Refined Types Testing
**File**: `rtfs/tests/test_refined_types.rs` (NEW)
**Status**: Not implemented
**Priority**: High

```rust
#[test]
fn test_refined_type_boundary_conditions() {
    // [:and :int [:> 0] [:< 100]] - int between 1 and 99
    let refined_type = TypeExpr::And(vec![
        TypeExpr::Primitive(PrimitiveType::Int),
        TypeExpr::Predicate(TypePredicate::GreaterThan(0)),
        TypeExpr::Predicate(TypePredicate::LessThan(100))
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept valid values
    assert!(validator.validate_value(&Value::Integer(50), &refined_type).is_ok());
    assert!(validator.validate_value(&Value::Integer(1), &refined_type).is_ok());
    assert!(validator.validate_value(&Value::Integer(99), &refined_type).is_ok());
    
    // Should reject boundary violations
    assert!(validator.validate_value(&Value::Integer(0), &refined_type).is_err());  // Too low
    assert!(validator.validate_value(&Value::Integer(100), &refined_type).is_err()); // Too high
    assert!(validator.validate_value(&Value::Integer(-1), &refined_type).is_err()); // Negative
}

#[test]
fn test_refined_type_with_string_patterns() {
    // [:and :string [:matches "^[A-Z].*$"]] - strings starting with uppercase
    let refined_string = TypeExpr::And(vec![
        TypeExpr::Primitive(PrimitiveType::String),
        TypeExpr::Predicate(TypePredicate::Regex("^[A-Z].*$".to_string()))
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept valid strings
    assert!(validator.validate_value(&Value::String("Hello".to_string()), &refined_string).is_ok());
    
    // Should reject invalid strings
    assert!(validator.validate_value(&Value::String("hello".to_string()), &refined_string).is_err());
}
```

### Optional Types Testing
**File**: `rtfs/tests/test_optional_types.rs` (NEW)
**Status**: Not implemented
**Priority**: High

```rust
#[test]
fn test_optional_type_validation() {
    // :string? equivalent to [:union :string :nil]
    let optional_string = TypeExpr::Union(vec![
        TypeExpr::Primitive(PrimitiveType::String),
        TypeExpr::Primitive(PrimitiveType::Nil)
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept string
    assert!(validator.validate_value(&Value::String("test".to_string()), &optional_string).is_ok());
    
    // Should accept nil
    assert!(validator.validate_value(&Value::Nil, &optional_string).is_ok());
    
    // Should reject other types
    assert!(validator.validate_value(&Value::Integer(42), &optional_string).is_err());
}

#[test]
fn test_optional_type_in_complex_structures() {
    // [:map {:name :string :age :int?}]
    let complex_map = TypeExpr::Map(vec![
        MapTypeEntry {
            key: MapKey::Keyword(Keyword::new("name")),
            value: TypeExpr::Primitive(PrimitiveType::String),
            required: true
        },
        MapTypeEntry {
            key: MapKey::Keyword(Keyword::new("age")),
            value: TypeExpr::Union(vec![
                TypeExpr::Primitive(PrimitiveType::Int),
                TypeExpr::Primitive(PrimitiveType::Nil)
            ]),
            required: false
        }
    ]);
    
    let validator = TypeValidator::new();
    
    // Should accept map with age
    let mut map_with_age = HashMap::new();
    map_with_age.insert(Value::Keyword(Keyword::new("name")), Value::String("Alice".to_string()));
    map_with_age.insert(Value::Keyword(Keyword::new("age")), Value::Integer(30));
    assert!(validator.validate_value(&Value::Map(map_with_age), &complex_map).is_ok());
    
    // Should accept map without age (optional)
    let mut map_without_age = HashMap::new();
    map_without_age.insert(Value::Keyword(Keyword::new("name")), Value::String("Bob".to_string()));
    assert!(validator.validate_value(&Value::Map(map_without_age), &complex_map).is_ok());
}
```

## 2. Pattern Matching Edge Cases (HIGH PRIORITY)

### Complex Destructuring Patterns
**File**: `rtfs/tests/test_complex_destructuring.rs` (NEW)
**Status**: Not implemented
**Priority**: High

```rust
#[test]
fn test_deeply_nested_destructuring() {
    let code = r#"
        (let [[[a b] [c d]] [[1 2] [3 4]]]
          (+ a b c d))
    "#;
    
    // Should evaluate to 10 (1+2+3+4)
    let result = evaluate_code(code);
    assert_eq!(result, Value::Integer(10));
}

#[test]
fn test_destructuring_with_type_guards() {
    let code = r#"
        (match {:value 42 :type :int}
          {:value (:int v) :type :int} v
          {:value (:string s) :type :string} s
          _ :unknown)
    "#;
    
    // Should return 42
    let result = evaluate_code(code);
    assert_eq!(result, Value::Integer(42));
}

#[test]
fn test_destructuring_exhaustiveness() {
    let code = r#"
        (match :unhandled-case
          :case1 1
          :case2 2)
    "#;
    
    // Should handle unmatched case gracefully or throw appropriate error
    let result = evaluate_code(code);
    // Expect either nil or error for unmatched pattern
    assert!(matches!(result, Value::Nil) || matches!(result, Value::Error(_)));
}
```

### Pattern Matching Performance
**File**: `rtfs/tests/test_pattern_performance.rs` (NEW)
**Status**: Not implemented
**Priority**: Medium

```rust
#[test]
fn test_pattern_matching_with_large_data() {
    // Create large vector for performance testing
    let large_vector = (0..1000).map(|i| Value::Integer(i)).collect::<Vec<_>>();
    
    let code = format!("
        (match {}
          [:vector & items] (count items)
          _ 0)
    ", format_vector(&large_vector));
    
    // Should handle large data efficiently
    let start = Instant::now();
    let result = evaluate_code(&code);
    let duration = start.elapsed();
    
    assert_eq!(result, Value::Integer(1000));
    assert!(duration.as_millis() < 100, "Pattern matching should be fast even with large data");
}
```

## 3. Error Handling Testing (MEDIUM PRIORITY)

### Type Validation Errors
**File**: `rtfs/tests/test_error_handling.rs` (NEW)
**Status**: Not implemented
**Priority**: Medium

```rust
#[test]
fn test_type_validation_error_messages() {
    let validator = TypeValidator::new();
    let int_type = TypeExpr::Primitive(PrimitiveType::Int);
    
    let result = validator.validate_value(&Value::String("not-a-number".to_string()), &int_type);
    
    match result {
        Err(ValidationError::TypeMismatch { expected, actual }) => {
            assert_eq!(expected, "int");
            assert_eq!(actual, "string");
        }
        _ => panic!("Expected type mismatch error")
    }
}

#[test]
fn test_pattern_matching_error_recovery() {
    let code = r#"
        (try
          (match :test
            :case1 (throw "error")
            :case2 42)
          (catch e
            "recovered"))
    "#;
    
    let result = evaluate_code(code);
    assert_eq!(result, Value::String("recovered".to_string()));
}
```

## 4. Metadata System Testing (LOW PRIORITY)

### Complex Metadata Structures
**File**: `rtfs/tests/test_metadata_complex.rs` (NEW)
**Status**: Not implemented
**Priority**: Low

```rust
#[test]
fn test_metadata_with_nested_structures() {
    let code = r#"
        ^{:doc "Complex function"
          :params {:x {:type :int :desc "input value"}
                   :y {:type :int :desc "multiplier"}}
          :returns {:type :int :desc "result"}}
        (defn complex-fn [x y]
          (* x y))
    "#;
    
    // Should parse and store complex metadata
    let parsed = parse(code);
    assert!(parsed.is_ok());
    
    if let TopLevel::Defn(defn) = &parsed.unwrap()[0] {
        let metadata = defn.metadata.as_ref().unwrap();
        assert!(metadata.contains_key("doc"));
        assert!(metadata.contains_key("params"));
        assert!(metadata.contains_key("returns"));
    } else {
        panic!("Expected defn with metadata");
    }
}
```

## 5. Collection Edge Cases (MEDIUM PRIORITY)

### Large Collection Performance
**File**: `rtfs/tests/test_large_collections.rs` (NEW)
**Status**: Not implemented
**Priority**: Medium

```rust
#[test]
fn test_very_large_vector_operations() {
    // Test with vector of 10,000 elements
    let large_code = format!("
        (let [data {}]
          (count data))
    ", format_large_vector(10000));
    
    let start = Instant::now();
    let result = evaluate_code(&large_code);
    let duration = start.elapsed();
    
    assert_eq!(result, Value::Integer(10000));
    assert!(duration.as_millis() < 500, "Large collection operations should be efficient");
}

#[test]
fn test_circular_reference_detection() {
    // This should be handled gracefully by the runtime
    let code = r#"
        (let [a []
              b a]
          (conj b a))  ; Try to create circular reference
    "#;
    
    let result = evaluate_code(code);
    // Should either succeed with proper handling or fail gracefully
    assert!(matches!(result, Value::Vector(_)) || matches!(result, Value::Error(_)));
}
```

## Implementation Priority

### Phase 1 (Critical - Next Sprint)
1. **Union Types Testing** - High priority for type safety
2. **Refined Types Testing** - Critical for data validation
3. **Optional Types Testing** - Essential for nullable patterns
4. **Complex Destructuring** - Important for pattern matching

### Phase 2 (Important - Following Sprint)
1. **Error Handling** - Robust error recovery
2. **Pattern Matching Performance** - Scalability
3. **Large Collection Operations** - Performance testing

### Phase 3 (Nice-to-have - Future)
1. **Metadata Complex Structures** - Documentation features
2. **Circular Reference Detection** - Edge case handling
3. **Symbol Edge Cases** - Unicode and boundary conditions

## Test Quality Standards

1. **Isolation**: Each test focuses on one specific feature
2. **Clarity**: Descriptive test names and comments
3. **Completeness**: Cover success and failure cases
4. **Performance**: Include performance benchmarks where relevant
5. **Documentation**: Add inline documentation for complex tests

## Expected Impact

- **Improved Reliability**: Better error handling and edge case coverage
- **Enhanced Type Safety**: Comprehensive type system validation
- **Production Readiness**: Robust pattern matching and collection handling
- **Developer Confidence**: Clear examples of correct usage patterns

**Estimated Test Count**: ~50-75 new tests
**Coverage Improvement**: From ~85% to ~95% comprehensive
**Risk Reduction**: Significant improvement in edge case handling

## Current Status Tracker

| Test Area | Status | Priority | Estimated Tests |
|-----------|--------|----------|-----------------|
| Union Types | ❌ Not implemented | High | 8-12 tests |
| Refined Types | ❌ Not implemented | High | 10-15 tests |
| Optional Types | ❌ Not implemented | High | 8-12 tests |
| Complex Destructuring | ❌ Not implemented | High | 12-18 tests |
| Error Handling | ❌ Not implemented | Medium | 10-15 tests |
| Pattern Performance | ❌ Not implemented | Medium | 5-8 tests |
| Large Collections | ❌ Not implemented | Medium | 6-10 tests |
| Metadata Complex | ❌ Not implemented | Low | 4-6 tests |

**Total Estimated New Tests**: ~63-91 tests
**Current Coverage**: 143 tests
**Target Coverage**: ~206-234 tests
**Coverage Improvement**: ~44% increase

## Implementation Notes

1. **Test Framework**: Use existing RTFS test infrastructure
2. **Code Quality**: Follow existing test patterns and conventions
3. **Documentation**: Add comprehensive inline comments
4. **Performance**: Include benchmarks for critical operations
5. **Error Handling**: Test both success and failure scenarios

## Risk Assessment

**High Risk Areas**:
- Type system edge cases (union/refined/optional types)
- Pattern matching complexity (nested destructuring)
- Error handling and recovery patterns

**Mitigation Strategy**:
- Implement comprehensive test coverage first
- Add thorough documentation and examples
- Include performance benchmarks
- Test error conditions explicitly

## Success Criteria

✅ **Phase 1 Complete**: Union, Refined, Optional types fully tested
✅ **Phase 2 Complete**: Error handling and performance tests implemented
✅ **Phase 3 Complete**: Metadata and edge case tests completed
✅ **Coverage Target**: 95%+ comprehensive test coverage achieved
✅ **Quality Target**: All tests passing with clear documentation

**Overall Status**: 📋 Planning Complete, Ready for Implementation