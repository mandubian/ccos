# Context Variables in RTFS Plans

## Overview

Context variables allow RTFS plans to reference data from previous plan executions within the same **Intent Graph**. This enables modular planning where each step can build upon the results of previous actions without re-collecting information from the user.

## Syntax

Context variables are referenced using the `<context/path>` syntax within strings or expressions.

```clojure
;; Referencing a destination collected in a previous plan
(call "ccos.echo" (str "Planning your trip to " <trip/destination>))
```

## How It Works

1.  **Output Extraction**: When a plan completes, CCOS extracts any map returned by the final step and stores it in the **Intent Context**.
2.  **Context Injection**: When generating a new plan for a related intent, the Arbiter injects the known context (e.g., `trip/destination`, `user/name`) into the prompt.
3.  **Variable Resolution**: The LLM can then use the `<...>` syntax to placeholders for these values in the generated RTFS code.

## Example Flow

### 1. Initial Plan (Knowledge Collection)
The first plan collects the core trip details.

```clojure
;; Plan 1
(step "Collect Core Info"
  (let [dest (call "ccos.user.ask" "Where are you going?")
        days (call "ccos.user.ask" "For how long?")]
    {:trip/destination dest :trip/duration days}))
```

### 2. Subsequent Plan (Context Reuse)
A later plan uses the previously collected data.

```clojure
;; Plan 2
(step "Specific Planning"
  (let [activities (call "ccos.user.ask" (str "What do you want to do in " <trip/destination> "?"))]
    (call "ccos.echo" (str "Great! I will add " activities " to your " <trip/duration> " trip."))
    {:itinerary/activities activities}))
```

## Best Practices

*   **Final Step Return**: Always return a map with descriptive keyword keys (e.g., `:user/id`) from your final plan step to ensure data is available for future plans.
*   **Don't Assume**: Only use context variables that you know were collected in parental or sibling intents.
*   **Mix with New Data**: You can freely mix context variables with new `let` bindings and host calls.

## Implementation Detail
In the CCOS implementation, the `accumulated_context` is tracked by the orchestrator and passed to the `DelegatingArbiter` during the planning phase.
