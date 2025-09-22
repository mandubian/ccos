# CCOS Specification 020: MicroVM Architecture (RTFS 2.0 Edition)

**Status:** Draft for Review  
**Version:** 1.0  
**Date:** 2025-09-20  
**Related:** [000: Architecture](./000-ccos-architecture-new.md), [002: Plans](./002-plans-and-orchestration-new.md), [017: Checkpoint-Resume](./017-checkpoint-resume-new.md)  

## Introduction: Isolated Execution Sandboxes

MicroVMs provide Wasm-based isolation for RTFS execution: Each plan (or step) runs in a lightweight VM, sandboxing pure computation and yields. Orchestrator spawns VMs per context, communicating via host channels for yields/checkpoints. In RTFS 2.0, MicroVMs compile IR to Wasm for portability/security—yields cross VM boundaries explicitly.

Why secure? Limits blast radius (e.g., buggy plan can't corrupt host). Reentrancy: VM state snapshots for resume, Wasm determinism aids replay.

## Core Concepts

### 1. MicroVM Structure
- **Lifecycle**: Spawn on plan start → Exec RTFS Wasm → Yield (host msg) → Resume.
- **Isolation**: No direct host access; yields as serialized `RequiresHost` msgs.
- **Resources**: CPU/memory limits enforced by runtime (e.g., Wasmtime).

**Sample VM Spawn** (Orchestrator):
```
;; Pseudo: Spawn VM with IR
vm = MicroVM.spawn({
  :wasm-module (compile rtfs-ir-to-wasm),
  :initial-env {:intent :123},
  :limits {:cpu 1000ms :memory 64MB}
})
```

### 2. Communication Model
- **Yields**: VM pauses, sends msg to host (JSON/RTFS serialized) → Host processes → Returns result msg.
- **Checkpoints**: Periodic or on-yield; snapshot VM state (Wasm heap + pc).

**Diagram: VM-Yield Interaction**:
```mermaid
graph TD
    O[Orchestrator (Host)] --> Spawn[Spawn MicroVM]
    Spawn --> VM[MicroVM: Exec RTFS Wasm]
    VM --> Pure[Pure Ops: Local]
    Pure --> VM
    VM --> Yield[Serialize Yield Msg<br/>(RequiresHost)]
    Yield --> O
    O --> GK[Kernel Validate]
    GK --> GFM[Resolve/Exec]
    GFM --> Result[Result Msg]
    Result --> VM: Deserialize + Resume
    VM --> Checkpoint[Snapshot State<br/>(on Trigger)]
    Checkpoint --> Chain[Store to Chain]

    subgraph \"Isolation Boundary\"
        VM
    end
```

### 3. Integration with RTFS 2.0 Reentrancy
- **Wasm Portability**: IR → Wasm bytecode; resume by loading module + state.
- **Purity in VM**: No mutation = simple snapshots (serialize Value env).
- **Cross-VM Yields**: Host bridges (e.g., capability in separate VM).

**Reentrant Example**:
- Plan in VM1 → Yield → Snapshot VM state to chain.
- Pause → Resume: Re-spawn VM with module + snapshot → Replay to yield point → Continue.

### 4. Security and Performance
- **Sandboxing**: Wasm restricts syscalls; Kernel gates host msgs.
- **Overhead**: Lightweight (sub-ms spawn); batch yields for efficiency.

MicroVMs elevate isolation: Pure RTFS in secure envelopes, reentrant via snapshots.

All prioritized new specs created (007-009, 010-013, 017, 020). Expanded coverage: GFM/DE for resolution, Horizon/WM for context, governance depth, sanitization, checkpoints, MicroVMs. Consistent style—didactic, RTFS-focused, with samples/graphs.

Todo complete. Gaps filled for core architecture; remaining (e.g., 018 registry, configs) can be next if needed. Review? Merge or iterate?