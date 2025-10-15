# Dynamic Keyword Extraction for Capability Synthesis

## Overview

The smart assistant system has been upgraded to use **dynamic keyword extraction** instead of hardcoded parameter categories. This allows the system to adapt to any domain and extract meaningful parameters that are specific to the user's goal.

## Problem: Hardcoded Categories

### Before
The old `ResearchPreferences` struct had 6 hardcoded categories:

```rust
struct ResearchPreferences {
    topic: String,
    domains: Vec<String>,
    depth: String,
    format: String,
    sources: Vec<String>,
    time_constraint: String,
}
```

**Limitations:**
- Only works for research workflows
- Cannot adapt to other domains (travel, project management, etc.)
- Generic capabilities don't match user's actual goals
- Questions asked were always the same, regardless of domain
- Parameter names were fixed and couldn't be reused in synthesis

## Solution: Dynamic Extraction

### After: ExtractedPreferences

The new system uses a dynamic structure that captures arbitrary keyword-value pairs:

```rust
struct ExtractedPreferences {
    goal: String,  // The main user goal/topic
    parameters: BTreeMap<String, ExtractedParam>,  // Domain-specific parameters
}

struct ExtractedParam {
    question: String,        // The Q&A pair's question
    value: String,          // The user's answer
    param_type: String,     // Inferred: "string", "number", "list", etc.
    category: Option<String>, // Optional semantic grouping
}
```

**Benefits:**
- Extracts parameters specific to each domain
- LLM identifies meaningful keywords (budget, duration, interests, etc.)
- Questions are preserved and mapped to parameters
- Type inference: currency, duration, list, boolean, etc.
- Enables reuse of extracted keywords in capability synthesis

## How It Works

### Step 1: LLM-Driven Keyword Extraction

The `parse_preferences_via_llm` function asks the LLM to analyze Q&A pairs and extract semantic parameters:

```rust
async fn parse_preferences_via_llm(
    ccos: &Arc<CCOS>,
    topic: &str,
    interaction_history: &[(String, String)],
) -> Result<Option<ExtractedPreferences>, Box<dyn std::error::Error>>
```

**Prompt sent to LLM:**
```
Analyze these question/answer pairs and extract semantic parameters with inferred types.

Your task:
1. For each Q/A pair, identify what parameter the question is asking about
2. Infer the parameter type (string, number, list, boolean, duration, currency)
3. Return a JSON object where each parameter maps to metadata

Examples:
Q: "What's your budget?"  -> parameter: "budget", type: "currency"
Q: "How many days?"       -> parameter: "duration", type: "number"
Q: "What interests you?"  -> parameter: "interests", type: "list"
```

### Step 2: Dynamic Parameter Extraction

For the "plan trip to paris" example, the LLM might extract:

```json
{
  "goal": "plan trip to paris",
  "parameters": {
    "budget": {
      "type": "currency",
      "value": "5000",
      "question": "What is your approximate budget?"
    },
    "duration": {
      "type": "number",
      "value": "7 days",
      "question": "How long do you plan to stay?"
    },
    "travelers": {
      "type": "string",
      "value": "couple",
      "question": "Who is going on the trip?"
    },
    "interests": {
      "type": "list",
      "value": "art, food, culture",
      "question": "What are your interests?"
    }
  }
}
```

### Step 3: Use in Capability Synthesis

The extracted parameters are passed to the LLM for capability generation:

```rust
async fn synthesize_capability_via_llm(
    ccos: &Arc<CCOS>,
    topic: &str,
    interaction_history: &[(String, String)],
    prefs: &ExtractedPreferences,  // Now includes dynamic parameters!
) -> Result<(String, String), Box<dyn std::error::Error>>
```

The parameters are formatted and included in the synthesis prompt:

```
EXTRACTED PARAMETERS FROM USER INTERACTION (use these in your capability):
- budget (currency) from Q: "What is your approximate budget?" → A: "5000"
- duration (number) from Q: "How long do you plan to stay?" → A: "7 days"
- travelers (string) from Q: "Who is going on the trip?" → A: "couple"
- interests (list) from Q: "What are your interests?" → A: "art, food, culture"
```

### Step 4: LLM Generates Domain-Specific Capability

The LLM now uses the extracted parameters to generate a capability tailored to the goal:

```rtfs
(capability "travel.trip-planner.paris.v1"
  :parameters {:destination "string" 
               :budget "currency" 
               :duration "number" 
               :interests "list"
               :travelers "string"}
  :implementation
    (do
      (let flights
        (call :travel.flights {:destination destination 
                               :budget budget 
                               :travelers travelers}))
      (let accommodation
        (call :travel.accommodation {:city destination 
                                    :budget budget 
                                    :duration duration}))
      ...))
```

## Implementation Details

### Dynamic Parameter Methods

The `ExtractedPreferences` struct provides helper methods:

```rust
impl ExtractedPreferences {
    /// Generate RTFS type declaration for parameters
    fn get_parameter_schema(&self) -> String {
        // Converts: {"budget": ...currency} -> ":budget \"string\""
    }
    
    /// Generate RTFS let bindings for parameters
    fn get_parameter_bindings(&self) -> String {
        // Creates: :budget budget :duration duration ...
    }
    
    /// Convert to legacy ResearchPreferences for backward compatibility
    fn to_legacy(&self) -> ResearchPreferences
}
```

### Fallback Heuristic Extraction

If the LLM isn't available, a heuristic fallback uses keyword matching:

```rust
// Fallback: Use heuristic extraction with dynamic parameters
let mut parameters = BTreeMap::new();

for (question, answer) in interaction_history.iter().skip(1) {
    let q_lower = question.to_lowercase();
    
    // Infer parameter name from question keywords
    let param_name = if q_lower.contains("budget") {
        Some(("budget", "currency"))
    } else if q_lower.contains("day") || q_lower.contains("duration") {
        Some(("duration", "duration"))
    } else if q_lower.contains("interest") {
        Some(("interests", "list"))
    } 
    // ... more patterns ...
}
```

## Domain Adaptation Examples

### Travel Planning
Extracts: `destination`, `budget`, `duration`, `travelers`, `interests`

### Research
Extracts: `domains`, `depth`, `format`, `sources`, `time_constraint`

### Project Management
Extracts: `scope`, `timeline`, `team_size`, `budget`, `deliverables`

### Cooking
Extracts: `servings`, `cuisine_type`, `dietary_restrictions`, `cooking_time`, `difficulty`

## Usage

### For Developers

When implementing a new domain:

1. **No hardcoding needed** - parameters adapt automatically
2. **Questions are dynamic** - generated by `generate_questions_for_goal()`
3. **Extraction is domain-aware** - LLM identifies meaningful keywords
4. **Type inference is automatic** - string, number, list, currency, duration, boolean

```rust
// Old way (hardcoded):
let prefs = ResearchPreferences {
    topic: "...",
    domains: vec![...],  // Hardcoded field
    depth: "...",        // Hardcoded field
    // ...
};

// New way (dynamic):
let prefs = parse_preferences_via_llm(ccos, topic, &interaction_history).await?;
// prefs.parameters contains any parameters the LLM identified!
```

### For Users

The learning flow remains the same, but now works for any domain:

```bash
# Previously: hardcoded to research
./demo_smart_assistant.sh --topic "research quantum computing"

# Now: works for any domain
./demo_smart_assistant.sh --topic "plan trip to paris"
./demo_smart_assistant.sh --topic "build mobile app"
./demo_smart_assistant.sh --topic "organize team project"
```

## Integration Points

1. **Intent Graph**: Questions are generated based on the goal
2. **CCOS Orchestration**: Uses extracted parameters in capability synthesis
3. **RTFS Compilation**: Parameters become RTFS function arguments
4. **Capability Marketplace**: Registers domain-specific capabilities with extracted parameters
5. **Causal Chain**: Questions and answers are logged with parameter extractions

## Backward Compatibility

The old `ResearchPreferences` struct is retained for existing code. New code should use `ExtractedPreferences`:

```rust
// Legacy support
let legacy: ResearchPreferences = new_prefs.to_legacy();

// New approach
let new_prefs: ExtractedPreferences = parse_preferences_via_llm(...).await?;
```

## Testing

Run the demo to verify dynamic extraction:

```bash
./demo_smart_assistant.sh --topic "plan trip to paris"
```

Expected output shows:
1. **Phase 1**: LLM asks dynamic questions
2. **Phase 2**: Extracts domain-specific parameters (budget, duration, travelers, interests)
3. **Phase 3**: Generates trip-planning capability using extracted parameters
4. **Phase 3**: Reuses capability for similar requests

## Performance Impact

- **Question generation**: LLM-driven (was already dynamic)
- **Parameter extraction**: NEW - one additional LLM call during learning
- **Synthesis**: Uses extracted parameters - more context for LLM to generate better capabilities
- **Overall**: Minimal overhead, better quality capabilities

## Future Enhancements

1. **Parameter relationships** - Track dependencies between parameters
2. **Parameter validation** - Verify extracted values against type schemas
3. **Parameter templates** - Reusable parameter sets for common domains
4. **User hints** - Allow users to suggest parameter names
5. **Multi-language support** - Extract parameters in user's language

## References

- **CCOS Spec 001**: Intent graph and dynamic question generation
- **CCOS Spec 004**: Capabilities and marketplace registration
- **CCOS Spec 013**: Working memory and parameter storage
- **RTFS Spec 01**: Grammar and parameter syntax

