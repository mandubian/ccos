# Prompt Assets Layout (CCOS Arbiter)

## Single Source of Truth
All arbiter prompt assets live under the repository root:
```
assets/prompts/arbiter/
```
Subdirectories group functional prompt families (intent, RTFS intent, plan generation variants, delegation analysis). Each family contains versioned directories (e.g. `v1/`) with modular sections:
```
(task.md, grammar.md, strategy.md, few_shots.md, anti_patterns.md)
```
The `PromptManager` assembles these sections in a deterministic order.

## Removed Nested Copy
A previous duplicate at `rtfs_compiler/assets/` was removed to avoid divergence. Runtime resolution logic in `DelegatingArbiter::new` prefers `../assets/prompts/arbiter` when executed from the `rtfs_compiler` crate directory; otherwise it falls back to `assets/prompts/arbiter`.

## Adding / Updating Prompts
1. Create or copy a version directory: `assets/prompts/arbiter/<family>/v<N+1>/`.
2. Update sections; keep them small and focused.
3. Reference the new ID+version via configuration or code (planned future config hook).
4. Add tests if the change alters model instructions materially.

## Naming Conventions
- Intent (JSON): `intent_generation`
- Intent (RTFS): `intent_generation_rtfs`
- Plan variants: `plan_generation_full`, `plan_generation_reduced`, `plan_generation_simple`, unified `plan_generation`
- Delegation: `delegation_analysis`

## Future Enhancements
- Config-driven selection of prompt family & version.
- Hash logging for prompt bundle integrity.
- Optional caching layer to reduce filesystem reads under high throughput.

## Rationale
Centralization simplifies maintenance, ensures consistent evolution, and eliminates subtle drift bugs introduced by editing only one copy of a duplicated asset tree.
