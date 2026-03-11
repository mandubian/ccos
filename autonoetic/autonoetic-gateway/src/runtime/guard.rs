//! Loop Guard Mechanism.
//!
//! Prevents agents from getting stuck in infinite reasoning loops
//! without making progress.

pub struct LoopGuard {
    max_loops_without_progress: u32,
    current_loops: u32,
    last_failure_hash: Option<u64>,
    consecutive_failures: u32,
}

impl LoopGuard {
    pub fn new(max_loops_without_progress: u32) -> Self {
        Self {
            max_loops_without_progress,
            current_loops: 0,
            last_failure_hash: None,
            consecutive_failures: 0,
        }
    }

    /// Call this before each agent reasoning cycle.
    pub fn check_loop(&mut self) -> anyhow::Result<()> {
        if self.current_loops >= self.max_loops_without_progress {
            anyhow::bail!(
                "LoopGuard tripped: Agent executed {} cycles without meaningful progress.",
                self.current_loops
            );
        }

        if self.consecutive_failures >= 3 {
            anyhow::bail!(
                "LoopGuard tripped: Agent is repeating a failing action (3 consecutive identical failures). Breaking loop to prevent resource waste."
            );
        }

        self.current_loops += 1;
        Ok(())
    }

    /// Track a tool failure to detect redundant failing loops.
    pub fn register_failure(&mut self, tool_name: &str, arguments: &str) {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

        let mut hasher = DefaultHasher::new();
        tool_name.hash(&mut hasher);
        arguments.hash(&mut hasher);
        let current_hash = hasher.finish();

        if let Some(last_hash) = self.last_failure_hash {
            if last_hash == current_hash {
                self.consecutive_failures += 1;
            } else {
                self.consecutive_failures = 1;
                self.last_failure_hash = Some(current_hash);
            }
        } else {
            self.consecutive_failures = 1;
            self.last_failure_hash = Some(current_hash);
        }
    }

    /// Call this when the agent takes a meaningful external action (e.g., writes a file, calls a tool successfully).
    pub fn register_progress(&mut self) {
        self.current_loops = 0;
        self.last_failure_hash = None;
        self.consecutive_failures = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loop_guard_trips() {
        let mut guard = LoopGuard::new(3);
        assert!(guard.check_loop().is_ok()); // 1
        assert!(guard.check_loop().is_ok()); // 2
        assert!(guard.check_loop().is_ok()); // 3
        assert!(guard.check_loop().is_err()); // Trips on 4th check
    }

    #[test]
    fn test_loop_guard_redundant_failures() {
        let mut guard = LoopGuard::new(10);
        assert!(guard.check_loop().is_ok());

        // Simulate 3 consecutive identical failures
        guard.register_failure("test_tool", "{\"a\": 1}");
        assert!(guard.check_loop().is_ok());

        guard.register_failure("test_tool", "{\"a\": 1}");
        assert!(guard.check_loop().is_ok());

        guard.register_failure("test_tool", "{\"a\": 1}");
        assert!(guard.check_loop().is_err()); // Should trip on 4th check after 3 failures
    }

    #[test]
    fn test_loop_guard_failure_reset() {
        let mut guard = LoopGuard::new(10);

        guard.register_failure("test_tool", "{\"a\": 1}");
        guard.register_failure("test_tool", "{\"a\": 1}");
        guard.register_progress(); // Success resets failures

        guard.register_failure("test_tool", "{\"a\": 1}");
        guard.register_failure("test_tool", "{\"a\": 1}");
        assert!(guard.check_loop().is_ok()); // Should be OK (only 2 consecutive)
    }
}
