# Full Plan Generation Strategy

## Structure Your Plan

1. **Choose a Name**: Short, descriptive name for the plan
2. **Specify Language**: Use `rtfs20` (or omit, will default)
3. **Build the Body**: Construct the `(do ...)` execution body
4. **Add Annotations**: Optional metadata (simple keyword/string pairs)

## Plan Naming

- Use descriptive names that indicate the plan's purpose
- Keep it concise (2-5 words)
- Examples: "user_greeting_flow", "data_processing_pipeline", "interactive_survey"

## Body Construction

Same as reduced mode:
- Break down into logical steps
- Name steps descriptively
- Handle variable scoping correctly (let bindings are step-local)
- Use appropriate control flow (if/match)

## Annotations

Use annotations for:
- Priority levels
- Tags or categories
- Owner or source information
- Version information

Keep values as simple strings.

## What NOT to Include

Do NOT include kernel-managed fields:
- plan_id (auto-generated)
- intent_ids (set by orchestrator)
- status (managed by runtime)
- policies (governance-controlled)
- capabilities_required (inferred from body)
- timestamps (auto-generated)
- metadata (kernel-managed)
- schemas (inferred from body)

These will be ignored or overwritten by the orchestrator.
