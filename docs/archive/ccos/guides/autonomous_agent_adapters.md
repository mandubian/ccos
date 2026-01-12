# Autonomous Agent: Generic Mocking & Adapter Synthesis

This guide explains the architecture of the generic autonomous agent implemented in `ccos/examples/autonomous_agent_demo.rs`. This agent demonstrates how to build a system that can plan, execute, and adapt to arbitrary goals without pre-programmed domain knowledge.

## 1. Core Philosophy

The goal of this implementation is to move away from hardcoded "demo logic" (e.g., `if goal.contains("github") { mock_github() }`) towards a fully generalized approach where the agent:
1.  **Discovers** tools based on semantic relevance.
2.  **Mocks** missing tools on the fly using LLM generation (for testing/demo purposes).
3.  **Synthesizes** adapter code (RTFS) to bridge data format mismatches between steps.

## 2. Generic Mocking System

Instead of manually writing mock implementations for every possible tool, the agent uses the LLM to generate sample data.

### Workflow:
1.  **Discovery**: The agent searches for a tool (e.g., `weather.get_current`).
2.  **Prompting**: If the tool is not found locally, the agent prompts the LLM:
    > "Generate a sample JSON return value for a tool named 'weather.get_current' which has the description: 'Fetch current weather'..."
3.  **Registration**: The generated JSON is parsed and converted into an RTFS value. A temporary capability is registered in the `CapabilityMarketplace` that returns this static data.

This allows the agent to "hallucinate" a working environment for any domain (Weather, GitHub, AWS, etc.) to verify its planning logic.

## 3. Adapter Synthesis (Phase C)

A common problem in autonomous agents is that the output of Step A often doesn't perfectly match the input requirements of Step B.

*   **Step A (`weather.get_current`)** returns: `{"temp": 20, "cond": "rain"}`
*   **Step B (`data.filter`)** expects: `{"data": {...}, "condition": "rain"}`

### The Solution: RTFS Adapters
The `IterativePlanner` now includes a `synthesize_adapter` step. Before executing a tool, it checks if the input comes from a previous step. If so, it asks the LLM to write a small RTFS snippet to transform the data.

**Prompt:**
> "We need to pass data from variable `step_1` to tool `data.filter`. Write an RTFS expression..."

**Generated RTFS:**
```clojure
(call "data.filter" {:data step_1})
```

This enables the agent to handle complex data flows, such as:
*   Wrapping data in maps (`{:data step_1}`).
*   Extracting fields (`(:items step_1)`).
*   Renaming keys.

## 4. Example Usage

Run the demo with any goal:

```bash
cargo run --example autonomous_agent_demo -- --goal "Find the weather in Paris and filter for rain"
```

Or a completely different domain:

```bash
cargo run --example autonomous_agent_demo -- --goal "Get the latest release of the linux kernel and count the number of characters in the version name"
```

The agent will:
1.  Plan the steps.
2.  Mock `github.get_latest_release` and `text.count_characters`.
3.  Synthesize adapters to pass the version string to the counter.
4.  Execute the full chain.
