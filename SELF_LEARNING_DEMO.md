# CCOS/RTFS Self-Learning Capability Demonstration

## Overview

This document demonstrates how CCOS/RTFS exhibits **self-learning** capabilities by synthesizing reusable capabilities from user interactions driven by the LLM-powered Arbiter.

> **🚀 Quick Start:** Want to run the demo immediately? See [QUICK_START.md](QUICK_START.md) for 3-step setup!

> **New!** We now have an enhanced demo using a **Smart Research Assistant** use case that showcases the learning loop more effectively. The demo is fully **dynamic** - it uses real CCOS execution and real LLM synthesis (no hardcoded simulations). See [Smart Research Assistant Demo](#smart-research-assistant-demo) below.

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
│  User Request   │ "I need to research quantum computing applications"
└────────┬────────┘
         │ 
         ▼
┌─────────────────┐
│ Multi-turn      │  5 turns of clarification:
│ Interaction     │  - What domains?
└────────┬────────┘  - How deep?
         │           - What format?
         │           - Which sources?
         ▼           - Time constraints?
┌─────────────────┐
│  Synthesis      │  LLM analyzes conversation
│  Analysis       │  → Extracts parameters
└────────┬────────┘  → Generates RTFS capability
         │
         ▼
┌─────────────────┐
│ Capability      │  (capability "research.smart-assistant.v1"
│ Registration    │    :parameters {...}
└────────┬────────┘    :implementation (...))
         │
         ▼
┌─────────────────┐
│  Proof of       │  Next similar request:
│  Learning       │  - Takes 1 turn instead of 6
└─────────────────┘  - Direct capability invocation
```

## Smart Research Assistant Demo

### The Use Case

The **Smart Research Assistant** learns how you prefer to conduct research by observing your first interaction, then applies that learned workflow to future research tasks.

**First Interaction (Learning):**
- User: "I need to research quantum computing applications in cryptography"
- System asks 5 clarifying questions about:
  - Preferred domains (academic, industry, etc.)
  - Analysis depth (overview vs comprehensive)
  - Output format (summary, detailed report, etc.)
  - Trusted sources (arxiv, IEEE, ACM, etc.)
  - Time constraints
- System synthesizes a `research.smart-assistant.v1` capability

**Second Interaction (Application):**
- User: "I need to research blockchain scalability solutions"
- System: **Directly applies learned workflow** (no repeated questions!)
- Result: Instant research execution based on learned preferences

### Running the Demo

```bash
# Quick start - Full learning loop
./demo_smart_assistant.sh full

# Just learn phase
./demo_smart_assistant.sh learn

# Apply learned capability
./demo_smart_assistant.sh apply

# Custom research topic
./demo_smart_assistant.sh --topic "neural architecture search" full

# With specific LLM profile
./demo_smart_assistant.sh --profile claude-fast full

# Debug mode
./demo_smart_assistant.sh --debug full
```

### What You'll See

#### Phase 1: Initial Learning

```
┌─────────────────────────────────────────────────────────────┐
│ PHASE 1: Initial Learning - Understanding Your Workflow    │
└─────────────────────────────────────────────────────────────┘

User Request: quantum computing applications in cryptography

💬 Interactive Preference Collection:

  Q1: What domains should I focus on?
  A1: academic papers, industry reports, expert blogs

  Q2: How deep should the analysis be?
  A2: comprehensive with examples and case studies

  Q3: What format do you prefer?
  A3: structured summary with key findings and citations

  Q4: Which sources do you trust?
  A4: peer-reviewed journals, arxiv, IEEE, ACM

  Q5: Any time constraints?
  A5: complete within 24 hours

📊 Learned Preferences:
   • Topic: quantum computing applications in cryptography
   • Domains: academic, industry, expert-analysis
   • Depth: comprehensive
   • Format: structured-summary
   • Sources: arxiv, ieee, acm
   • Time: 24h
```

#### Phase 2: Capability Synthesis

```
┌─────────────────────────────────────────────────────────────┐
│ PHASE 2: Capability Synthesis                              │
└─────────────────────────────────────────────────────────────┘

🔬 Analyzing interaction patterns...
✓ Extracted parameter schema
✓ Identified workflow pattern
✓ Generated RTFS capability

📦 Synthesized Capability:
```rtfs
(capability "research.smart-assistant.v1"
  :description "Smart research assistant that gathers, analyzes, and synthesizes information"
  :parameters {
    :topic "string"
    :domains (list "string")
    :depth "string"
    :format "string"
    :sources (list "string")
    :time_constraint "string"
  }
  :implementation
    (do
      (step "Gather Sources"
        (call :research.sources.gather {...}))
      (step "Analyze Content"
        (call :research.content.analyze {...}))
      (step "Synthesize Findings"
        (call :research.synthesis.create {...}))
      (step "Format Report"
        (call :research.report.format {...}))
      (step "Return Results"
        {:status "completed" :summary formatted_report})))
```

✓ Registered capability in marketplace
✓ Persisted to capabilities/generated/research.smart-assistant.v1.rtfs
```

#### Phase 3: Applying Learned Capability

```
┌─────────────────────────────────────────────────────────────┐
│ PHASE 3: Applying Learned Capability                       │
└─────────────────────────────────────────────────────────────┘

User Request: blockchain scalability solutions

🔍 Checking capability marketplace...
✓ Found learned capability: research.smart-assistant.v1

⚡ Executing research workflow...
  → Gathering sources...
  → Analyzing content...
  → Synthesizing findings...
  → Formatting report...

✓ Research completed using learned workflow!
```

#### Phase 4: Impact Analysis

```
═══════════════════════════════════════════════════════════════
                    LEARNING IMPACT ANALYSIS
═══════════════════════════════════════════════════════════════

┌─────────────────────┬───────────────┬───────────────┬──────────┐
│ Metric              │ Before Learn  │ After Learn   │ Gain     │
├─────────────────────┼───────────────┼───────────────┼──────────┤
│ Interaction Turns   │             6 │             1 │      6x  │
│ Questions Asked     │             5 │             0 │      -5  │
│ Time Elapsed        │       2847ms  │       1456ms  │    -48%  │
└─────────────────────┴───────────────┴───────────────┴──────────┘

🎯 Key Achievements:
   ✓ Reduced interaction from 6 turns to 1 turn
   ✓ Eliminated 5 redundant questions
   ✓ Capability reusable for similar tasks
   ✓ Knowledge persisted in marketplace

💡 What This Means:
   The system learned your research workflow and can now apply it
   instantly to new topics without repeating the same questions.
   This represents genuine learning and knowledge accumulation.
```

## Architecture Highlights

### Synthesis Pipeline

1. **Interaction Capture**: Multi-turn conversation records user preferences
2. **Pattern Analysis**: System identifies workflow patterns from interaction
3. **Schema Extraction**: Parameters discovered from user responses  
4. **Code Generation**: RTFS capability definition synthesized
5. **Validation**: Parser ensures well-formed RTFS
6. **Registration**: Capability added to marketplace with handler
7. **Persistence**: Disk storage for replay/audit

### Components Involved

- **Arbiter (LLM-powered)**: Drives conversation and synthesis
- **Intent Graph**: Tracks refinement relationships
- **Causal Chain**: Records all actions for analysis
- **Capability Marketplace**: Central registry
- **RTFS Parser/Compiler**: Validates synthesized code

## Key Features Demonstrated

### 1. **Automated Learning**
- No manual capability definition required
- System learns from natural interaction
- LLM-driven analysis and synthesis

### 2. **Complexity Reduction**
- Multi-turn conversations → Single capability invocation
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

## Alternative Examples

### Progressive Intent Graph (Original)

The original demonstration uses an AI-powered code review system:

```bash
# Basic synthesis
cd rtfs_compiler
cargo run --example user_interaction_progressive_graph -- \
  --config ../config/agent_config.toml \
  --enable-delegation \
  --synthesize-capability

# Full learning loop
cargo run --example user_interaction_progressive_graph -- \
  --config ../config/agent_config.toml \
  --enable-delegation \
  --synthesize-capability \
  --demo-learning-loop \
  --persist-synthesized
```

### Synthetic Agent Builder

Two-turn conversation → capability discovery → executor synthesis:

```bash
cd rtfs_compiler
cargo run --example user_interaction_synthetic_agent -- \
  --config ../config/agent_config.toml
```

## Configuration

All demos use `config/agent_config.toml` which includes several LLM profiles:

- **openai-fast** (default): GPT-4o-mini - fast and cost-effective
- **openai-balanced**: GPT-4o - higher quality for complex tasks
- **claude-fast**: Claude 3.5 Sonnet - excellent reasoning
- **openrouter-free**: Free tier models for testing

To use a different profile:

```bash
./demo_smart_assistant.sh --profile claude-fast full
```

Or set environment variables:

```bash
export CCOS_LLM_PROFILE=openrouter-free
./demo_smart_assistant.sh full
```

## Generated Artifacts

When capabilities are persisted, they're saved to:

```
capabilities/generated/
├── research.smart-assistant.v1.rtfs
├── ai-powered-code-review-system.rtfs
├── synth.collector.rtfs
├── synth.planner.rtfs
└── synth.stub.rtfs
```

These RTFS files can be:
- Imported into other RTFS programs
- Modified and enhanced manually
- Versioned and shared across teams
- Analyzed for pattern extraction

## Advanced Usage

### Custom Research Topics

```bash
# Machine Learning
RESEARCH_TOPIC="transformer architectures for NLP" \
  ./demo_smart_assistant.sh full

# Distributed Systems
RESEARCH_TOPIC="consensus algorithms in distributed databases" \
  ./demo_smart_assistant.sh full

# Security
RESEARCH_TOPIC="zero-knowledge proofs in blockchain" \
  ./demo_smart_assistant.sh full
```

### Multiple Iterations

```bash
# First topic
./demo_smart_assistant.sh --topic "quantum computing" learn

# Apply to second topic (uses learned preferences)
SECOND_RESEARCH_TOPIC="edge computing architectures" \
  ./demo_smart_assistant.sh apply

# Try a third topic
SECOND_RESEARCH_TOPIC="federated learning privacy" \
  ./demo_smart_assistant.sh apply
```

### Integration with Other Systems

The synthesized capabilities can be invoked programmatically:

```clojure
;; Import the learned capability
(import :research.smart-assistant.v1)

;; Use it in your workflow
(call :research.smart-assistant.v1 {
  :topic "neural architecture search"
  :domains ["academic" "industry"]
  :depth "comprehensive"
  :format "structured-summary"
  :sources ["arxiv" "papers-with-code"]
  :time_constraint "48h"
})
```

## Troubleshooting

### API Keys

Ensure your LLM API keys are set:

```bash
# OpenAI
export OPENAI_API_KEY="sk-..."

# Anthropic/Claude
export ANTHROPIC_API_KEY="sk-ant-..."

# OpenRouter
export OPENROUTER_API_KEY="sk-or-..."
```

### Debug Mode

Use `--debug` to see detailed prompts and responses:

```bash
./demo_smart_assistant.sh --debug full
```

### Configuration Issues

Validate your config:

```bash
cat config/agent_config.toml
```

Ensure it has at least one valid LLM profile configured.

## Conclusion

CCOS/RTFS demonstrates genuine self-learning through:

- **Observation**: Multi-turn LLM-driven dialogue
- **Analysis**: Pattern and parameter extraction
- **Synthesis**: Automated RTFS code generation
- **Integration**: Seamless marketplace registration
- **Reuse**: Immediate availability for future requests

This creates a virtuous cycle where each interaction potentially enriches the system's capability library, making it progressively more capable over time.

## Next Steps

1. **Run the demo**: `./demo_smart_assistant.sh full`
2. **Examine generated code**: `cat capabilities/generated/research.smart-assistant.v1.rtfs`
3. **Try custom topics**: Experiment with different research areas
4. **Build upon it**: Enhance the generated capability or create new ones
5. **Integrate**: Import learned capabilities into your RTFS programs

---

For implementation details:
- Smart Assistant: `rtfs_compiler/examples/user_interaction_smart_assistant.rs`
- Progressive Graph: `rtfs_compiler/examples/user_interaction_progressive_graph.rs`
- Synthetic Agent: `rtfs_compiler/examples/user_interaction_synthetic_agent.rs`
