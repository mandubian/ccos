use std::collections::HashMap;

use ccos::capability_marketplace::types::{CapabilityManifest, MCPCapability, ProviderType};
use ccos::catalog::{CatalogEntryKind, CatalogFilter, CatalogService, CatalogSource};
use ccos::types::{Plan, PlanBody, PlanLanguage, PlanStatus};
use rtfs::ast::{Keyword, MapKey, MapTypeEntry, PrimitiveType, TypeExpr};
use rtfs::runtime::values::Value;

fn sample_manifest() -> CapabilityManifest {
    let mut manifest = CapabilityManifest::new(
        "github.issues.list".to_string(),
        "List GitHub Issues".to_string(),
        "Fetch issues from GitHub".to_string(),
        ProviderType::MCP(MCPCapability {
            server_url: "https://example.com/mcp".to_string(),
            tool_name: "issues".to_string(),
            timeout_ms: 30_000,
        }),
        "1.0.0".to_string(),
    );

    manifest.input_schema = Some(TypeExpr::Map {
        entries: vec![
            MapTypeEntry {
                key: Keyword::new("owner"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
            MapTypeEntry {
                key: Keyword::new("repo"),
                value_type: Box::new(TypeExpr::Primitive(PrimitiveType::String)),
                optional: false,
            },
        ],
        wildcard: None,
    });

    manifest
        .metadata
        .insert("source".to_string(), "discovered".to_string());
    manifest
}

fn sample_plan(plan_id: &str) -> Plan {
    let mut metadata = HashMap::new();
    metadata.insert(
        "goal".to_string(),
        Value::String("List GitHub issues mentioning RTFS".to_string()),
    );

    let mut annotations = HashMap::new();
    annotations.insert(
        "goal".to_string(),
        Value::String("List GitHub issues mentioning RTFS".to_string()),
    );

    let mut input_schema = HashMap::new();
    input_schema.insert(
        MapKey::Keyword(Keyword::new("owner")),
        Value::Keyword(Keyword::new("string")),
    );
    input_schema.insert(
        MapKey::Keyword(Keyword::new("repo")),
        Value::Keyword(Keyword::new("string")),
    );

    Plan {
        plan_id: plan_id.to_string(),
        name: Some("List and filter issues".to_string()),
        intent_ids: Vec::new(),
        language: PlanLanguage::Rtfs20,
        body: PlanBody::Rtfs("(plan ...)".to_string()),
        status: PlanStatus::Draft,
        created_at: 0,
        metadata,
        input_schema: Some(Value::Map(input_schema)),
        output_schema: None,
        policies: HashMap::new(),
        capabilities_required: vec![
            "mcp.github.github-mcp.list_issues".to_string(),
            "github.issues.filter_by_language".to_string(),
        ],
        annotations,
    }
}

#[test]
fn catalog_registers_capability_and_finds_by_keyword() {
    let catalog = CatalogService::new();
    let manifest = sample_manifest();

    catalog.register_capability(&manifest, CatalogSource::Discovered);

    let hits = catalog.search_keyword(
        "github issues",
        Some(&CatalogFilter::for_kind(CatalogEntryKind::Capability)),
        5,
    );

    assert!(
        !hits.is_empty(),
        "expected to find at least one capability result"
    );

    let top = &hits[0];
    assert_eq!(top.entry.id, "github.issues.list");
    assert!(
        top.score > 0.0,
        "keyword score should be positive for a matching entry"
    );
}

#[test]
fn catalog_registers_plan_and_semantic_search() {
    let catalog = CatalogService::new();
    let plan = sample_plan("plan-123");

    catalog.register_plan(&plan, CatalogSource::Generated, None);

    let hits = catalog.search_semantic(
        "list github issues about rtfs",
        Some(&CatalogFilter::for_kind(CatalogEntryKind::Plan)),
        5,
    );

    assert!(
        !hits.is_empty(),
        "semantic search should surface the registered plan"
    );

    let top = &hits[0];
    assert_eq!(top.entry.id, "plan-123");
    assert!(
        top.score > 0.0,
        "semantic score should be positive for the matching plan"
    );
}
