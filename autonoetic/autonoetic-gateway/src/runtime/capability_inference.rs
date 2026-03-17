//! Automatic capability inference from code analysis.
//!
//! Analyzes agent code (scripts, instructions) to detect required capabilities
//! like NetworkAccess, FileAccess, CodeExecution. This prevents agents from
//! being installed without proper permissions.

use autonoetic_types::capability::Capability;
use serde::{Deserialize, Serialize};

/// Evidence of a capability requirement found in code.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityEvidence {
    /// File name where the pattern was found
    pub file: String,
    /// Line number (if detectable)
    pub line: Option<usize>,
    /// The pattern that matched
    pub pattern: String,
    /// The type of capability detected
    pub capability_type: String,
    /// Confidence level (0.0 - 1.0)
    pub confidence: f32,
}

/// Result of capability inference analysis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityInference {
    /// Detected capability types (e.g., "NetworkAccess", "ReadAccess")
    pub inferred_types: Vec<String>,
    /// Overall confidence (0.0 - 1.0)
    pub confidence: f32,
    /// Detailed evidence for each capability
    pub evidence: Vec<CapabilityEvidence>,
    /// Whether analysis completed successfully
    pub analysis_complete: bool,
}

/// Simple file holder for analysis.
#[derive(Debug, Clone)]
pub struct AnalyzableFile {
    pub path: String,
    pub content: String,
}

/// Trait for providing file content for analysis.
pub trait FileProvider {
    fn path(&self) -> &str;
    fn content(&self) -> &str;
}

impl FileProvider for AnalyzableFile {
    fn path(&self) -> &str {
        &self.path
    }

    fn content(&self) -> &str {
        &self.content
    }
}

/// Implement FileProvider for InstallAgentFile.
impl FileProvider for super::tools::InstallAgentFile {
    fn path(&self) -> &str {
        &self.path
    }

    fn content(&self) -> &str {
        &self.content
    }
}

/// Network access patterns to detect
const NETWORK_PATTERNS: &[&str] = &[
    "urllib.request",
    "urllib.urlopen",
    "requests.get",
    "requests.post",
    "httpx.",
    "fetch(",
    "axios.",
    "XMLHttpRequest",
    "http://",
    "https://",
    "socket.socket",
    "WebSocket",
    "curl ",
    "wget ",
];

/// File system access patterns to detect
/// Note: Use specific patterns to avoid false positives (e.g., urlopen)
const FILE_READ_PATTERNS: &[&str] = &[
    "with open(", // File open in context manager
    "open(path",  // File open with variable
    "read_file",
    "fs.readFile",
    "fs.readFileSync",
    "os.path.exists",
    "pathlib.Path(",
    ".read_text()",  // pathlib text read
    ".read_bytes()", // pathlib bytes read
];

const FILE_WRITE_PATTERNS: &[&str] = &[
    "os.remove",
    "os.unlink",
    "shutil.rmtree",
    "fs.unlink",
    "fs.rm",
];

/// Code execution patterns to detect
const CODE_EXECUTION_PATTERNS: &[&str] = &[
    "subprocess.call",
    "subprocess.run",
    "subprocess.Popen",
    "subprocess.check_output",
    "os.system",
    "os.popen",
    "child_process.exec",
    "child_process.spawn",
    "exec(",
    "eval(",
    "shell=True",
];

/// Infer capabilities from a list of files.
///
/// Returns capability types that are likely required based on code analysis.
pub fn infer_capabilities(files: &[impl FileProvider]) -> CapabilityInference {
    let mut evidence = Vec::new();
    let mut detected_types: std::collections::HashSet<String> = std::collections::HashSet::new();

    for file in files {
        let content = file.content();
        let path = file.path();

        // Analyze for network access
        for pattern in NETWORK_PATTERNS {
            if let Some((line_num, _)) = find_pattern(content, pattern) {
                detected_types.insert("NetworkAccess".to_string());
                evidence.push(CapabilityEvidence {
                    file: path.to_string(),
                    line: Some(line_num),
                    pattern: pattern.to_string(),
                    capability_type: "NetworkAccess".to_string(),
                    confidence: 0.95,
                });
            }
        }

        // Analyze for file read access
        for pattern in FILE_READ_PATTERNS {
            if let Some((line_num, _)) = find_pattern(content, pattern) {
                detected_types.insert("ReadAccess".to_string());
                evidence.push(CapabilityEvidence {
                    file: path.to_string(),
                    line: Some(line_num),
                    pattern: pattern.to_string(),
                    capability_type: "ReadAccess".to_string(),
                    confidence: 0.85,
                });
            }
        }

        // Analyze for file write access
        for pattern in FILE_WRITE_PATTERNS {
            if let Some((line_num, _)) = find_pattern(content, pattern) {
                detected_types.insert("WriteAccess".to_string());
                evidence.push(CapabilityEvidence {
                    file: path.to_string(),
                    line: Some(line_num),
                    pattern: pattern.to_string(),
                    capability_type: "WriteAccess".to_string(),
                    confidence: 0.90,
                });
            }
        }

        // Analyze for code execution
        for pattern in CODE_EXECUTION_PATTERNS {
            if let Some((line_num, _)) = find_pattern(content, pattern) {
                detected_types.insert("CodeExecution".to_string());
                evidence.push(CapabilityEvidence {
                    file: path.to_string(),
                    line: Some(line_num),
                    pattern: pattern.to_string(),
                    capability_type: "CodeExecution".to_string(),
                    confidence: 0.95,
                });
            }
        }
    }

    let inferred_types: Vec<String> = detected_types.into_iter().collect();

    // Calculate overall confidence
    let overall_confidence = if evidence.is_empty() {
        0.5 // No evidence = uncertain
    } else {
        evidence.iter().map(|e| e.confidence).sum::<f32>() / evidence.len() as f32
    };

    CapabilityInference {
        inferred_types,
        confidence: overall_confidence,
        evidence,
        analysis_complete: true,
    }
}

/// Find a pattern in content (case-insensitive), returns (line_number, line_content).
/// Excludes false positives like urlopen when searching for open.
fn find_pattern(content: &str, pattern: &str) -> Option<(usize, String)> {
    let pattern_lower = pattern.to_lowercase();

    // Patterns to exclude (false positives)
    const EXCLUDE_PATTERNS: &[&str] = &["urlopen", "fileopen", "reopen"];

    for (line_num, line) in content.lines().enumerate() {
        let line_lower = line.to_lowercase();
        if let Some(pos) = line_lower.find(&pattern_lower) {
            // Check if this is a false positive (pattern preceded by certain prefixes)
            let before_pattern = if pos > 0 { &line_lower[..pos] } else { "" };
            let is_excluded = EXCLUDE_PATTERNS
                .iter()
                .any(|ex| before_pattern.ends_with(ex));

            if !is_excluded {
                return Some((line_num + 1, line.to_string()));
            }
        }
    }
    None
}

/// Check if a capability type is covered by declared capabilities.
fn is_capability_covered(cap_type: &str, declared: &[Capability]) -> bool {
    for cap in declared {
        let declared_type = capability_type_name(cap);
        if declared_type == cap_type {
            return true;
        }
    }
    false
}

/// Get type name for a capability.
fn capability_type_name(cap: &Capability) -> &'static str {
    match cap {
        Capability::NetworkAccess { .. } => "NetworkAccess",
        Capability::ReadAccess { .. } => "ReadAccess",
        Capability::WriteAccess { .. } => "WriteAccess",
        Capability::CodeExecution { .. } => "CodeExecution",
        Capability::AgentSpawn { .. } => "AgentSpawn",
        Capability::AgentMessage { .. } => "AgentMessage",
        Capability::SandboxFunctions { .. } => "SandboxFunctions",
        Capability::BackgroundReevaluation { .. } => "BackgroundReevaluation",
    }
}

/// Check if declared capabilities cover all inferred capabilities.
///
/// Returns list of missing capability types (inferred but not declared).
pub fn find_missing_capabilities(
    declared: &[Capability],
    inferred: &CapabilityInference,
) -> Vec<String> {
    let mut missing = Vec::new();

    for inferred_type in &inferred.inferred_types {
        if !is_capability_covered(inferred_type, declared) {
            missing.push(inferred_type.clone());
        }
    }

    missing
}

/// Validate capabilities against inferred requirements.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityValidation {
    pub is_valid: bool,
    pub missing: Vec<String>,
    pub inferred_types: Vec<String>,
    pub message: String,
}

/// Validate that declared capabilities match inferred requirements.
pub fn validate_capabilities(
    declared: &[Capability],
    files: &[impl FileProvider],
) -> CapabilityValidation {
    let inferred = infer_capabilities(files);
    let missing = find_missing_capabilities(declared, &inferred);

    let is_valid = missing.is_empty();
    let message = if missing.is_empty() {
        "Capabilities match code requirements".to_string()
    } else {
        format!(
            "Missing capabilities: {}. Code requires these capabilities but they were not declared.",
            missing.join(", ")
        )
    };

    CapabilityValidation {
        is_valid,
        missing,
        inferred_types: inferred.inferred_types.clone(),
        message,
    }
}

/// Convert inferred types to actual Capability objects.
pub fn types_to_capabilities(types: &[String]) -> Vec<Capability> {
    types
        .iter()
        .map(|t| match t.as_str() {
            "NetworkAccess" => Capability::NetworkAccess {
                hosts: vec!["*".to_string()],
            },
            "ReadAccess" => Capability::ReadAccess {
                scopes: vec!["*".to_string()],
            },
            "WriteAccess" => Capability::WriteAccess {
                scopes: vec!["*".to_string()],
            },
            "CodeExecution" => Capability::CodeExecution {
                patterns: vec!["*".to_string()],
            },
            "AgentSpawn" => Capability::AgentSpawn { max_children: 1 },
            "AgentMessage" => Capability::AgentMessage {
                patterns: vec!["*".to_string()],
            },
            "SandboxFunctions" => Capability::SandboxFunctions {
                allowed: vec!["*".to_string()],
            },
            "BackgroundReevaluation" => Capability::BackgroundReevaluation {
                min_interval_secs: 60,
                allow_reasoning: false,
            },
            _ => Capability::ReadAccess {
                scopes: vec!["*".to_string()],
            }, // Default fallback
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestFile {
        path: String,
        content: String,
    }

    impl FileProvider for TestFile {
        fn path(&self) -> &str {
            &self.path
        }

        fn content(&self) -> &str {
            &self.content
        }
    }

    #[test]
    fn test_infer_network_access_from_urllib() {
        let files = vec![TestFile {
            path: "main.py".to_string(),
            content: r#"
import urllib.request

def fetch_weather(location):
    url = f"https://api.open-meteo.com/v1/forecast?location={location}"
    with urllib.request.urlopen(url) as response:
        return response.read()
"#
            .to_string(),
        }];

        let result = infer_capabilities(&files);
        assert!(result.analysis_complete);
        assert!(!result.evidence.is_empty());
        assert!(
            result.inferred_types.contains(&"NetworkAccess".to_string()),
            "Should detect NetworkAccess from urllib.request"
        );
    }

    #[test]
    fn test_infer_network_access_from_requests() {
        let files = vec![TestFile {
            path: "script.py".to_string(),
            content: r#"
import requests

response = requests.get("https://example.com/api")
data = response.json()
"#
            .to_string(),
        }];

        let result = infer_capabilities(&files);
        assert!(
            result.inferred_types.contains(&"NetworkAccess".to_string()),
            "Should detect NetworkAccess from requests.get"
        );
    }

    #[test]
    fn test_infer_file_access() {
        let files = vec![TestFile {
            path: "processor.py".to_string(),
            content: r#"
def process_file(path):
    with open(path, 'r') as f:
        data = f.read()
    return data.upper()
"#
            .to_string(),
        }];

        let result = infer_capabilities(&files);
        assert!(
            result.inferred_types.contains(&"ReadAccess".to_string()),
            "Should detect ReadAccess from open()"
        );
    }

    #[test]
    fn test_infer_code_execution() {
        let files = vec![TestFile {
            path: "runner.py".to_string(),
            content: r#"
import subprocess

def run_command(cmd):
    result = subprocess.run(cmd, shell=True, capture_output=True)
    return result.stdout
"#
            .to_string(),
        }];

        let result = infer_capabilities(&files);
        assert!(
            result.inferred_types.contains(&"CodeExecution".to_string()),
            "Should detect CodeExecution from subprocess"
        );
    }

    #[test]
    fn test_missing_capabilities_detected() {
        let files = vec![TestFile {
            path: "api_client.py".to_string(),
            content: r#"
import urllib.request

def call_api():
    response = urllib.request.urlopen("https://api.example.com")
    return response.read()
"#
            .to_string(),
        }];

        // Declare only ReadAccess, missing NetworkAccess
        let declared = vec![Capability::ReadAccess {
            scopes: vec!["*".to_string()],
        }];
        let validation = validate_capabilities(&declared, &files);

        assert!(!validation.is_valid, "Should detect missing NetworkAccess");
        assert!(!validation.missing.is_empty());
        assert!(
            validation.missing.contains(&"NetworkAccess".to_string()),
            "Missing should include NetworkAccess"
        );
    }

    #[test]
    fn test_valid_capabilities_pass() {
        let files = vec![TestFile {
            path: "api_client.py".to_string(),
            content: r#"
import urllib.request

def call_api():
    response = urllib.request.urlopen("https://api.example.com")
    return response.read()
"#
            .to_string(),
        }];

        // Declare NetworkAccess (no file operations, so ReadAccess not needed)
        let declared = vec![Capability::NetworkAccess {
            hosts: vec!["*".to_string()],
        }];
        let validation = validate_capabilities(&declared, &files);

        assert!(
            validation.is_valid,
            "Should pass when NetworkAccess is declared"
        );
        assert!(validation.missing.is_empty());
    }

    #[test]
    fn test_valid_capabilities_with_file_access() {
        let files = vec![TestFile {
            path: "worker.py".to_string(),
            content: r#"
from pathlib import Path
import json

def process():
    path = Path('state/data.json')
    data = json.loads(path.read_text())
    data['count'] += 1
    path.write_text(json.dumps(data))
"#
            .to_string(),
        }];

        // Declare both NetworkAccess and file access capabilities
        let declared = vec![
            Capability::ReadAccess {
                scopes: vec!["*".to_string()],
            },
            Capability::WriteAccess {
                scopes: vec!["*".to_string()],
            },
        ];
        let validation = validate_capabilities(&declared, &files);

        assert!(
            validation.is_valid,
            "Should pass when ReadAccess and WriteAccess are declared"
        );
        assert!(validation.missing.is_empty());
    }

    #[test]
    fn test_no_network_in_pure_code() {
        let files = vec![TestFile {
            path: "calculator.py".to_string(),
            content: r#"
def add(a, b):
    return a + b

def multiply(a, b):
    return a * b
"#
            .to_string(),
        }];

        let result = infer_capabilities(&files);
        assert!(
            !result.inferred_types.contains(&"NetworkAccess".to_string()),
            "Pure math code should not require NetworkAccess"
        );
    }

    #[test]
    fn test_multiple_capabilities_inferred() {
        let files = vec![TestFile {
            path: "agent.py".to_string(),
            content: r#"
import urllib.request
import subprocess
import json

def process():
    # Network access
    data = urllib.request.urlopen("https://api.example.com").read()
    
    # Code execution
    subprocess.run(["echo", "done"])
    
    # File access
    with open("/tmp/data.json", "w") as f:
        json.dump(data, f)
"#
            .to_string(),
        }];

        let result = infer_capabilities(&files);
        assert!(
            result.inferred_types.len() >= 3,
            "Should detect multiple capabilities"
        );
        assert!(result.inferred_types.contains(&"NetworkAccess".to_string()));
        assert!(result.inferred_types.contains(&"CodeExecution".to_string()));
        assert!(result.inferred_types.contains(&"ReadAccess".to_string()));
    }
}
