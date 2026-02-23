# Causal Chain Persistence Architecture (SQLite)

## Objective
The `ImmutableLedger` requires an append-heavy, sequentially consistent storage mechanism that avoids the operational overhead of a standalone database like Redis or PostgreSQL. We need to be able to reliably recover the causal history of a **session** on restart while keeping inserts fast and memory proportional to live work, not to total audit history.

Using an embedded SQLite database (`rusqlite`) achieves this effectively without adding new dependencies (the project already includes `rusqlite`).

## High-Level Architecture

### Session-scoped in-memory working set

The causal chain DB accumulates the audit trail of every session that has ever run.  Loading the entire history into memory on every startup is wasteful and slow.  Instead:

1. **`CausalChain::load_from_db(path)`** opens the database and applies schema migrations, but loads **nothing** into memory.
2. **`CausalChain::load_session(session_id)`** is called by the Gateway when an agent reconnects or a run is resumed.  It reads only the rows for that session into the in-memory `Vec<Action>` and rebuilds the `LedgerIndices`.
3. **`CausalChain::unload_session(session_id)`** evicts a finished session from memory (the rows remain in SQLite).
4. **`CausalChain::append_action`** always persists to SQLite first, then updates the in-memory working set.

This keeps RAM usage proportional to the number of *active* sessions, not to total DB size.

### Global hash chain (audit, not memory)
Every row in the DB stores a `chain_hash` computed over all previously written rows in insertion order.  This gives a tamper-evident ledger for audit and governance tools.  The in-memory hash chain covers only the loaded actions; full global verification is done by scanning the DB directly when needed.

## Handling Large Payloads (Code & Stdout)
Actions keep only summaries inline.  Full generated code and stdout can be saved to `~/.ccos/runs/<run_id>/artifacts/` with only the file path referenced in the `metadata` JSON.  This keeps individual rows small and makes artifacts directly mountable into sandboxes.

## Database Schema

```sql
CREATE TABLE IF NOT EXISTS causal_chain (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    action_id        TEXT    NOT NULL,
    action_type      TEXT    NOT NULL,
    plan_id          TEXT    NOT NULL,
    intent_id        TEXT    NOT NULL,
    session_id       TEXT,               -- first-class column; indexed for fast per-session queries
    parent_action_id TEXT,
    function_name    TEXT,
    timestamp        INTEGER NOT NULL,
    data             TEXT    NOT NULL,   -- JSON: arguments, result, cost, duration_ms, metadata
    chain_hash       TEXT    NOT NULL    -- rolling cryptographic hash for tamper-evidence
);

CREATE INDEX IF NOT EXISTS idx_session_id  ON causal_chain(session_id);
CREATE INDEX IF NOT EXISTS idx_intent_id   ON causal_chain(intent_id);
CREATE INDEX IF NOT EXISTS idx_plan_id     ON causal_chain(plan_id);
CREATE INDEX IF NOT EXISTS idx_timestamp   ON causal_chain(timestamp);
CREATE INDEX IF NOT EXISTS idx_action_id   ON causal_chain(action_id);
```

`session_id` is a **dedicated column** (not buried in the `data` JSON blob) so that `WHERE session_id = ?` can use the index.  A migration (`ALTER TABLE ADD COLUMN session_id TEXT`) is applied automatically for databases written by earlier code.

## Implementation

### `ccos/src/causal_chain/ledger.rs`
| Method | What it does |
|---|---|
| `ImmutableLedger::new()` | Pure in-memory ledger (no DB) |
| `ImmutableLedger::open_db(path)` | Open/create DB, apply migrations, **nothing loaded** |
| `ImmutableLedger::load_from_db(path)` | Alias for `open_db` (backward compat) |
| `ImmutableLedger::load_session(session_id)` | Load one session into working set |
| `ImmutableLedger::unload_session(session_id)` | Evict session from memory |
| `ImmutableLedger::append_action(action)` | Persist to DB + update working set |

### `ccos/src/causal_chain/mod.rs`
`CausalChain` exposes `load_session` / `unload_session` as thin wrappers over the ledger.

### `ccos/src/chat/gateway.rs`
The Gateway holds `Arc<Mutex<CausalChain>>`.  The recommended integration point:

```rust
// On startup
let chain = Arc::new(Mutex::new(CausalChain::load_from_db(&db_path)?));

// When an agent reconnects with an existing session_id
chain.lock().unwrap().load_session(&session_id)?;

// When a session finishes (optional – frees RAM)
chain.lock().unwrap().unload_session(&session_id);
```

### Configuration
`ChatGatewayConfig::causal_chain_db_path: Option<PathBuf>` — defaults to `None` (in-memory).
CLI / env: `--causal-chain-db` / `CCOS_CAUSAL_CHAIN_DB`.
`StorageConfig::db_path: Option<String>` in `agent_config.toml` (under `[causal_chain.storage]`).

## Gateway load_session integration (TODO)
The Gateway currently calls `CausalChain::load_from_db` on startup (open only).  The next step is to hook `load_session` into the session-resume path:
- When a `RunStore` restores a paused/interrupted run, call `chain.load_session(session_id)`.
- When the gateway receives a reconnect for an existing session, call `chain.load_session(session_id)`.
- On session cleanup / idle eviction, call `chain.unload_session(session_id)`.
