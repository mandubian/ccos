# CCOS/RTFS Self-Learning Capability Demonstration

## Overview

This document demonstrates how CCOS/RTFS exhibits **self-learning** capabilities by synthesizing reusable capabilities from user interactions driven by the LLM-powered Arbiter.

## What is Self-Learning?

In CCOS/RTFS, self-learning means:
1. **Observe**: System engages in multi-turn conversation with user to understand their goal
2. **Analyze**: System extracts patterns, parameters, and workflows from the interaction
3. **Synthesize**: System generates RTFS capability definitions that encapsulate the learned workflow
4. **Register**: New capabilities are added to the marketplace for future reuse
5. **Apply**: Subsequent similar requests can leverage synthesized capabilities directly

## The Learning Loop

```
┌─────────────────┐
│  User Request   │ "I want to build an AI code review system"
└────────┬────────┘
         │ 
         ▼
┌─────────────────┐
│ Multi-turn      │  6 turns of refinement:
│ Interaction     │  - What's your team size?
└────────┬────────┘  - Which repository?
         │           - What language?
         │           - What rules to check?
         ▼           - Integration needs?
┌─────────────────┐
│  Synthesis      │  LLM analyzes conversation
│  Analysis       │  → Extracts parameters
└────────┬────────┘  → Generates RTFS capability
         │
         ▼
┌─────────────────┐
│ Capability      │  (capability "ai-powered-code-review-system"
│ Registration    │    :parameters {...}
└────────┬────────┘    :implementation (...))
         │
         ▼
┌─────────────────┐
│  Proof of       │  Next similar request:
│  Learning       │  - Takes 1 turn instead of 6
└─────────────────┘  - Direct capability invocation
```

## Running the Demo

### Basic Synthesis (Option A - Enhanced Visualization)

```bash
# Using the convenient demo script (recommended)
./demo_self_learning.sh basic

# Or manually with cargo:
cd rtfs_compiler
cargo run --example user_interaction_progressive_graph -- \
  --config ../config/agent_config.toml \
  --enable-delegation \
  --synthesize-capability
```

### Full Learning Loop Demo

```bash
# Complete demonstration with proof-of-learning
./demo_self_learning.sh full

# Or manually with cargo:
cd rtfs_compiler
cargo run --example user_interaction_progressive_graph -- \
  --config ../config/agent_config.toml \
  --enable-delegation \
  --synthesize-capability \
  --demo-learning-loop \
  --persist-synthesized
```

### Configuration

The demo uses `config/agent_config.toml` which includes several LLM profiles:
- **openai-fast** (default): GPT-4o-mini - fast and cost-effective
- **openai-balanced**: GPT-4o - higher quality for complex tasks
- **claude-fast**: Claude 3.5 Sonnet - excellent reasoning
- **openrouter-free**: Free tier models for testing

To use a different profile, edit the config file or override via CLI:
```bash
# Override specific settings
cargo run --example user_interaction_progressive_graph -- \
  --config ../config/agent_config.toml \
  --llm-model "gpt-4o" \
  --synthesize-capability
```

## What You'll See

### 1. Learning Baseline
```
═══ LEARNING BASELINE ═══
📚 15 capabilities currently registered
Sample capabilities:
  • ccos.echo
  • ccos.user.ask
  • ccos.plan.execute
  • ccos.capability.list
  • ccos.capability.invoke
  ... and 10 more
```

### 2. Interactive Learning Phase
```
--- Running Simulated Interaction ---
User Input: I want to build an AI-powered code review system for my team.

Turn 1/8 | 0 questions asked
[Intent created] ai-powered-code-review-system-for-my-team
[Plan] Asking: What's your team size?
...
```

### 3. Synthesis Analysis
```
--- Capability Synthesis Analysis (LLM) ---
Initial Goal: I want to build an AI-powered code review system for my team.
Total Interaction Turns: 6 turns
Refinements:
  1. Team size specification
  2. Repository selection
  3. Language requirements
  4. Review rules definition
  5. Integration points
...
[synthesis] Running quick local synthesis pipeline (schema extraction + artifact generation)...
[synthesis] Requesting capability proposal from LLM...
[synthesis] Candidate capability id: ai-powered-code-review-system
✓ [synthesis] Registered capability: ai-powered-code-review-system
```

### 4. Learning Outcome
```
═══ LEARNING OUTCOME ═══
✨ 1 NEW capability learned from 6 interaction turns!
📦 Synthesized: ai-powered-code-review-system

📊 Complexity Reduction:
  • Original: 6 turns of back-and-forth
  • Now: Single capability invocation
  • Efficiency gain: 6x

📚 Total capabilities: 15 → 16
```

### 5. Proof of Learning
```
═══ PROOF OF LEARNING ═══
🧪 Testing if CCOS can now use the synthesized capability...

💬 Test request: I need help with a code review system setup (use capability: ai-powered-code-review-system)
🔍 Checking marketplace...
✓ Capability 'ai-powered-code-review-system' is registered and available!

🎓 The system has learned and can now:
  • Recognize requests similar to the original interaction
  • Directly invoke the synthesized capability
  • Avoid repeating the multi-turn refinement process

🎉 Self-learning demonstrated successfully!
```

## Key Features Demonstrated

### 1. **Automated Learning**
- No manual capability definition required
- System learns from natural interaction
- LLM-driven analysis and synthesis

### 2. **Complexity Reduction**
- Multi-turn conversations → Single capability
- 6x efficiency improvement in this example
- Reusable across similar future requests

### 3. **Knowledge Accumulation**
- Capabilities persist in marketplace
- Building library grows over time
- Each interaction can contribute new capabilities

### 4. **Homoiconic Advantage**
- RTFS code = RTFS data
- Capabilities are first-class citizens
- System can reason about and manipulate its own capabilities

## Generated Artifacts

When `--persist-synthesized` is used, capabilities are saved to:

```
generated_capabilities/
├── ai-powered-code-review-system.rtfs
├── synth.collector.rtfs
├── synth.planner.rtfs
└── synth.stub.rtfs
```

Example synthesized capability:

```clojure
(capability "ai-powered-code-review-system"
  :description "Orchestrates setup of AI-powered code review for development teams"
  :parameters {
    :team_size "string"
    :repository "string"
    :language "string"
    :review_rules "string"
    :integration "string"
  }
  :implementation (do
    (validate.team_size :size team_size)
    (configure.repository :repo repository :lang language)
    (setup.review_rules :rules review_rules)
    (integrate.ci_cd :integration integration)
    (deploy.review_bot)
  )
)
```

## Architecture Highlights

### Synthesis Pipeline

1. **Interaction Capture**: `InteractionTurn` records each conversation step
2. **Schema Extraction**: Parameters discovered from user responses  
3. **Artifact Generation**: RTFS code generation from patterns
4. **Validation**: Parser ensures well-formed RTFS before registration
5. **Registration**: Capability added to marketplace with handler
6. **Persistence**: Optional disk storage for replay/audit

### Components Involved

- **Arbiter (LLM-powered)**: Drives conversation and synthesis
- **Intent Graph**: Tracks refinement relationships
- **Causal Chain**: Records all actions for analysis
- **Capability Marketplace**: Central registry
- **RTFS Parser/Compiler**: Validates synthesized code

## Future Enhancements (Option B & C)

### Option B: Full Learning Loop
- Automatic second interaction using synthesized capability
- Side-by-side comparison metrics (before vs after)
- Demonstration of efficiency gains in action

### Option C: Interactive Learning Session
- User prompt after synthesis: "Test the new capability?"
- Live demonstration of learned capability
- Immediate feedback loop

## Conclusion

CCOS/RTFS demonstrates genuine self-learning through:
- **Observation**: Multi-turn LLM-driven dialogue
- **Analysis**: Pattern and parameter extraction
- **Synthesis**: Automated RTFS code generation
- **Integration**: Seamless marketplace registration
- **Reuse**: Immediate availability for future requests

This creates a virtuous cycle where each interaction potentially enriches the system's capability library, making it progressively more capable over time.
