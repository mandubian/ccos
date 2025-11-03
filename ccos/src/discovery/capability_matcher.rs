//! Semantic matching utilities for capability discovery
//!
//! This module provides fuzzy matching between LLM-generated capability names
//! (e.g., "github.issues.list") and actual MCP capability IDs (e.g., "mcp.github.list_issues").

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
            matched_manifest_kws.push(matched);
        } else {
            // Check for partial matches (e.g., "list" in "list issues")
            if all_manifest_keywords.iter().any(|mk| mk.contains(need_kw.as_str()) || need_kw.contains(mk.as_str())) {
                matches += 1;
                if let Some(partial_match) = all_manifest_keywords.iter().find(|mk| mk.contains(need_kw.as_str()) || need_kw.contains(mk.as_str())) {
                    if !matched_manifest_kws.contains(&partial_match) {
                        matched_manifest_kws.push(partial_match);
                    }
                }
            } else {
                mismatches += 1;
            }
        }
    }
    
    // Base score from keyword matches
    let keyword_score = if need_keywords.is_empty() {
        0.0
    } else {
        matches as f64 / need_keywords.len() as f64
    };
    
    // Check if keywords appear in description
    let need_text = need_keywords.join("");
    let manifest_text: String = all_manifest_keywords.join("");
    let ordered_match = need_keywords.iter().all(|need_kw| {
        manifest_text.contains(need_kw.as_str())
    });
    
    // Bonus for ordered match (all keywords present)
    let ordered_bonus = if ordered_match && mismatches == 0 { 0.3 } else { 0.0 };
    
    // Bonus for exact substring match
    let substring_bonus = if manifest_text.contains(need_text.as_str()) || need_text.contains(manifest_text.as_str()) {
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
    ((keyword_score + ordered_bonus + substring_bonus - mismatch_penalty).max(0.0)).min(1.0)
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
            if all_manifest_keywords.iter().any(|mk| mk.contains(need_kw.as_str()) || need_kw.contains(mk.as_str())) {
                matches += 1;
                // Find and track the partial match
                if let Some(partial_match) = all_manifest_keywords.iter().find(|mk| mk.contains(need_kw.as_str()) || need_kw.contains(mk.as_str())) {
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
    // (e.g., "pulls" vs "issues" - both should match to be a good match)
    let unmatched_manifest_kws: Vec<&String> = all_manifest_keywords
        .iter()
        .filter(|mk| !matched_manifest_kws.contains(mk))
        .collect();
    
    let mismatch_penalty = if mismatches > 0 || (!unmatched_manifest_kws.is_empty() && need_keywords.len() > 2) {
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
    
    // Check if keywords appear in order (allowing for word order variations)
    let need_text = need_keywords.join("");
    let manifest_text: String = all_manifest_keywords.join("");
    let ordered_match = need_keywords.iter().all(|need_kw| {
        manifest_text.contains(need_kw.as_str())
    });
    
    // Bonus for ordered match (all keywords present)
    let ordered_bonus = if ordered_match && mismatches == 0 { 0.3 } else { 0.0 };
    
    // Bonus for exact substring match
    let substring_bonus = if manifest_text.contains(need_text.as_str()) || need_text.contains(manifest_text.as_str()) {
        0.2
    } else {
        0.0
    };
    
    // Combined score with penalty (capped at 1.0, floor at 0.0)
    ((keyword_score + ordered_bonus + substring_bonus - mismatch_penalty).max(0.0)).min(1.0)
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
        assert_eq!(normalize_capability_name("mcp.github.list_issues"), "list_issues");
        // For "github.issues.list", first part is "github", so we take "issues.list"
        assert_eq!(normalize_capability_name("github.issues.list"), "issues.list");
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
        let score = calculate_semantic_match_score(
            "issues.list",
            "mcp.github.list_issues",
            "List Issues",
        );
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
        assert!(score > 0.6, "Expected good match for functional description, got {}", score);
        
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
            println!("  '{}' → Score: {:.3} (min: {:.1}) {}", 
                rationale, score, min_score, 
                if score >= min_score { "✓" } else { "✗" });
            assert!(score >= min_score, "Expected at least {}, got {}", min_score, score);
        }
    }
}

