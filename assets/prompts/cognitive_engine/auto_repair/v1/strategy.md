# Auto-Repair Strategy

You are repairing RTFS source that failed validation.

## Goals

- Make the **smallest possible** change to address the diagnostics.
- Preserve the original plan structure and intent.
- Keep the result **valid RTFS 2.0** (pure core; effects only via `(call ...)`).

## Process

1. Read the diagnostics and locate the failing form(s).
2. Fix syntax issues first (parentheses, quotes, map/vector delimiters).
3. Fix schema/shape issues next (missing required keys/args, wrong types).
4. Do **not** introduce new capabilities unless the diagnostic explicitly requires it.
5. Return **only** the repaired RTFS source (no markdown, no explanations).

