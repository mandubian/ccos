# Dynamic Keyword Extraction: Implementation Reasoning

## Problem Statement

The original implementation had **hardcoded parameter categories** in `ResearchPreferences`:
- `topic`, `domains`, `depth`, `format`, `sources`, `time_constraint`

These 6 fields were specific to research workflows and didn't adapt to other domains like travel planning, project management, or cooking.

**Key Issue**: The parameter names were hard-coded in:
1. The struct definition
2. The LLM prompt asking for extraction
3. The capability synthesis prompt

This meant capabilities always included the same parameters regardless of domain.

## Solution Overview

Replace fixed parameter extraction with **domain-agnostic dynamic extraction** where:
1. The LLM identifies semantic parameters from Q&A pairs
2. Parameters can be any name meaningful to the domain
3. Type inference categorizes values (currency, duration, list, etc.)
4. Extracted parameters are passed back to the LLM for capability synthesis

## Design Choices

### 1. BTreeMap for Parameters Storage

**Choice**: Use `BTreeMap<String, ExtractedParam>` instead of hardcoded struct fields

**Rationale**:
- **Flexibility**: Any parameter name can be extracted
- **Ordered**: BTreeMap maintains sorted order (deterministic)
- **Type-safe**: Still strongly typed through ExtractedParam struct
- **Queryable**: Can ask for "budget", "duration", etc. via `.get()`

**Alternative considered**: HashMap
- Rejected: Non-deterministic iteration order would cause inconsistent capability generation

### 2. Type Inference System

**Choice**: Infer 6 parameter types: `string`, `number`, `list`, `boolean`, `duration`, `currency`

**Rationale**:
- **Semantic meaning**: "currency" is more meaningful than just "string" with dollar signs
- **Conversion-ready**: Types can be mapped to RTFS type declarations
- **Extensible**: New types can be added without changing core logic
- **LLM-friendly**: Simple enough for LLM to infer accurately

**Mapping to RTFS**:
```rust
match param.param_type {
    "list" => "(list \"string\")",
    "number" => "\"number\"",
    "currency" | "duration" => "\"string\"",  // Represented as strings in RTFS
    _ => "\"string\"",
}
```

### 3. Question Preservation

**Choice**: Store the original question with each parameter

**Rationale**:
- **Traceability**: Know which question extracted each parameter
- **Synthesis context**: LLM can reference the original question in capability generation
- **User experience**: Can show "This parameter came from: 'What is your budget?'"
- **Debugging**: Easier to debug why a parameter was extracted

```rust
struct ExtractedParam {
    question: String,  // Preserved from interaction
    value: String,
    param_type: String,
    category: Option<String>,
}
```

### 4. LLM-Driven vs. Heuristic Extraction

**Choice**: Try LLM extraction first, fall back to heuristic

**Rationale**:
- **Quality**: LLM extraction is more accurate and domain-aware
- **Resilience**: Falls back gracefully if LLM unavailable
- **Hybrid approach**: Gets best of both worlds

```rust
// Try LLM first
if let Ok(Some(parsed)) = parse_preferences_via_llm(ccos, topic, &interaction_history).await {
    return Ok((parsed, interaction_history));
}

// Fall back to heuristic
let prefs = ExtractedPreferences {
    goal: topic.to_string(),
    parameters: extract_parameters_heuristically(&interaction_history),
};
```

### 5. Backward Compatibility Layer

**Choice**: Keep `ResearchPreferences` struct and provide `to_legacy()` method

**Rationale**:
- **Migration path**: Existing code doesn't break
- **Gradual adoption**: Can migrate module by module
- **Testing**: Old tests still work
- **Interop**: Can convert between old and new formats

```rust
impl ExtractedPreferences {
    fn to_legacy(&self) -> ResearchPreferences {
        ResearchPreferences {
            topic: self.goal.clone(),
            domains: self.parameters.get("domains")
                .or_else(|| self.parameters.get("interests"))
                .map(|p| vec![p.value.clone()])
                .unwrap_or_default(),
            // ... more mappings ...
        }
    }
}
```

## Prompt Engineering Choices

### LLM Extraction Prompt

**Key decision**: Ask LLM to identify parameter NAME from each Q&A pair

```
For each Q/A pair, identify what parameter the question is asking about
(e.g., "budget", "duration", "interests")
```

**Why this works**:
1. LLM understands semantic meaning of questions
2. Output is JSON with arbitrary parameter names
3. Examples guide the LLM: "budget", "duration", "interests"
4. Type inference is clearer than trying to map raw answers

### Synthesis Prompt Inclusion

**Key decision**: Include extracted parameters in synthesis prompt to capability generator

```
EXTRACTED PARAMETERS FROM USER INTERACTION (use these in your capability):
- budget (currency) from Q: "What is your approximate budget?" → A: "5000"
- duration (number) from Q: "How long do you plan to stay?" → A: "7 days"
...
```

**Why this helps**:
1. LLM sees what parameters were extracted
2. LLM understands user's answers (the values)
3. LLM can match parameters in capability definition
4. Generated capabilities use consistent parameter names

## Testing Strategy

### Demo Script Validation

**Why we ran**: `./demo_smart_assistant.sh --topic "plan trip to paris"`

**What validates**:
1. ✅ Dynamic questions are generated (for the goal)
2. ✅ Parameters are extracted from Q&A (budget, duration, travelers, interests)
3. ✅ Capability is synthesized with extracted parameters
4. ✅ Capability ID matches goal (travel.trip-planner.paris.v1)
5. ✅ Reuse works for similar requests

### Build Validation

```bash
cargo build --release --example user_interaction_smart_assistant
```

**Verified**: No compilation errors, only expected warnings about unused code in helpers

## Trade-offs Made

### Trade-off 1: LLM Latency vs. Accuracy

**Decision**: Accept one additional LLM call for extraction

**Trade-off**:
- **Cost**: ~1-2 seconds per learning phase
- **Benefit**: Much better parameter extraction than heuristics
- **Mitigation**: Only happens during learning phase, not application phase

### Trade-off 2: String Values vs. Typed Values

**Decision**: Store parameter values as `String`, not `serde_json::Value`

**Trade-off**:
- **Simpler**: Easier to work with strings
- **Lost info**: No nested structures in parameter values
- **Benefit**: Simpler serialization, clearer LLM prompts
- **Mitigation**: RTFS can parse strings as needed

### Trade-off 3: Parameter Scope

**Decision**: Extract parameters at parsing level, not deeper semantic analysis

**Trade-off**:
- **Simple**: Doesn't try to infer relationships between parameters
- **Limited**: Can't detect "budget" and "travelers" affect flight cost calculation
- **Benefit**: Keeps implementation focused and working
- **Future**: Can add relationship analysis later

## Code Structure

### Core Structs (ExtractedPreferences.rs concepts)

```
ExtractedPreferences          ExtractedParam
├─ goal: String             ├─ question: String
└─ parameters: BTreeMap     ├─ value: String
                            ├─ param_type: String
                            └─ category: Option<String>
```

### Core Functions

1. **`parse_preferences_via_llm`**: LLM-driven extraction
   - Builds JSON prompt with Q&A pairs
   - Calls LLM to identify parameters
   - Returns `ExtractedPreferences`

2. **Fallback heuristic** (in `gather_preferences_via_ccos`):
   - Keyword matching on questions
   - Type inference from keywords
   - Returns `ExtractedPreferences`

3. **`synthesize_capability_via_llm`**: Uses extracted parameters
   - Formats parameters summary from extracted data
   - Includes in synthesis prompt
   - LLM generates domain-specific capability

## Future Improvements

### 1. Parameter Relationships
Track which parameters affect which (budget + travelers → cost)

### 2. Constraint Validation
Verify extracted parameters against type constraints

### 3. Parameter Templates
Store reusable parameter sets for common domains

### 4. User Suggestions
Let users suggest/correct parameter names

### 5. Multilingual Extraction
Extract parameters that are language-specific

## Compliance Notes

### CCOS Compliance
- ✅ Follows intent graph pattern (questions → parameters → capability)
- ✅ Uses causal chain for Q&A logging
- ✅ Respects capability marketplace registration

### RTFS Compliance
- ✅ Generated parameters compile to valid RTFS
- ✅ Types match RTFS type system
- ✅ Parameters used in (let ...) bindings

## Conclusion

The dynamic keyword extraction system successfully generalizes the hardcoded research workflow to work with any domain. By letting the LLM identify meaningful parameter names and types, capabilities can be generated for travel planning, project management, or any other domain—without changing the core implementation.

The key insight: **Don't hardcode parameter names; let the LLM extract them semantically from the user's actual questions and answers.**
