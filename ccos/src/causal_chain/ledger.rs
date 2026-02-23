use super::super::types::{Action, ActionId, ActionType, CapabilityId, ExecutionResult, IntentId, PlanId};
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use rtfs::runtime::error::RuntimeError;
use rtfs::runtime::values::Value;
use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Mutex;

// ---------------------------------------------------------------------------
// SQLite helpers
// ---------------------------------------------------------------------------

/// Newtype wrapping `Connection` in a `Mutex` so that `ImmutableLedger` (and
/// therefore `CausalChain`) is both `Send` **and** `Sync`.
///
/// `rusqlite::Connection` is `Send` but not `Sync`; wrapping it in `Mutex`
/// gives us `Sync` because `Mutex<T>: Sync` whenever `T: Send`.
struct DbConn(Mutex<Connection>);

impl std::fmt::Debug for DbConn {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "DbConn(<sqlite>)")
    }
}

/// JSON payload stored in the `data` column – all Action fields not promoted
/// to dedicated SQL columns (arguments, result, cost, duration_ms, metadata).
///
/// `session_id` has been promoted to its own column; it is intentionally
/// absent here.  For rows written by an older version of the code (where
/// `session_id` lived inside `data`), the field is ignored on deserialise
/// because serde skips unknown fields by default.
#[derive(Serialize, Deserialize)]
struct ActionData {
    #[serde(skip_serializing_if = "Option::is_none")]
    arguments: Option<Vec<serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_success: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_value: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    result_metadata: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cost: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_ms: Option<u64>,
    metadata: HashMap<String, serde_json::Value>,
}

fn action_type_to_str(at: &ActionType) -> &'static str {
    match at {
        ActionType::PlanStarted => "PlanStarted",
        ActionType::PlanCompleted => "PlanCompleted",
        ActionType::PlanAborted => "PlanAborted",
        ActionType::PlanPaused => "PlanPaused",
        ActionType::PlanResumed => "PlanResumed",
        ActionType::PlanStepStarted => "PlanStepStarted",
        ActionType::PlanStepCompleted => "PlanStepCompleted",
        ActionType::PlanStepFailed => "PlanStepFailed",
        ActionType::PlanStepRetrying => "PlanStepRetrying",
        ActionType::CapabilityCall => "CapabilityCall",
        ActionType::CapabilityResult => "CapabilityResult",
        ActionType::CatalogReuse => "CatalogReuse",
        ActionType::InternalStep => "InternalStep",
        ActionType::StepProfileDerived => "StepProfileDerived",
        ActionType::IntentCreated => "IntentCreated",
        ActionType::IntentStatusChanged => "IntentStatusChanged",
        ActionType::IntentRelationshipCreated => "IntentRelationshipCreated",
        ActionType::IntentRelationshipModified => "IntentRelationshipModified",
        ActionType::IntentArchived => "IntentArchived",
        ActionType::IntentReactivated => "IntentReactivated",
        ActionType::CapabilityRegistered => "CapabilityRegistered",
        ActionType::CapabilityRemoved => "CapabilityRemoved",
        ActionType::CapabilityUpdated => "CapabilityUpdated",
        ActionType::CapabilityDiscoveryCompleted => "CapabilityDiscoveryCompleted",
        ActionType::CapabilityVersionCreated => "CapabilityVersionCreated",
        ActionType::CapabilityRollback => "CapabilityRollback",
        ActionType::CapabilitySynthesisStarted => "CapabilitySynthesisStarted",
        ActionType::CapabilitySynthesisCompleted => "CapabilitySynthesisCompleted",
        ActionType::GovernanceApprovalRequested => "GovernanceApprovalRequested",
        ActionType::GovernanceApprovalGranted => "GovernanceApprovalGranted",
        ActionType::GovernanceApprovalDenied => "GovernanceApprovalDenied",
        ActionType::BoundedExplorationLimitReached => "BoundedExplorationLimitReached",
        ActionType::GovernanceCheckpointDecision => "GovernanceCheckpointDecision",
        ActionType::GovernanceCheckpointOutcome => "GovernanceCheckpointOutcome",
        ActionType::HintApplied => "HintApplied",
        ActionType::BudgetAllocated => "BudgetAllocated",
        ActionType::BudgetConsumptionRecorded => "BudgetConsumptionRecorded",
        ActionType::BudgetWarningIssued => "BudgetWarningIssued",
        ActionType::BudgetExhausted => "BudgetExhausted",
        ActionType::BudgetExtended => "BudgetExtended",
        ActionType::AgentLlmConsultation => "AgentLlmConsultation",
    }
}

fn str_to_action_type(s: &str) -> ActionType {
    match s {
        "PlanStarted" => ActionType::PlanStarted,
        "PlanCompleted" => ActionType::PlanCompleted,
        "PlanAborted" => ActionType::PlanAborted,
        "PlanPaused" => ActionType::PlanPaused,
        "PlanResumed" => ActionType::PlanResumed,
        "PlanStepStarted" => ActionType::PlanStepStarted,
        "PlanStepCompleted" => ActionType::PlanStepCompleted,
        "PlanStepFailed" => ActionType::PlanStepFailed,
        "PlanStepRetrying" => ActionType::PlanStepRetrying,
        "CapabilityCall" => ActionType::CapabilityCall,
        "CapabilityResult" => ActionType::CapabilityResult,
        "CatalogReuse" => ActionType::CatalogReuse,
        "InternalStep" => ActionType::InternalStep,
        "StepProfileDerived" => ActionType::StepProfileDerived,
        "IntentCreated" => ActionType::IntentCreated,
        "IntentStatusChanged" => ActionType::IntentStatusChanged,
        "IntentRelationshipCreated" => ActionType::IntentRelationshipCreated,
        "IntentRelationshipModified" => ActionType::IntentRelationshipModified,
        "IntentArchived" => ActionType::IntentArchived,
        "IntentReactivated" => ActionType::IntentReactivated,
        "CapabilityRegistered" => ActionType::CapabilityRegistered,
        "CapabilityRemoved" => ActionType::CapabilityRemoved,
        "CapabilityUpdated" => ActionType::CapabilityUpdated,
        "CapabilityDiscoveryCompleted" => ActionType::CapabilityDiscoveryCompleted,
        "CapabilityVersionCreated" => ActionType::CapabilityVersionCreated,
        "CapabilityRollback" => ActionType::CapabilityRollback,
        "CapabilitySynthesisStarted" => ActionType::CapabilitySynthesisStarted,
        "CapabilitySynthesisCompleted" => ActionType::CapabilitySynthesisCompleted,
        "GovernanceApprovalRequested" => ActionType::GovernanceApprovalRequested,
        "GovernanceApprovalGranted" => ActionType::GovernanceApprovalGranted,
        "GovernanceApprovalDenied" => ActionType::GovernanceApprovalDenied,
        "BoundedExplorationLimitReached" => ActionType::BoundedExplorationLimitReached,
        "GovernanceCheckpointDecision" => ActionType::GovernanceCheckpointDecision,
        "GovernanceCheckpointOutcome" => ActionType::GovernanceCheckpointOutcome,
        "HintApplied" => ActionType::HintApplied,
        "BudgetAllocated" => ActionType::BudgetAllocated,
        "BudgetConsumptionRecorded" => ActionType::BudgetConsumptionRecorded,
        "BudgetWarningIssued" => ActionType::BudgetWarningIssued,
        "BudgetExhausted" => ActionType::BudgetExhausted,
        "BudgetExtended" => ActionType::BudgetExtended,
        "AgentLlmConsultation" => ActionType::AgentLlmConsultation,
        _ => ActionType::InternalStep,
    }
}

/// The tuple type returned by the row-loading helper.
/// Columns: action_id, action_type, plan_id, intent_id, session_id,
///          parent_action_id, function_name, timestamp, data, chain_hash
type DbRow = (
    String,         // action_id
    String,         // action_type
    Option<String>, // plan_id    ← NULL for infra/system actions
    Option<String>, // intent_id  ← NULL for infra/system actions
    Option<String>, // session_id ← dedicated column
    Option<String>, // parent_action_id
    Option<String>, // function_name
    i64,            // timestamp
    String,         // data
    String,         // chain_hash
);

/// Load rows for a specific `session_id` in insertion order.
///
/// Extracted into a free function so `Statement` lifetimes stay local.
/// Uses `and_then` to avoid a `?` on the `MappedRows` iterator (which would
/// create a borrow-checker-visible temporary that holds a reference to `stmt`).
fn load_session_rows(conn: &Connection, session_id: &str) -> Result<Vec<DbRow>, RuntimeError> {
    let mut stmt = conn
        .prepare(
            "SELECT action_id, action_type, plan_id, intent_id, session_id, \
             parent_action_id, function_name, timestamp, data, chain_hash \
             FROM causal_chain WHERE session_id = ?1 ORDER BY id ASC",
        )
        .map_err(|e| RuntimeError::Generic(format!("Failed to prepare SELECT: {}", e)))?;

    stmt.query_map([session_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, Option<String>>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, Option<String>>(4)?,
            row.get::<_, Option<String>>(5)?,
            row.get::<_, Option<String>>(6)?,
            row.get::<_, i64>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, String>(9)?,
        ))
    })
    .and_then(|mapped| mapped.collect::<Result<Vec<_>, _>>())
    .map_err(|e| RuntimeError::Generic(format!("Failed to load session rows: {}", e)))
}

fn action_to_data_json(action: &Action) -> Result<String, RuntimeError> {
    let arguments = action
        .arguments
        .as_ref()
        .map(|args| {
            args.iter()
                .map(rtfs_value_to_json)
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let (result_success, result_value, result_metadata) = match &action.result {
        Some(r) => {
            let json_val = rtfs_value_to_json(&r.value)?;
            let meta: HashMap<String, serde_json::Value> = r
                .metadata
                .iter()
                .map(|(k, v)| rtfs_value_to_json(v).map(|jv| (k.clone(), jv)))
                .collect::<Result<HashMap<_, _>, _>>()?;
            (Some(r.success), Some(json_val), Some(meta))
        }
        None => (None, None, None),
    };

    let metadata: HashMap<String, serde_json::Value> = action
        .metadata
        .iter()
        .map(|(k, v)| rtfs_value_to_json(v).map(|jv| (k.clone(), jv)))
        .collect::<Result<HashMap<_, _>, _>>()?;

    let data = ActionData {
        arguments,
        result_success,
        result_value,
        result_metadata,
        cost: action.cost,
        duration_ms: action.duration_ms,
        metadata,
    };

    serde_json::to_string(&data)
        .map_err(|e| RuntimeError::Generic(format!("Failed to serialize action data: {}", e)))
}

#[allow(clippy::too_many_arguments)]
fn action_from_row(
    action_id: String,
    action_type_str: String,
    plan_id: Option<String>,
    intent_id: Option<String>,
    session_id: Option<String>,
    parent_action_id: Option<String>,
    function_name: Option<String>,
    timestamp: i64,
    data_json: String,
) -> Result<Action, RuntimeError> {
    let data: ActionData = serde_json::from_str(&data_json).map_err(|e| {
        RuntimeError::Generic(format!("Failed to deserialize action data: {}", e))
    })?;

    let arguments = data
        .arguments
        .map(|args| {
            args.into_iter()
                .map(|v| json_to_rtfs_value(&v))
                .collect::<Result<Vec<_>, _>>()
        })
        .transpose()?;

    let result = match data.result_success {
        Some(success) => {
            let value = match data.result_value.as_ref() {
                Some(v) => json_to_rtfs_value(v)?,
                None => Value::Nil,
            };
            let result_metadata: HashMap<String, Value> = data
                .result_metadata
                .unwrap_or_default()
                .into_iter()
                .map(|(k, v)| json_to_rtfs_value(&v).map(|rv| (k, rv)))
                .collect::<Result<HashMap<_, _>, _>>()?;
            Some(ExecutionResult {
                success,
                value,
                metadata: result_metadata,
            })
        }
        None => None,
    };

    let metadata: HashMap<String, Value> = data
        .metadata
        .into_iter()
        .map(|(k, v)| json_to_rtfs_value(&v).map(|rv| (k, rv)))
        .collect::<Result<HashMap<_, _>, _>>()?;

    Ok(Action {
        action_id,
        parent_action_id,
        session_id,
        plan_id,
        intent_id,
        action_type: str_to_action_type(&action_type_str),
        function_name,
        arguments,
        result,
        cost: data.cost,
        duration_ms: data.duration_ms,
        timestamp: timestamp as u64,
        metadata,
    })
}

/// DDL for the `causal_chain` table and its indices.
///
/// `session_id` is a first-class column so that per-session queries
/// (`WHERE session_id = ?`) can use the index rather than scanning the
/// `data` JSON blob.
const CREATE_SCHEMA_SQL: &str = "
CREATE TABLE IF NOT EXISTS causal_chain (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    action_id        TEXT    NOT NULL,
    action_type      TEXT    NOT NULL,
    plan_id          TEXT,
    intent_id        TEXT,
    session_id       TEXT,
    parent_action_id TEXT,
    function_name    TEXT,
    timestamp        INTEGER NOT NULL,
    data             TEXT    NOT NULL,
    chain_hash       TEXT    NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_session_id  ON causal_chain(session_id);
CREATE INDEX IF NOT EXISTS idx_intent_id   ON causal_chain(intent_id);
CREATE INDEX IF NOT EXISTS idx_plan_id     ON causal_chain(plan_id);
CREATE INDEX IF NOT EXISTS idx_timestamp   ON causal_chain(timestamp);
CREATE INDEX IF NOT EXISTS idx_action_id   ON causal_chain(action_id);
";

/// Migration: add `session_id` column to databases created before it was
/// promoted from the `data` JSON blob.
const MIGRATE_ADD_SESSION_ID_SQL: &str =
    "ALTER TABLE causal_chain ADD COLUMN session_id TEXT";

// ---------------------------------------------------------------------------
// ImmutableLedger
// ---------------------------------------------------------------------------

/// Immutable ledger storage – optionally backed by a SQLite database.
///
/// ## Session-scoped in-memory working set
///
/// The database is the source of truth and can hold years of audit history.
/// The in-memory `actions` / `hash_chain` / `indices` vectors hold only the
/// **currently active or restored sessions**, which keeps RAM proportional to
/// live work rather than to the total history.
///
/// Typical lifecycle:
/// 1. `ImmutableLedger::open_db(path)` – open / migrate the DB; nothing is
///    loaded into memory.
/// 2. `ledger.load_session(session_id)` – hydrate one session on connect or
///    resume; called by the gateway when an agent reconnects.
/// 3. `ledger.append_action(action)` – write new actions; always persisted to
///    the DB and added to the in-memory working set.
/// 4. `ledger.unload_session(session_id)` – evict a finished session to free
///    RAM (optional; the DB retains the full history).
pub struct ImmutableLedger {
    pub actions: Vec<Action>,
    pub hash_chain: Vec<String>,
    pub indices: LedgerIndices,
    /// Live SQLite connection; `None` for pure in-memory operation.
    conn: Option<DbConn>,
}

impl std::fmt::Debug for ImmutableLedger {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ImmutableLedger")
            .field("actions_len", &self.actions.len())
            .field("hash_chain_len", &self.hash_chain.len())
            .field("conn", &self.conn)
            .finish()
    }
}

impl ImmutableLedger {
    // ------------------------------------------------------------------
    // Constructors
    // ------------------------------------------------------------------

    /// Create a pure in-memory ledger (no persistence).
    pub fn new() -> Self {
        Self {
            actions: Vec::new(),
            hash_chain: Vec::new(),
            indices: LedgerIndices::new(),
            conn: None,
        }
    }

    /// Open (or create) a SQLite-backed ledger at `path`.
    ///
    /// **No rows are loaded into memory** – the in-memory working set starts
    /// empty.  Call [`load_session`] to hydrate individual sessions when
    /// needed (e.g., on agent reconnect / resume).
    ///
    /// Handles schema creation and the `session_id` column migration for DBs
    /// written by earlier versions of the code.
    pub fn open_db(path: &Path) -> Result<Self, RuntimeError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                RuntimeError::Generic(format!("Failed to create causal-chain db dir: {}", e))
            })?;
        }

        let conn = Connection::open(path).map_err(|e| {
            RuntimeError::Generic(format!("Failed to open causal-chain SQLite db: {}", e))
        })?;

        // WAL mode for better concurrent read performance.
        conn.execute_batch("PRAGMA journal_mode=WAL;").ok();

        // Create table + indices (idempotent).
        conn.execute_batch(CREATE_SCHEMA_SQL).map_err(|e| {
            RuntimeError::Generic(format!("Failed to initialise causal_chain schema: {}", e))
        })?;

        // Migrate: add `session_id` column if it doesn't exist yet (older DBs).
        // ALTER TABLE ADD COLUMN fails if the column already exists; we treat
        // that as a no-op by ignoring the error.
        let _ = conn.execute_batch(MIGRATE_ADD_SESSION_ID_SQL);

        log::info!(
            "[CausalChain] Opened DB at {} (empty working set; call load_session to restore sessions)",
            path.display()
        );

        Ok(Self {
            actions: Vec::new(),
            hash_chain: Vec::new(),
            indices: LedgerIndices::new(),
            conn: Some(DbConn(Mutex::new(conn))),
        })
    }

    /// Backward-compat alias: opens the DB without bulk-loading.
    ///
    /// Previously this method loaded every row on startup; it now just opens
    /// the connection and initialises the schema.  Callers that need prior
    /// data should follow up with [`load_session`].
    #[inline]
    pub fn load_from_db(path: &Path) -> Result<Self, RuntimeError> {
        Self::open_db(path)
    }

    // ------------------------------------------------------------------
    // Session lifecycle
    // ------------------------------------------------------------------

    /// Load all persisted actions for `session_id` into the in-memory working
    /// set and rebuild the indices for that session.
    ///
    /// Returns the number of actions loaded.  Calling this for a session that
    /// is already in memory is safe (duplicate entries are skipped).
    pub fn load_session(&mut self, session_id: &str) -> Result<usize, RuntimeError> {
        let db = match self.conn.as_ref() {
            Some(db) => db,
            None => return Ok(0), // in-memory only; nothing to load
        };

        // Collect already-loaded action IDs as owned Strings to avoid
        // holding a shared borrow on `self.actions` while we push below.
        let already_loaded: std::collections::HashSet<String> = self
            .actions
            .iter()
            .filter(|a| a.session_id.as_deref() == Some(session_id))
            .map(|a| a.action_id.clone())
            .collect();

        let rows = {
            let conn = db.0.lock().map_err(|e| {
                RuntimeError::Generic(format!("Failed to acquire SQLite lock: {}", e))
            })?;
            load_session_rows(&conn, session_id)?
        };

        if rows.is_empty() {
            log::debug!("[CausalChain] No persisted actions found for session '{}'", session_id);
            return Ok(0);
        }

        let mut loaded = 0usize;
        for (
            action_id,
            action_type_str,
            plan_id,
            intent_id,
            db_session_id,
            parent_action_id,
            function_name,
            timestamp,
            data_json,
            stored_chain_hash,
        ) in rows
        {
            if already_loaded.contains(&action_id) {
                continue;
            }

            let action = action_from_row(
                action_id,
                action_type_str,
                plan_id,
                intent_id,
                db_session_id,
                parent_action_id,
                function_name,
                timestamp,
                data_json,
            )?;

            self.actions.push(action.clone());
            self.hash_chain.push(stored_chain_hash);
            self.indices.index_action(&action)?;
            loaded += 1;
        }

        log::info!(
            "[CausalChain] Loaded {} actions for session '{}'",
            loaded,
            session_id
        );
        Ok(loaded)
    }

    /// Evict all in-memory actions belonging to `session_id`.
    ///
    /// The actions remain in the SQLite database and can be reloaded later via
    /// [`load_session`].  Use this to free RAM for sessions that have finished
    /// or been idle for a long time.
    ///
    /// Returns the number of actions removed from memory.
    pub fn unload_session(&mut self, session_id: &str) -> usize {
        let before = self.actions.len();

        // Collect action_ids that belong to this session.
        let ids_to_remove: std::collections::HashSet<String> = self
            .actions
            .iter()
            .filter(|a| a.session_id.as_deref() == Some(session_id))
            .map(|a| a.action_id.clone())
            .collect();

        if ids_to_remove.is_empty() {
            return 0;
        }

        // Rebuild the parallel Vecs without the evicted actions.
        let mut new_actions = Vec::with_capacity(before);
        let mut new_hashes = Vec::with_capacity(before);
        for (action, hash) in self.actions.drain(..).zip(self.hash_chain.drain(..)) {
            if ids_to_remove.contains(&action.action_id) {
                // Skip (evict).
            } else {
                new_actions.push(action);
                new_hashes.push(hash);
            }
        }

        self.actions = new_actions;
        self.hash_chain = new_hashes;

        // Rebuild all indices from scratch (simpler than partial removal).
        self.indices = LedgerIndices::new();
        for action in &self.actions {
            // index_action only returns Err for unrecoverable bugs, ignore here.
            let _ = self.indices.index_action(action);
        }

        let removed = before - self.actions.len();
        log::info!(
            "[CausalChain] Evicted {} actions for session '{}' from memory",
            removed,
            session_id
        );
        removed
    }

    // ------------------------------------------------------------------
    // Core mutation
    // ------------------------------------------------------------------

    pub fn append_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        let action_hash = self.calculate_action_hash(action);
        let chain_hash = self.calculate_chain_hash(&action_hash);

        // Persist to SQLite when a connection is open.
        if let Some(ref db) = self.conn {
            let data_json = action_to_data_json(action)?;
            let conn = db.0.lock().map_err(|e| {
                RuntimeError::Generic(format!("Failed to acquire SQLite lock: {}", e))
            })?;
            conn.execute(
                "INSERT INTO causal_chain \
                 (action_id, action_type, plan_id, intent_id, session_id, \
                  parent_action_id, function_name, timestamp, data, chain_hash) \
                 VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
                params![
                    action.action_id,
                    action_type_to_str(&action.action_type),
                    action.plan_id.as_deref(),
                    action.intent_id.as_deref(),
                    action.session_id.as_deref(),
                    action.parent_action_id.as_deref(),
                    action.function_name.as_deref(),
                    action.timestamp as i64,
                    data_json,
                    chain_hash,
                ],
            )
            .map_err(|e| {
                RuntimeError::Generic(format!("Failed to INSERT action into SQLite: {}", e))
            })?;
        }

        self.actions.push(action.clone());
        self.hash_chain.push(chain_hash);
        self.indices.index_action(action)?;

        Ok(())
    }

    pub fn append(&mut self, action: &Action) -> Result<String, RuntimeError> {
        self.append_action(action)?;
        Ok(action.action_id.clone())
    }

    // ------------------------------------------------------------------
    // Queries
    // ------------------------------------------------------------------

    /// Get children actions for a given parent_action_id
    pub fn get_children(&self, parent_id: &ActionId) -> Vec<&Action> {
        self.indices
            .get_children(parent_id)
            .iter()
            .filter_map(|id| self.get_action(id))
            .collect()
    }

    /// Get parent action for a given action_id
    pub fn get_parent(&self, action_id: &ActionId) -> Option<&Action> {
        self.get_action(action_id)
            .and_then(|action| action.parent_action_id.as_ref())
            .and_then(|parent_id| self.get_action(parent_id))
    }

    pub fn get_action(&self, action_id: &ActionId) -> Option<&Action> {
        // If an action is appended multiple times with the same ID (e.g.,
        // initial log then a later result record), prefer the most recent one.
        self.actions
            .iter()
            .rev()
            .find(|a| a.action_id == *action_id)
    }

    pub fn get_actions_by_intent(&self, intent_id: &IntentId) -> Vec<&Action> {
        self.indices
            .intent_actions
            .get(intent_id)
            .map(|ids| ids.iter().filter_map(|id| self.get_action(id)).collect())
            .unwrap_or_default()
    }

    pub fn get_actions_by_plan(&self, plan_id: &PlanId) -> Vec<&Action> {
        self.indices
            .plan_actions
            .get(plan_id)
            .map(|ids| ids.iter().filter_map(|id| self.get_action(id)).collect())
            .unwrap_or_default()
    }

    pub fn get_actions_by_capability(&self, capability_id: &CapabilityId) -> Vec<&Action> {
        self.indices
            .capability_actions
            .get(capability_id)
            .map(|ids| ids.iter().filter_map(|id| self.get_action(id)).collect())
            .unwrap_or_default()
    }

    pub fn get_all_actions(&self) -> &[Action] {
        &self.actions
    }

    // ------------------------------------------------------------------
    // Integrity (over in-memory working set)
    // ------------------------------------------------------------------

    pub fn verify_integrity(&self) -> Result<bool, RuntimeError> {
        let mut last_chain_hash: Option<&String> = None;
        for (i, action) in self.actions.iter().enumerate() {
            let action_hash = self.calculate_action_hash(action);
            let mut hasher = Sha256::new();
            if let Some(prev_hash) = last_chain_hash {
                hasher.update(prev_hash.as_bytes());
            }
            hasher.update(action_hash.as_bytes());
            let expected_chain_hash = format!("{:x}", hasher.finalize());

            if self.hash_chain[i] != expected_chain_hash {
                return Ok(false);
            }
            last_chain_hash = Some(&self.hash_chain[i]);
        }
        Ok(true)
    }

    // ------------------------------------------------------------------
    // Private helpers
    // ------------------------------------------------------------------

    fn calculate_action_hash(&self, action: &Action) -> String {
        let mut hasher = Sha256::new();

        hasher.update(action.action_id.as_bytes());
        if let Some(plan_id) = &action.plan_id {
            hasher.update(plan_id.as_bytes());
        }
        if let Some(intent_id) = &action.intent_id {
            hasher.update(intent_id.as_bytes());
        }
        if let Some(function_name) = &action.function_name {
            hasher.update(function_name.as_bytes());
        }
        hasher.update(action.timestamp.to_string().as_bytes());

        if let Some(args) = &action.arguments {
            for arg in args {
                hasher.update(format!("{:?}", arg).as_bytes());
            }
        }

        if let Some(result) = &action.result {
            hasher.update(format!("{:?}", result).as_bytes());
        }

        for (key, value) in &action.metadata {
            hasher.update(key.as_bytes());
            hasher.update(format!("{:?}", value).as_bytes());
        }

        format!("{:x}", hasher.finalize())
    }

    fn calculate_chain_hash(&self, action_hash: &str) -> String {
        let mut hasher = Sha256::new();
        if let Some(prev_hash) = self.hash_chain.last() {
            hasher.update(prev_hash.as_bytes());
        }
        hasher.update(action_hash.as_bytes());
        format!("{:x}", hasher.finalize())
    }
}

// ---------------------------------------------------------------------------
// LedgerIndices
// ---------------------------------------------------------------------------

/// Indices for fast lookup
#[derive(Debug)]
pub struct LedgerIndices {
    pub intent_actions: HashMap<IntentId, Vec<ActionId>>,
    pub plan_actions: HashMap<PlanId, Vec<ActionId>>,
    pub capability_actions: HashMap<CapabilityId, Vec<ActionId>>,
    pub function_actions: HashMap<String, Vec<ActionId>>,
    pub session_actions: HashMap<String, Vec<ActionId>>,
    pub run_actions: HashMap<String, Vec<ActionId>>,
    pub timestamp_index: Vec<ActionId>, // Chronological order
    pub parent_to_children: HashMap<ActionId, Vec<ActionId>>, // Tree traversal
}

impl LedgerIndices {
    pub fn get_children(&self, parent_id: &ActionId) -> Vec<ActionId> {
        self.parent_to_children
            .get(parent_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn new() -> Self {
        Self {
            intent_actions: HashMap::new(),
            plan_actions: HashMap::new(),
            capability_actions: HashMap::new(),
            function_actions: HashMap::new(),
            session_actions: HashMap::new(),
            run_actions: HashMap::new(),
            timestamp_index: Vec::new(),
            parent_to_children: HashMap::new(),
        }
    }

    pub fn index_action(&mut self, action: &Action) -> Result<(), RuntimeError> {
        if let Some(intent_id) = &action.intent_id {
            self.intent_actions
                .entry(intent_id.clone())
                .or_default()
                .push(action.action_id.clone());
        }

        if let Some(plan_id) = &action.plan_id {
            self.plan_actions
                .entry(plan_id.clone())
                .or_default()
                .push(action.action_id.clone());
        }

        if action.action_type == ActionType::CapabilityCall
            || action.action_type == ActionType::CapabilityResult
        {
            if let Some(function_name) = &action.function_name {
                self.capability_actions
                    .entry(function_name.clone())
                    .or_default()
                    .push(action.action_id.clone());
            }
        }

        if let Some(function_name) = &action.function_name {
            self.function_actions
                .entry(function_name.clone())
                .or_default()
                .push(action.action_id.clone());
        }

        self.timestamp_index.push(action.action_id.clone());

        if let Some(parent_id) = &action.parent_action_id {
            self.parent_to_children
                .entry(parent_id.clone())
                .or_default()
                .push(action.action_id.clone());
        }

        if let Some(session_id) = &action.session_id {
            self.session_actions
                .entry(session_id.clone())
                .or_default()
                .push(action.action_id.clone());
        }

        if let Some(run_id) = action.metadata.get("run_id").and_then(|v| v.as_string()) {
            self.run_actions
                .entry(run_id.to_string())
                .or_default()
                .push(action.action_id.clone());
        }

        Ok(())
    }
}
