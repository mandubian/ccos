# CCOS Specification 017: Checkpoint-Resume (RTFS 2.0 Edition)

**Status: Implemented**
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [002: Plans](./002-plans-and-orchestration-new.md), [003: Causal Chain](./003-causal-chain-new.md)  

## Introduction: Pausable Execution for Long-Running Tasks

Checkpoint-Resume enables reentrant RTFS execution: Capture plan state at yields or errors, persist to chain, and resume later from exact point. Orchestrator snapshots env (immutable, since pure), allowing interruptions (e.g., human review, quotas) without loss. In RTFS 2.0, leverages yield continuations—no internal state means simple replay of pure prefix.

Why important? Agents handle async/real-world flows; reentrancy scales to distributed/multi-session. Purity simplifies: Snapshots are just Values.

## Core Concepts

### 1. Checkpoint Structure
- **Trigger**: Yield, step end, error, or explicit `(checkpoint :name)`.
- **Snapshot**: {:action-id, :env (RTFS Value), :ir-pos (bytecode offset), :pending-yield (if any)}.
- **Storage**: Append to chain as `Action {:type :Checkpoint, :snapshot {...}}`.

**Sample Snapshot** (RTFS Map):
```
{:checkpoint-id :chk-789
 :plan-id :plan-v1
 :ir-pos 42  ;; After map, before save yield
 :env {:reviews [...] :sentiments {:positive 5}}  ;; Pure data
 :pending-yield nil
 :timestamp \"2025-09-20T10:10:00Z\"}
```

### 2. Resume Workflow
1. Orchestrator detects trigger → Serialize env (pure, no locks).
2. Yield to `:checkpoint.store` → Chain append.
3. Later: `:resume.from {:checkpoint :chk-789}` → Load snapshot, replay pure ops to pos, resume.

**Diagram: Checkpoint-Resume Cycle**:
```mermaid
sequenceDiagram
    O[Orchestrator] --> RTFS[Execute IR]
    RTFS --> Yield[Hit Yield/Trigger]
    Yield --> Snap[Snapshot Env + Pos]
    Snap --> Chain[Store Checkpoint Action]
    Note over O: Pause (e.g., Quota/Human)
    later->>O: Resume Request
    O --> Load[Load Snapshot from Chain]
    Load --> Replay[Replay Pure Prefix<br/>(Deterministic)]
    Replay --> RTFS: Continue from Pos
    RTFS --> Yield2[Next Yield or Complete]
```

### 3. Integration with RTFS 2.0 Reentrancy
- **Purity Enables**: Env is immutable Value—easy serialize/deserialize.
- **Continuation**: RTFS runtime supports resume via env injection + pc (program counter).
- **Idempotency**: Pending yields use keys to avoid dupes on replay.

**Reentrant Example** (Interrupted Plan):
- Exec to yield :report.save → Checkpoint env {:summary computed}.
- Pause 1hr → Resume: Replay pure map (fast), re-issue save (idempotent), complete.

### 4. Governance and Limits
Kernel validates checkpoints (e.g., size limits). Explicit resumes require approval.

Checkpoint-Resume makes CCOS resilient: Pure pauses, seamless continues for real-world agents.

Next: MicroVM Architecture in 020.