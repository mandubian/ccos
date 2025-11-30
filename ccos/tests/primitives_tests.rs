use ccos::discovery::need_extractor::CapabilityNeed;
use ccos::synthesis::primitives::{
    executor::RestrictedRtfsExecutor, PrimitiveContext, PrimitiveRegistry, PrimitiveTemplateId,
};
use rtfs::ast::{Keyword, MapKey, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::values::Value;
use serde_json::json;

#[test]
fn filter_template_prefers_annotations() {
    let registry = PrimitiveRegistry::new();

    let need = CapabilityNeed::new(
        "demo.filter.by_topic".to_string(),
        vec!["items".to_string(), "topic".to_string()],
        vec!["filtered".to_string()],
        "Filter items by topic".to_string(),
    );

    let mut input_schemas = std::collections::HashMap::new();
    input_schemas.insert(
        "items".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );
    input_schemas.insert(
        "topic".to_string(),
        TypeExpr::Primitive(PrimitiveType::String),
    );

    let mut output_schemas = std::collections::HashMap::new();
    output_schemas.insert(
        "filtered".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );

    let annotations = json!({
        "primitive": {
            "kind": "filter",
            "collection_input": "items",
            "search_input": "topic",
            "output_key": "filtered",
            "search_fields": [":title", ":body"]
        }
    });

    let ctx = PrimitiveContext::new(&need, input_schemas, output_schemas, annotations);

    let synthesized = registry.synthesize(&ctx).expect("filter primitive");
    assert_eq!(synthesized.primitive_id, PrimitiveTemplateId::Filter);
    assert!(
        synthesized
            .rtfs_code
            .contains("(string-contains field_0 search-str)"),
        "filter RTFS should contain generated predicate lines"
    );
    assert!(
        synthesized.rtfs_code.contains("{:filtered filtered-items}"),
        "filter RTFS should emit filtered items under requested keyword"
    );
    match synthesized.input_schema {
        TypeExpr::Map { ref entries, .. } => {
            assert_eq!(entries.len(), 2);
            assert!(entries.iter().any(|entry| entry.key.0 == "items"));
        }
        _ => panic!("expected input schema to be a map"),
    }
    match synthesized.output_schema {
        TypeExpr::Map { ref entries, .. } => {
            assert!(entries.iter().any(|entry| entry.key.0 == "filtered"));
        }
        _ => panic!("expected output schema to be a map"),
    }
}

#[test]
fn filter_primitive_executes_in_restricted_runtime() {
    let registry = PrimitiveRegistry::new();
    let need = CapabilityNeed::new(
        "demo.filter.runtime".to_string(),
        vec!["issues".to_string(), "topic".to_string()],
        vec!["filtered".to_string()],
        "Filter items by topic".to_string(),
    );

    let mut inputs = std::collections::HashMap::new();
    inputs.insert("issues".to_string(), TypeExpr::Any);
    inputs.insert("topic".to_string(), TypeExpr::Any);

    let mut outputs = std::collections::HashMap::new();
    outputs.insert("filtered".to_string(), TypeExpr::Any);

    let annotations = json!({
        "primitive": {
            "kind": "filter",
            "collection_input": "issues",
            "search_input": "topic",
            "output_key": "filtered",
            "search_fields": [":title", ":body"]
        }
    });

    let ctx = PrimitiveContext::new(&need, inputs, outputs, annotations);
    let primitive = registry
        .synthesize(&ctx)
        .expect("filter primitive should synthesize");

    let executor = RestrictedRtfsExecutor::new();

    let mut issue_rtfs = std::collections::HashMap::new();
    issue_rtfs.insert(
        MapKey::Keyword(Keyword("title".to_string())),
        Value::String("Learning RTFS primitives".to_string()),
    );
    issue_rtfs.insert(
        MapKey::Keyword(Keyword("body".to_string())),
        Value::String("All about schema-aware primitives".to_string()),
    );

    let mut unrelated_issue = std::collections::HashMap::new();
    unrelated_issue.insert(
        MapKey::Keyword(Keyword("title".to_string())),
        Value::String("Weekend plans".to_string()),
    );
    unrelated_issue.insert(
        MapKey::Keyword(Keyword("body".to_string())),
        Value::String("Discuss hiking".to_string()),
    );

    let issues_value = Value::Vector(vec![Value::Map(issue_rtfs), Value::Map(unrelated_issue)]);

    let mut input_map = std::collections::HashMap::new();
    input_map.insert(MapKey::Keyword(Keyword("issues".to_string())), issues_value);
    input_map.insert(
        MapKey::Keyword(Keyword("topic".to_string())),
        Value::String("rtfs".to_string()),
    );

    let result = executor
        .evaluate(&primitive.rtfs_code, Value::Map(input_map))
        .expect("restricted runtime should execute primitive");

    let filtered_items = match result {
        Value::Map(map) => map
            .get(&MapKey::Keyword(Keyword("filtered".to_string())))
            .cloned()
            .expect("filtered key present"),
        _ => panic!("Expected map output"),
    };

    let vector = match filtered_items {
        Value::Vector(vec) => vec,
        _ => panic!("Expected filtered vector"),
    };

    assert_eq!(vector.len(), 1);
    match &vector[0] {
        Value::Map(map) => {
            let title = map
                .get(&MapKey::Keyword(Keyword("title".to_string())))
                .and_then(|v| match v {
                    Value::String(s) => Some(s.clone()),
                    _ => None,
                })
                .expect("title present");
            assert!(title.contains("RTFS"));
        }
        _ => panic!("Expected retained issue map"),
    }
}

#[test]
fn map_template_requires_mapping() {
    let registry = PrimitiveRegistry::new();

    // Missing mapping should fail.
    let missing_need = CapabilityNeed::new(
        "demo.map.missing.mapping".to_string(),
        vec!["items".to_string()],
        vec!["mapped".to_string()],
        "Map without specifying mapping".to_string(),
    );
    let mut missing_inputs = std::collections::HashMap::new();
    missing_inputs.insert(
        "items".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );
    let mut missing_outputs = std::collections::HashMap::new();
    missing_outputs.insert(
        "mapped".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );
    let missing_annotations = json!({ "primitive": { "kind": "map" } });
    let missing_mapping_ctx = PrimitiveContext::new(
        &missing_need,
        missing_inputs,
        missing_outputs,
        missing_annotations,
    );

    assert!(
        registry.synthesize(&missing_mapping_ctx).is_err(),
        "map primitive should error when mapping is absent"
    );

    // Valid mapping should succeed.
    let valid_need = CapabilityNeed::new(
        "demo.map.summary".to_string(),
        vec!["issues".to_string()],
        vec!["summaries".to_string()],
        "Map issues to summaries".to_string(),
    );
    let mut valid_inputs = std::collections::HashMap::new();
    valid_inputs.insert(
        "issues".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );
    let mut valid_outputs = std::collections::HashMap::new();
    valid_outputs.insert(
        "summaries".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );
    let valid_annotations = json!({
        "primitive": {
            "kind": "map",
            "collection_input": "issues",
            "output_key": "summaries",
            "mapping": {
                ":title": ":title",
                ":number": ":number"
            }
        }
    });
    let valid_mapping_ctx =
        PrimitiveContext::new(&valid_need, valid_inputs, valid_outputs, valid_annotations);

    let synthesized = registry
        .synthesize(&valid_mapping_ctx)
        .expect("map primitive");
    assert_eq!(synthesized.primitive_id, PrimitiveTemplateId::Map);
    assert!(
        synthesized.rtfs_code.contains("(:summaries mapped-items)")
            || synthesized.rtfs_code.contains("{:summaries mapped-items}"),
        "map RTFS should return mapped items under the requested output key"
    );
    assert!(matches!(synthesized.input_schema, TypeExpr::Map { .. }));
    assert!(matches!(synthesized.output_schema, TypeExpr::Map { .. }));
}

#[test]
fn project_template_keeps_requested_fields() {
    let registry = PrimitiveRegistry::new();

    let need = CapabilityNeed::new(
        "demo.project.fields".to_string(),
        vec!["issues".to_string()],
        vec!["subset".to_string()],
        "Project issues to subset".to_string(),
    );
    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "issues".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );
    let mut outputs = std::collections::HashMap::new();
    outputs.insert(
        "subset".to_string(),
        TypeExpr::Vector(Box::new(TypeExpr::Primitive(PrimitiveType::String))),
    );
    let annotations = json!({
        "primitive": {
            "kind": "project",
            "collection_input": "issues",
            "output_key": "subset",
            "fields": [":title", ":state"]
        }
    });

    let ctx = PrimitiveContext::new(&need, inputs, outputs, annotations);

    let synthesized = registry
        .synthesize(&ctx)
        .expect("project primitive should be synthesized");
    assert_eq!(synthesized.primitive_id, PrimitiveTemplateId::Project);
    assert!(
        synthesized.rtfs_code.contains(":subset"),
        "project RTFS should emit subset keyword"
    );
    assert!(
        synthesized
            .rtfs_code
            .contains("(assoc acc :title (get item :title))"),
        "project RTFS should include field copies"
    );
    assert!(matches!(synthesized.input_schema, TypeExpr::Map { .. }));
    assert!(matches!(synthesized.output_schema, TypeExpr::Map { .. }));
}

fn vector_of_map_with_fields(fields: Vec<(&str, TypeExpr)>) -> TypeExpr {
    TypeExpr::Vector(Box::new(TypeExpr::Map {
        entries: fields
            .into_iter()
            .map(|(name, ty)| MapTypeEntry {
                key: Keyword(name.trim_start_matches(':').to_string()),
                value_type: Box::new(ty),
                optional: false,
            })
            .collect(),
        wildcard: Some(Box::new(TypeExpr::Any)),
    }))
}

#[test]
fn reduce_template_generates_sum_rtfs() {
    let registry = PrimitiveRegistry::new();

    let need = CapabilityNeed::new(
        "demo.reduce.sum".to_string(),
        vec!["items".to_string()],
        vec!["total".to_string()],
        "Reduce items to a total".to_string(),
    );

    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "items".to_string(),
        vector_of_map_with_fields(vec![(":estimate", TypeExpr::Primitive(PrimitiveType::Int))]),
    );

    let mut outputs = std::collections::HashMap::new();
    outputs.insert("total".to_string(), TypeExpr::Primitive(PrimitiveType::Int));

    let annotations = json!({
        "primitive": {
            "kind": "reduce",
            "collection_input": "items",
            "output_key": "total",
            "reducer": {
                "fn": "+",
                "item_field": ":estimate",
                "initial": 0,
                "item_default": 0
            }
        }
    });

    let ctx = PrimitiveContext::new(&need, inputs, outputs, annotations);
    let synthesized = registry
        .synthesize(&ctx)
        .expect("reduce primitive should synthesize");

    assert_eq!(synthesized.primitive_id, PrimitiveTemplateId::Reduce);
    assert!(
        synthesized.rtfs_code.contains("(reduce")
            && synthesized.rtfs_code.contains("(get item :estimate 0)"),
        "reduce RTFS should include reduction over item estimates"
    );
    assert!(matches!(synthesized.input_schema, TypeExpr::Map { .. }));
    assert!(matches!(synthesized.output_schema, TypeExpr::Map { .. }));
}

#[test]
fn reduce_primitive_executes_in_restricted_runtime() {
    let registry = PrimitiveRegistry::new();
    let need = CapabilityNeed::new(
        "demo.reduce.runtime".to_string(),
        vec!["items".to_string()],
        vec!["total".to_string()],
        "Reduce items to a total".to_string(),
    );

    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "items".to_string(),
        vector_of_map_with_fields(vec![(":estimate", TypeExpr::Primitive(PrimitiveType::Int))]),
    );

    let mut outputs = std::collections::HashMap::new();
    outputs.insert("total".to_string(), TypeExpr::Primitive(PrimitiveType::Int));

    let annotations = json!({
        "primitive": {
            "kind": "reduce",
            "collection_input": "items",
            "output_key": "total",
            "reducer": {
                "fn": "+",
                "item_field": ":estimate",
                "initial": 0,
                "item_default": 0
            }
        }
    });

    let ctx = PrimitiveContext::new(&need, inputs, outputs, annotations);
    let primitive = registry
        .synthesize(&ctx)
        .expect("reduce primitive should synthesize");

    let executor = RestrictedRtfsExecutor::new();

    let mut item_one = std::collections::HashMap::new();
    item_one.insert(
        MapKey::Keyword(Keyword("estimate".to_string())),
        Value::Integer(3),
    );

    let mut item_two = std::collections::HashMap::new();
    item_two.insert(
        MapKey::Keyword(Keyword("estimate".to_string())),
        Value::Integer(5),
    );

    let items_value = Value::Vector(vec![Value::Map(item_one), Value::Map(item_two)]);

    let mut input_map = std::collections::HashMap::new();
    input_map.insert(MapKey::Keyword(Keyword("items".to_string())), items_value);

    let result = executor
        .evaluate(&primitive.rtfs_code, Value::Map(input_map))
        .expect("restricted runtime should execute reduce primitive");

    let total_value = match result {
        Value::Map(map) => map
            .get(&MapKey::Keyword(Keyword("total".to_string())))
            .cloned()
            .expect("total key present"),
        _ => panic!("expected map result"),
    };

    assert!(matches!(total_value, Value::Integer(8)));
}

#[test]
fn sort_template_generates_reverse_for_desc() {
    let registry = PrimitiveRegistry::new();
    let need = CapabilityNeed::new(
        "demo.sort".to_string(),
        vec!["items".to_string()],
        vec!["sorted".to_string()],
        "Sort items".to_string(),
    );

    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "items".to_string(),
        vector_of_map_with_fields(vec![(":priority", TypeExpr::Primitive(PrimitiveType::Int))]),
    );

    let mut outputs = std::collections::HashMap::new();
    outputs.insert("sorted".to_string(), TypeExpr::Any);

    let annotations = json!({
        "primitive": {
            "kind": "sort",
            "collection_input": "items",
            "output_key": "sorted",
            "sort_key": ":priority",
            "order": ":desc"
        }
    });

    let ctx = PrimitiveContext::new(&need, inputs, outputs, annotations);
    let synthesized = registry
        .synthesize(&ctx)
        .expect("sort primitive should synthesize");

    assert_eq!(synthesized.primitive_id, PrimitiveTemplateId::Sort);
    assert!(
        synthesized.rtfs_code.contains("(sort-by")
            && synthesized.rtfs_code.contains("(reverse sorted-items)"),
        "sort RTFS should include sort-by and reverse for descending order"
    );
}

#[test]
fn groupby_template_builds_bucket_map() {
    let registry = PrimitiveRegistry::new();
    let need = CapabilityNeed::new(
        "demo.groupby".to_string(),
        vec!["items".to_string()],
        vec!["grouped".to_string()],
        "Group items".to_string(),
    );

    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "items".to_string(),
        vector_of_map_with_fields(vec![(
            ":category",
            TypeExpr::Primitive(PrimitiveType::String),
        )]),
    );

    let mut outputs = std::collections::HashMap::new();
    outputs.insert(
        "grouped".to_string(),
        TypeExpr::Map {
            entries: vec![],
            wildcard: Some(Box::new(TypeExpr::Vector(Box::new(TypeExpr::Any)))),
        },
    );

    let annotations = json!({
        "primitive": {
            "kind": "groupBy",
            "collection_input": "items",
            "output_key": "grouped",
            "group_key": ":category"
        }
    });

    let ctx = PrimitiveContext::new(&need, inputs, outputs, annotations);
    let synthesized = registry
        .synthesize(&ctx)
        .expect("groupBy primitive should synthesize");

    assert_eq!(synthesized.primitive_id, PrimitiveTemplateId::GroupBy);
    assert!(
        synthesized
            .rtfs_code
            .contains("(assoc acc key (conj bucket item))"),
        "groupBy RTFS should associate grouped buckets"
    );
}

#[test]
fn join_template_requires_join_configuration() {
    let registry = PrimitiveRegistry::new();
    let need = CapabilityNeed::new(
        "demo.join".to_string(),
        vec!["left".to_string(), "right".to_string()],
        vec!["joined".to_string()],
        "Join collections".to_string(),
    );

    let mut inputs = std::collections::HashMap::new();
    inputs.insert(
        "left".to_string(),
        vector_of_map_with_fields(vec![(":id", TypeExpr::Primitive(PrimitiveType::Int))]),
    );
    inputs.insert(
        "right".to_string(),
        vector_of_map_with_fields(vec![(":left_id", TypeExpr::Primitive(PrimitiveType::Int))]),
    );

    let mut outputs = std::collections::HashMap::new();
    outputs.insert("joined".to_string(), TypeExpr::Any);

    // Missing `on` configuration should error
    let missing_on_annotations = json!({
        "primitive": {
            "kind": "join",
            "output_key": "joined"
        }
    });
    let missing_ctx = PrimitiveContext::new(
        &need,
        inputs.clone(),
        outputs.clone(),
        missing_on_annotations,
    );
    assert!(
        registry.synthesize(&missing_ctx).is_err(),
        "join primitive should require on annotation"
    );

    let annotations = json!({
        "primitive": {
            "kind": "join",
            "left_input": "left",
            "right_input": "right",
            "output_key": "joined",
            "on": [":id", ":left_id"],
            "type": ":inner"
        }
    });

    let ctx = PrimitiveContext::new(&need, inputs, outputs, annotations);
    let synthesized = registry
        .synthesize(&ctx)
        .expect("join primitive should synthesize");

    assert_eq!(synthesized.primitive_id, PrimitiveTemplateId::Join);
    assert!(
        synthesized
            .rtfs_code
            .contains("(merge left-item right-item)"),
        "join RTFS should merge matching records"
    );
}

#[test]
fn context_from_type_schemas_populates_binding_map() {
    let need = CapabilityNeed::new(
        "demo.manifest.reduce".to_string(),
        vec!["issues".to_string()],
        vec!["total".to_string()],
        "Reduce issues to total".to_string(),
    );

    let manifest_input = TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword("issues".to_string()),
                value_type: Box::new(vector_of_map_with_fields(vec![(
                    ":estimate",
                    TypeExpr::Primitive(PrimitiveType::Int),
                )])),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword("notes".to_string()),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: true,
            },
        ],
        wildcard: None,
    };

    let manifest_output = TypeExpr::Map {
        entries: vec![MapTypeEntry {
            key: Keyword("total".to_string()),
            value_type: Box::new(TypeExpr::Primitive(PrimitiveType::Int)),
            optional: false,
        }],
        wildcard: None,
    };

    let ctx = PrimitiveContext::from_type_schemas(
        &need,
        Some(&manifest_input),
        Some(&manifest_output),
        json!({ "primitive": { "kind": "reduce" } }),
    );

    assert!(ctx.input_schemas.contains_key(":issues"));
    if let Some(TypeExpr::Optional(inner)) = ctx.input_schemas.get(":notes") {
        assert!(matches!(
            **inner,
            TypeExpr::Primitive(PrimitiveType::String)
        ));
    } else {
        panic!("expected optional notes binding");
    }
    assert!(ctx.output_schemas.contains_key(":total"));
}
