//! Progressive Markdown session timeline writer.
//!
//! Writes `.gateway/sessions/{base_session_id}/timeline.md` incrementally as
//! causal chain entries flow in.  The file is created on first write with a
//! Markdown table header; subsequent invocations append rows.
//!
//! Both humans and agents can tail or read the file mid-session to understand
//! what has happened so far.  Errors (DENIED, ERROR) are highlighted in the
//! status column with bold markers so they stand out at a glance.

use autonoetic_types::causal_chain::EntryStatus;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

// ─── public helpers ──────────────────────────────────────────────────────────

/// Returns the root session id — the portion before the first `/`.
///
/// `"demo-session/coder.default-abc"` → `"demo-session"`
pub fn base_session_id(session_id: &str) -> &str {
    session_id.split('/').next().unwrap_or(session_id)
}

// ─── writer ──────────────────────────────────────────────────────────────────

/// Appends one Markdown table row per causal-chain event to
/// `{gateway_dir}/sessions/{base_session_id}/timeline.md`.
pub struct SessionTimelineWriter {
    path: PathBuf,
    row_count: u32,
}

impl SessionTimelineWriter {
    /// Open (or resume) the timeline file for `base_session_id`.
    ///
    /// If the file does not yet exist it is created and a header block is
    /// written.  If it already exists (hibernate/wake cycle) rows are simply
    /// appended and the existing row count is preserved for numbering.
    pub fn open(gateway_dir: &Path, base_session_id: &str) -> anyhow::Result<Self> {
        let dir = gateway_dir.join("sessions").join(base_session_id);
        std::fs::create_dir_all(&dir)?;
        let path = dir.join("timeline.md");

        let row_count = if path.exists() {
            count_data_rows(&path)
        } else {
            let mut f = File::create(&path)?;
            writeln!(f, "# Session Timeline: `{}`", base_session_id)?;
            writeln!(f)?;
            writeln!(
                f,
                "| # | Time (UTC) | Session | Actor | Category | Action | Details | Status |"
            )?;
            writeln!(
                f,
                "|---|------------|---------|-------|----------|--------|---------|--------|"
            )?;
            0
        };

        Ok(Self { path, row_count })
    }

    /// Append one row for the given causal-chain parameters.
    pub fn append(
        &mut self,
        actor_id: &str,
        session_id: &str,
        timestamp: &str,
        category: &str,
        action: &str,
        status: &EntryStatus,
        payload: Option<&serde_json::Value>,
    ) -> anyhow::Result<()> {
        self.row_count += 1;
        let row = format_row(
            self.row_count,
            actor_id,
            session_id,
            timestamp,
            category,
            action,
            status,
            payload,
        );
        let mut f = OpenOptions::new().append(true).open(&self.path)?;
        writeln!(f, "{}", row)?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

// ─── formatting ──────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn format_row(
    n: u32,
    actor_id: &str,
    session_id: &str,
    timestamp: &str,
    category: &str,
    action: &str,
    status: &EntryStatus,
    payload: Option<&serde_json::Value>,
) -> String {
    let time = extract_time(timestamp);
    let session_col = shorten_session(session_id);
    let actor_col = cell(actor_id);
    let cat_col = cell(category);
    let act_col = cell(action);
    let details_col = extract_details(category, action, payload);
    let status_col = format_status(status, category, action, payload);

    format!("| {n} | {time} | {session_col} | {actor_col} | {cat_col} | {act_col} | {details_col} | {status_col} |")
}

/// Take `HH:MM:SS` from an ISO-8601 timestamp string.
fn extract_time(ts: &str) -> String {
    if ts.len() >= 19 {
        ts[11..19].to_string()
    } else {
        ts.to_string()
    }
}

/// Shorten sub-session ids so the table stays readable.
fn shorten_session(session_id: &str) -> String {
    if let Some(slash_pos) = session_id.find('/') {
        let sub = &session_id[slash_pos + 1..];
        let sub_short = if sub.len() > 22 { &sub[..22] } else { sub };
        format!("…/{}", cell(sub_short))
    } else {
        cell(session_id)
    }
}

/// Escape `|` and backtick-escape nothing (plain text in the cell).
fn cell(s: &str) -> String {
    s.replace('|', "\\|")
}

/// Truncate a string and append `…` if it exceeds `max`.
fn truncate(s: &str, max: usize) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= max {
        s.to_string()
    } else {
        let t: String = chars[..max].iter().collect();
        format!("{}…", t)
    }
}

fn str_field<'a>(payload: Option<&'a serde_json::Value>, key: &str) -> &'a str {
    payload
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_str())
        .unwrap_or("")
}

fn u64_field(payload: Option<&serde_json::Value>, key: &str) -> u64 {
    payload
        .and_then(|v| v.get(key))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

fn nested_u64(payload: Option<&serde_json::Value>, outer: &str, inner: &str) -> u64 {
    payload
        .and_then(|v| v.get(outer))
        .and_then(|v| v.get(inner))
        .and_then(|v| v.as_u64())
        .unwrap_or(0)
}

/// Build the human-readable details cell from the event category/action and payload.
fn extract_details(
    category: &str,
    action: &str,
    payload: Option<&serde_json::Value>,
) -> String {
    match (category, action) {
        ("session", "start") => {
            let preview = str_field(payload, "trigger_preview");
            format!("`{}`", cell(&truncate(preview, 110)))
        }
        ("session", "end") => {
            format!("reason: {}", str_field(payload, "reason"))
        }
        ("session", "history.persisted") => {
            let msgs = u64_field(payload, "message_count");
            let handle = str_field(payload, "content_handle");
            format!("msgs: {} \\| handle: `{}`", msgs, &handle[..handle.len().min(14)])
        }
        ("lifecycle", "wake") => {
            let msgs = u64_field(payload, "history_messages");
            format!("history: {} msgs", msgs)
        }
        ("lifecycle", "hibernate") | ("lifecycle", "stopped") => {
            format!("stop: {}", str_field(payload, "stop_reason"))
        }
        ("llm", "completion") => {
            let model = str_field(payload, "model");
            // Keep only the model name after the last `/`
            let model_short = model.split('/').last().unwrap_or(model);
            let stop = str_field(payload, "stop_reason");
            let in_t = nested_u64(payload, "usage", "input_tokens");
            let out_t = nested_u64(payload, "usage", "output_tokens");
            format!(
                "`{}` \\| stop: {} \\| in={} out={} tok",
                cell(model_short),
                stop,
                in_t,
                out_t
            )
        }
        ("tool_invoke", "requested") => {
            let tool = str_field(payload, "tool_name");
            let args = str_field(payload, "arguments");
            let args_short = truncate(args, 90);
            let approval_id = find_approval_id_in_payload(payload);
            if approval_id.is_empty() {
                format!("tool: `{}` args: `{}`", cell(tool), cell(&args_short))
            } else {
                format!(
                    "tool: `{}` args: `{}` \\| approval: `{}`",
                    cell(tool),
                    cell(&args_short),
                    cell(&approval_id)
                )
            }
        }
        ("tool_invoke", "completed") => {
            let tool = str_field(payload, "tool_name");
            let preview = str_field(payload, "result_preview");
            if preview.contains("\"approval_required\":true")
                || preview.contains("approval_required")
            {
                let apr_id = find_approval_id_in_payload(payload);
                if apr_id.is_empty() {
                    format!("tool: `{}` **[APPROVAL NEEDED]**", cell(tool))
                } else {
                    format!("tool: `{}` **[APPROVAL NEEDED: `{}`]**", cell(tool), apr_id)
                }
            } else if !preview.is_empty() {
                format!(
                    "tool: `{}` → `{}`",
                    cell(tool),
                    cell(&truncate(preview, 90))
                )
            } else {
                format!("tool: `{}`", cell(tool))
            }
        }
        ("gateway", "event.ingest.requested") => {
            let evt = str_field(payload, "event_type");
            let len = u64_field(payload, "message_len");
            let target = str_field(payload, "target_agent_id");
            format!("type: {} \\| len={} → `{}`", evt, len, cell(target))
        }
        ("gateway", "event.ingest.completed") => {
            let evt = str_field(payload, "event_type");
            let len = u64_field(payload, "assistant_reply_len");
            format!("type: {} \\| reply_len={}", evt, len)
        }
        ("gateway", action) if action.starts_with("agent.spawn") => {
            let agent = str_field(payload, "agent_id");
            if action.ends_with("requested") {
                let len = u64_field(payload, "message_len");
                format!("→ agent: `{}` \\| msg_len={}", cell(agent), len)
            } else {
                let len = u64_field(payload, "assistant_reply_len");
                format!("← agent: `{}` \\| reply_len={}", cell(agent), len)
            }
        }
        ("gateway", action) if action.contains("approval") => {
            let kind = str_field(payload, "action_kind");
            let req = str_field(payload, "request_id");
            let by = str_field(payload, "decided_by");
            format!("kind: {} \\| approval: `{}` \\| by: {}", kind, req, by)
        }
        _ => {
            // Fallback: dump the first 120 chars of the serialised payload.
            if let Some(p) = payload {
                let raw = serde_json::to_string(p).unwrap_or_default();
                cell(&truncate(&raw, 120))
            } else {
                String::new()
            }
        }
    }
}

/// Scan `s` for the first `apr-XXXXXXXX` token.
fn find_approval_id(s: &str) -> String {
    if let Some(start) = s.find("apr-") {
        let rest = &s[start..];
        let end = rest
            .find(|c: char| !c.is_ascii_alphanumeric() && c != '-')
            .unwrap_or(rest.len());
        return rest[..end].to_string();
    }
    String::new()
}

/// Extract approval id from known payload fields and text blobs.
fn find_approval_id_in_payload(payload: Option<&serde_json::Value>) -> String {
    let Some(p) = payload else {
        return String::new();
    };

    // First try explicit fields.
    for key in ["approval_request_id", "request_id", "approval_ref"] {
        if let Some(id) = p.get(key).and_then(|v| v.as_str()) {
            let found = find_approval_id(id);
            if !found.is_empty() {
                return found;
            }
        }
    }

    // Then scan common text fields that may contain approval instructions.
    for key in ["arguments", "result_preview", "message", "reason"] {
        if let Some(text) = p.get(key).and_then(|v| v.as_str()) {
            let found = find_approval_id(text);
            if !found.is_empty() {
                return found;
            }
        }
    }

    // Last resort: scan the full serialized payload.
    let raw = serde_json::to_string(p).unwrap_or_default();
    find_approval_id(&raw)
}

fn format_status(
    status: &EntryStatus,
    category: &str,
    action: &str,
    payload: Option<&serde_json::Value>,
) -> String {
    match status {
        EntryStatus::Error => "**[ERROR]**".to_string(),
        EntryStatus::Denied => "**[DENIED]**".to_string(),
        EntryStatus::Success => {
            if category == "tool_invoke" && action == "completed" {
                let preview = str_field(payload, "result_preview");
                // Surface approval-required pauses.
                if preview.contains("approval_required") {
                    return "**[WAIT]**".to_string();
                }
                // Surface explicit tool failures: `"ok":false` in the result.
                if preview.contains("\"ok\":false") || preview.contains("\"exit_code\":1") {
                    return "**[FAIL]**".to_string();
                }
            }
            "ok".to_string()
        }
    }
}

// ─── helpers ─────────────────────────────────────────────────────────────────

/// Count rows that already exist in the timeline file so row numbers are
/// continuous after a hibernate/wake cycle.
fn count_data_rows(path: &Path) -> u32 {
    let Ok(content) = std::fs::read_to_string(path) else {
        return 0;
    };
    content
        .lines()
        .filter(|l| {
            l.starts_with("| ")
                && !l.contains("| # |")   // header row
                && !l.contains("|---|")    // separator row
        })
        .count() as u32
}

// ─── tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_base_session_id() {
        assert_eq!(base_session_id("demo-session"), "demo-session");
        assert_eq!(
            base_session_id("demo-session/coder.default-abc"),
            "demo-session"
        );
    }

    #[test]
    fn test_creates_header_on_first_open() {
        let tmp = tempdir().unwrap();
        let gw = tmp.path().join("gateway");
        std::fs::create_dir_all(&gw).unwrap();

        let mut w = SessionTimelineWriter::open(&gw, "my-session").unwrap();
        assert!(w.path().exists());

        w.append(
            "planner.default",
            "my-session",
            "2026-03-18T11:02:08.909354335+00:00",
            "session",
            "start",
            &EntryStatus::Success,
            Some(&serde_json::json!({"trigger_preview": "hello world", "trigger_type": "user_input"})),
        )
        .unwrap();

        let content = std::fs::read_to_string(w.path()).unwrap();
        assert!(content.contains("# Session Timeline"));
        assert!(content.contains("| # |"));
        assert!(content.contains("| 1 |"));
    }

    #[test]
    fn test_row_numbers_continue_after_reopen() {
        let tmp = tempdir().unwrap();
        let gw = tmp.path().join("gateway");
        std::fs::create_dir_all(&gw).unwrap();

        {
            let mut w = SessionTimelineWriter::open(&gw, "s1").unwrap();
            w.append(
                "agent",
                "s1",
                "2026-03-18T11:00:00+00:00",
                "lifecycle",
                "wake",
                &EntryStatus::Success,
                Some(&serde_json::json!({"history_messages": 2})),
            )
            .unwrap();
            w.append(
                "agent",
                "s1",
                "2026-03-18T11:00:01+00:00",
                "lifecycle",
                "hibernate",
                &EntryStatus::Success,
                Some(&serde_json::json!({"stop_reason": "EndTurn"})),
            )
            .unwrap();
        }

        // Reopen — row counter should resume from 2.
        let mut w2 = SessionTimelineWriter::open(&gw, "s1").unwrap();
        assert_eq!(w2.row_count, 2);
        w2.append(
            "agent",
            "s1",
            "2026-03-18T11:01:00+00:00",
            "lifecycle",
            "wake",
            &EntryStatus::Success,
            Some(&serde_json::json!({"history_messages": 4})),
        )
        .unwrap();

        let content = std::fs::read_to_string(w2.path()).unwrap();
        assert!(content.contains("| 3 |"), "third row should be numbered 3");
    }

    #[test]
    fn test_error_status_highlighted() {
        let tmp = tempdir().unwrap();
        let gw = tmp.path().join("gateway");
        std::fs::create_dir_all(&gw).unwrap();
        let mut w = SessionTimelineWriter::open(&gw, "err-session").unwrap();
        w.append(
            "coder.default",
            "err-session",
            "2026-03-18T12:00:00+00:00",
            "lifecycle",
            "stopped",
            &EntryStatus::Error,
            Some(&serde_json::json!({"stop_reason": "timeout"})),
        )
        .unwrap();
        let content = std::fs::read_to_string(w.path()).unwrap();
        assert!(content.contains("**[ERROR]**"));
    }

    #[test]
    fn test_approval_wait_status() {
        let tmp = tempdir().unwrap();
        let gw = tmp.path().join("gateway");
        std::fs::create_dir_all(&gw).unwrap();
        let mut w = SessionTimelineWriter::open(&gw, "apr-session").unwrap();
        w.append(
            "coder.default",
            "apr-session",
            "2026-03-18T12:00:00+00:00",
            "tool_invoke",
            "completed",
            &EntryStatus::Success,
            Some(&serde_json::json!({
                "tool_name": "sandbox.exec",
                "result_preview": "{\"approval_required\":true,\"request_id\":\"apr-15c26ab6\"}"
            })),
        )
        .unwrap();
        let content = std::fs::read_to_string(w.path()).unwrap();
        assert!(content.contains("**[WAIT]**"));
        assert!(content.contains("APPROVAL NEEDED"));
    }

    #[test]
    fn test_requested_row_includes_approval_ref_when_present() {
        let tmp = tempdir().unwrap();
        let gw = tmp.path().join("gateway");
        std::fs::create_dir_all(&gw).unwrap();
        let mut w = SessionTimelineWriter::open(&gw, "apr-ref-session").unwrap();
        w.append(
            "coder.default",
            "apr-ref-session",
            "2026-03-18T12:00:00+00:00",
            "tool_invoke",
            "requested",
            &EntryStatus::Success,
            Some(&serde_json::json!({
                "tool_name": "sandbox.exec",
                "arguments": "{\"approval_ref\":\"apr-42a17c8a\",\"command\":\"python3 /tmp/get_paris_weather.py\"}"
            })),
        )
        .unwrap();
        let content = std::fs::read_to_string(w.path()).unwrap();
        assert!(content.contains("approval: `apr-42a17c8a`"));
    }

    #[test]
    fn test_completed_row_uses_approval_request_id_field() {
        let tmp = tempdir().unwrap();
        let gw = tmp.path().join("gateway");
        std::fs::create_dir_all(&gw).unwrap();
        let mut w = SessionTimelineWriter::open(&gw, "apr-id-field-session").unwrap();
        w.append(
            "coder.default",
            "apr-id-field-session",
            "2026-03-18T12:00:00+00:00",
            "tool_invoke",
            "completed",
            &EntryStatus::Success,
            Some(&serde_json::json!({
                "tool_name": "sandbox.exec",
                "approval_request_id": "apr-1234abcd",
                "result_preview": "{\"approval_required\":true,\"detected_patterns\":[{\"category\":\"import\"}]}"
            })),
        )
        .unwrap();
        let content = std::fs::read_to_string(w.path()).unwrap();
        assert!(content.contains("APPROVAL NEEDED: `apr-1234abcd`"));
    }
}
