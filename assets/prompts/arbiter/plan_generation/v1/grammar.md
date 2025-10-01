# RTFS Plan Grammar (Subset)
```
(do
    (step "name" (call :ccos.echo {:message "hi"}))
    (edge :IsSubgoalOf "child" "parent")
)
```
Rules:
- Exactly one top-level `(do ...)`.
- Use `edge` relations only from allowed list: :IsSubgoalOf :DependsOn :ConflictsWith :Enables :RelatedTo :TriggeredBy :Blocks
- Keep plan minimal; only necessary steps.
