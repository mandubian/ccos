# Recursive Capability Discovery & Generation Plan

**Vision**: Transform the smart assistant into a self-organizing system that recursively discovers, synthesizes, and chains capabilities to fulfill complex goals, building a living graph of intent-to-implementation relationships.

## Core Concept

Implement a **recursive goal refinement and capability discovery engine** that progressively builds a graph:

### Main Flow

```
1. Goal Input (e.g., "Plan a trip to Paris")
   ↓
2. Refine Goal via Clarifying Questions
   - Identify meaningful aspects (budget, dates, preferences, constraints)
   - Auto-answered by LLM with context awareness
   ↓
3. Decompose into Steps (via delegation arbiter)
   ↓
4. For Each Step:
   a. Find existing capability (Marketplace → MCP → OpenAPI)
   b. If NOT FOUND:
      → Treat step as NEW GOAL
      → Restart refinement process (back to step 2)
      → Recursively build sub-graph
   ↓
5. Build Graph of Intents & Capabilities
   ↓
6. Generate Orchestrator RTFS
   ↓
7. SIMULATION LOOP (if applicable):
   - Execute in sandbox with test data
   - Analyze results, edge cases, failures
   - Refine goal/decomposition based on simulation insights
   - Regenerate orchestrator
   - Repeat until quality threshold met
   ↓
8. Test, Refine, Execute
```

### Key Insight

Each missing capability triggers a **fresh goal refinement cycle**, building a hierarchical graph where:
- Root: Original user goal
- Nodes: Refined intents (with clarifying question answers)
- Edges: Dependencies between steps
- Leaves: Concrete capabilities (found or synthesized)

## Architecture Components

### 1. Discovery Pipeline (`DiscoveryEngine`)

```rust
pub struct DiscoveryEngine {
    marketplace: Arc<CapabilityMarketplace>,
    mcp_registry: Option<MCPRegistryClient>,
    openapi_discovery: Option<OpenAPIDiscovery>,
    recursive_synthesizer: RecursiveSynthesizer,
    intent_graph: Arc<IntentGraph>,
    llm_provider: Arc<LlmProvider>,
}
```

**Discovery Priority Chain**:
```
Missing Capability → 
  1. Local Marketplace search (semantic matching)
  2. MCP Registry search
  3. OpenAPI introspection
  4. Recursive synthesis (treat as sub-intent)
  5. User fallback
```

### 2. Recursive Synthesizer (`RecursiveSynthesizer`)

Treats a missing capability requirement as a **new intent** and applies the same decomposition process:

```rust
pub struct RecursiveSynthesizer {
    max_depth: usize,
    visited_intents: HashSet<IntentId>,
    synthesis_history: Vec<SynthesisAttempt>,
}

impl RecursiveSynthesizer {
    async fn synthesize_as_intent(
        &mut self,
        capability_need: &CapabilityNeed,
        context: &DiscoveryContext,
    ) -> Result<SynthesizedCapability>;
}
```

**Process** (Treats missing step as NEW GOAL):
1. **Step as Goal**: Missing "restaurant.reservation.book" → Goal "I need to book restaurant reservations"
2. **Refine Goal**: Generate clarifying questions to identify aspects
   - "What cuisine?" "Budget?" "Group size?" etc.
   - Auto-answered by LLM with parent context
3. **Decompose**: Break into sub-steps via delegation arbiter
4. **Discover/Recurse**: For each sub-step:
   - Try Marketplace/MCP/OpenAPI discovery
   - If missing, **RECURSE** (back to step 1 with sub-step as new goal)
5. **Build**: Create orchestrator RTFS once all dependencies resolved
6. **Register**: Save as new capability in marketplace

This creates a **self-similar graph** where each missing capability spawns its own refinement tree.

### 3. Intent Graph Builder (`IntentGraphBuilder`)

Manages relationships between intents and their implementations:

```rust
pub struct IntentGraphBuilder {
    intent_graph: Arc<IntentGraph>,
    orchestrators: HashMap<IntentId, String>, // intent → RTFS orchestrator
    capability_edges: Vec<(IntentId, CapabilityId, EdgeType)>,
}

impl IntentGraphBuilder {
    fn add_intent_with_implementation(
        &mut self,
        intent: &Intent,
        orchestrator_rtfs: &str,
        dependencies: &[CapabilityId],
    );
    
    fn build_execution_graph(&self, root_intent: &IntentId) -> ExecutionGraph;
}
```

**Graph Structure**:
```
root_intent (Plan trip to Paris)
  ├─ intent_1 (Search flights)
  │   └─ [capability: travel.flights.search]
  ├─ intent_2 (Find lodging)
  │   └─ [capability: travel.lodging.reserve]
  └─ intent_3 (Plan activities)
      ├─ intent_3a (Get weather forecast) [RECURSIVELY SYNTHESIZED]
      │   └─ [capability: synth.weather.forecast]
      └─ [capability: travel.activities.plan]
```

### 4. Capability Need Extraction (`CapabilityNeedExtractor`)

Analyzes plan steps to extract missing capabilities:

```rust
pub struct CapabilityNeed {
    capability_class: String,
    required_inputs: Vec<String>,
    expected_outputs: Vec<String>,
    rationale: String,  // Why this capability is needed
}

impl CapabilityNeedExtractor {
    fn extract_from_plan(plan: &Plan) -> Vec<CapabilityNeed>;
    fn extract_from_orchestrator(rtfs: &str) -> Vec<CapabilityNeed>;
}
```

### 5. Execution Graph (`ExecutionGraph`)

Final artifact representing the complete solution:

```rust
pub struct ExecutionGraph {
    root_intent: IntentId,
    nodes: HashMap<IntentId, GraphNode>,
    edges: Vec<(IntentId, IntentId, DependencyType)>,
    execution_order: Vec<IntentId>,
}

pub struct GraphNode {
    intent: StorableIntent,
    orchestrator_rtfs: Option<String>,
    capabilities: Vec<CapabilityManifest>,
    status: ExecutionStatus,
}
```

### 6. Simulation Refinement Engine (`SimulationRefiner`)

Executes orchestrators in a sandbox to identify failures, refine the approach, and iterate:

```rust
pub struct SimulationRefiner {
    sandbox_env: Arc<SandboxEnvironment>,
    llm_provider: Arc<LlmProvider>,
    max_iterations: usize,
    quality_threshold: f64,
}

impl SimulationRefiner {
    async fn simulate_and_refine(
        &self,
        orchestrator: &str,
        test_data: &SimulationData,
    ) -> Result<RefinedOrchestrator>;
    
    async fn analyze_results(
        &self,
        results: &SimulationResults,
    ) -> Vec<RefinementSuggestion>;
}
```

**Simulation Loop**:
1. Execute orchestrator in sandbox with test data
2. Collect results, errors, edge cases
3. Analyze failures and missing edge cases
4. Generate refinement suggestions
5. Refine goal/clarifying questions or re-decompose
6. Regenerate orchestrator
7. Repeat until quality meets threshold or max iterations

**Example**: Trading Algorithm
```
Goal: "Trade bitcoin on market"
  ↓ Refinement & Decomposition
Generated: Algo orchestrator
  ↓
SIMULATION:
  → Execute with historical price data
  → Result: Algorithm loses money in bear market
  → Analysis: Missing stop-loss logic
  → Refinement: Add clarifying question "Risk tolerance?"
  → Re-decompose: Add risk management step
  → Re-generate orchestrator with stop-loss
  ↓
SIMULATION (iteration 2):
  → Execute again
  → Result: Too conservative, misses opportunities
  → Analysis: Static stop-loss threshold
  → Refinement: Use trailing stop-loss
  → Iterate...
  ↓
Quality threshold met → Execute for real
```

## Implementation Phases

### Phase 1: Foundation (Week 1)

**Deliverables**:
1. `DiscoveryEngine` structure
2. Capability need extraction from plans
3. Discovery priority chain (marketplace, MCP, OpenAPI)
4. Basic intent graph integration

**Files**:
- `ccos/src/discovery/engine.rs` - Main discovery orchestrator
- `ccos/src/discovery/need_extractor.rs` - Extract missing capabilities
- `ccos/src/discovery/priority_chain.rs` - Implement discovery order

**Tests**:
- Extract needs from simple plan
- Find capability in marketplace
- Fall back to MCP registry

### Phase 2: Recursive Synthesis (Week 2)

**Deliverables**:
1. `RecursiveSynthesizer` implementation
2. Capability need → Intent transformation
3. Sub-intent generation pipeline
4. Cycle detection & depth limiting

**Files**:
- `ccos/src/discovery/recursive_synthesizer.rs` - Recursive logic
- `ccos/src/discovery/intent_transformer.rs` - Need→Intent conversion
- `ccos/src/discovery/cycle_detector.rs` - Prevent infinite loops

**Tests**:
- Recursively synthesize missing capability
- Detect and prevent cycles
- Depth limit enforcement

### Phase 3: Graph Building (Week 3)

**Deliverables**:
1. `IntentGraphBuilder` implementation
2. Graph visualization (textual/dot format)
3. Execution order computation (topological sort)
4. Dependency resolution

**Files**:
- `ccos/src/discovery/graph_builder.rs` - Build intent graph
- `ccos/src/discovery/execution_planner.rs` - Compute execution order
- `ccos/src/discovery/graph_viz.rs` - Export graph

**Tests**:
- Build graph with dependencies
- Compute valid execution order
- Handle circular dependencies gracefully

### Phase 4: Smart Assistant Integration (Week 4)

**Deliverables**:
1. Wire into `smart_assistant_demo.rs`
2. Discovery progress UI
3. Graph visualization output
4. Fallback to user interaction

**Files**:
- `ccos/examples/smart_assistant_demo.rs` - Integration
- `ccos/examples/discovery_demo.rs` - Standalone demo

**Features**:
- Real-time discovery progress
- Graph DOT export
- Interactive fallback prompts

### Phase 5: Advanced Features (Week 5)

**Deliverables**:
1. Online search integration (Google, Bing, etc.)
2. LLM-based capability synthesis from search results
3. Cross-repository capability sharing
4. Persistent capability cache

**Files**:
- `ccos/src/discovery/web_search.rs` - Online search
- `ccos/src/discovery/synthesis_from_search.rs` - Synthesize from results
- `ccos/src/discovery/cache.rs` - Persistent cache

### Phase 6: Simulation Refinement (Week 6)

**Deliverables**:
1. Sandbox execution environment
2. Result analysis and failure detection
3. LLM-based refinement suggestion generation
4. Iterative improvement loop
5. Quality metrics and thresholds

**Files**:
- `ccos/src/discovery/simulation_refiner.rs` - Main simulation engine
- `ccos/src/discovery/sandbox_env.rs` - Isolated execution sandbox
- `ccos/src/discovery/result_analyzer.rs` - Analyze simulation results
- `ccos/src/discovery/refinement_suggester.rs` - Generate improvements

**Tests**:
- Simulate trading algorithm with historical data
- Detect missing edge cases
- Refine and iterate
- Quality threshold enforcement

## Example Flow

### Input:
```
Goal: "Plan a romantic weekend getaway to Paris with dinner reservations at a Michelin star restaurant"
```

### Phase 1: Initial Decomposition
```
Intent: "Plan romantic Paris getaway"
  Constraints: [destination: Paris, duration: weekend, romance_focus: true]
  
Capability needs:
  - travel.flights.search (AVAILABLE in marketplace)
  - travel.lodging.reserve (AVAILABLE in marketplace)
  - restaurant.reservation.book (MISSING - needs discovery)
  - experience.recommend (MISSING - needs discovery)
```

### Phase 2: Recursive Discovery & Refinement

**Missing Step**: `restaurant.reservation.book`

```
Step becomes NEW GOAL: "Book restaurant reservations"
  ↓
REFINEMENT: Clarifying Questions (auto-answered by LLM):
  - "What cuisine type?" → "Fine French dining (Michelin)"
  - "Price range?" → "Fine dining budget"
  - "Group size?" → "2 people, romantic dinner"
  - "Location?" → "Paris"
  ↓
DECOMPOSITION: Break into steps:
  1. Find Michelin French restaurants in Paris
  2. Check availability for 2 people
  3. Make reservation
  
For Each Step:
  Step 1: "Find Michelin French restaurants"
    → Marketplace: ✓ FOUND "restaurant.search" capability
  Step 2: "Check availability"
    → Marketplace: NOT FOUND
    → MCP: NOT FOUND
    → RECURSIVE: "Check restaurant availability" becomes NEW GOAL
      → REFINEMENT: "Date?" "Time?" → Auto-answered
      → Decomposed: Use restaurant.search with filters
      → Step resolved
  Step 3: "Make reservation"
    → Marketplace: NOT FOUND
    → MCP: NOT FOUND
    → OpenAPI: ✓ Found from restaurant.search results
    → Synthesized: restaurant.reservation.api.book
  ↓
GENERATE: Orchestrator RTFS chains all steps
REGISTER: Capability "restaurant.reservation.book" saved

experience.recommend:
  → Step becomes NEW GOAL: "Recommend romantic experiences"
    → REFINEMENT: Clarifying Questions
      - "Activity type?" → "Cultural and romantic"
      - "Time?" → "Evening"
    → DECOMPOSITION: Steps identified
    → DISCOVERY: All steps missing
    → RECURSIVE SYNTHESIS: LLM generates complete solution
    → REGISTER: Capability saved
```

### Phase 3: Graph Construction
```
root_intent (Plan getaway)
  ├─ intent_flights → [travel.flights.search]
  ├─ intent_lodging → [travel.lodging.reserve]
  ├─ intent_dinner → [restaurant.reservation.book]
  │   ├─ intent_1 → [restaurant.search]
  │   └─ intent_2 → [restaurant.reservation.api.book]
  └─ intent_experiences → [experience.paris.romantic.recommend]
```

### Phase 4: Execution
```
Generated orchestrator chains all capabilities in correct order,
with proper data flow between sub-intents.
```

### Phase 5: Simulation Refinement (Trading Example)

For goals where simulation makes sense (trading, logistics, optimization):

```
Goal: "Trade bitcoin profitably on the market"
  ↓
Generated: Trading strategy orchestrator
  ↓
SIMULATION ITERATION 1:
  → Execute on historical 6-month data
  → Result: Lost 15% in bear market
  → Failure Analysis: No stop-loss, held losing positions
  → Suggestion: Add risk management
  
Refinement: Add clarifying question "Maximum acceptable loss?"
  → Answer: "10% portfolio limit"
  → Re-decompose: Add risk check step
  → Re-generate orchestrator with stop-loss
  ↓
SIMULATION ITERATION 2:
  → Execute on same data
  → Result: Too conservative, missed 40% of profit
  → Failure Analysis: Fixed 10% stop-loss too tight
  → Suggestion: Use trailing stop-loss
  
Refinement: Add "Stop-loss strategy" question
  → Answer: "Trailing stop at 5% below peak"
  → Re-generate with trailing logic
  ↓
SIMULATION ITERATION 3:
  → Execute on data
  → Result: 8% profit, handled volatility well
  → Quality: Meets threshold
  ↓
APPROVED: Execute on live market
```

For non-simulatable goals (like trip planning), skip simulation and go straight to execution.

## Success Metrics

1. **Discovery Success Rate**: % of needed capabilities found/synthesized without user intervention
2. **Graph Depth**: Average depth of recursive synthesis
3. **Cache Hit Rate**: % of capabilities found in local cache
4. **Execution Time**: Total time from goal to execution-ready graph
5. **User Interaction Count**: Number of fallback prompts
6. **Simulation Iterations**: Average iterations to meet quality threshold
7. **Quality Improvement**: % improvement in each simulation iteration
8. **Edge Case Coverage**: % of edge cases discovered during simulation

## Future Enhancements

### Short-term (next 3 months)
- Multi-model LLM fallbacks for synthesis
- Parallel discovery across all sources
- Capability versioning and compatibility checking
- Learning from past synthesis success

### Long-term (6+ months)
- Cross-agent capability sharing via distributed marketplace
- Autonomous capability testing and validation
- Collaborative synthesis (multiple agents building graph together)
- Self-healing graphs (auto-replace broken capabilities)

## Technical Debt & Risks

**Risks**:
1. Infinite recursion (mitigation: depth limits, cycle detection)
2. LLM hallucinations in synthesis (mitigation: validation, testing)
3. Performance degradation with deep graphs (mitigation: caching, parallelization)
4. Security of synthesized code (mitigation: sandboxing, governance)

**Technical Debt**:
1. Need better LLM prompts for recursive synthesis
2. Capability marketplace needs semantic search improvements
3. RTFS orchestrator generation needs better optimization
4. Graph serialization for persistence

## Clarifying Questions as a Discovery Tool

### Why Clarifying Questions Are Essential

In the recursive goal refinement flow, clarifying questions serve a **critical dual purpose**:

1. **Goal Refinement**: They identify meaningful aspects (dates, budget, preferences, constraints) that help decompose the goal into concrete steps
2. **Context Propagation**: Answers propagate down the recursive tree, ensuring each sub-goal inherits relevant context

### Example Without Questions (Poor)
```
Goal: "Book restaurant reservations"
  ↓
Decomposition fails: Unclear what "restaurant reservations" means
  → Generic steps generated
  → Poor capability matching
  → Useless orchestrator
```

### Example With Questions (Better)
```
Goal: "Book restaurant reservations"
  ↓
REFINEMENT via Clarifying Questions:
  - "What cuisine type?" → "Michelin-starred French"
  - "For how many people?" → "2, romantic dinner"
  - "Price range?" → "Fine dining budget"
  - "Location?" → "Paris"
  ↓
Decomposition succeeds with clear aspects:
  → Step 1: Search Michelin French restaurants in Paris
  → Step 2: Filter by availability for 2
  → Step 3: Make reservation
  
Each step can now be properly discovered/matched
  → Generated orchestrator is targeted and useful
```

### Auto-Answering with Context

When recursively synthesizing, the LLM answers questions using:
- **Goal context**: Original user goal (romantic getaway)
- **Parent intent**: Parent capability requirements
- **Existing answers**: Previously answered questions in this synthesis branch
- **Domain knowledge**: LLM's understanding of what "makes sense"

This creates a **coherent capability** that fits the overall goal, not a random standalone function.

### Questions → Better Decomposition

Well-answered clarifying questions result in:
- ✅ Better goal decomposition into concrete steps
- ✅ More relevant capability discovery/matching
- ✅ Cleaner sub-goal generation during recursion
- ✅ Higher-quality synthesized orchestrators
- ✅ Progressive graph building with proper context inheritance

## Open Questions

1. **Parallel vs Sequential Discovery**: Should we try all sources simultaneously or in priority order?
2. **Synthesis Confidence**: How to measure quality of LLM-synthesized capabilities?
3. **Graph Immutability**: Should we track evolution of capability graphs over time?
4. **User Preferences**: How to learn and cache user preferences across sessions?

