# Intent Graph API Documentation

**Module:** `src/ccos/intent_graph.rs`

## Core Data Structures

### Intent

```rust
pub struct Intent {
    pub intent_id: IntentId,           // Unique identifier
    pub goal: String,                  // Human-readable goal
    pub constraints: HashMap<String, Value>, // Runtime constraints
    pub preferences: HashMap<String, Value>, // User preferences
    pub success_criteria: Option<Value>,     // RTFS validation function
    pub emotional_tone: Option<String>,      // Desired emotional context
    pub parent_intent: Option<IntentId>,     // Parent goal
    pub created_at: u64,                     // Creation timestamp
    pub updated_at: u64,                     // Last update
    pub status: IntentStatus,                // Lifecycle status
    pub metadata: HashMap<String, Value>,    // Additional data
}
```

### Edge Relationships

```rust
pub enum EdgeType {
    DependsOn,      // One intent waits for another
    IsSubgoalOf,    // Component of larger goal
    ConflictsWith,  // May compromise another
    Enables,        // Makes another goal possible
    RelatedTo,      // General relationship
}
```

## Key Methods

### Intent Management

```rust
// Create and store intent
let intent = Intent::new("Analyze sales data".to_string())
    .with_constraint("max_cost".to_string(), Value::Number(100.0))
    .with_preference("priority".to_string(), Value::String("high".to_string()));

graph.store_intent(intent)?;

// Find relevant intents
let relevant = graph.find_relevant_intents("sales analysis");

// Load context window
let context_intents = graph.load_context_window(&intent_ids);

// Update with results
graph.update_intent(intent, &result)?;
```

### Relationship Queries

```rust
// Get dependent intents
let dependencies = graph.get_dependent_intents("intent-001");

// Get subgoals
let subgoals = graph.get_subgoals("intent-001");

// Get conflicts
let conflicts = graph.get_conflicting_intents("intent-001");

// Get related intents
let related = graph.get_related_intents("intent-001");
```

### Lifecycle Management

```rust
// Archive completed intents
graph.archive_completed_intents()?;

// Get statistics
let counts = graph.get_intent_count_by_status();
let active = graph.get_active_intents();
```

## Context Horizon Management

### Token Estimation

```rust
// Estimate tokens for intents
let tokens = context_manager.estimate_tokens(&intents);

// Check if truncation needed
if context_manager.should_truncate(&intents) {
    // Apply reduction strategies
}
```

### Semantic Search

```rust
// Find intents by semantic similarity
let relevant_ids = semantic_search.search("sales analysis")?;

// Load with virtualization
let context_intents = virtualization.load_context_window(&relevant_ids, &storage);
```

## Usage Examples

### Example 1: Goal Hierarchy

```rust
// Main goal
let main = Intent::new("Grow revenue 50%".to_string());
graph.store_intent(main.clone())?;

// Subgoals
let marketing = Intent::new("Launch campaign".to_string())
    .with_parent(main.intent_id.clone());
let product = Intent::new("Release features".to_string())
    .with_parent(main.intent_id.clone());

graph.store_intent(marketing)?;
graph.store_intent(product)?;
```

### Example 2: Context-Aware Execution

```rust
// Find relevant intents
let relevant = graph.find_relevant_intents("marketing");

// Load context window
let context = graph.load_context_window(&relevant.iter().map(|i| i.intent_id.clone()).collect());

// Execute with context
let result = execute_with_context(&plan, &context)?;

// Update intent
graph.update_intent(intent, &result)?;
```

### Example 3: Conflict Detection

```rust
// Potentially conflicting intents
let budget = Intent::new("Minimize costs".to_string())
    .with_constraint("max_cost".to_string(), Value::Number(5000.0));
let features = Intent::new("Add premium features".to_string())
    .with_constraint("min_budget".to_string(), Value::Number(8000.0));

graph.store_intent(budget.clone())?;
graph.store_intent(features)?;

// Auto-detected conflicts
let conflicts = graph.get_conflicting_intents(&budget.intent_id);
```

## Performance Notes

- **Storage:** In-memory HashMap (future: vector/graph databases)
- **Search:** Keyword-based (future: semantic embeddings)
- **Scaling:** O(n²) edge inference, O(log n) lookups
- **Context:** Token estimation with truncation strategies

## Future Enhancements

1. Semantic embeddings for search
2. Graph database for relationships
3. ML-based conflict detection
4. Real-time collaboration
5. Temporal reasoning
6. Emotional intelligence
