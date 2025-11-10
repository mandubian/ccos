use std::collections::HashMap;

use anyhow::Result;
use ccos::discovery::need_extractor::CapabilityNeed;
use ccos::synthesis::primitives::{PrimitiveContext, PrimitiveRegistry, PrimitiveTemplateId};
use rtfs::ast::{Keyword, MapTypeEntry, PrimitiveType, TypeExpr};
fn keyword(name: &str) -> Keyword {
    Keyword(name.trim_start_matches(':').to_string())
}

fn map_entry(name: &str, ty: TypeExpr) -> MapTypeEntry {
    MapTypeEntry {
        key: keyword(name),
        value_type: Box::new(ty),
        optional: false,
    }
}

fn string_type() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveType::String)
}

fn integer_type() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveType::Int)
}

fn label_vector_type() -> TypeExpr {
    TypeExpr::Vector(Box::new(string_type()))
}

fn issue_record_type() -> TypeExpr {
    TypeExpr::Map {
        entries: vec![
            map_entry(":id", integer_type()),
            map_entry(":number", integer_type()),
            map_entry(":title", string_type()),
            map_entry(":body", string_type()),
            map_entry(":state", string_type()),
            map_entry(":labels", label_vector_type()),
            map_entry(":estimate", integer_type()),
            map_entry(":html_url", string_type()),
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    }
}

fn issues_collection_type() -> TypeExpr {
    TypeExpr::Vector(Box::new(issue_record_type()))
}

fn issue_summary_type() -> TypeExpr {
    TypeExpr::Vector(Box::new(TypeExpr::Map {
        entries: vec![
            map_entry(":title", string_type()),
            map_entry(":number", integer_type()),
            map_entry(":url", string_type()),
        ],
        wildcard: None,
    }))
}

fn issue_subset_type() -> TypeExpr {
    TypeExpr::Vector(Box::new(TypeExpr::Map {
        entries: vec![
            map_entry(":title", string_type()),
            map_entry(":state", string_type()),
            map_entry(":labels", label_vector_type()),
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    }))
}

fn issue_comments_collection_type() -> TypeExpr {
    TypeExpr::Vector(Box::new(TypeExpr::Map {
        entries: vec![
            map_entry(":comment_id", integer_type()),
            map_entry(":issue_id", integer_type()),
            map_entry(":author", string_type()),
            map_entry(":body", string_type()),
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    }))
}

fn grouped_issues_type() -> TypeExpr {
    TypeExpr::Map {
        entries: vec![],
        wildcard: Some(Box::new(issues_collection_type())),
    }
}

fn joined_issue_comment_type() -> TypeExpr {
    TypeExpr::Vector(Box::new(TypeExpr::Map {
        entries: vec![
            map_entry(":id", integer_type()),
            map_entry(":number", integer_type()),
            map_entry(":title", string_type()),
            map_entry(":state", string_type()),
            map_entry(":labels", label_vector_type()),
            map_entry(":estimate", integer_type()),
            map_entry(":html_url", string_type()),
            map_entry(":comment_id", integer_type()),
            map_entry(":issue_id", integer_type()),
            map_entry(":author", string_type()),
            map_entry(":body", string_type()),
        ],
        wildcard: Some(Box::new(TypeExpr::Any)),
    }))
}

use serde_json::json;

fn main() -> Result<()> {
    let registry = PrimitiveRegistry::new();

    // --- Filter primitive ---------------------------------------------------
    let filter_need = CapabilityNeed::new(
        "demo.issues.filter_by_language".to_string(),
        vec!["issues".to_string(), "language".to_string()],
        vec!["filtered_issues".to_string()],
        "Filter issues returned by a listing capability using a language keyword.".to_string(),
    );

    let mut filter_inputs = HashMap::new();
    filter_inputs.insert("issues".to_string(), issues_collection_type());
    filter_inputs.insert("language".to_string(), string_type());

    let mut filter_outputs = HashMap::new();
    filter_outputs.insert("filtered_issues".to_string(), issues_collection_type());

    let filter_annotations = json!({
        "primitive": {
            "kind": "filter",
            "collection_input": "issues",
            "search_input": "language",
            "output_key": "filtered_issues",
            "search_fields": [":title", ":body", ":labels"]
        }
    });

    let filter_ctx = PrimitiveContext::new(
        &filter_need,
        filter_inputs.clone(),
        filter_outputs.clone(),
        filter_annotations,
    );
    let filter_primitive = registry.synthesize(&filter_ctx)?;

    dump(&filter_primitive, &filter_inputs, &filter_outputs);

    // --- Map primitive ------------------------------------------------------
    let map_need = CapabilityNeed::new(
        "demo.issues.map_to_summary".to_string(),
        vec!["filtered_issues".to_string()],
        vec!["issue_summaries".to_string()],
        "Transform filtered issues into lightweight summary maps.".to_string(),
    );

    let mut map_inputs = HashMap::new();
    map_inputs.insert("filtered_issues".to_string(), issues_collection_type());

    let mut map_outputs = HashMap::new();
    map_outputs.insert("issue_summaries".to_string(), issue_summary_type());

    let map_annotations = json!({
        "primitive": {
            "kind": "map",
            "collection_input": "filtered_issues",
            "output_key": "issue_summaries",
            "mapping": {
                ":title": ":title",
                ":number": ":number",
                ":url": ":html_url"
            }
        }
    });

    let map_ctx = PrimitiveContext::new(
        &map_need,
        map_inputs.clone(),
        map_outputs.clone(),
        map_annotations,
    );
    let map_primitive = registry.synthesize(&map_ctx)?;
    dump(&map_primitive, &map_inputs, &map_outputs);

    // --- Project primitive --------------------------------------------------
    let project_need = CapabilityNeed::new(
        "demo.issues.project_fields".to_string(),
        vec!["issues".to_string()],
        vec!["issue_subset".to_string()],
        "Keep only a subset of fields for display.".to_string(),
    );

    let mut project_inputs = HashMap::new();
    project_inputs.insert("issues".to_string(), issues_collection_type());

    let mut project_outputs = HashMap::new();
    project_outputs.insert("issue_subset".to_string(), issue_subset_type());

    let project_annotations = json!({
        "primitive": {
            "kind": "project",
            "collection_input": "issues",
            "output_key": "issue_subset",
            "fields": [":title", ":state", ":labels"]
        }
    });

    let project_ctx = PrimitiveContext::new(
        &project_need,
        project_inputs.clone(),
        project_outputs.clone(),
        project_annotations,
    );
    let project_primitive = registry.synthesize(&project_ctx)?;
    dump(&project_primitive, &project_inputs, &project_outputs);

    // --- Reduce primitive ---------------------------------------------------
    let reduce_need = CapabilityNeed::new(
        "demo.issues.sum_estimate".to_string(),
        vec!["issues".to_string()],
        vec!["total_estimate".to_string()],
        "Sum numeric estimates across issues.".to_string(),
    );

    let mut reduce_inputs = HashMap::new();
    reduce_inputs.insert("issues".to_string(), issues_collection_type());

    let mut reduce_outputs = HashMap::new();
    reduce_outputs.insert(
        "total_estimate".to_string(),
        TypeExpr::Primitive(PrimitiveType::Int),
    );

    let reduce_annotations = json!({
        "primitive": {
            "kind": "reduce",
            "collection_input": "issues",
            "output_key": "total_estimate",
            "reducer": {
                "fn": "+",
                "item_field": ":estimate",
                "initial": 0,
                "item_default": 0
            }
        }
    });

    let reduce_ctx = PrimitiveContext::new(
        &reduce_need,
        reduce_inputs.clone(),
        reduce_outputs.clone(),
        reduce_annotations,
    );
    let reduce_primitive = registry.synthesize(&reduce_ctx)?;
    dump(&reduce_primitive, &reduce_inputs, &reduce_outputs);

    // --- Sort primitive -----------------------------------------------------
    let sort_need = CapabilityNeed::new(
        "demo.issues.sort_by_number".to_string(),
        vec!["issues".to_string()],
        vec!["sorted_issues".to_string()],
        "Sort issues by their number field.".to_string(),
    );

    let mut sort_inputs = HashMap::new();
    sort_inputs.insert("issues".to_string(), issues_collection_type());

    let mut sort_outputs = HashMap::new();
    sort_outputs.insert("sorted_issues".to_string(), issues_collection_type());

    let sort_annotations = json!({
        "primitive": {
            "kind": "sort",
            "collection_input": "issues",
            "output_key": "sorted_issues",
            "sort_key": ":number",
            "order": ":desc"
        }
    });

    let sort_ctx = PrimitiveContext::new(
        &sort_need,
        sort_inputs.clone(),
        sort_outputs.clone(),
        sort_annotations,
    );
    let sort_primitive = registry.synthesize(&sort_ctx)?;
    dump(&sort_primitive, &sort_inputs, &sort_outputs);

    // --- GroupBy primitive --------------------------------------------------
    let group_need = CapabilityNeed::new(
        "demo.issues.group_by_state".to_string(),
        vec!["issues".to_string()],
        vec!["issues_by_state".to_string()],
        "Group issues by their state.".to_string(),
    );

    let mut group_inputs = HashMap::new();
    group_inputs.insert("issues".to_string(), issues_collection_type());

    let mut group_outputs = HashMap::new();
    group_outputs.insert("issues_by_state".to_string(), grouped_issues_type());

    let group_annotations = json!({
        "primitive": {
            "kind": "groupBy",
            "collection_input": "issues",
            "output_key": "issues_by_state",
            "group_key": ":state"
        }
    });

    let group_ctx = PrimitiveContext::new(
        &group_need,
        group_inputs.clone(),
        group_outputs.clone(),
        group_annotations,
    );
    let group_primitive = registry.synthesize(&group_ctx)?;
    dump(&group_primitive, &group_inputs, &group_outputs);

    // --- Join primitive -----------------------------------------------------
    let join_need = CapabilityNeed::new(
        "demo.issues.join_comments".to_string(),
        vec!["issues".to_string(), "comments".to_string()],
        vec!["joined_records".to_string()],
        "Join issues with their associated comments.".to_string(),
    );

    let mut join_inputs = HashMap::new();
    join_inputs.insert("issues".to_string(), issues_collection_type());
    join_inputs.insert("comments".to_string(), issue_comments_collection_type());

    let mut join_outputs = HashMap::new();
    join_outputs.insert("joined_records".to_string(), joined_issue_comment_type());

    let join_annotations = json!({
        "primitive": {
            "kind": "join",
            "left_input": "issues",
            "right_input": "comments",
            "output_key": "joined_records",
            "on": [":id", ":issue_id"],
            "type": ":inner"
        }
    });

    let join_ctx = PrimitiveContext::new(
        &join_need,
        join_inputs.clone(),
        join_outputs.clone(),
        join_annotations,
    );
    let join_primitive = registry.synthesize(&join_ctx)?;
    dump(&join_primitive, &join_inputs, &join_outputs);

    Ok(())
}

fn dump(
    primitive: &ccos::synthesis::primitives::SynthesizedPrimitive,
    inputs: &HashMap<String, TypeExpr>,
    outputs: &HashMap<String, TypeExpr>,
) {
    println!("---");
    println!("Primitive synthesized for '{}':", primitive.capability_id);
    println!(
        "Primitive kind: {}",
        match primitive.primitive_id {
            PrimitiveTemplateId::Filter => "filter",
            PrimitiveTemplateId::Map => "map",
            PrimitiveTemplateId::Project => "project",
            PrimitiveTemplateId::Reduce => "reduce",
            PrimitiveTemplateId::Sort => "sort",
            PrimitiveTemplateId::GroupBy => "groupBy",
            PrimitiveTemplateId::Join => "join",
        }
    );
    println!("Input schemas:");
    for (name, schema) in inputs {
        println!("  {} -> {}", name, schema);
    }
    println!("Output schemas:");
    for (name, schema) in outputs {
        println!("  {} -> {}", name, schema);
    }
    println!("Capability input schema: {}", primitive.input_schema);
    println!("Capability output schema: {}", primitive.output_schema);
    println!("RTFS implementation:\n{}", primitive.rtfs_code);
    println!("Metadata: {}", primitive.metadata);
}
