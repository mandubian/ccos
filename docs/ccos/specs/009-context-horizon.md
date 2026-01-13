# CCOS Specification 009: Context Horizon (RTFS 2.0 Edition)

**Status:** Draft for Review (Enhanced)
**Version:** 1.1
**Date:** 2025-01-10
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [001: Intent Graph](./001-intent-graph-new.md), [003: Causal Chain](./003-causal-chain.md), [013: Working Memory](./013-working-memory.md), [006: Cognitive Engine](./006-cognitive-engine-and-cognitive-control.md)  

## Introduction: Compact, Queryable Context for LLM Inputs

Context Horizon provides compact, queryable context bundles optimized for Large Language Model (LLM) inputs. Unlike Working Memory (which is persistent and global), Horizons are ephemeral, task-specific snapshots constructed on-demand.

Why essential? LLMs have finite token budgets; Context Horizon intelligently compresses relevant execution history, intent structure, and domain knowledge into a coherent RTFS structure. This enables Cognitive Engine to reason with full context without exceeding model capacity.

**Key Characteristics**:
- Ephemeral: Built per-task, not persisted
- Bounded: Configurable token limits (default: 4096)
- Queryable: Filter by intent, time, relevance
- RTFS-native: Emits as RTFS data structures

### Distinction from Working Memory

| Aspect           | Context Horizon                | Working Memory        |
|------------------|------------------------------|----------------------|
| **Persistence**   | Ephemeral (task-specific)      | Persistent           |
| **Scope**        | Single intent/plan              | Global               |
| **Query Speed**   | Fast (in-memory construction)    | Slower (disk-backed) |
| **Size**         | Bounded (4K tokens default)     | Unbounded            |
| **Purpose**       | LLM inputs for reasoning       | Long-term knowledge  |
| **Sources**       | Intent Graph + Causal Chain + WM | Causal Chain ingestion |
| **Boundaries**    | Temporal, privacy, jurisdictional | Same (applied during ingestion) |
| **Reduction**     | Summarization, sampling         | Full retention       |

## Core Concepts

### 1. Horizon Structure
- **Input**: Query spec (e.g., {:for :cognitive engine, :intent :123, :max-tokens 4000, :focus :failures}).
- **Sources**: Intent Graph (goals), Chain (history), WM (indexed summaries).
- **Processing**: Rank/relevance (e.g., TF-IDF or embedding match), summarize (pure RTFS functions), truncate.
- **Output**: Structured payload (RTFS Map) for LLM prompt.

### 1.1 Horizon Building Process

Horizons are assembled from multiple system sources to provide comprehensive context:

#### Horizon Sources

**Intent Graph**:
- Active intent subtree (current intent + ancestors + descendants)
- Intent goals, constraints, preferences, success criteria
- Intent relationships (DependsOn, IsSubgoalOf, Enables)

**Causal Chain**:
- Recent actions and outcomes
- Capability calls and results
- Errors and retries
- Governance decisions

**Working Memory**:
- Retrieved entries matching intent keywords
- Domain-specific knowledge and patterns
- Learned strategies and heuristics

**Execution Context**:
- Current step and plan state
- Variable bindings and intermediate results
- Resource usage and quotas

#### Building Algorithm

```
Input: Current intent_id, optional constraints
Output: RTFS Horizon struct

Algorithm:
1. Root at current Intent (intent_id)
2. Traverse up to N levels of parent/child intents (default: depth=3)
3. Extract K recent Causal Chain actions (default: last 50 actions)
4. Query Working Memory with intent keywords
5. Apply boundary filters:
   - Temporal boundary: exclude actions older than T
   - Privacy boundary: exclude entries with sensitivity > threshold
   - Jurisdictional boundary: exclude entries outside allowed regions
6. Apply reduction strategies:
   - Summarization: compress repetitive patterns
   - Sampling: select representative examples
   - Prioritization: prefer recent and high-impact actions
7. Emit horizon as RTFS structure via :horizon.build yield
```

### 1.2 Reduction Strategies

When horizon exceeds token limits, system applies reduction strategies:

**Summarization**:
- Replace similar consecutive actions with summary
- Example: Ten consecutive `:CapabilityCall` for `:nlp.sentiment` →
  `(summary "analyzed 10 sentiment batches" :count 10)`

**Sampling**:
- Keep representative subset of actions
- Preserve recent actions (exponential decay weighting)
- Preserve high-impact actions (GovernanceCheckpoint, CapabilitySynthesis)

**Prioritization**:
- Keep actions with `:provenance {:governance :true}` (governance decisions)
- Keep actions with `:type :CapabilityCall` that returned errors
- Keep actions referenced by current intent constraints

**Sample Query and Output**:
```
;; RTFS Yield for Horizon
(call :horizon.build
      {:intent :intent-123
       :sources [:graph :chain]
       :max-tokens 2048
       :relevance :recent-failures})
```
Result Payload (RTFS Map):
```
{:summary \"Intent-123: Analyze sentiment. Last plan failed on :nlp yield (latency timeout). Related intents: 456 (optimize). Chain excerpt: 3 actions with errors.\"
 :full-graph-subtree [intent-123, intent-456]
 :token-count 1800}
```

### 2. Workflow
1. Cognitive Engine requests context (yield or direct).
2. Horizon queries sources (e.g., `:wm.search` for chain).
3. Pure RTFS processing: Filter/map/summarize (e.g., `(reduce summarize recent-actions)`).
4. Assemble payload; yield to LLM cap if needed (e.g., `:llm.summarize` for further compression).
5. Deliver to consumer.

### 2.1 Horizon Size Management

#### Horizon Limits

Configurable per horizon build request:
- `:max-tokens` (default: 4096): Maximum token budget
- `:max-depth` (default: 3): Maximum intent hierarchy depth
- `:max-actions` (default: 50): Maximum Causal Chain actions
- `:max-memory-entries` (default: 20): Maximum Working Memory entries

#### Pruning Strategies

When limits are exceeded:
1. Drop entries exceeding temporal boundary
2. Remove low-relevance actions (based on metadata scores)
3. Summarize repetitive patterns
4. Prioritize recent and high-impact actions
5. If still exceeding: Reduce depth level by 1 and retry

**Diagram: Context Building**:
```mermaid
 graph TD
     Req[Cognitive Engine Request<br/>:intent-123 + Focus]
     H[Context Horizon]
     IG[Intent Graph<br/>Query Subtree]
     CC[Causal Chain<br/>Query Actions]
     WM[Working Memory<br/>Indexed Search]
     Pure[Pure RTFS Transform<br/>(Rank, Summarize)]
     LLM[Optional LLM Yield<br/>:summarize.payload]
     Payload[Final Context Payload<br/>(RTFS Map)]

     Req --> H
     H --> IG & CC & WM
     IG & CC & WM --> Pure
     Pure --> LLM
     LLM --> Payload
     Payload --> Cognitive Engine[Resume Cognitive Engine]
 ```

### 3. RTFS Integration

Cognitive Engine builds horizons via RTFS yields:

```
(call :horizon.build
  {:intent "intent-123"
   :max-depth 2
   :max-tokens 2048
   :boundaries {:temporal {:since "2025-01-01"}
                :privacy {:max-level :low}}})
```

Returns RTFS struct:
```
{:horizon
 {:intent "i-001" "analyze logs" {:status :active}
  :parent "i-000" "monitor system"
  :actions
   [{:action-id "a-001" :type :PlanStarted
     :timestamp "2025-01-10T10:00:00Z"}
    {:action-id "a-002" :type :CapabilityCall
     :function-name :fs/read_logs
     :result {:log-lines [...]}}]
  :working-memory
   [{:entry-id "wm-001"
     :content "User prefers detailed reports"}]}}
```

### 3.1 LLM Prompt Integration

Cognitive Engine includes horizon in LLM prompts as structured RTFS:

```
[Context Horizon]
Intent: analyze logs
Parent: monitor system
Recent Actions:
  - PlanStarted (2025-01-10T10:00:00Z)
  - CapabilityCall :fs/read_logs → Success
Working Memory:
  - "User prefers detailed reports"

[Task]
Generate RTFS plan for intent "analyze logs"
Constraints: Max 5 yields, capabilities: [:fs/* :nlp/*]
```

Benefits:
- Structured context reduces hallucination
- Provenance preserved (intent → action → result)
- Easy to trace LLM decisions back to source data

### 3.2 Horizon Query API

```
(:horizon.query
  :intent "intent-123"
  :filter (:or (:contains "error") (:since "2025-01-01"))
  :limit 20)
```

Filter options:
- `(:contains "keyword")` - Match content containing keyword
- `(:since date)` - Actions/entries after date
- `(:before date)` - Actions/entries before date
- `(:type action-type)` - Filter by action type
- `(:or filter1 filter2)` - Logical OR
- `(:and filter1 filter2)` - Logical AND

Response Format: Returns RTFS list of matching actions/intents/memories as compact tuples

### 4. Example Horizon

Complete horizon for log analysis intent:

```
{:horizon
 {:intent {:id "i-001"
          :goal "analyze logs"
          :status :active
          :constraints {:max-cost 10.0}
          :preferences {:verbosity "high"}}
  :parent {:id "i-000" :goal "monitor system"}
  :actions
   [{:id "a-001" :type :PlanStarted
     :timestamp "2025-01-10T10:00:00Z"}
    {:id "a-002" :type :PlanStepStarted
     :step-id "load-logs"
     :timestamp "2025-01-10T10:00:01Z"}
    {:id "a-003" :type :CapabilityCall
     :function-name :fs/read_logs
     :args ["system.log"]
     :result {:log-lines [...]}
     :success true
     :cost 0.01
     :duration-ms 150
     :timestamp "2025-01-10T10:00:02Z"}
    {:id "a-004" :type :GovernanceCheckpointDecision
     :step-id "analyze-patterns"
     :security-level "medium"
     :decision "approved"
     :timestamp "2025-01-10T10:00:03Z"}]
  :working-memory
   [{:id "wm-001"
     :content "User prefers detailed reports with error counts"}
    {:id "wm-002"
     :content "Historical pattern: log spikes correlate with deployment events"}]}}
```

### 5. Integration with RTFS 2.0 Reentrancy
- **Incremental Builds**: For resumes, query from last checkpoint (`:horizon.build {:from-action :act-456}`) → Append deltas purely.
- **Purity**: All processing in RTFS (local transforms); yields only for sources/LLM.
- **Efficiency**: Caches summaries in WM; reentrant queries reuse.

**Reentrant Example**:
- Session 1: Build context for plan gen → 2000 tokens.
- Pause → Chain advances.
- Resume: Horizon diffs (`recent-since :act-100`) → New payload + prior summary → Cognitive Engine continues with updated view.

### 6. Governance and Limits
Kernel enforces query quotas (e.g., no full-chain access). Policies: Redact sensitive data in payloads.

### 7. Implementation Status

- ✅ Horizon building algorithm
- ✅ Reduction strategies (summarization, sampling, prioritization)
- ✅ RTFS integration (`:horizon.build`, `:horizon.query`)
- ✅ Boundary support (temporal, privacy, jurisdictional)
- ⚠️ LLM prompt integration (in progress)
- ⚠️ Advanced pruning strategies (research)

### 8. Future Enhancements

- **Semantic similarity search**: Use vector embeddings for relevance scoring
- **Horizon caching**: Cache frequently requested horizons
- **Adaptive sizing**: Dynamically adjust horizon size based on task complexity
- **Horizon templates**: Pre-configured horizon structures for common patterns
- **Multi-horizon fusion**: Combine horizons from multiple related intents

---

Horizon bridges data to cognition: Vast history → Focused context, enabling smart, reentrant AI without token waste.

---

**Related Specifications**:
- [001: Intent Graph](./001-intent-graph-new.md) - Intent structure and relationships
- [003: Causal Chain](./003-causal-chain.md) - Action logging and provenance
- [013: Working Memory](./013-working-memory.md) - Persistent knowledge store
- [006: Cognitive Engine](./006-cognitive-engine-and-cognitive-control.md) - LLM integration and prompting
Next: Ethical Governance in 010.