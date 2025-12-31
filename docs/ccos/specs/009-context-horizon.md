# CCOS Specification 009: Context Horizon (RTFS 2.0 Edition)

**Status: Implemented**
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [001: Intent Graph](./001-intent-graph-new.md), [013: Working Memory](./013-working-memory-new.md), [006: Arbiter](./006-arbiter-and-cognitive-control-new.md)  

## Introduction: Virtualizing Information for AI

The Context Horizon is CCOS's payload manager: It queries vast data (Intent Graph, Causal Chain, Working Memory) and virtualizes it into concise, relevant contexts for the Arbiter (or other LLMs). Handles token limits, relevance ranking, and summarization via RTFS yields. In RTFS 2.0, it uses pure transforms on queried data before yielding to LLM capabilities, ensuring efficient, focused reasoning.

Why needed? LLMs have finite context; Horizon prevents overload while preserving key history. Reentrancy: Builds incremental contexts for resumed sessions.

## Core Concepts

### 1. Horizon Structure
- **Input**: Query spec (e.g., {:for :arbiter, :intent :123, :max-tokens 4000, :focus :failures}).
- **Sources**: Intent Graph (goals), Chain (history), WM (indexed summaries).
- **Processing**: Rank/relevance (e.g., TF-IDF or embedding match), summarize (pure RTFS functions), truncate.
- **Output**: Structured payload (RTFS Map) for LLM prompt.

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
1. Arbiter requests context (yield or direct).
2. Horizon queries sources (e.g., `:wm.search` for chain).
3. Pure RTFS processing: Filter/map/summarize (e.g., `(reduce summarize recent-actions)`).
4. Assemble payload; yield to LLM cap if needed (e.g., `:llm.summarize` for further compression).
5. Deliver to consumer.

**Diagram: Context Building**:
```mermaid
graph TD
    Req[Arbiter Request<br/>:intent-123 + Focus]
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
    Payload --> Arbiter[Resume Arbiter]
```

### 3. Integration with RTFS 2.0 Reentrancy
- **Incremental Builds**: For resumes, query from last checkpoint (`:horizon.build {:from-action :act-456}`) → Append deltas purely.
- **Purity**: All processing in RTFS (local transforms); yields only for sources/LLM.
- **Efficiency**: Caches summaries in WM; reentrant queries reuse.

**Reentrant Example**:
- Session 1: Build context for plan gen → 2000 tokens.
- Pause → Chain advances.
- Resume: Horizon diffs (`recent-since :act-100`) → New payload + prior summary → Arbiter continues with updated view.

### 4. Governance and Limits
Kernel enforces query quotas (e.g., no full-chain access). Policies: Redact sensitive data in payloads.

Horizon bridges data to cognition: Vast history → Focused context, enabling smart, reentrant AI without token waste.

Next: Ethical Governance in 010.