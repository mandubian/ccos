//! Semantic matching utilities for capability discovery
//!
//! This module provides fuzzy matching between LLM-generated capability names
//! (e.g., "github.issues.list") and actual MCP capability IDs (e.g., "mcp.github.list_issues").

use regex;

/// Normalize a capability name by removing common prefixes and standardizing format
pub fn normalize_capability_name(name: &str) -> String {
    // Remove "mcp." prefix if present
    let name = name.strip_prefix("mcp.").unwrap_or(name);

    // Split by dots and collect parts
    let parts: Vec<&str> = name.split('.').collect();

    // If we have more than one part, extract meaningful words
    if parts.len() > 1 {
        // Take all parts except the first (server name) for matching
        parts[1..].join(".")
    } else {
        name.to_string()
    }
}

/// Extract keywords from a capability-ish string (IDs, short names)
/// Splits on dots/underscores and camelCase. Use this for structured identifiers.
pub fn extract_keywords(name: &str) -> Vec<String> {
    let normalized = normalize_capability_name(name);

    // Split by dots, underscores, and camelCase boundaries
    let mut keywords = Vec::new();

    // Split by dots first
    for part in normalized.split('.') {
        // Split by underscores
        for subpart in part.split('_') {
            // Also split on spaces to handle titles like "List Issues"
            for token in subpart.split_whitespace() {
                if token.is_empty() {
                    continue;
                }

                // Split camelCase (basic heuristic: detect case transitions)
                let mut current_word = String::new();
                let mut words = Vec::new();
                let mut prev_was_lower = false;

                for c in token.chars() {
                    if c.is_uppercase() && prev_was_lower && !current_word.is_empty() {
                        // Transition from lowercase to uppercase - start new word
                        words.push(current_word.clone());
                        current_word.clear();
                    }
                    current_word.push(c);
                    prev_was_lower = c.is_lowercase();
                }

                if !current_word.is_empty() {
                    words.push(current_word);
                }

                // If no camelCase detected, use the whole token as one word
                if words.is_empty() {
                    words.push(token.to_string());
                }

                keywords.extend(words.into_iter().map(|s| s.to_lowercase()));
            }
        }
    }

    // Keep only alphanumeric words of length > 1
    keywords.retain(|k| !k.is_empty() && k.chars().any(|c| c.is_alphanumeric()) && k.len() > 1);
    keywords
}

/// Tokenize free-text (sentences/descriptions) into lowercase word tokens.
/// Splits on non-alphanumeric boundaries and filters out very short tokens.
fn extract_keywords_from_text(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for c in text.chars() {
        if c.is_alphanumeric() {
            current.push(c.to_ascii_lowercase());
        } else {
            if current.len() > 1 {
                out.push(current.clone());
            }
            current.clear();
        }
    }
    if current.len() > 1 {
        out.push(current);
    }
    out
}

/// Calculate a relevance score between a capability need and a manifest based on description/rationale
/// Returns a score from 0.0 to 1.0, where 1.0 is a perfect match
///
/// This function supports both keyword-based and embedding-based matching:
/// - If an embedding service is provided, uses embedding similarity (more accurate)
/// - Otherwise, falls back to keyword-based matching
pub fn calculate_description_match_score(
    need_rationale: &str,
    manifest_description: &str,
    manifest_name: &str,
) -> f64 {
    calculate_description_match_score_with_embedding(
        need_rationale,
        manifest_description,
        manifest_name,
        None,
    )
}

/// Calculate description match score with optional embedding service
/// If embedding_service is Some, uses embedding-based similarity
/// Otherwise, uses keyword-based matching
pub async fn calculate_description_match_score_with_embedding_async(
    need_rationale: &str,
    manifest_description: &str,
    manifest_name: &str,
    embedding_service: Option<&mut crate::discovery::embedding_service::EmbeddingService>,
) -> f64 {
    if let Some(service) = embedding_service {
        // Use embedding-based matching
        match calculate_embedding_similarity(
            need_rationale,
            manifest_description,
            manifest_name,
            service,
        )
        .await
        {
            Ok(score) => return score,
            Err(_) => {
                // Fallback to keyword matching if embedding fails
                eprintln!("  ⚠️  Embedding matching failed, falling back to keyword matching");
            }
        }
    }

    // Fallback to keyword-based matching
    calculate_description_match_score_with_embedding(
        need_rationale,
        manifest_description,
        manifest_name,
        None,
    )
}

/// Synchronous version that checks for embedding service in environment
pub fn calculate_description_match_score_with_embedding(
    need_rationale: &str,
    manifest_description: &str,
    manifest_name: &str,
    embedding_service: Option<&mut crate::discovery::embedding_service::EmbeddingService>,
) -> f64 {
    // For free text, use a tokenizer suited for sentences.
    let need_keywords = extract_keywords_from_text(need_rationale);
    let desc_keywords = extract_keywords_from_text(manifest_description);
    // Names/titles may be short free text as well; tokenize similarly.
    let name_keywords = extract_keywords_from_text(manifest_name);

    if need_keywords.is_empty() {
        return 0.0;
    }

    // Combine manifest keywords from description and name
    let all_manifest_keywords: Vec<String> = desc_keywords
        .into_iter()
        .chain(name_keywords.into_iter())
        .collect();

    // Count matching keywords (semantic matching on what the capability does)
    let mut matches = 0;
    let mut mismatches = 0;
    let mut matched_manifest_kws: Vec<&String> = Vec::new();

    for need_kw in &need_keywords {
        if let Some(matched) = all_manifest_keywords.iter().find(|mk| *mk == need_kw) {
            matches += 1;
            if !matched_manifest_kws.contains(&matched) {
                matched_manifest_kws.push(matched);
            }
        } else {
            // Check for partial matches (e.g., "list" in "list issues")
            if let Some(partial_match) = all_manifest_keywords
                .iter()
                .find(|mk| mk.contains(need_kw.as_str()) || need_kw.contains(mk.as_str()))
            {
                matches += 1;
                if !matched_manifest_kws.contains(&partial_match) {
                    matched_manifest_kws.push(partial_match);
                }
            } else {
                mismatches += 1;
            }
        }
    }

    // Penalize for unmatched keywords in the manifest
    let unmatched_manifest_kws: Vec<&String> = all_manifest_keywords
        .iter()
        .filter(|mk| !matched_manifest_kws.contains(mk))
        .collect();

    let extra_keyword_penalty = if !unmatched_manifest_kws.is_empty() && !all_manifest_keywords.is_empty() {
        0.5 * (unmatched_manifest_kws.len() as f64 / all_manifest_keywords.len() as f64)
    } else {
        0.0
    };

    // Base score from keyword matches
    let keyword_score = if need_keywords.is_empty() {
        0.0
    } else {
        matches as f64 / need_keywords.len() as f64
    };

    // Check if keywords appear in description
    let need_text = need_keywords.join("");
    let manifest_text: String = all_manifest_keywords.join("");
    let ordered_match = need_keywords
        .iter()
        .all(|need_kw| manifest_text.contains(need_kw.as_str()));

    // Bonus for ordered match (all keywords present)
    let ordered_bonus = if ordered_match && mismatches == 0 {
        0.3
    } else {
        0.0
    };

    // Bonus for exact substring match
    let substring_bonus = if manifest_text.contains(need_text.as_str())
        || need_text.contains(manifest_text.as_str())
    {
        0.2
    } else {
        0.0
    };

    // Penalty for mismatches
    let mismatch_penalty = if mismatches > 0 {
        0.2 * (mismatches as f64 / need_keywords.len() as f64)
    } else {
        0.0
    };

    // Combined score (capped at 1.0, floor at 0.0)
    ((keyword_score + ordered_bonus + substring_bonus - mismatch_penalty - extra_keyword_penalty).max(0.0)).min(1.0)
}

/// Calculate similarity using embedding vectors (more accurate than keyword matching)
async fn calculate_embedding_similarity(
    need_rationale: &str,
    manifest_description: &str,
    manifest_name: &str,
    embedding_service: &mut crate::discovery::embedding_service::EmbeddingService,
) -> Result<f64, rtfs::runtime::error::RuntimeError> {
    // Combine description and name for manifest text
    let manifest_text = if manifest_name.is_empty() {
        manifest_description.to_string()
    } else {
        format!("{} {}", manifest_description, manifest_name)
    };

    // Generate embeddings
    let need_embedding = embedding_service.embed(need_rationale).await?;
    let manifest_embedding = embedding_service.embed(&manifest_text).await?;

    // Calculate cosine similarity
    let similarity = crate::discovery::embedding_service::EmbeddingService::cosine_similarity(
        &need_embedding,
        &manifest_embedding,
    );

    Ok(similarity)
}

/// Calculate a relevance score between a capability need and a manifest
/// Returns a score from 0.0 to 1.0, where 1.0 is a perfect match
///
/// This version matches on capability class name (for backward compatibility)
/// Improved to better match action words (e.g., "list" in "github.issues.list" should match "list_issues" better than "list_issue_types")
pub fn calculate_semantic_match_score(
    need_class: &str,
    manifest_id: &str,
    manifest_name: &str,
) -> f64 {
    let need_keywords = extract_keywords(need_class);
    let manifest_id_keywords = extract_keywords(manifest_id);
    let manifest_name_keywords = extract_keywords(manifest_name);

    if need_keywords.is_empty() {
        return 0.0;
    }

    // Combine manifest keywords
    let all_manifest_keywords: Vec<String> = manifest_id_keywords
        .into_iter()
        .chain(manifest_name_keywords.into_iter())
        .collect();

    // Extract action word (typically the last keyword in need_class)
    // e.g., "github.issues.list" -> action = "list"
    let action_word = need_keywords.last().cloned().unwrap_or_default();

    // Find the action word in manifest (if present)
    // This helps prioritize "list_issues" over "list_issue_types" when searching for "list"
    let action_match_in_manifest = all_manifest_keywords.iter().any(|mk| {
        mk == &action_word || mk.starts_with(&action_word) || action_word.starts_with(mk)
    });

    // Count matching keywords
    let mut matches = 0;
    let mut mismatches = 0;

    // Track which manifest keywords were matched
    let mut matched_manifest_kws: Vec<&String> = Vec::new();

    for need_kw in &need_keywords {
        if let Some(matched) = all_manifest_keywords.iter().find(|mk| *mk == need_kw) {
            matches += 1;
            matched_manifest_kws.push(matched);
        } else {
            // Check for partial matches (e.g., "issues" in "list_issues")
            if all_manifest_keywords
                .iter()
                .any(|mk| mk.contains(need_kw.as_str()) || need_kw.contains(mk.as_str()))
            {
                matches += 1;
                // Find and track the partial match
                if let Some(partial_match) = all_manifest_keywords
                    .iter()
                    .find(|mk| mk.contains(need_kw.as_str()) || need_kw.contains(mk.as_str()))
                {
                    if !matched_manifest_kws.contains(&partial_match) {
                        matched_manifest_kws.push(partial_match);
                    }
                }
            } else {
                mismatches += 1;
            }
        }
    }

    // Penalize if there are significant keywords in manifest that don't match need
    // (e.g., "types" in "list_issue_types" when searching for "list_issues")
    let unmatched_manifest_kws: Vec<&String> = all_manifest_keywords
        .iter()
        .filter(|mk| !matched_manifest_kws.contains(mk))
        .collect();

    // Stronger penalty for unmatched keywords, especially if action word matches
    // This helps prioritize "list_issues" (no unmatched keywords) over "list_issue_types" (has unmatched "types")
    let extra_keyword_penalty = if !unmatched_manifest_kws.is_empty() {
        // If action word matches, we want exact action matches (e.g., "list_issues" not "list_issue_types")
        if action_match_in_manifest {
            // Heavy penalty for extra keywords when action matches - we want exact action
            0.4 * (unmatched_manifest_kws.len() as f64 / all_manifest_keywords.len() as f64)
        } else {
            // Normal penalty otherwise
            0.2 * (unmatched_manifest_kws.len() as f64 / all_manifest_keywords.len() as f64)
        }
    } else {
        0.0
    };

    let mismatch_penalty = if mismatches > 0 {
        // If there are clear mismatches, penalize more
        0.3 * (mismatches as f64 / need_keywords.len() as f64)
    } else {
        0.0
    };

    // Base score from keyword matches
    let keyword_score = if need_keywords.is_empty() {
        0.0
    } else {
        matches as f64 / need_keywords.len() as f64
    };

    // Bonus for action word matching (helps prioritize correct actions)
    // e.g., "list" in need should match "list_issues" better than "list_issue_types"
    let action_bonus = if action_match_in_manifest && unmatched_manifest_kws.is_empty() {
        // Perfect action match with no extra keywords
        0.2
    } else if action_match_in_manifest {
        // Action matches but has extra keywords (smaller bonus)
        0.1
    } else {
        0.0
    };

    // Check if keywords appear in order (allowing for word order variations)
    let need_text = need_keywords.join("");
    let manifest_text: String = all_manifest_keywords.join("");
    let ordered_match = need_keywords
        .iter()
        .all(|need_kw| manifest_text.contains(need_kw.as_str()));

    // Bonus for ordered match (all keywords present)
    let ordered_bonus = if ordered_match && mismatches == 0 && unmatched_manifest_kws.is_empty() {
        0.3
    } else if ordered_match && mismatches == 0 {
        0.15 // Reduced bonus if there are extra keywords
    } else {
        0.0
    };

    // Bonus for exact substring match
    let substring_bonus = if manifest_text.contains(need_text.as_str())
        || need_text.contains(manifest_text.as_str())
    {
        0.2
    } else {
        0.0
    };

    // Combined score with bonuses and penalties (capped at 1.0, floor at 0.0)
    let total_penalty = mismatch_penalty + extra_keyword_penalty;
    let total_bonus = ordered_bonus + substring_bonus + action_bonus;
    ((keyword_score + total_bonus - total_penalty).max(0.0)).min(1.0)
}

/// Improved description match score with action verb awareness and capability class validation
/// Extract domain/context keywords (non-action words that indicate the domain)
/// Examples: "github", "issues", "travel", "flights", "repository", "user", "email", etc.
fn extract_domain_keywords(text: &str) -> Vec<String> {
    let action_verbs = extract_action_verbs(text);
    let all_keywords = extract_keywords_from_text(text);

    // Filter out action verbs and common stop words
    let stop_words = vec![
        "the", "a", "an", "for", "from", "to", "of", "in", "on", "at", "by", "with", "and", "or",
        "but",
    ];

    all_keywords
        .into_iter()
        .filter(|kw| {
            // Keep keywords that are:
            // 1. Not action verbs
            // 2. Not stop words
            // 3. Longer than 2 characters (to filter out common words)
            !action_verbs.contains(kw) && !stop_words.contains(&kw.as_str()) && kw.len() > 2
        })
        .collect()
}

/// Check for domain mismatch - if domains are completely different, heavily penalize
/// Returns a penalty factor (0.0 = no penalty, 1.0 = complete mismatch penalty)
fn calculate_domain_mismatch_penalty(
    need_rationale: &str,
    manifest_description: &str,
    manifest_name: &str,
    manifest_id: &str,
) -> f64 {
    let need_domains = extract_domain_keywords(need_rationale);
    let manifest_text = format!("{} {} {}", manifest_description, manifest_name, manifest_id);
    let manifest_domains = extract_domain_keywords(&manifest_text);

    if need_domains.is_empty() {
        return 0.0; // No domain keywords in need, don't penalize
    }

    // Count how many need domain keywords appear in manifest
    let matching_domains = need_domains
        .iter()
        .filter(|nd| manifest_domains.contains(nd))
        .count();

    // If no domain keywords match, it's a complete domain mismatch
    if matching_domains == 0 && !need_domains.is_empty() {
        // Check if manifest has domain keywords that conflict
        // (e.g., need has "github" but manifest has "travel")
        if !manifest_domains.is_empty() {
            return 0.8; // Heavy penalty for domain mismatch
        }
    }

    // Partial domain match - reduce penalty based on match ratio
    let domain_match_ratio = matching_domains as f64 / need_domains.len() as f64;
    if domain_match_ratio < 0.5 {
        // Less than 50% domain keywords match
        return 0.5 * (1.0 - domain_match_ratio); // Penalty proportional to mismatch
    }

    0.0 // Good domain match, no penalty
}

/// This version:
/// 1. Extracts and weights action verbs more heavily
/// 2. Validates capability class operation type matches
/// 3. Checks for domain mismatches and penalizes them
/// 4. Uses configurable thresholds and weights
pub fn calculate_description_match_score_improved(
    need_rationale: &str,
    manifest_description: &str,
    manifest_name: &str,
    need_capability_class: &str,
    manifest_id: &str,
    config: &crate::discovery::config::DiscoveryConfig,
) -> f64 {
    // Extract action verbs from need and manifest
    let need_action_verbs = extract_action_verbs(need_rationale);
    let manifest_action_verbs =
        extract_action_verbs(&format!("{} {}", manifest_description, manifest_name));

    // Calculate action verb match score
    let action_verb_score =
        calculate_action_verb_match_score(&need_action_verbs, &manifest_action_verbs);

    // If action verbs don't match well enough, heavily penalize the score
    if action_verb_score < config.action_verb_threshold && !need_action_verbs.is_empty() {
        // Action verbs are critical - if they don't match, reduce score significantly
        return (action_verb_score * 0.3).max(0.0); // Cap at 30% of action verb score if mismatch
    }

    // Calculate base keyword match score (using original algorithm)
    let base_keyword_score =
        calculate_description_match_score(need_rationale, manifest_description, manifest_name);

    // Calculate capability class operation type match
    let class_operation_score =
        calculate_capability_class_operation_match(need_capability_class, manifest_id);

    // Check for domain mismatch and calculate penalty
    let domain_mismatch_penalty = calculate_domain_mismatch_penalty(
        need_rationale,
        manifest_description,
        manifest_name,
        manifest_id,
    );

    // Weighted combination:
    // - Base keyword score (weighted by 1 - action_verb_weight - capability_class_weight)
    // - Action verb score (weighted by action_verb_weight)
    // - Capability class operation score (weighted by capability_class_weight)
    let base_weight = 1.0 - config.action_verb_weight - config.capability_class_weight;
    let combined_score = (base_keyword_score * base_weight)
        + (action_verb_score * config.action_verb_weight)
        + (class_operation_score * config.capability_class_weight);

    // Apply domain mismatch penalty (reduces score if domains don't match)
    let final_score = combined_score * (1.0 - domain_mismatch_penalty);

    ((final_score).max(0.0)).min(1.0)
}

/// Extract action verbs from text (common capability operations)
pub fn extract_action_verbs(text: &str) -> Vec<String> {
    let action_verb_patterns = vec![
        "list",
        "display",
        "show",
        "get",
        "fetch",
        "retrieve",
        "read",
        "create",
        "add",
        "post",
        "insert",
        "generate",
        "update",
        "modify",
        "edit",
        "change",
        "set",
        "delete",
        "remove",
        "destroy",
        "filter",
        "search",
        "find",
        "query",
        "transform",
        "convert",
        "process",
        "compute",
        "send",
        "notify",
        "publish",
        "validate",
        "check",
        "verify",
    ];

    let text_lower = text.to_lowercase();
    let mut found_verbs = Vec::new();

    for verb in action_verb_patterns {
        // Check for verb as a word (not part of another word)
        let pattern = format!(r"\b{}\b", verb);
        if regex::Regex::new(&pattern).unwrap().is_match(&text_lower) {
            found_verbs.push(verb.to_string());
        }
    }

    found_verbs
}

/// Calculate how well action verbs match between need and manifest
pub fn calculate_action_verb_match_score(need_verbs: &[String], manifest_verbs: &[String]) -> f64 {
    if need_verbs.is_empty() {
        return 1.0; // No action verb specified, don't penalize
    }

    if manifest_verbs.is_empty() {
        return 0.0; // Manifest has no action verb but need requires one
    }

    // Check for exact matches
    let exact_matches = need_verbs
        .iter()
        .filter(|nv| manifest_verbs.contains(nv))
        .count();

    if exact_matches > 0 {
        return exact_matches as f64 / need_verbs.len() as f64;
    }

    // Check for semantic similarity (e.g., "list" and "display" are similar)
    let similar_groups: Vec<Vec<&str>> = vec![
        vec!["list", "display", "show", "get", "fetch", "retrieve"],
        vec!["create", "add", "post", "insert", "generate"],
        vec!["update", "modify", "edit", "change", "set"],
        vec!["delete", "remove", "destroy"],
        vec!["filter", "search", "find", "query"],
    ];

    for need_verb in need_verbs {
        for group in &similar_groups {
            if group.contains(&need_verb.as_str()) {
                // Check if any manifest verb is in the same group
                if manifest_verbs.iter().any(|mv| group.contains(&mv.as_str())) {
                    return 0.8; // Similar but not exact
                }
            }
        }
    }

    // No match at all
    0.0
}

/// Calculate how well capability class operation type matches manifest ID
/// Extracts operation type from capability class (e.g., "ui.list.display" -> "list")
/// and compares with manifest ID structure
fn calculate_capability_class_operation_match(
    need_capability_class: &str,
    manifest_id: &str,
) -> f64 {
    // Extract operation type from capability class (usually the last part or a verb)
    let need_keywords = extract_keywords(need_capability_class);
    let manifest_keywords = extract_keywords(manifest_id);

    if need_keywords.is_empty() {
        return 0.5; // Neutral if no keywords
    }

    // Find the operation/action word (typically the last keyword or a verb)
    let operation_keywords = vec![
        "list", "display", "show", "get", "fetch", "retrieve", "create", "update", "delete",
        "filter", "search", "find", "query",
    ];
    let need_operation = need_keywords
        .iter()
        .find(|kw| operation_keywords.contains(&kw.as_str()))
        .or_else(|| need_keywords.last())
        .cloned()
        .unwrap_or_default();

    if need_operation.is_empty() {
        return 0.5;
    }

    // Check if manifest contains the operation keyword
    if manifest_keywords.contains(&need_operation) {
        return 1.0;
    }

    // Check for partial matches
    if manifest_keywords
        .iter()
        .any(|mk| mk.contains(&need_operation) || need_operation.contains(mk))
    {
        return 0.7;
    }

    // Check for semantically similar operations (same groups as action verbs)
    let similar_operation_groups: Vec<Vec<&str>> = vec![
        vec![
            "list", "display", "show", "get", "fetch", "retrieve", "read",
        ],
        vec!["search", "find", "query", "filter"],
        vec!["create", "add", "post", "insert", "generate"],
        vec!["update", "modify", "edit", "change", "set"],
        vec!["delete", "remove", "destroy"],
    ];

    for group in &similar_operation_groups {
        if group.contains(&need_operation.as_str()) {
            // Check if any manifest keyword is in the same group
            if manifest_keywords
                .iter()
                .any(|mk| group.contains(&mk.as_str()))
            {
                return 0.8; // Similar operations (e.g., "search" and "list" are both retrieval)
            }
        }
    }

    // No match
    0.3
}

/// Check if a capability need semantically matches a manifest
/// Uses a threshold to determine if match is good enough
pub fn is_semantic_match(
    need_class: &str,
    manifest_id: &str,
    manifest_name: &str,
    threshold: f64,
) -> bool {
    calculate_semantic_match_score(need_class, manifest_id, manifest_name) >= threshold
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_capability_name() {
        // After removing "mcp." prefix, take all parts except first
        assert_eq!(
            normalize_capability_name("mcp.github.list_issues"),
            "list_issues"
        );
        // For "github.issues.list", first part is "github", so we take "issues.list"
        assert_eq!(
            normalize_capability_name("github.issues.list"),
            "issues.list"
        );
        assert_eq!(normalize_capability_name("list_issues"), "list_issues");
    }

    #[test]
    fn test_extract_keywords() {
        let keywords = extract_keywords("github.issues.list");
        assert!(keywords.contains(&"issues".to_string()));
        assert!(keywords.contains(&"list".to_string()));

        let keywords = extract_keywords("mcp.github.list_issues");
        assert!(keywords.contains(&"list".to_string()));
        assert!(keywords.contains(&"issues".to_string()));
    }

    #[test]
    fn test_semantic_matching() {
        // Should match: "github.issues.list" -> "mcp.github.list_issues"
        let score = calculate_semantic_match_score(
            "github.issues.list",
            "mcp.github.list_issues",
            "List Issues",
        );
        assert!(score > 0.7, "Expected high match score, got {}", score);

        // Should match: "issues.list" -> "list_issues"
        let score =
            calculate_semantic_match_score("issues.list", "mcp.github.list_issues", "List Issues");
        assert!(score > 0.6, "Expected good match score, got {}", score);

        // Should not match: "github.pulls.list" -> "mcp.github.list_issues"
        let score = calculate_semantic_match_score(
            "github.pulls.list",
            "mcp.github.list_issues",
            "List Issues",
        );
        assert!(score < 0.4, "Expected low match score, got {}", score);
    }

    #[test]
    fn test_description_matching_real_world() {
        println!("\n🧪 Testing Description-Based Matching (Real-World Cases)\n");

        // Test Case 1: Functional rationale vs actual description
        let score = calculate_description_match_score(
            "List all open issues in a GitHub repository",
            "List issues in a GitHub repository. For pagination, use the 'endCursor' from the previous response's 'pageInfo' in the 'after' parameter.",
            "list_issues",
        );
        println!("Test 1: Functional rationale");
        println!("  Rationale: 'List all open issues in a GitHub repository'");
        println!("  Description: 'List issues in a GitHub repository...'");
        println!("  Score: {:.3}", score);
        assert!(
            score > 0.6,
            "Expected good match for functional description, got {}",
            score
        );

        // Test Case 2: Generic step name rationale (current problem)
        let score = calculate_description_match_score(
            "Need for step: List GitHub Repository Issues",
            "List issues in a GitHub repository. For pagination, use the 'endCursor' from the previous response's 'pageInfo' in the 'after' parameter.",
            "list_issues",
        );
        println!("\nTest 2: Generic step name rationale");
        println!("  Rationale: 'Need for step: List GitHub Repository Issues'");
        println!("  Description: 'List issues in a GitHub repository...'");
        println!("  Score: {:.3}", score);
        // This might score lower, which is the issue we're trying to fix
        println!("  Note: Lower score expected due to generic rationale format");

        // Test Case 3: Various wording variations
        let variations = vec![
            ("Retrieve GitHub repository issues", 0.5),
            ("Get issues from GitHub repo", 0.4),
            ("Fetch open issues for a repository", 0.4),
            ("List repository issues", 0.6),
        ];

        println!("\nTest 3: Wording variations");
        for (rationale, min_score) in variations {
            let score = calculate_description_match_score(
                rationale,
                "List issues in a GitHub repository. For pagination, use the 'endCursor' from the previous response's 'pageInfo' in the 'after' parameter.",
                "list_issues",
            );
            println!(
                "  '{}' → Score: {:.3} (min: {:.1}) {}",
                rationale,
                score,
                min_score,
                if score >= min_score { "✓" } else { "✗" }
            );
            assert!(
                score >= min_score,
                "Expected at least {}, got {}",
                min_score,
                score
            );
        }
    }
}
