//! Loop Guard Mechanism.
//!
//! Prevents agents from getting stuck in infinite reasoning loops
//! without making progress.

pub struct LoopGuard {
    max_loops_without_progress: u32,
    current_loops: u32,
}

impl LoopGuard {
    pub fn new(max_loops_without_progress: u32) -> Self {
        Self {
            max_loops_without_progress,
            current_loops: 0,
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
        self.current_loops += 1;
        Ok(())
    }

    /// Call this when the agent takes a meaningful external action (e.g., writes a file, calls a tool).
    pub fn register_progress(&mut self) {
        self.current_loops = 0;
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
    fn test_loop_guard_resets() {
        let mut guard = LoopGuard::new(3);
        assert!(guard.check_loop().is_ok()); // 1
        assert!(guard.check_loop().is_ok()); // 2
        guard.register_progress(); // Resets to 0
        assert!(guard.check_loop().is_ok()); // 1
        assert!(guard.check_loop().is_ok()); // 2
        assert!(guard.check_loop().is_ok()); // 3
        assert!(guard.check_loop().is_err()); // Trips
    }
}
