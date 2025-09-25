use reqwest::Client;
use serde_json::{json, Value};
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Get GitHub token from environment
    let github_token =
        env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN environment variable required");

    let client = Client::new();
    let base_url = "https://api.github.com";

    // Close Issue #2
    println!("Closing Issue #2...");
    let close_response = client
        .patch(&format!("{}/repos/mandubian/ccos/issues/2", base_url))
        .header("Authorization", format!("token {}", github_token))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "CCOS-Issue-Manager")
        .json(&json!({
            "state": "closed",
            "body": "Issue #2 completed successfully! ✅\n\n- Enhanced Intent Graph with parent-child relationships\n- Added weighted edges with metadata support\n- Implemented subgraph storage and restore functionality\n- Added comprehensive test coverage\n- Updated documentation with new features\n\nAll requirements from Issue #2 have been implemented and tested."
        }))
        .send()
        .await?;

    if close_response.status().is_success() {
        println!("✅ Issue #2 closed successfully!");
    } else {
        let error_text = close_response.text().await?;
        println!("❌ Failed to close Issue #2: {}", error_text);
        return Ok(());
    }

    // Create new issue for CCOS RTFS Library
    println!("Creating new issue for CCOS RTFS Library...");
    let create_response = client
        .post(&format!("{}/repos/mandubian/ccos/issues", base_url))
        .header("Authorization", format!("token {}", github_token))
        .header("Accept", "application/vnd.github.v3+json")
        .header("User-Agent", "CCOS-Issue-Manager")
        .json(&json!({
            "title": "Create CCOS RTFS Library for Intent Graph Functions",
            "body": "## Overview

Create a comprehensive RTFS library that provides bindings for all Intent Graph functions, enabling RTFS programs to interact with the Intent Graph system directly.

## Background

Currently, all Intent Graph operations are implemented in Rust but lack RTFS bindings. This creates a gap where RTFS programs cannot directly:
- Create and manage intents
- Build and traverse intent relationships  
- Store and restore intent subgraphs
- Query intent hierarchies and metadata

## Requirements

### 1. Core Intent Management Functions
- `(create-intent name goal constraints preferences)`
- `(update-intent intent-id updates)`
- `(delete-intent intent-id)`
- `(get-intent intent-id)`
- `(find-intents-by-goal goal-pattern)`

### 2. Relationship Management Functions
- `(create-edge from-intent to-intent edge-type weight metadata)`
- `(update-edge edge-id updates)`
- `(delete-edge edge-id)`
- `(get-edges-for-intent intent-id)`
- `(get-relationship-strength from-intent to-intent)`

### 3. Graph Traversal Functions
- `(get-parent-intents intent-id)`
- `(get-child-intents intent-id)`
- `(get-intent-hierarchy intent-id)`
- `(get-strongly-connected-intents intent-id)`
- `(find-intents-by-relationship edge-type)`

### 4. Subgraph Operations
- `(store-subgraph-from-root root-intent-id file-path)`
- `(store-subgraph-from-child child-intent-id file-path)`
- `(restore-subgraph file-path)`
- `(backup-graph file-path)`

### 5. Advanced Query Functions
- `(query-intents-by-metadata metadata-filters)`
- `(get-intent-statistics)`
- `(find-circular-dependencies)`
- `(get-orphaned-intents)`

## Implementation Plan

### Phase 1: Core Function Bindings
1. Create RTFS function stubs for all core operations
2. Implement proper error handling and validation
3. Add comprehensive type checking for inputs/outputs
4. Create integration tests with the Rust Intent Graph

### Phase 2: Advanced Features
1. Implement graph visualization helpers
2. Add batch operations for efficiency
3. Create intent templates and patterns
4. Add performance monitoring and caching

### Phase 3: Integration & Documentation
1. Create comprehensive RTFS documentation
2. Add examples and tutorials
3. Integrate with CCOS Arbiter for automated intent management
4. Create migration tools for existing intent data

## Success Criteria

- [ ] All Intent Graph functions have RTFS bindings
- [ ] Comprehensive test coverage (>90%)
- [ ] Full documentation with examples
- [ ] Performance benchmarks established
- [ ] Integration with CCOS Arbiter working
- [ ] Migration path for existing data

## Dependencies

- Issue #2: Intent Graph enhancements (✅ COMPLETED)
- RTFS Compiler capability system
- CCOS Intent Graph Rust implementation

## Estimated Effort

- **Phase 1**: 2-3 weeks
- **Phase 2**: 1-2 weeks  
- **Phase 3**: 1 week
- **Total**: 4-6 weeks

## Related Issues

- [Issue #2: Support parent-child and arbitrary relationships in Intent Graph](https://github.com/mandubian/ccos/issues/2) - ✅ COMPLETED
- Future: Integration with CCOS Arbiter for automated intent management

## Notes

This library will be a critical component for enabling RTFS programs to fully leverage the Intent Graph system, making CCOS more accessible and powerful for developers.",
            "labels": ["enhancement", "rtfs", "intent-graph", "library"],
            "assignees": ["mandubian"]
        }))
        .send()
        .await?;

    if create_response.status().is_success() {
        let issue: Value = create_response.json().await?;
        let issue_number = issue["number"].as_u64().unwrap_or(0);
        let html_url = issue["html_url"].as_str().unwrap_or("");
        println!("✅ New issue created successfully!");
        println!("   Issue #{}: {}", issue_number, html_url);

        // Now let's rename the local file to match the new issue number
        println!("Renaming local issue file...");
        let old_path = "ISSUE_3_CCOS_RTFS_LIBRARY.md";
        let new_path = format!("ISSUE_{}_CCOS_RTFS_LIBRARY.md", issue_number);

        if std::path::Path::new(old_path).exists() {
            std::fs::rename(old_path, &new_path)?;
            println!("✅ Renamed {} to {}", old_path, new_path);
        } else {
            println!("⚠️  Local file {} not found, skipping rename", old_path);
        }
    } else {
        let error_text = create_response.text().await?;
        println!("❌ Failed to create new issue: {}", error_text);
    }

    println!("🎉 GitHub issue management completed!");
    Ok(())
}
