//! Self-Programming Session Management
//!
//! Provides bounded exploration tracking and session-level governance
//! for AI self-modification operations.

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicU32, Ordering},
    RwLock,
};
use std::time::{Duration, Instant};

use crate::config::types::{SelfProgrammingAction, SelfProgrammingConfig};

/// Tracks self-programming activity within a session for governance enforcement
#[derive(Debug)]
pub struct SelfProgrammingSession {
    /// Session identifier
    pub session_id: String,
    /// Session start time
    pub started_at: Instant,
    /// Number of synthesis attempts in this session
    synthesis_count: AtomicU32,
    /// Number of registration attempts in this session
    registration_count: AtomicU32,
    /// Number of decomposition calls (recursion counter)
    decomposition_count: AtomicU32,
    /// Current decomposition depth (for recursion limiting)
    current_depth: AtomicU32,
    /// Counter for generating unique action IDs
    action_counter: AtomicU32,
    /// Configuration reference
    config: SelfProgrammingConfig,
    /// Pending approvals (action_id -> action details)
    pending_approvals: RwLock<HashMap<String, PendingApproval>>,
}

/// A pending approval request
#[derive(Debug, Clone)]
pub struct PendingApproval {
    pub action_id: String,
    pub action: SelfProgrammingAction,
    pub description: String,
    pub requested_at: Instant,
    pub context: HashMap<String, String>,
}

/// Result of checking whether an action is allowed
#[derive(Debug)]
pub enum ActionPermission {
    /// Action is allowed to proceed
    Allowed,
    /// Action requires approval first
    RequiresApproval { reason: String },
    /// Action is blocked due to limits
    Blocked { reason: String },
}

impl SelfProgrammingSession {
    /// Create a new session with the given config
    pub fn new(session_id: String, config: SelfProgrammingConfig) -> Self {
        Self {
            session_id,
            started_at: Instant::now(),
            synthesis_count: AtomicU32::new(0),
            registration_count: AtomicU32::new(0),
            decomposition_count: AtomicU32::new(0),
            current_depth: AtomicU32::new(0),
            action_counter: AtomicU32::new(0),
            config,
            pending_approvals: RwLock::new(HashMap::new()),
        }
    }

    /// Check if self-programming is enabled
    pub fn is_enabled(&self) -> bool {
        self.config.enabled
    }

    /// Check if an action is allowed, and track it if so
    pub fn check_action(&self, action: SelfProgrammingAction) -> ActionPermission {
        if !self.config.enabled {
            return ActionPermission::Blocked {
                reason: "Self-programming is disabled".to_string(),
            };
        }

        // Check limits first
        match action {
            SelfProgrammingAction::Synthesize => {
                let count = self.synthesis_count.load(Ordering::Relaxed);
                if count >= self.config.max_synthesis_per_session {
                    return ActionPermission::Blocked {
                        reason: format!(
                            "Synthesis limit reached ({} of {} per session)",
                            count, self.config.max_synthesis_per_session
                        ),
                    };
                }
            }
            _ => {}
        }

        // Check trust-level based approval
        if self.config.is_auto_approved(action) {
            ActionPermission::Allowed
        } else {
            ActionPermission::RequiresApproval {
                reason: format!(
                    "{:?} requires approval at trust level {}",
                    action, self.config.trust_level
                ),
            }
        }
    }

    /// Record that an action was performed
    pub fn record_action(&self, action: SelfProgrammingAction) {
        match action {
            SelfProgrammingAction::Synthesize => {
                self.synthesis_count.fetch_add(1, Ordering::Relaxed);
            }
            SelfProgrammingAction::Register => {
                self.registration_count.fetch_add(1, Ordering::Relaxed);
            }
            _ => {}
        }
    }

    /// Enter a decomposition level (returns current depth)
    pub fn enter_decomposition(&self) -> Result<u32, String> {
        let _count = self.decomposition_count.fetch_add(1, Ordering::Relaxed);
        let current = self.current_depth.fetch_add(1, Ordering::Relaxed);

        if current >= self.config.max_decomposition_depth {
            // Rollback
            self.current_depth.fetch_sub(1, Ordering::Relaxed);
            return Err(format!(
                "Max decomposition depth reached ({} of {})",
                current, self.config.max_decomposition_depth
            ));
        }

        Ok(current + 1)
    }

    /// Exit a decomposition level
    pub fn exit_decomposition(&self) {
        self.current_depth.fetch_sub(1, Ordering::Relaxed);
    }

    /// Get current synthesis count
    pub fn synthesis_count(&self) -> u32 {
        self.synthesis_count.load(Ordering::Relaxed)
    }

    /// Get remaining synthesis budget
    pub fn remaining_synthesis_budget(&self) -> u32 {
        let used = self.synthesis_count.load(Ordering::Relaxed);
        self.config.max_synthesis_per_session.saturating_sub(used)
    }

    /// Queue an action for approval
    pub fn queue_for_approval(
        &self,
        action: SelfProgrammingAction,
        description: String,
        context: HashMap<String, String>,
    ) -> String {
        // Use atomic counter for unique action IDs (no external crates needed)
        let counter = self.action_counter.fetch_add(1, Ordering::Relaxed);
        let action_id = format!(
            "{}_{}_{}",
            self.session_id,
            self.started_at.elapsed().as_millis(),
            counter
        );
        let pending = PendingApproval {
            action_id: action_id.clone(),
            action,
            description,
            requested_at: Instant::now(),
            context,
        };

        if let Ok(mut approvals) = self.pending_approvals.write() {
            approvals.insert(action_id.clone(), pending);
        }

        action_id
    }

    /// Approve a pending action
    pub fn approve(&self, action_id: &str) -> Option<PendingApproval> {
        if let Ok(mut approvals) = self.pending_approvals.write() {
            approvals.remove(action_id)
        } else {
            None
        }
    }

    /// Get session statistics
    pub fn stats(&self) -> SessionStats {
        SessionStats {
            session_id: self.session_id.clone(),
            duration: self.started_at.elapsed(),
            synthesis_count: self.synthesis_count.load(Ordering::Relaxed),
            registration_count: self.registration_count.load(Ordering::Relaxed),
            decomposition_count: self.decomposition_count.load(Ordering::Relaxed),
            current_depth: self.current_depth.load(Ordering::Relaxed),
            pending_approvals: self.pending_approvals.read().map(|a| a.len()).unwrap_or(0),
        }
    }
}

/// Session statistics for monitoring
#[derive(Debug, Clone)]
pub struct SessionStats {
    pub session_id: String,
    pub duration: Duration,
    pub synthesis_count: u32,
    pub registration_count: u32,
    pub decomposition_count: u32,
    pub current_depth: u32,
    pub pending_approvals: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config(trust_level: u8) -> SelfProgrammingConfig {
        SelfProgrammingConfig {
            enabled: true,
            trust_level,
            max_synthesis_per_session: 3,
            max_decomposition_depth: 2,
            require_approval_for_registration: true,
            enable_versioning: true,
            auto_rollback_on_failure: true,
        }
    }

    #[test]
    fn test_session_synthesis_limit() {
        let session = SelfProgrammingSession::new("test".to_string(), test_config(4));

        // Should allow up to max_synthesis_per_session
        for i in 0..3 {
            let result = session.check_action(SelfProgrammingAction::Synthesize);
            assert!(
                matches!(result, ActionPermission::Allowed),
                "iteration {}",
                i
            );
            session.record_action(SelfProgrammingAction::Synthesize);
        }

        // 4th should be blocked
        let result = session.check_action(SelfProgrammingAction::Synthesize);
        assert!(matches!(result, ActionPermission::Blocked { .. }));
    }

    #[test]
    fn test_session_decomposition_depth() {
        let session = SelfProgrammingSession::new("test".to_string(), test_config(4));

        // Should allow up to max_decomposition_depth
        assert!(session.enter_decomposition().is_ok()); // depth 1
        assert!(session.enter_decomposition().is_ok()); // depth 2
        assert!(session.enter_decomposition().is_err()); // depth 3 - blocked

        // Exit and try again
        session.exit_decomposition();
        assert!(session.enter_decomposition().is_ok()); // depth 2 again
    }

    #[test]
    fn test_session_requires_approval() {
        let session = SelfProgrammingSession::new("test".to_string(), test_config(0));

        // At trust level 0, everything requires approval
        let result = session.check_action(SelfProgrammingAction::Synthesize);
        assert!(matches!(result, ActionPermission::RequiresApproval { .. }));
    }

    #[test]
    fn test_session_disabled() {
        let mut config = test_config(4);
        config.enabled = false;
        let session = SelfProgrammingSession::new("test".to_string(), config);

        let result = session.check_action(SelfProgrammingAction::Synthesize);
        assert!(matches!(result, ActionPermission::Blocked { .. }));
    }
}
