//! Shared adapters that bridge core CCOS services to planner traits.
//!
//! These helpers were previously duplicated across demos. They now live in the
//! planner module so runtime code (CLI, examples, tests) can reuse them without
//! re‑implementing boilerplate conversions.

use std::sync::Arc;

use async_trait::async_trait;

use crate::catalog::{CatalogHit, CatalogService};
use crate::planner::modular_planner::resolution::semantic::{CapabilityCatalog, CapabilityInfo};

/// Adapts the `CatalogService` to the `CapabilityCatalog` trait used by the planner.
///
/// The adapter fetches capabilities via the catalog's keyword/semantic search
/// APIs and converts each hit into the lightweight `CapabilityInfo` structure
/// that the planner expects.
pub struct CcosCatalogAdapter {
    catalog: Arc<CatalogService>,
}

impl CcosCatalogAdapter {
    pub fn new(catalog: Arc<CatalogService>) -> Self {
        Self { catalog }
    }

    fn catalog_hit_to_info(hit: CatalogHit) -> CapabilityInfo {
        CapabilityInfo {
            id: hit.entry.id,
            name: hit.entry.name.unwrap_or_else(|| "unknown".to_string()),
            description: hit.entry.description.unwrap_or_default(),
            input_schema: hit.entry.input_schema,
            output_schema: hit.entry.output_schema,
            domains: hit.entry.domains,
            categories: hit.entry.categories,
        }
    }
}

#[async_trait(?Send)]
impl CapabilityCatalog for CcosCatalogAdapter {
    async fn list_capabilities(&self, _domain: Option<&str>) -> Vec<CapabilityInfo> {
        // Limit to keep the prompt manageable; catalog search already prioritizes relevance.
        let hits = self.catalog.search_keyword("", None, 100).await;
        hits.into_iter().map(Self::catalog_hit_to_info).collect()
    }

    async fn get_capability(&self, id: &str) -> Option<CapabilityInfo> {
        let hits = self.catalog.search_keyword(id, None, 10).await;
        hits.into_iter()
            .find(|h| h.entry.id == id)
            .map(Self::catalog_hit_to_info)
    }

    async fn search(&self, query: &str, limit: usize) -> Vec<CapabilityInfo> {
        let hits = self.catalog.search_keyword(query, None, limit).await;
        hits.into_iter().map(Self::catalog_hit_to_info).collect()
    }
}
