# Description-Based Semantic Matching Algorithm

This document explains how `calculate_description_match_score` computes semantic similarity between a capability need's rationale and a capability's description.

## Overview

The algorithm matches **what you need** (rationale: "List all open issues in a GitHub repository") with **what a capability does** (description: "List issues in a GitHub repository"). It returns a score from 0.0 to 1.0, where 1.0 is a perfect match.

## Algorithm Steps

### Step 1: Tokenization

**Input:** Free-text rationale and description (sentences, not structured IDs)

**Process:**
```rust
// Tokenize both texts into lowercase word tokens
need_keywords = extract_keywords_from_text(need_rationale)
desc_keywords = extract_keywords_from_text(manifest_description)
name_keywords = extract_keywords_from_text(manifest_name)
```

**Example:**
- Rationale: `"List all open issues in a GitHub repository"`
- Keywords: `["list", "all", "open", "issues", "in", "a", "github", "repository"]`
- (Filters out words < 2 chars like "a", "in")

### Step 2: Combine Manifest Keywords

The algorithm considers **both** the capability description and name:

```rust
all_manifest_keywords = desc_keywords + name_keywords
```

This ensures that even if the description is short, the name can contribute relevant keywords.

**Example:**
- Description: `"List issues in a GitHub repository"`
- Name: `"list_issues"`
- Combined keywords: `["list", "issues", "in", "a", "github", "repository", "list", "issues"]`

### Step 3: Keyword Matching

For each keyword in the need:

1. **Exact match**: If keyword exists in manifest keywords → `matches += 1`
2. **Partial match**: If keyword is substring of manifest keyword (or vice versa) → `matches += 1`
   - Example: "issue" matches "issues" via substring
3. **Mismatch**: If no match found → `mismatches += 1`

**Example:**
```
Need: ["list", "all", "open", "issues", "github", "repository"]
Manifest: ["list", "issues", "github", "repository"]

Matches:
  "list" → ✓ exact match
  "all" → ✗ mismatch
  "open" → ✗ mismatch  
  "issues" → ✓ exact match
  "github" → ✓ exact match
  "repository" → ✓ exact match

Result: 4 matches, 2 mismatches
```

### Step 4: Base Score Calculation

```rust
keyword_score = matches / need_keywords.len()
```

This is the **proportion of need keywords that matched**.

**Example:**
- 4 matches out of 6 keywords = `4/6 = 0.667`

### Step 5: Bonuses (Score Enhancements)

#### A. Ordered Match Bonus (+0.3)

If **all** need keywords appear in the manifest AND there are no mismatches:

```rust
ordered_bonus = 0.3  // All keywords found, perfect semantic alignment
```

**Example:**
- Need: `["list", "issues", "github"]`
- Manifest: `["list", "issues", "in", "a", "github", "repository"]`
- Result: All keywords present → `+0.3 bonus`

#### B. Substring Bonus (+0.2)

If the need text appears as a substring in the manifest (or vice versa):

```rust
substring_bonus = 0.2  // Very close textual match
```

**Example:**
- Need: `"list issues"`
- Manifest: `"list issues in a GitHub repository"`
- Result: Need is substring → `+0.2 bonus`

### Step 6: Penalties (Score Reductions)

#### Mismatch Penalty (-0.2 × mismatch_ratio)

If there are keywords in the need that don't match the manifest:

```rust
mismatch_penalty = 0.2 * (mismatches / need_keywords.len())
```

This penalizes capabilities that are **missing relevant keywords**.

**Example:**
- 2 mismatches out of 6 keywords = `0.2 * (2/6) = 0.067 penalty`

### Step 7: Final Score

```rust
final_score = keyword_score + ordered_bonus + substring_bonus - mismatch_penalty
final_score = max(0.0, min(1.0, final_score))  // Clamp to [0.0, 1.0]
```

## Complete Example

**Input:**
- Rationale: `"List all open issues in a GitHub repository"`
- Description: `"List issues in a GitHub repository. For pagination..."`
- Name: `"list_issues"`

**Step-by-step:**

1. **Tokenization:**
   - Need: `["list", "all", "open", "issues", "in", "a", "github", "repository"]`
   - Desc: `["list", "issues", "in", "a", "github", "repository"]`
   - Name: `["list", "issues"]`

2. **Matching:**
   - `"list"` → ✓ match
   - `"all"` → ✗ mismatch
   - `"open"` → ✗ mismatch
   - `"issues"` → ✓ match
   - `"github"` → ✓ match
   - `"repository"` → ✓ match
   - (Filtering "in", "a" as too short)

3. **Scores:**
   - `keyword_score = 4/6 = 0.667`
   - `ordered_bonus = 0.0` (not all keywords, has mismatches)
   - `substring_bonus = 0.2` ("list issues" is substring)
   - `mismatch_penalty = 0.2 * (2/6) = 0.067`

4. **Final:**
   - `0.667 + 0.0 + 0.2 - 0.067 = 0.800`

## Key Characteristics

### ✅ Strengths

1. **Semantic over syntactic**: Matches meaning, not exact words
   - "list issues" matches "List Issues" even with different casing

2. **Handles partial matches**: "issue" matches "issues"
   - Useful for plural/singular variations

3. **Considers context**: Uses both description and name
   - If description is brief, name provides additional keywords

4. **Rewards alignment**: Bonuses for good matches
   - Ordered match bonus rewards comprehensive keyword coverage

### ⚠️ Limitations

1. **No synonym handling**: "get" vs "retrieve" treated as different
   - Future: Could add synonym dictionary

2. **No semantic embeddings**: Purely keyword-based
   - Future: Could use word embeddings (Word2Vec, BERT) for deeper semantics

3. **Order-independent**: "list issues" matches "issues list"
   - Can be both strength and weakness

4. **Sensitive to wording**: "List issues" vs "Get issues" score differently
   - Rationale quality matters

## Comparison: Description vs Name Matching

| Aspect | Description-Based | Name-Based |
|--------|-------------------|------------|
| **Input** | Free-text rationale | Structured capability class |
| **Tokenization** | Sentence tokenizer | Structured ID parser (dots/underscores/camelCase) |
| **Use Case** | LLM generates rationale | LLM generates capability class |
| **Best For** | Functional descriptions | Structured identifiers |

**Example:**
- **Description matching**: `"List all open issues"` → `"List issues in a GitHub repository"` (0.80)
- **Name matching**: `"github.issues.list"` → `"mcp.github.list_issues"` (0.75)

## Threshold Recommendations

- **≥ 0.7**: Strong match (likely correct)
- **0.5 - 0.7**: Good match (probably correct)
- **0.3 - 0.5**: Weak match (might be correct, needs review)
- **< 0.3**: Poor match (likely incorrect)

## Testing

Run the test suite to see examples:

```bash
cargo test --lib --package ccos capability_matcher::tests::test_description_matching_real_world -- --nocapture
```

This shows real-world matching scenarios and their scores.

