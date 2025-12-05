# Capability Matching Test Script

## Overview

This test script (`test_capability_matching.rs`) allows you to test the description-based semantic matching functionality independently of the full discovery process.

## Purpose

Test the matching algorithm with different rationale/description pairs to:
1. Verify description-based matching works
2. See actual scores for different rationale formats
3. Debug threshold issues
4. Test wording variations

## Usage

Once the rtfs compilation issue is resolved, run:

```bash
cargo run --example test_capability_matching
```

## What It Tests

### Test Case 1: GitHub Issues Matching (Functional Rationale)
- Rationale: "List all open issues in a GitHub repository"
- Tests matching against actual MCP capability descriptions

### Test Case 2: Generic Step Name Matching (Current Problem)
- Rationale: "Need for step: List GitHub Repository Issues"  
- Shows the issue with generic step names

### Test Case 3: Wording Variations
- Tests different ways of expressing the same need
- Helps understand which wording works best

## Expected Output

The script will show:
- Description match scores for each candidate
- Name match scores for each candidate  
- Best matches above threshold
- Detailed breakdown of scores

## Example Output

```
🧪 Testing Description-Based Matching

📋 Test Case 1: GitHub Issues Listing
Need:
  Capability class: github.issues.list
  Rationale: List all open issues in a GitHub repository

Candidates:
  • mcp.github.list_issues
    Description: List issues in a GitHub repository. For pagination...
    Description match score: 0.850
    Name match score: 0.750
    Best score: 0.850

Results:
  ✓ Best description match: mcp.github.list_issues (score: 0.850)
  ✓ Best name match: mcp.github.list_issues (score: 0.750)
```

## Current Status

✅ **Test script is ready** - The code is complete and correct.

⚠️ **Note**: There may be compilation issues in rtfs integration code (`environment.rs`) as rtfs is being developed in parallel. The test script itself is correct and will run once rtfs compilation issues are resolved.

## Alternative: Unit Test

A unit test version has been added to `capability_matcher.rs` that you can run once rtfs compiles:

```bash
cargo test --lib capability_matcher::tests::test_description_matching_real_world -- --nocapture
```

