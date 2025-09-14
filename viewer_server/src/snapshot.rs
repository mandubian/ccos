//! Snapshot building utilities for the viewer server.
//!
//! This module exposes `build_architecture_snapshot` which collects a point‑in‑time
//! JSON representation of the CCOS architecture suitable for the web UI
//! architecture/observability tab and for integration tests.
//!
//! Features provided:
//! - Intent graph metrics (totals, active/completed/failed counts)
//! - Root / leaf intent counts derived from `IsSubgoalOf` edges
//! - Recent intents (capped by caller supplied limit)
//! - Parent intent best‑effort backfill using edge relationships
//! - Capability marketplace aggregate + optional detailed list
//! - Isolation policy snapshot & heuristic security warning
//! - Environment (delegation availability & sanitized secret flags)
//! - Heuristic meta warnings:
//!   * All intents appear as roots (likely missing parent linkage)
//!   * High proportion of active intents (>80%)
//!   * Fully permissive isolation policy (allowed="*" and no denies)

use std::sync::Arc;
use rtfs_compiler::ccos::CCOS;

/// Build a structured snapshot of the running CCOS architecture for UI & tests.
///
/// Heuristic warnings are inserted under `meta.warnings` when anomalies are
/// detected. If any internal locking/backfill step fails a `degraded` flag is
/// set (also under `meta`).
pub async fn build_architecture_snapshot(
    ccos: &Arc<CCOS>,
    include_capabilities: bool,
    recent_intents_limit: usize,
    cap_limit: Option<usize>,
) -> serde_json::Value {
    let mut meta_warnings: Vec<String> = Vec::new();
    let mut degraded = false;

    // --- Intent graph metrics + parent backfill ---
    let intent_graph_json = {
        match ccos.get_intent_graph().lock() {
            Ok(graph_lock) => {
                use rtfs_compiler::ccos::types::{IntentStatus, EdgeType};
                let all = graph_lock.storage.get_all_intents_sync();

                // Derive hierarchy from edges (more reliable than stored parent_intent in current state)
                let mut has_children: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut is_subgoal: std::collections::HashSet<String> = std::collections::HashSet::new();
                let mut child_parent: std::collections::HashMap<String, String> = std::collections::HashMap::new();
                for intent in &all {
                    let edges = graph_lock.get_edges_for_intent(&intent.intent_id);
                    for edge in edges {
                        if edge.edge_type == EdgeType::IsSubgoalOf {
                            // from is subgoal of to
                            has_children.insert(edge.to.clone());
                            is_subgoal.insert(edge.from.clone());
                            // track parent mapping for potential backfill
                            child_parent.entry(edge.from.clone()).or_insert(edge.to.clone());
                        }
                    }
                }

                // Backfill parent_intent where missing (best-effort). Collect updates first.
                let mut to_update = Vec::new();
                for intent in &all {
                    if intent.parent_intent.is_none() {
                        if let Some(parent) = child_parent.get(&intent.intent_id) {
                            let mut updated = intent.clone();
                            updated.parent_intent = Some(parent.clone());
                            to_update.push(updated);
                        }
                    }
                }
                drop(graph_lock); // release lock before async updates
                if !to_update.is_empty() {
                    if let Ok(mut graph_lock2) = ccos.get_intent_graph().lock() {
                        for updated in to_update {
                            if let Err(e) = graph_lock2.storage.update_intent(&updated).await {
                                meta_warnings.push(format!("Failed to backfill parent for {}: {}", updated.intent_id, e));
                            }
                        }
                    } else {
                        meta_warnings.push("Failed to lock intent graph for parent backfill".into());
                    }
                }

                // Re-lock to compute final metrics (may include backfills)
                match ccos.get_intent_graph().lock() {
                    Ok(graph) => {
                        let all2 = graph.storage.get_all_intents_sync();
                        let mut total=0usize; let mut active=0usize; let mut completed=0usize; let mut failed=0usize; let mut leaf_count=0usize; let mut root_count=0usize;
                        for i in &all2 {
                            total+=1;
                            match i.status { IntentStatus::Active => active+=1, IntentStatus::Completed => completed+=1, IntentStatus::Failed => failed+=1, _=>{} }
                        }
                        // root: not a subgoal (edge-derived)
                        for i in &all2 { if !is_subgoal.contains(&i.intent_id) { root_count+=1; } }
                        // leaf: not in has_children
                        for i in &all2 { if !has_children.contains(&i.intent_id) { leaf_count+=1; } }
                        let mut recent: Vec<_> = all2.iter().collect();
                        recent.sort_by_key(|i| std::cmp::Reverse(i.created_at));
                        let recent: Vec<_> = recent.into_iter().take(recent_intents_limit).map(|i| serde_json::json!({
                            "id": i.intent_id,
                            "goal": i.goal,
                            "status": format!("{:?}", i.status),
                        })).collect();
                        serde_json::json!({
                            "total": total,
                            "active": active,
                            "completed": completed,
                            "failed": failed,
                            "root_count": root_count,
                            "leaf_count": leaf_count,
                            "recent": recent,
                        })
                    }
                    Err(_) => { degraded=true; meta_warnings.push("Failed to relock intent graph".into()); serde_json::json!({"degraded": true}) }
                }
            }
            Err(_) => { degraded=true; meta_warnings.push("Failed to lock intent graph".into()); serde_json::json!({"degraded": true}) }
        }
    };

    let marketplace = ccos.get_capability_marketplace();
    let caps_aggregate = marketplace.public_capabilities_aggregate().await;
    let caps_list = if include_capabilities { Some(marketplace.public_capabilities_snapshot(cap_limit).await) } else { None };
    let isolation = marketplace.isolation_policy_snapshot();

    let delegation_enabled = ccos.get_delegating_arbiter().is_some();
    let (provider, model) = if delegation_enabled {
        let provider = if std::env::var("OPENROUTER_API_KEY").is_ok() { "openrouter" } else if std::env::var("OPENAI_API_KEY").is_ok() { "openai" } else { "unknown" };
        let model = std::env::var("LLM_MODEL").unwrap_or_else(|_| "unknown".to_string());
        (provider.to_string(), model)
    } else { ("none".to_string(), "".to_string()) };

    let graph_model = serde_json::json!({
        "nodes": [
            {"id":"arbiter","label":"Arbiter","group":"arbiter"},
            {"id":"delegating_arbiter","label":"Delegating Arbiter","group":"arbiter","present":delegation_enabled},
            {"id":"governance_kernel","label":"Governance Kernel","group":"governance"},
            {"id":"orchestrator","label":"Orchestrator","group":"orchestrator"},
            {"id":"intent_graph","label":"Intent Graph","group":"store"},
            {"id":"causal_chain","label":"Causal Chain","group":"ledger"},
            {"id":"capability_marketplace","label":"Capability Marketplace","group":"marketplace"},
            {"id":"plan_archive","label":"Plan Archive","group":"storage"},
            {"id":"rtfs_runtime","label":"RTFS Runtime","group":"runtime"}
        ],
        "flow_edges": [
            {"from":"arbiter","to":"governance_kernel","relation":"proposes_plan"},
            {"from":"governance_kernel","to":"orchestrator","relation":"validated_plan"},
            {"from":"orchestrator","to":"capability_marketplace","relation":"dispatches_calls"},
            {"from":"orchestrator","to":"intent_graph","relation":"updates_intents"},
            {"from":"arbiter","to":"intent_graph","relation":"creates_intents"},
            {"from":"orchestrator","to":"causal_chain","relation":"records_actions"},
            {"from":"governance_kernel","to":"causal_chain","relation":"records_validation"}
        ]
    });

    let security = serde_json::json!({
        "isolation_policy": isolation,
    });

    // Heuristic warnings
    if let (Some(total), Some(root_count)) = (
        intent_graph_json.get("total").and_then(|v| v.as_u64()),
        intent_graph_json.get("root_count").and_then(|v| v.as_u64())
    ) {
        if total > 5 && root_count == total { meta_warnings.push("All intents appear as roots (parent linkage may be missing)".into()); }
        if let Some(active) = intent_graph_json.get("active").and_then(|v| v.as_u64()) {
            if total > 5 && active as f64 / total as f64 > 0.8 { meta_warnings.push("High proportion of active intents (>80%)".into()); }
        }
    }
    if let Some(policy) = security.get("isolation_policy") {
        if let (Some(allowed), Some(denied)) = (
            policy.get("allowed_patterns").and_then(|v| v.as_array()),
            policy.get("denied_patterns").and_then(|v| v.as_array())
        ) {
            if allowed.len()==1 && allowed[0].as_str()==Some("*") && denied.is_empty() {
                meta_warnings.push("Isolation policy is fully permissive ('*')".into());
            }
        }
    }

    let mut root = serde_json::json!({
        "version": "1",
        "generated_at": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "environment": {
            "delegation_enabled": delegation_enabled,
            "llm": {"provider": provider, "model": model, "available": delegation_enabled},
            "flags": {
                "OPENROUTER_API_KEY": if std::env::var("OPENROUTER_API_KEY").is_ok() { "SET" } else { "NOT_SET" },
                "OPENAI_API_KEY": if std::env::var("OPENAI_API_KEY").is_ok() { "SET" } else { "NOT_SET" },
            }
        },
        "components": {
            "intent_graph": intent_graph_json,
            "capability_marketplace": caps_aggregate,
        },
        "security": security,
        "graph_model": graph_model,
    });

    if let Some(list) = caps_list { root.as_object_mut().unwrap().insert("capabilities".to_string(), serde_json::Value::Array(list)); }
    if degraded || !meta_warnings.is_empty() {
        root.as_object_mut().unwrap().insert("meta".to_string(), serde_json::json!({
            "degraded": degraded,
            "warnings": meta_warnings,
        }));
    }
    root
}
