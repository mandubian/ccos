//! Approved Sandbox Exec Replay Cache.
//!
//! Caches approved sandbox.exec fingerprints so identical future executions
//! skip creating new approval requests.
//!
//! Cache key = SHA256(agent_id + normalized_remote_targets + code_to_analyze)
//!
//! Only caches when ALL detected remote access evidence resolves to concrete hosts.
//! If any detected pattern is opaque (imports, function calls with variables),
//! the exec is never cached.

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex};

use sha2::{Digest, Sha256};

use crate::runtime::remote_access::DetectedPattern;

/// A cached approved sandbox exec entry.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ApprovedExecEntry {
    /// Unique cache key (SHA256 fingerprint).
    pub fingerprint: String,
    /// The agent that was approved.
    pub agent_id: String,
    /// Concrete remote targets extracted from code (sorted, deduplicated).
    pub remote_targets: Vec<String>,
    /// The analyzed code content that was approved.
    pub code_content: String,
    /// The original approval request ID.
    pub approval_request_id: String,
    /// ISO timestamp when approval was granted.
    pub approved_at: String,
    /// Who approved (typically "operator").
    pub approved_by: String,
    /// ISO timestamp of last successful use.
    pub last_used_at: String,
}

/// Thread-safe cache for approved sandbox exec fingerprints.
pub struct ApprovedExecCache {
    cache_path: std::path::PathBuf,
    entries: Arc<Mutex<HashMap<String, ApprovedExecEntry>>>,
}

impl ApprovedExecCache {
    /// Creates a new ApprovedExecCache, loading existing entries from disk.
    pub fn new(gateway_dir: &Path) -> anyhow::Result<Self> {
        let cache_dir = gateway_dir
            .join("scheduler")
            .join("approvals")
            .join("exec_cache");
        let cache_path = cache_dir.join("index.json");

        let entries = if cache_path.exists() {
            let json = std::fs::read_to_string(&cache_path)?;
            let entries: HashMap<String, ApprovedExecEntry> = serde_json::from_str(&json)?;
            tracing::info!(
                target: "approved_exec_cache",
                path = %cache_path.display(),
                count = entries.len(),
                "Loaded existing approved exec cache"
            );
            entries
        } else {
            HashMap::new()
        };

        Ok(Self {
            cache_path,
            entries: Arc::new(Mutex::new(entries)),
        })
    }

    /// Records a new approved exec entry.
    pub fn record(&self, entry: ApprovedExecEntry) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        entries.insert(entry.fingerprint.clone(), entry);
        self.flush(&entries)?;
        Ok(())
    }

    /// Looks up an entry by fingerprint.
    pub fn find(&self, fingerprint: &str) -> Option<ApprovedExecEntry> {
        let entries = self.entries.lock().unwrap();
        entries.get(fingerprint).cloned()
    }

    /// Updates the last_used_at timestamp for an entry.
    pub fn update_last_used(&self, fingerprint: &str) -> anyhow::Result<()> {
        let mut entries = self.entries.lock().unwrap();
        if let Some(entry) = entries.get_mut(fingerprint) {
            entry.last_used_at = chrono::Utc::now().to_rfc3339();
            self.flush(&entries)?;
        }
        Ok(())
    }

    /// Returns the number of cached entries.
    pub fn len(&self) -> usize {
        self.entries.lock().unwrap().len()
    }

    fn flush(&self, entries: &HashMap<String, ApprovedExecEntry>) -> anyhow::Result<()> {
        if let Some(parent) = self.cache_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(entries)?;
        std::fs::write(&self.cache_path, json)?;
        Ok(())
    }
}

/// Checks if ALL remote access evidence is concrete (cacheable).
///
/// Returns true only if EVERY detected pattern is a concrete target
/// (url_literal or ip_address). Returns false if ANY pattern is opaque
/// (import or function_call), even if concrete targets also exist.
///
/// This ensures that mixed concrete + opaque behavior is never cached.
/// For example, code like:
/// ```python
/// import requests
/// requests.get("https://api.example.com")  # concrete
/// requests.get(variable_url)              # opaque
/// ```
/// will NOT be cached because the opaque function_call means we cannot
/// guarantee what hosts might be accessed at runtime.
pub fn has_concrete_targets(patterns: &[DetectedPattern]) -> bool {
    if patterns.is_empty() {
        return false;
    }

    patterns
        .iter()
        .all(|p| matches!(p.category.as_str(), "url_literal" | "ip_address"))
}

/// Extracts concrete host targets from detected patterns and normalizes them.
///
/// For URL literals: extracts the host (e.g., "https://api.example.com/path" → "api.example.com")
/// For IP addresses: uses the IP as-is (e.g., "192.168.1.100")
///
/// Returns sorted, deduplicated list of hosts.
pub fn normalize_targets(patterns: &[DetectedPattern]) -> Vec<String> {
    let mut hosts = Vec::new();

    for pattern in patterns {
        match pattern.category.as_str() {
            "url_literal" => {
                // Extract host from URL literal
                if let Some(host) = extract_host_from_url(&pattern.pattern) {
                    if !hosts.contains(&host) {
                        hosts.push(host);
                    }
                }
            }
            "ip_address" => {
                // IP address is already a host
                if !hosts.contains(&pattern.pattern) {
                    hosts.push(pattern.pattern.clone());
                }
            }
            _ => {
                // Skip non-concrete patterns
            }
        }
    }

    hosts.sort();
    hosts
}

/// Computes the fingerprint for a sandbox exec request.
///
/// Fingerprint = SHA256(agent_id + "|" + sorted_targets + "|" + code_to_analyze)
pub fn compute_fingerprint(agent_id: &str, targets: &[String], code_to_analyze: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(agent_id.as_bytes());
    hasher.update(b"|");
    hasher.update(targets.join(",").as_bytes());
    hasher.update(b"|");
    hasher.update(code_to_analyze.as_bytes());
    format!("sha256:{:x}", hasher.finalize())
}

/// Extracts the host from a URL literal using regex.
///
/// Examples:
/// - "https://api.example.com/v1/forecast" → "api.example.com"
/// - "http://192.168.1.1:8080/api" → "192.168.1.1"
fn extract_host_from_url(url: &str) -> Option<String> {
    // Match scheme://host[:port][/path...]
    // Host can be a domain name or IP address
    let re = regex::Regex::new(r"(?i)^[a-z]+://([^/:]+)").ok()?;
    let captures = re.captures(url)?;
    let host = captures.get(1)?.as_str();
    if host.is_empty() {
        None
    } else {
        Some(host.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_concrete_targets_url_only() {
        let patterns = vec![DetectedPattern {
            category: "url_literal".to_string(),
            pattern: "https://api.example.com/data".to_string(),
            line_number: Some(1),
            reason: "URL literal".to_string(),
        }];
        assert!(has_concrete_targets(&patterns));
    }

    #[test]
    fn test_has_concrete_targets_ip_only() {
        let patterns = vec![DetectedPattern {
            category: "ip_address".to_string(),
            pattern: "192.168.1.100".to_string(),
            line_number: Some(1),
            reason: "IP address".to_string(),
        }];
        assert!(has_concrete_targets(&patterns));
    }

    #[test]
    fn test_has_concrete_targets_mixed_concrete() {
        let patterns = vec![
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://api.example.com/data".to_string(),
                line_number: Some(1),
                reason: "URL literal".to_string(),
            },
            DetectedPattern {
                category: "ip_address".to_string(),
                pattern: "10.0.0.1".to_string(),
                line_number: Some(2),
                reason: "IP address".to_string(),
            },
        ];
        assert!(has_concrete_targets(&patterns));
    }

    #[test]
    fn test_has_concrete_targets_with_import() {
        // Import + literal URL should NOT cache - import is opaque
        let patterns = vec![
            DetectedPattern {
                category: "import".to_string(),
                pattern: "import requests".to_string(),
                line_number: Some(1),
                reason: "HTTP client".to_string(),
            },
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://api.example.com/data".to_string(),
                line_number: Some(2),
                reason: "URL literal".to_string(),
            },
        ];
        // Should NOT cache because import is opaque
        assert!(!has_concrete_targets(&patterns));
    }

    #[test]
    fn test_has_concrete_targets_with_function_call() {
        // URL literal + function call should NOT cache - function_call is opaque
        let patterns = vec![
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://api.example.com/data".to_string(),
                line_number: Some(1),
                reason: "URL literal".to_string(),
            },
            DetectedPattern {
                category: "function_call".to_string(),
                pattern: ".connect(".to_string(),
                line_number: Some(2),
                reason: "Socket connection".to_string(),
            },
        ];
        // Should NOT cache because function_call is opaque
        assert!(!has_concrete_targets(&patterns));
    }

    #[test]
    fn test_has_concrete_targets_only_import_no_url() {
        // Only imports, no concrete URL - should NOT cache
        let patterns = vec![
            DetectedPattern {
                category: "import".to_string(),
                pattern: "import requests".to_string(),
                line_number: Some(1),
                reason: "HTTP client".to_string(),
            },
            DetectedPattern {
                category: "function_call".to_string(),
                pattern: "requests.get(".to_string(),
                line_number: Some(2),
                reason: "HTTP GET".to_string(),
            },
        ];
        // No concrete target - should NOT cache
        assert!(!has_concrete_targets(&patterns));
    }

    #[test]
    fn test_has_concrete_targets_empty() {
        assert!(!has_concrete_targets(&[]));
    }

    #[test]
    fn test_normalize_targets_urls() {
        let patterns = vec![
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://api.example.com/v1/data".to_string(),
                line_number: Some(1),
                reason: "URL literal".to_string(),
            },
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://status.github.com/api".to_string(),
                line_number: Some(2),
                reason: "URL literal".to_string(),
            },
        ];
        let targets = normalize_targets(&patterns);
        assert_eq!(targets, vec!["api.example.com", "status.github.com"]);
    }

    #[test]
    fn test_normalize_targets_dedup() {
        let patterns = vec![
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://api.example.com/v1".to_string(),
                line_number: Some(1),
                reason: "URL literal".to_string(),
            },
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://api.example.com/v2".to_string(),
                line_number: Some(2),
                reason: "URL literal".to_string(),
            },
        ];
        let targets = normalize_targets(&patterns);
        assert_eq!(targets, vec!["api.example.com"]);
    }

    #[test]
    fn test_normalize_targets_ip() {
        let patterns = vec![DetectedPattern {
            category: "ip_address".to_string(),
            pattern: "192.168.1.100".to_string(),
            line_number: Some(1),
            reason: "IP address".to_string(),
        }];
        let targets = normalize_targets(&patterns);
        assert_eq!(targets, vec!["192.168.1.100"]);
    }

    #[test]
    fn test_normalize_targets_skips_imports() {
        let patterns = vec![
            DetectedPattern {
                category: "import".to_string(),
                pattern: "import requests".to_string(),
                line_number: Some(1),
                reason: "HTTP client".to_string(),
            },
            DetectedPattern {
                category: "url_literal".to_string(),
                pattern: "https://api.example.com/data".to_string(),
                line_number: Some(2),
                reason: "URL literal".to_string(),
            },
        ];
        let targets = normalize_targets(&patterns);
        // Only concrete hosts, imports are skipped
        assert_eq!(targets, vec!["api.example.com"]);
    }

    #[test]
    fn test_compute_fingerprint_deterministic() {
        let fp1 = compute_fingerprint("agent.id", &["host.com".to_string()], "code");
        let fp2 = compute_fingerprint("agent.id", &["host.com".to_string()], "code");
        assert_eq!(fp1, fp2);
        assert!(fp1.starts_with("sha256:"));
    }

    #[test]
    fn test_compute_fingerprint_different_agents() {
        let fp1 = compute_fingerprint("agent.a", &["host.com".to_string()], "code");
        let fp2 = compute_fingerprint("agent.b", &["host.com".to_string()], "code");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_compute_fingerprint_different_code() {
        let fp1 = compute_fingerprint("agent.id", &["host.com".to_string()], "code_a");
        let fp2 = compute_fingerprint("agent.id", &["host.com".to_string()], "code_b");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_compute_fingerprint_different_targets() {
        let fp1 = compute_fingerprint("agent.id", &["host_a.com".to_string()], "code");
        let fp2 = compute_fingerprint("agent.id", &["host_b.com".to_string()], "code");
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_extract_host_from_url() {
        assert_eq!(
            extract_host_from_url("https://api.example.com/v1/forecast"),
            Some("api.example.com".to_string())
        );
        assert_eq!(
            extract_host_from_url("http://192.168.1.1:8080/api"),
            Some("192.168.1.1".to_string())
        );
        assert_eq!(
            extract_host_from_url("https://status.github.com"),
            Some("status.github.com".to_string())
        );
    }
}
