# CCOS Plan: Self-Improving Capabilities via Interaction Synthesis

## 1. Vision & Goal

To evolve CCOS from a system that executes predefined tasks into a cognitive framework that learns and improves autonomously.

The primary goal is to **enable CCOS to automatically synthesize new, reusable capabilities by observing, analyzing, and generalizing from its interactions with users.**

This will transform the `CapabilityMarketplace` from a static, developer-defined list into a dynamic, self-populating ecosystem of skills, embodying the project's core cognitive philosophy.

## 2. Core Concepts & Components

This feature revolves around a new component and the intelligent use of existing ones.

*   **Interaction Record (`IntentGraph` & `CausalChain`)**: These existing structures are the foundation. They provide a rich, chronological, and context-aware recording of every interaction—user utterances, agent actions, and goal refinements. This is the raw data for learning.

*   **`ProcessSynthesizer` (New Component)**: This will be a new component within CCOS responsible for "learning." Its job is to:
    1.  Analyze a completed interaction record (`IntentGraph`, `CausalChain`).
    2.  Identify patterns, such as a sequence of questions and answers that lead to a successful outcome.
    3.  Abstract concrete values (e.g., `destination: "Vienna"`) into general parameters (e.g., `destination: (param :destination)`).
    4.  Generate a new, reusable capability definition in RTFS format.

*   **Dynamic `CapabilityMarketplace`**: The marketplace will be extended to allow for the registration of new capabilities at runtime. The `ProcessSynthesizer` will feed its newly created capabilities into the marketplace, making them immediately available for future tasks.

## 3. The Synthesis Workflow

The process of creating a new capability will follow these steps:

1.  **Trigger**: The synthesis process is triggered at the end of a user session or a logical task completion. A new method on the main `CCOS` object, such as `session.conclude_and_learn()`, will initiate this.

2.  **Analysis**: The `ProcessSynthesizer` receives the `IntentGraph` and `CausalChain` for the completed session. It traverses the graphs to identify a "learning pattern," for example:
    *   An initial, underspecified `Intent`.
    *   A sequence of `user.ask` capabilities executed by the agent.
    *   Corresponding user responses that provide the missing information.
    *   A final, specified `Intent` that leads to a successful plan execution.

3.  **Abstraction**: The synthesizer identifies the concrete values provided by the user (e.g., "Vienna") and replaces them with parameter placeholders. It also identifies the agent's questions and uses them to define the prompts for the new capability's parameters.

4.  **Generation**: A new `(capability ...)` definition is generated in RTFS. This new capability encapsulates the conversational logic needed to acquire the necessary parameters before executing the final, concrete steps.

5.  **Registration**: The newly generated capability is registered with the `CapabilityMarketplace`. It can be flagged as "synthesized" or "unverified" initially. Over time, successful re-use of the capability could increase its trust score.

## 4. Implementation Tasks

We will implement this vision in phases, starting with a simulation to validate the logic before building the core components.

*   **Phase 1: Simulation & Validation (Our Immediate Task)**
    *   **Task 1.1**: Modify the `user_interaction_progressive_graph.rs` example to simulate a multi-turn conversation where a goal is progressively refined (e.g., "Plan a trip" -> "To Vienna").
    *   **Task 1.2**: At the end of the simulated conversation, implement "post-mortem analysis" logic directly within the example.
    *   **Task 1.3**: This analysis will print two key outputs to the console:
        1.  A **"Synthesis-Ready Summary"**: A human-readable summary of the interaction, showing the goal refinement steps.
        2.  A **"Potential New Capability"**: The full RTFS definition of the learned capability that could be generated from this interaction.
    *   **Goal**: To prove that our data structures contain all the necessary information for learning and to define the target output for the real synthesizer.

*   **Phase 2: Core Component Implementation**
    *   **Task 2.1**: Create the `ProcessSynthesizer` module and struct (`rtfs_compiler/src/ccos/process_synthesizer.rs`).
    *   **Task 2.2**: Implement a `synthesize_from_session(...)` method. Initially, this method's logic can be ported directly from the validated simulation in Phase 1.
    *   **Task 2.3**: Add the `ProcessSynthesizer` to the main `CCOS` struct.

*   **Phase 3: Integration & Runtime Registration**
    *   **Task 3.1**: Implement the `session.conclude_and_learn()` flow in the CCOS lifecycle.
    *   **Task 3.2**: Modify the `CapabilityMarketplace` to support adding new capabilities at runtime.
    *   **Task 3.3**: Connect the `ProcessSynthesizer` output to the `CapabilityMarketplace` so that newly generated capabilities are registered and become usable in subsequent sessions.

*   **Phase 4: Generalization & Refinement (Long-Term)**
    *   **Task 4.1**: Evolve the `ProcessSynthesizer` from simple pattern matching to a more robust generalization engine.
    *   **Task 4.2**: Introduce a trust/validation system for synthesized capabilities.
    *   **Task 4.3**: Explore mechanisms for CCOS to proactively suggest the use of its newly learned skills.
