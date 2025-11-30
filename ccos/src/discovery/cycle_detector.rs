//! Cycle detection for recursive capability discovery

use std::collections::HashSet;

/// Tracks visited capabilities to prevent infinite recursion
#[derive(Debug, Clone)]
pub struct CycleDetector {
    visited_capabilities: HashSet<String>,
    max_depth: usize,
    current_depth: usize,
}

impl CycleDetector {
    /// Create a new cycle detector with a maximum depth
    pub fn new(max_depth: usize) -> Self {
        Self {
            visited_capabilities: HashSet::new(),
            max_depth,
            current_depth: 0,
        }
    }

    /// Check if we've already visited this capability (indicates a cycle)
    pub fn has_cycle(&self, capability_class: &str) -> bool {
        self.visited_capabilities.contains(capability_class)
    }

    /// Check if we've reached the maximum depth
    pub fn is_max_depth(&self) -> bool {
        self.current_depth >= self.max_depth
    }

    /// Check if we can continue deeper
    pub fn can_go_deeper(&self) -> bool {
        !self.is_max_depth()
    }

    /// Mark a capability as visited
    pub fn visit(&mut self, capability_class: &str) {
        self.visited_capabilities
            .insert(capability_class.to_string());
    }

    /// Create a new detector one level deeper
    pub fn go_deeper(&self) -> Self {
        Self {
            visited_capabilities: self.visited_capabilities.clone(),
            max_depth: self.max_depth,
            current_depth: self.current_depth + 1,
        }
    }

    /// Get current depth
    pub fn current_depth(&self) -> usize {
        self.current_depth
    }

    /// Get the set of visited capabilities
    pub fn visited(&self) -> &HashSet<String> {
        &self.visited_capabilities
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cycle_detection() {
        let mut detector = CycleDetector::new(5);

        assert!(!detector.has_cycle("capability.a"));
        detector.visit("capability.a");
        assert!(detector.has_cycle("capability.a"));
    }

    #[test]
    fn test_depth_limits() {
        let detector = CycleDetector::new(3);
        assert!(detector.can_go_deeper());

        let deeper = detector.go_deeper().go_deeper().go_deeper();
        assert!(!deeper.can_go_deeper());
        assert!(deeper.is_max_depth());
    }

    #[test]
    fn test_visited_tracking() {
        let mut detector = CycleDetector::new(5);

        detector.visit("capability.x");
        detector.visit("capability.y");

        assert_eq!(detector.visited().len(), 2);
        assert!(detector.visited().contains("capability.x"));
        assert!(detector.visited().contains("capability.y"));
    }
}
