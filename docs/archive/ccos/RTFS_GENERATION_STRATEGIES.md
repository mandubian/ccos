# RTFS Plan Generation & Validation Strategies

_A living design note consolidating options for making large-language models reliably produce **valid RTFS code** through CCOS._

---

## 1. Prompt Engineering with a Mini-Spec

1. Embed a condensed excerpt of the Pest grammar (or an EBNF summary) in the system prompt.
2. Supply 1-2 short **few-shot** RTFS examples (`(do …)` forms).
3. Add a strict rule: *"Return plain RTFS only (no Markdown, no Python). Your output must parse with the grammar above."*

*Benefit:* Zero-code change, immediate effect on GPT-class models.

## 2. Parser Round-Trip Guard

```rust
let rtfs_code = model_output;
if parser::parse(&rtfs_code).is_err() {
    // Retry once with the parse error fed back to the model
}
```

• Automatic second attempt when syntax is invalid.  
• After N retries, escalate to fallback (see §3).

## 3. High-Level JSON → RTFS Template Builder (Fallback)

1. Ask the model for a *structured* JSON plan (list of steps `{fn, args}`).
2. Deterministically convert JSON into RTFS `(do …)` code on the Rust side (using the grammar for guarantee).

*Benefit:* LLM never needs perfect syntax; still yields executable RTFS.

## 4. Grammar-Aware Unit Tests

Add tests that:
* Call `DelegatingArbiter::intent_to_plan()` with fixed prompts.
* Assert `parser::parse()` succeeds.

CI fails if the prompt/grammar drift.

## 5. Fine-Tuning Local Models

* LoRA/QLoRA on 500-1000 RTFS snippets.
* Reinforcement via rejection sampling: keep generations that parse.
* Even a light tune (1-2 GPU hours) dramatically improves syntax compliance for Phi-2/Mistral.

## 6. Logging & Telemetry

* Log the full NL → Intent → Plan → Execution pipeline (`[Arbiter]` log lines already added).
* Record parse errors & retry counts to measure prompt quality over time.

---

### Next Steps

1. Implement §1 (mini-spec prompt) inside `DelegatingArbiter::intent_to_plan`.
2. Add the parser guard (§2).
3. Spike a JSON→RTFS template builder (§3).
4. Write unit tests (§4).
5. Plan a small LoRA dataset (§5).

_This document should evolve as we experiment.  Feel free to append findings or new ideas._ 