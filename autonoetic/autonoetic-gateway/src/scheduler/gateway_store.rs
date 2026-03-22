use anyhow::Result;
use autonoetic_types::background::ApprovalRequest;
use autonoetic_types::notification::{NotificationRecord, NotificationStatus, NotificationType};
use autonoetic_types::workflow::WorkflowEventRecord;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
struct WorkflowIndexFile {
    workflow_id: String,
    root_session_id: String,
}

pub struct GatewayStore {
    conn: std::sync::Mutex<Connection>,
}

impl GatewayStore {
    pub fn open(gateway_dir: &Path) -> Result<Self> {
        std::fs::create_dir_all(gateway_dir).map_err(|e| {
            anyhow::anyhow!(
                "Failed to create gateway directory {:?}: {}",
                gateway_dir,
                e
            )
        })?;
        let db_path = gateway_dir.join("gateway.db");
        let conn = Connection::open(&db_path)?;

        // Optimizations
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA busy_timeout=5000;")?;

        let store = Self {
            conn: std::sync::Mutex::new(conn),
        };
        store.migrate()?;
        store.backfill_workflow_index(gateway_dir)?;
        Ok(store)
    }

    fn backfill_workflow_index(&self, gateway_dir: &Path) -> Result<()> {
        let index_dir = gateway_dir
            .join("scheduler")
            .join("workflows")
            .join("index")
            .join("by_root");
        if !index_dir.exists() {
            return Ok(());
        }

        let conn = self.conn.lock().unwrap();
        let count: i64 =
            conn.query_row("SELECT COUNT(*) FROM workflow_index", [], |row| row.get(0))?;

        if count > 0 {
            return Ok(()); // Already backfilled
        }

        tracing::info!(target: "gateway_store", "Backfilling workflow_index from file-based index");

        for entry in std::fs::read_dir(&index_dir)? {
            let entry = entry?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str::<WorkflowIndexFile>(&content) {
                    Ok(idx) => {
                        let now = chrono::Utc::now().to_rfc3339();
                        if let Err(e) = conn.execute(
                                "INSERT OR IGNORE INTO workflow_index (root_session_id, workflow_id, created_at) VALUES (?1, ?2, ?3)",
                                rusqlite::params![idx.root_session_id, idx.workflow_id, now],
                            ) {
                                tracing::warn!(
                                    target: "gateway_store",
                                    path = %path.display(),
                                    error = %e,
                                    "Failed to backfill workflow index entry"
                                );
                            }
                    }
                    Err(e) => {
                        tracing::warn!(
                            target: "gateway_store",
                            path = %path.display(),
                            error = %e,
                            "Failed to parse workflow index file"
                        );
                    }
                },
                Err(e) => {
                    tracing::warn!(
                        target: "gateway_store",
                        path = %path.display(),
                        error = %e,
                        "Failed to read workflow index file"
                    );
                }
            }
        }

        Ok(())
    }

    fn migrate(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS approvals (
                request_id TEXT PRIMARY KEY,
                agent_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                root_session_id TEXT,
                workflow_id TEXT,
                task_id TEXT,
                action_type TEXT NOT NULL,
                action_payload TEXT NOT NULL,
                reason TEXT,
                evidence_ref TEXT,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                decided_at TEXT,
                decided_by TEXT
            );

            CREATE TABLE IF NOT EXISTS notifications (
                notification_id TEXT PRIMARY KEY,
                notification_type TEXT NOT NULL,
                request_id TEXT,
                target_session_id TEXT NOT NULL,
                target_agent_id TEXT,
                workflow_id TEXT,
                task_id TEXT,
                payload TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                created_at TEXT NOT NULL,
                action_completed_at TEXT,
                delivered_at TEXT,
                consumed_at TEXT,
                attempt_count INTEGER NOT NULL DEFAULT 0,
                last_attempt_at TEXT,
                error_message TEXT
            );

            CREATE TABLE IF NOT EXISTS workflow_events (
                event_id TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                event_type TEXT NOT NULL,
                task_id TEXT,
                agent_id TEXT,
                payload TEXT,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_approvals_status ON approvals(status);
            CREATE INDEX IF NOT EXISTS idx_approvals_session ON approvals(session_id);
            CREATE INDEX IF NOT EXISTS idx_approvals_root_session ON approvals(root_session_id);
            CREATE INDEX IF NOT EXISTS idx_approvals_workflow ON approvals(workflow_id);
            CREATE INDEX IF NOT EXISTS idx_notifications_status ON notifications(status);
            CREATE INDEX IF NOT EXISTS idx_notifications_target ON notifications(target_session_id);
            CREATE INDEX IF NOT EXISTS idx_workflow_events_workflow ON workflow_events(workflow_id);
            CREATE INDEX IF NOT EXISTS idx_workflow_events_created ON workflow_events(created_at);

            CREATE TABLE IF NOT EXISTS workflow_index (
                root_session_id TEXT PRIMARY KEY,
                workflow_id TEXT NOT NULL,
                created_at TEXT NOT NULL
            );
            ",
        )?;
        Ok(())
    }

    // --- Approvals ---

    pub fn create_approval(&self, request: &ApprovalRequest) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let action_payload = serde_json::to_string(&request.action)?;
        conn.execute(
            "INSERT INTO approvals (
                request_id, agent_id, session_id, root_session_id, workflow_id, task_id,
                action_type, action_payload, reason, evidence_ref, status, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            params![
                request.request_id,
                request.agent_id,
                request.session_id,
                request.root_session_id,
                request.workflow_id,
                request.task_id,
                request.action.kind(),
                action_payload,
                request.reason,
                request.evidence_ref,
                "pending",
                request.created_at
            ],
        )?;
        Ok(())
    }

    fn get_approval_with_conn(
        conn: &Connection,
        request_id: &str,
    ) -> Result<Option<ApprovalRequest>> {
        conn.query_row(
            "SELECT request_id, agent_id, session_id, action_payload, created_at, workflow_id, task_id, root_session_id, status, decided_at, decided_by, reason, evidence_ref FROM approvals WHERE request_id = ?1",
            params![request_id],
            |row| {
                let action_payload: String = row.get(3)?;
                let status_str: Option<String> = row.get(8)?;
                let status = status_str.and_then(|s| match s.as_str() {
                    "approved" => Some(autonoetic_types::background::ApprovalStatus::Approved),
                    "rejected" => Some(autonoetic_types::background::ApprovalStatus::Rejected),
                    _ => None,
                });
                let action = serde_json::from_str(&action_payload).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, Box::new(e))
                })?;
                Ok(ApprovalRequest {
                    request_id: row.get(0)?,
                    agent_id: row.get(1)?,
                    session_id: row.get(2)?,
                    action,
                    created_at: row.get(4)?,
                    workflow_id: row.get(5)?,
                    task_id: row.get(6)?,
                    root_session_id: row.get(7)?,
                    status,
                    decided_at: row.get(9)?,
                    decided_by: row.get(10)?,
                    reason: row.get(11)?,
                    evidence_ref: row.get(12)?,
                })
            },
        ).optional().map_err(Into::into)
    }

    pub fn get_approval(&self, request_id: &str) -> Result<Option<ApprovalRequest>> {
        let conn = self.conn.lock().unwrap();
        Self::get_approval_with_conn(&conn, request_id)
    }

    pub fn record_decision(
        &self,
        request_id: &str,
        status: &str,
        decided_by: &str,
        decided_at: &str,
    ) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE approvals SET status = ?1, decided_by = ?2, decided_at = ?3 WHERE request_id = ?4",
            params![status, decided_by, decided_at, request_id],
        )?;
        Ok(())
    }

    pub fn get_pending_approvals(&self) -> Result<Vec<ApprovalRequest>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT request_id FROM approvals WHERE status = 'pending'")?;
        let rows = stmt.query_map([], |row| {
            let id: String = row.get(0)?;
            Ok(id)
        })?;

        let mut results = Vec::new();
        for id_result in rows {
            if let Ok(id) = id_result {
                if let Ok(Some(app)) = Self::get_approval_with_conn(&conn, &id) {
                    results.push(app);
                }
            }
        }
        Ok(results)
    }

    pub fn get_pending_approvals_for_root(
        &self,
        root_session_id: &str,
    ) -> Result<Vec<ApprovalRequest>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT request_id FROM approvals WHERE root_session_id = ?1 AND status = 'pending'",
        )?;
        let rows = stmt.query_map(params![root_session_id], |row| {
            let id: String = row.get(0)?;
            Ok(id)
        })?;

        let mut results = Vec::new();
        for id_result in rows {
            if let Ok(id) = id_result {
                if let Ok(Some(app)) = Self::get_approval_with_conn(&conn, &id) {
                    results.push(app);
                }
            }
        }
        Ok(results)
    }

    // --- Notifications ---

    pub fn create_notification_record(&self, n: &NotificationRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let payload = serde_json::to_string(&n.payload)?;
        conn.execute(
            "INSERT INTO notifications (
                notification_id, notification_type, request_id, target_session_id, target_agent_id,
                workflow_id, task_id, payload, status, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                n.notification_id,
                serde_json::to_string(&n.notification_type)?,
                n.request_id,
                n.target_session_id,
                n.target_agent_id,
                n.workflow_id,
                n.task_id,
                payload,
                serde_json::to_string(&n.status)?,
                n.created_at
            ],
        )?;
        Ok(())
    }

    pub fn create_notification(&self, session_id: &str, payload: &serde_json::Value) -> Result<()> {
        let n = NotificationRecord::new(
            format!("ntf-{}", &uuid::Uuid::new_v4().to_string()[..8]),
            NotificationType::ApprovalResolved,
            session_id.to_string(),
            payload.clone(),
        );
        self.create_notification_record(&n)
    }

    pub fn list_pending_notifications(&self) -> Result<Vec<NotificationRecord>> {
        let conn = self.conn.lock().unwrap();
        let status = serde_json::to_string(&NotificationStatus::Pending)?;
        let mut stmt = conn.prepare(
            "SELECT notification_id FROM notifications WHERE status = ?1 ORDER BY created_at ASC, notification_id ASC",
        )?;
        let rows = stmt.query_map(params![status], |row| {
            let id: String = row.get(0)?;
            Ok(id)
        })?;

        let mut results = Vec::new();
        for id_result in rows {
            if let Ok(id) = id_result {
                if let Ok(Some(n)) = Self::get_notification_with_conn(&conn, &id) {
                    results.push(n);
                }
            }
        }
        Ok(results)
    }

    fn get_notification_with_conn(
        conn: &Connection,
        id: &str,
    ) -> Result<Option<NotificationRecord>> {
        conn.query_row(
            "SELECT * FROM notifications WHERE notification_id = ?1",
            params![id],
            |row| {
                let n_type_str: String = row.get(1)?;
                let status_str: String = row.get(8)?;
                let payload_str: String = row.get(7)?;

                let notification_type = serde_json::from_str(&n_type_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        1,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let payload = serde_json::from_str(&payload_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;
                let status = serde_json::from_str(&status_str).map_err(|e| {
                    rusqlite::Error::FromSqlConversionFailure(
                        8,
                        rusqlite::types::Type::Text,
                        Box::new(e),
                    )
                })?;

                Ok(NotificationRecord {
                    notification_id: row.get(0)?,
                    notification_type,
                    request_id: row.get(2)?,
                    target_session_id: row.get(3)?,
                    target_agent_id: row.get(4)?,
                    workflow_id: row.get(5)?,
                    task_id: row.get(6)?,
                    payload,
                    status,
                    created_at: row.get(9)?,
                    action_completed_at: row.get(10)?,
                    delivered_at: row.get(11)?,
                    consumed_at: row.get(12)?,
                    attempt_count: row.get(13)?,
                    last_attempt_at: row.get(14)?,
                    error_message: row.get(15)?,
                })
            },
        )
        .optional()
        .map_err(Into::into)
    }

    pub fn get_notification(&self, id: &str) -> Result<Option<NotificationRecord>> {
        let conn = self.conn.lock().unwrap();
        Self::get_notification_with_conn(&conn, id)
    }

    pub fn update_notification_status(&self, id: &str, status: NotificationStatus) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let status_str = serde_json::to_string(&status)?;
        let now = chrono::Utc::now().to_rfc3339();

        match status {
            NotificationStatus::ActionExecuted => {
                conn.execute(
                    "UPDATE notifications SET status = ?1, action_completed_at = ?2 WHERE notification_id = ?3",
                    params![status_str, now, id],
                )?;
            }
            NotificationStatus::Delivered => {
                conn.execute(
                    "UPDATE notifications SET status = ?1, delivered_at = ?2 WHERE notification_id = ?3",
                    params![status_str, now, id],
                )?;
            }
            NotificationStatus::Consumed => {
                conn.execute(
                    "UPDATE notifications SET status = ?1, consumed_at = ?2 WHERE notification_id = ?3",
                    params![status_str, now, id],
                )?;
            }
            _ => {
                conn.execute(
                    "UPDATE notifications SET status = ?1 WHERE notification_id = ?2",
                    params![status_str, id],
                )?;
            }
        }
        Ok(())
    }

    pub fn mark_consumed(&self, id: &str) -> Result<()> {
        self.update_notification_status(id, NotificationStatus::Consumed)
    }

    pub fn list_notifications_for_session(
        &self,
        session_id: &str,
        status: NotificationStatus,
    ) -> Result<Vec<NotificationRecord>> {
        let conn = self.conn.lock().unwrap();
        let status_str = serde_json::to_string(&status)?;
        let mut stmt = conn.prepare("SELECT notification_id FROM notifications WHERE target_session_id = ?1 AND status = ?2")?;
        let rows = stmt.query_map(params![session_id, status_str], |row| {
            let id: String = row.get(0)?;
            Ok(id)
        })?;

        let mut results = Vec::new();
        for id_result in rows {
            if let Ok(id) = id_result {
                if let Ok(Some(n)) = Self::get_notification_with_conn(&conn, &id) {
                    results.push(n);
                }
            }
        }
        Ok(results)
    }

    pub fn increment_attempt(&self, id: &str, error: Option<&str>) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = chrono::Utc::now().to_rfc3339();
        conn.execute(
            "UPDATE notifications SET attempt_count = attempt_count + 1, last_attempt_at = ?1, error_message = ?2 WHERE notification_id = ?3",
            params![now, error, id],
        )?;
        Ok(())
    }

    // --- Workflow events ---

    pub fn append_workflow_event(&self, event: &WorkflowEventRecord) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let payload = serde_json::to_string(&event.payload)?;
        conn.execute(
            "INSERT INTO workflow_events (
                event_id, workflow_id, event_type, task_id, agent_id, payload, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                event.event_id,
                event.workflow_id,
                event.event_type,
                event.task_id,
                event.agent_id,
                payload,
                event.occurred_at
            ],
        )?;
        Ok(())
    }

    pub fn list_workflow_events(&self, workflow_id: &str) -> Result<Vec<WorkflowEventRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT * FROM workflow_events WHERE workflow_id = ?1 ORDER BY created_at ASC",
        )?;
        let rows = stmt.query_map(params![workflow_id], |row| {
            let payload_str: String = row.get(5)?;
            let payload = serde_json::from_str(&payload_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(WorkflowEventRecord {
                event_id: row.get(0)?,
                workflow_id: row.get(1)?,
                event_type: row.get(2)?,
                task_id: row.get(3)?,
                agent_id: row.get(4)?,
                payload,
                occurred_at: row.get(6)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn list_workflow_events_since(
        &self,
        workflow_id: &str,
        since: &str,
    ) -> Result<Vec<WorkflowEventRecord>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT * FROM workflow_events WHERE workflow_id = ?1 AND created_at > ?2 ORDER BY created_at ASC")?;
        let rows = stmt.query_map(params![workflow_id, since], |row| {
            let payload_str: String = row.get(5)?;
            let payload = serde_json::from_str(&payload_str).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    5,
                    rusqlite::types::Type::Text,
                    Box::new(e),
                )
            })?;
            Ok(WorkflowEventRecord {
                event_id: row.get(0)?,
                workflow_id: row.get(1)?,
                event_type: row.get(2)?,
                task_id: row.get(3)?,
                agent_id: row.get(4)?,
                payload,
                occurred_at: row.get(6)?,
            })
        })?;

        let mut results = Vec::new();
        for r in rows {
            results.push(r?);
        }
        Ok(results)
    }

    pub fn cleanup_stale_notifications(&self, max_age_hours: u64) -> Result<u64> {
        let conn = self.conn.lock().unwrap();
        let cutoff =
            (chrono::Utc::now() - chrono::Duration::hours(max_age_hours as i64)).to_rfc3339();
        let rows = conn.execute(
            "DELETE FROM notifications WHERE consumed_at < ?1 OR (status = ?2 AND created_at < ?3)",
            params![
                cutoff,
                serde_json::to_string(&NotificationStatus::Failed)?,
                cutoff
            ],
        )?;
        Ok(rows as u64)
    }

    // --- Workflow Index ---

    pub fn set_workflow_index(&self, root_session_id: &str, workflow_id: &str) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT OR REPLACE INTO workflow_index (root_session_id, workflow_id, created_at) VALUES (?1, ?2, ?3)",
            params![root_session_id, workflow_id, chrono::Utc::now().to_rfc3339()],
        )?;
        Ok(())
    }

    pub fn resolve_workflow_id(&self, root_session_id: &str) -> Result<Option<String>> {
        let conn = self.conn.lock().unwrap();
        let result: Option<String> = conn
            .query_row(
                "SELECT workflow_id FROM workflow_index WHERE root_session_id = ?1",
                params![root_session_id],
                |row| row.get(0),
            )
            .optional()?;
        Ok(result)
    }
}
