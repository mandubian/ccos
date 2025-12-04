use crate::capability_marketplace::types::CapabilityManifest;
use crate::capability_marketplace::CapabilityMarketplace;
use crate::plan_archive::PlanArchive;
use crate::types::Plan;
use rtfs::ast::{MapKey, MapTypeEntry, TypeExpr};
use rtfs::runtime::values::Value;
use std::collections::hash_map::DefaultHasher;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::RwLock;

/// Result entry returned from catalog queries
#[derive(Clone, Debug)]
pub struct CatalogHit {
    pub entry: CatalogEntry,
    pub score: f32,
}

/// High level kind of catalog entry
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CatalogEntryKind {
    Capability,
    Plan,
}

/// Provenance of the registered artifact
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum CatalogSource {
    Discovered,
    Generated,
    User,
    System,
    Unknown,
}

/// Location of an artifact inside storage
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum CatalogLocation {
    Filesystem(std::path::PathBuf),
    ArchiveHash(String),
    Other(String),
}

/// Filters applied during catalog search
#[derive(Clone, Debug, Default)]
pub struct CatalogFilter {
    pub kind: Option<CatalogEntryKind>,
    pub source: Option<CatalogSource>,
    /// Filter by domains (any match)
    pub domains: Vec<String>,
    /// Filter by categories (any match)
    pub categories: Vec<String>,
}

impl CatalogFilter {
    pub fn for_kind(kind: CatalogEntryKind) -> Self {
        Self {
            kind: Some(kind),
            ..Default::default()
        }
    }

    /// Create a filter for a specific domain
    pub fn for_domain(domain: impl Into<String>) -> Self {
        Self {
            domains: vec![domain.into()],
            ..Default::default()
        }
    }

    /// Create a filter for specific domains
    pub fn for_domains(domains: Vec<String>) -> Self {
        Self {
            domains,
            ..Default::default()
        }
    }

    /// Add domain filter
    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domains.push(domain.into());
        self
    }

    /// Add category filter
    pub fn with_category(mut self, category: impl Into<String>) -> Self {
        self.categories.push(category.into());
        self
    }

    fn matches(&self, entry: &CatalogEntry) -> bool {
        if let Some(kind) = self.kind {
            if entry.kind != kind {
                return false;
            }
        }
        if let Some(source) = self.source {
            if entry.source != source {
                return false;
            }
        }
        // Domain filter: match if any domain matches (prefix matching supported)
        if !self.domains.is_empty() && !entry.matches_any_domain(&self.domains) {
            return false;
        }
        // Category filter: match if any category matches
        if !self.categories.is_empty() && !entry.has_any_category(&self.categories) {
            return false;
        }
        true
    }
}

/// A catalog entry describing a plan or capability
#[derive(Clone, Debug)]
pub struct CatalogEntry {
    pub id: String,
    pub kind: CatalogEntryKind,
    pub name: Option<String>,
    pub description: Option<String>,
    pub source: CatalogSource,
    pub provider: Option<String>,
    pub tags: Vec<String>,
    pub inputs: Vec<String>,
    pub outputs: Vec<String>,
    pub location: Option<CatalogLocation>,
    pub capability_refs: Vec<String>,
    pub goal: Option<String>,
    pub search_blob: String,
    pub embedding: Option<Vec<f32>>,
    /// Domains this entry belongs to (hierarchical, e.g., "github.issues", "cloud.aws")
    pub domains: Vec<String>,
    /// Categories describing operation type (e.g., "crud", "search", "transform")
    pub categories: Vec<String>,
}

impl CatalogEntry {
    /// Check if entry matches a domain (supports prefix matching)
    pub fn matches_domain(&self, domain: &str) -> bool {
        self.domains.iter().any(|d| {
            d == domain
                || d.starts_with(&format!("{}.", domain))
                || domain.starts_with(&format!("{}.", d))
        })
    }

    /// Check if entry matches any of the given domains
    pub fn matches_any_domain(&self, domains: &[String]) -> bool {
        domains.iter().any(|d| self.matches_domain(d))
    }

    /// Check if entry has a specific category
    pub fn has_category(&self, category: &str) -> bool {
        self.categories.iter().any(|c| c == category)
    }

    /// Check if entry has any of the given categories
    pub fn has_any_category(&self, categories: &[String]) -> bool {
        categories.iter().any(|c| self.has_category(c))
    }

    fn new_capability(manifest: &CapabilityManifest, source: CatalogSource) -> Self {
        let tags = manifest.metadata.keys().cloned().collect::<Vec<_>>();
        let inputs = extract_schema_fields(manifest.input_schema.as_ref());
        let outputs = extract_schema_fields(manifest.output_schema.as_ref());
        let provider = Some(provider_to_string(&manifest.provider));

        // Include domains and categories in search blob for better discoverability
        let domain_text = manifest.domains.join(" ");
        let category_text = manifest.categories.join(" ");
        let search_blob = build_search_blob(
            &[
                &manifest.id,
                &manifest.name,
                &manifest.description,
                &domain_text,
                &category_text,
            ],
            &tags,
            &inputs,
            &outputs,
        );
        let embedding = Some(embed_text(&search_blob));

        Self {
            id: manifest.id.clone(),
            kind: CatalogEntryKind::Capability,
            name: Some(manifest.name.clone()),
            description: Some(manifest.description.clone()),
            source,
            provider,
            tags,
            inputs,
            outputs,
            location: None,
            capability_refs: Vec::new(),
            goal: None,
            search_blob,
            embedding,
            domains: manifest.domains.clone(),
            categories: manifest.categories.clone(),
        }
    }

    fn new_plan(plan: &Plan, source: CatalogSource, location: Option<CatalogLocation>) -> Self {
        let tags = plan.metadata.keys().cloned().collect::<Vec<_>>();

        let inputs = plan
            .input_schema
            .as_ref()
            .map(extract_schema_from_value)
            .unwrap_or_default();

        let outputs = plan
            .output_schema
            .as_ref()
            .map(extract_schema_from_value)
            .unwrap_or_default();

        let goal = plan
            .metadata
            .get("goal")
            .and_then(value_to_string)
            .or_else(|| plan.annotations.get("goal").and_then(value_to_string));
        let description = goal.clone().or_else(|| plan.name.clone());

        let capability_refs = plan.capabilities_required.clone();

        let mut text_components = vec![plan.plan_id.as_str()];
        if let Some(desc) = description.as_deref() {
            text_components.push(desc);
        }
        if let Some(goal_text) = goal.as_deref() {
            text_components.push(goal_text);
        }
        for capability in &capability_refs {
            text_components.push(capability.as_str());
        }

        let search_blob = build_search_blob(&text_components, &tags, &inputs, &outputs);
        let embedding = Some(embed_text(&search_blob));

        Self {
            id: plan.plan_id.clone(),
            kind: CatalogEntryKind::Plan,
            name: plan.name.clone(),
            description,
            source,
            provider: None,
            tags,
            inputs,
            outputs,
            location,
            capability_refs,
            goal,
            search_blob,
            embedding,
            domains: Vec::new(),    // Plans don't have domains yet
            categories: Vec::new(), // Plans don't have categories yet
        }
    }
}

/// Unified catalog service, providing registration and search capabilities
pub struct CatalogService {
    entries: RwLock<HashMap<String, CatalogEntry>>,
    embedding_index: RwLock<Vec<(String, Vec<f32>)>>,
}

impl CatalogService {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
            embedding_index: RwLock::new(Vec::new()),
        }
    }

    /// Upsert a capability manifest into the catalog
    pub fn register_capability(&self, manifest: &CapabilityManifest, source: CatalogSource) {
        let entry = CatalogEntry::new_capability(manifest, source);
        self.insert_entry(entry);
    }

    /// Upsert a plan into the catalog
    pub fn register_plan(
        &self,
        plan: &Plan,
        source: CatalogSource,
        location: Option<CatalogLocation>,
    ) {
        let entry = CatalogEntry::new_plan(plan, source, location);
        self.insert_entry(entry);
    }

    /// Re-index all capabilities currently present in the marketplace
    pub async fn ingest_marketplace(&self, marketplace: &CapabilityMarketplace) {
        let manifests = marketplace.list_capabilities().await;
        let source_infer = |manifest: &CapabilityManifest| match infer_source_from_metadata(
            manifest.metadata.get("source"),
        ) {
            CatalogSource::Unknown => infer_source_from_provider(&manifest.provider),
            other => other,
        };

        for manifest in manifests {
            self.register_capability(&manifest, source_infer(&manifest));
        }
    }

    /// Re-index plans contained within the provided plan archive
    pub fn ingest_plan_archive(&self, plan_archive: &PlanArchive) {
        for plan_id in plan_archive.list_plan_ids() {
            if let Some(archivable_plan) = plan_archive.get_plan_by_id(&plan_id) {
                let plan =
                    crate::orchestrator::Orchestrator::archivable_plan_to_plan(&archivable_plan);
                self.register_plan(
                    &plan,
                    CatalogSource::Generated,
                    Some(CatalogLocation::ArchiveHash(plan_id.clone())),
                );
            }
        }
    }

    /// Keyword-based search across the catalog
    pub fn search_keyword(
        &self,
        query: &str,
        filter: Option<&CatalogFilter>,
        limit: usize,
    ) -> Vec<CatalogHit> {
        let filter = filter.cloned().unwrap_or_default();
        let query_lower = query.to_lowercase();

        // Split query into tokens, ignoring short words to reduce noise
        let tokens: Vec<&str> = query_lower
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() > 2)
            .collect();

        let mut hits = Vec::new();

        let entries = self.entries.read().expect("catalog entries poisoned");

        // If no query or no tokens, return all matching entries
        if query.is_empty() || tokens.is_empty() {
            for entry in entries.values() {
                if filter.matches(entry) {
                    hits.push(CatalogHit {
                        entry: entry.clone(),
                        score: 1.0,
                    });
                }
            }
        } else {
            // Score-based search for non-empty queries
            for entry in entries.values() {
                if !filter.matches(entry) {
                    continue;
                }
                let haystack = entry.search_blob.to_lowercase();

                let mut score = 0.0;

                // Check for exact phrase match first (highest priority)
                if !query_lower.is_empty() && haystack.contains(&query_lower) {
                    score += 10.0;
                }

                // Check for token matches
                for token in &tokens {
                    if haystack.contains(token) {
                        score += 1.0;
                    }
                }

                if score > 0.0 {
                    hits.push(CatalogHit {
                        entry: entry.clone(),
                        score,
                    });
                }
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if limit > 0 && hits.len() > limit {
            hits.truncate(limit);
        }
        hits
    }

    /// Semantic search using simple hashed embeddings
    pub fn search_semantic(
        &self,
        query: &str,
        filter: Option<&CatalogFilter>,
        limit: usize,
    ) -> Vec<CatalogHit> {
        let filter = filter.cloned().unwrap_or_default();
        let query_embedding = embed_text(query);
        let mut hits = Vec::new();

        let entries_guard = self.entries.read().expect("catalog entries poisoned");
        let embeddings_guard = self
            .embedding_index
            .read()
            .expect("embedding index poisoned");

        for (entry_id, embedding) in embeddings_guard.iter() {
            if let Some(entry) = entries_guard.get(entry_id) {
                if !filter.matches(entry) {
                    continue;
                }
                let score = cosine_similarity(&query_embedding, embedding);
                hits.push(CatalogHit {
                    entry: entry.clone(),
                    score,
                });
            }
        }

        hits.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        if limit > 0 && hits.len() > limit {
            hits.truncate(limit);
        }
        hits
    }

    fn insert_entry(&self, entry: CatalogEntry) {
        let id = entry.id.clone();
        let embedding = entry.embedding.clone();

        {
            let mut entries = self.entries.write().expect("catalog entries poisoned");
            entries.insert(id.clone(), entry);
        }

        let mut index = self
            .embedding_index
            .write()
            .expect("embedding index poisoned");
        if let Some(vector) = embedding {
            if let Some(existing) = index.iter_mut().find(|(entry_id, _)| entry_id == &id) {
                existing.1 = vector;
            } else {
                index.push((id, vector));
            }
        }
    }
}

fn infer_source_from_metadata(value: Option<&String>) -> CatalogSource {
    match value.map(|s| s.to_lowercase()) {
        Some(ref s) if s.contains("discovered") => CatalogSource::Discovered,
        Some(ref s) if s.contains("user") => CatalogSource::User,
        Some(ref s) if s.contains("system") => CatalogSource::System,
        Some(ref s) if s.contains("generated") => CatalogSource::Generated,
        _ => CatalogSource::Unknown,
    }
}

fn infer_source_from_provider(
    provider: &crate::capability_marketplace::types::ProviderType,
) -> CatalogSource {
    use crate::capability_marketplace::types::ProviderType;
    match provider {
        ProviderType::MCP(_) | ProviderType::OpenApi(_) | ProviderType::Registry(_) => {
            CatalogSource::Discovered
        }
        ProviderType::Local(_)
        | ProviderType::RemoteRTFS(_)
        | ProviderType::Stream(_)
        | ProviderType::Native(_) => CatalogSource::Generated,
        ProviderType::Http(_) | ProviderType::A2A(_) | ProviderType::Plugin(_) => {
            CatalogSource::Unknown
        }
    }
}

fn provider_to_string(provider: &crate::capability_marketplace::types::ProviderType) -> String {
    use crate::capability_marketplace::types::ProviderType;
    match provider {
        ProviderType::Local(_) => "local",
        ProviderType::Http(_) => "http",
        ProviderType::MCP(_) => "mcp",
        ProviderType::A2A(_) => "a2a",
        ProviderType::OpenApi(_) => "openapi",
        ProviderType::Plugin(_) => "plugin",
        ProviderType::RemoteRTFS(_) => "remote_rtfs",
        ProviderType::Stream(_) => "stream",
        ProviderType::Registry(_) => "registry",
        ProviderType::Native(_) => "native",
    }
    .to_string()
}

fn extract_schema_from_value(value: &Value) -> Vec<String> {
    match value {
        Value::Map(map) => map.keys().map(map_key_to_string).collect::<Vec<_>>(),
        Value::Vector(items) => items.iter().flat_map(extract_schema_from_value).collect(),
        Value::Keyword(k) => vec![k.0.clone()],
        Value::Symbol(sym) => vec![sym.0.clone()],
        _ => Vec::new(),
    }
}

fn extract_schema_fields(schema: Option<&TypeExpr>) -> Vec<String> {
    fn walk(expr: &TypeExpr, acc: &mut Vec<String>) {
        match expr {
            TypeExpr::Primitive(p) => acc.push(format!("{:?}", p)),
            TypeExpr::Alias(sym) => acc.push(sym.0.clone()),
            TypeExpr::Literal(lit) => acc.push(format!("{:?}", lit)),
            TypeExpr::Vector(inner) | TypeExpr::Optional(inner) => walk(inner, acc),
            TypeExpr::Array { element_type, .. } => walk(element_type, acc),
            TypeExpr::Tuple(types) | TypeExpr::Union(types) | TypeExpr::Intersection(types) => {
                for ty in types {
                    walk(ty, acc);
                }
            }
            TypeExpr::Map { entries, wildcard } => {
                for MapTypeEntry {
                    key, value_type, ..
                } in entries
                {
                    acc.push(key.0.clone());
                    walk(value_type, acc);
                }
                if let Some(wild) = wildcard {
                    walk(wild, acc);
                }
            }
            TypeExpr::Enum(values) => {
                for value in values {
                    acc.push(format!("{:?}", value));
                }
            }
            TypeExpr::Refined { base_type, .. } => walk(base_type, acc),
            TypeExpr::Function {
                param_types,
                variadic_param_type,
                return_type,
            } => {
                for param in param_types {
                    if let rtfs::ast::ParamType::Simple(inner) = param {
                        walk(inner, acc);
                    }
                }
                if let Some(var) = variadic_param_type {
                    walk(var, acc);
                }
                walk(return_type, acc);
            }
            TypeExpr::Resource(sym) => acc.push(format!("resource::{}", sym.0)),
            TypeExpr::Any | TypeExpr::Never => {}
        }
    }

    let mut acc = Vec::new();
    if let Some(expr) = schema {
        walk(expr, &mut acc);
    }
    acc
}

fn build_search_blob(
    text_components: &[&str],
    tags: &[String],
    inputs: &[String],
    outputs: &[String],
) -> String {
    let mut parts = Vec::new();
    for value in text_components.iter().filter(|v| !v.is_empty()) {
        parts.push((*value).to_string());
    }
    parts.extend(tags.iter().cloned());
    parts.extend(inputs.iter().cloned());
    parts.extend(outputs.iter().cloned());
    parts.join(" ")
}

fn embed_text(text: &str) -> Vec<f32> {
    const DIM: usize = 64;
    let mut vector = vec![0.0; DIM];

    if text.trim().is_empty() {
        return vector;
    }

    for token in text.split_whitespace() {
        let mut hasher = DefaultHasher::new();
        token.to_lowercase().hash(&mut hasher);
        let idx = (hasher.finish() as usize) % DIM;
        vector[idx] += 1.0;
    }
    normalize_vector(&mut vector);
    vector
}

fn normalize_vector(vec: &mut [f32]) {
    let norm = vec.iter().map(|v| v * v).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in vec.iter_mut() {
            *value /= norm;
        }
    }
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let numerator = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum::<f32>();
    let denom_a = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let denom_b = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if denom_a == 0.0 || denom_b == 0.0 {
        0.0
    } else {
        numerator / (denom_a * denom_b)
    }
}

fn map_key_to_string(key: &MapKey) -> String {
    match key {
        MapKey::Keyword(k) => k.0.clone(),
        MapKey::String(s) => s.clone(),
        MapKey::Integer(i) => i.to_string(),
    }
}

fn value_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(s) => Some(s.clone()),
        Value::Keyword(k) => Some(k.0.clone()),
        Value::Symbol(sym) => Some(sym.0.clone()),
        Value::Integer(i) => Some(i.to_string()),
        Value::Float(f) => Some(f.to_string()),
        Value::Boolean(b) => Some(b.to_string()),
        Value::Vector(items) => {
            let joined: Vec<String> = items.iter().filter_map(value_to_string).collect();
            if joined.is_empty() {
                None
            } else {
                Some(joined.join(" "))
            }
        }
        _ => None,
    }
}
