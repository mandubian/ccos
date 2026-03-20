//! Python stdlib `ast`-based scan for `agent.install` (no pip deps: runs `python3` + bundled script).
//! Falls back to [`PatternAnalyzer`](super::PatternAnalyzer) if `python3` is missing or the script errors.

use super::pattern::PatternAnalyzer;
use super::provider::{
    AnalysisProvider, CapabilityAnalysis, CapabilityEvidence, FileToAnalyze, SecurityAnalysis,
    SecurityThreat, SecurityThreatType, ThreatSeverity,
};
use serde::Deserialize;
use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

static SCRIPT: &str = include_str!("minimal_python_scan.py");

/// Runs the embedded `minimal_python_scan.py` via `python3` on stdin JSON.
#[derive(Debug, Clone, Default)]
pub struct PythonAstAnalyzer {
    fallback: PatternAnalyzer,
}

impl PythonAstAnalyzer {
    pub fn new() -> Self {
        Self {
            fallback: PatternAnalyzer::new(),
        }
    }

    fn run_python_scan(&self, files: &[FileToAnalyze]) -> Option<PythonScanOut> {
        let payload = serde_json::json!({
            "files": files.iter().map(|f| {
                serde_json::json!({ "path": f.path, "content": f.content })
            }).collect::<Vec<_>>()
        });
        let stdin_json = serde_json::to_string(&payload).ok()?;

        let mut child = Command::new("python3")
            .arg("-c")
            .arg(SCRIPT)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .ok()?;

        let mut stdin = child.stdin.take()?;
        stdin.write_all(stdin_json.as_bytes()).ok()?;
        drop(stdin);

        let out = child.wait_with_output().ok()?;
        if !out.status.success() {
            tracing::warn!(
                target: "code_analysis",
                stderr = %String::from_utf8_lossy(&out.stderr),
                "python_ast scan failed; falling back to pattern analyzer"
            );
            return None;
        }
        serde_json::from_slice(&out.stdout).ok()
    }

    fn into_capability_analysis(&self, scan: PythonScanOut) -> CapabilityAnalysis {
        CapabilityAnalysis {
            inferred_types: scan.inferred_types,
            missing: vec![],
            excessive: vec![],
            confidence: if scan.evidence.is_empty() { 0.55 } else { 0.88 },
            evidence: scan
                .evidence
                .into_iter()
                .map(|e| CapabilityEvidence {
                    file: e.file,
                    line: e.line,
                    pattern: e.pattern,
                    capability_type: e.capability_type,
                    confidence: e.confidence,
                })
                .collect(),
            provider: "python_ast".to_string(),
        }
    }

    fn into_security_analysis(&self, scan: PythonScanOut) -> SecurityAnalysis {
        let threats: Vec<SecurityThreat> = scan
            .threats
            .into_iter()
            .map(|t| SecurityThreat {
                threat_type: map_threat_type(&t.threat_type),
                severity: map_severity(&t.severity),
                description: t.description,
                file: t.file,
                line: t.line,
                pattern: t.pattern,
                confidence: t.confidence,
            })
            .collect();

        let has_critical = threats
            .iter()
            .any(|t| matches!(t.severity, ThreatSeverity::Critical));
        let has_high = threats
            .iter()
            .any(|t| matches!(t.severity, ThreatSeverity::High));
        let passed = !has_critical && !has_high;
        let threats_empty = threats.is_empty();

        SecurityAnalysis {
            passed,
            threats,
            remote_access_detected: scan.remote_access_detected,
            confidence: if threats_empty { 0.72 } else { 0.88 },
            provider: "python_ast".to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
struct PythonScanOut {
    inferred_types: Vec<String>,
    evidence: Vec<PythonEvidence>,
    threats: Vec<PythonThreat>,
    remote_access_detected: bool,
}

#[derive(Debug, Deserialize)]
struct PythonEvidence {
    file: String,
    line: Option<usize>,
    pattern: String,
    capability_type: String,
    confidence: f32,
}

#[derive(Debug, Deserialize)]
struct PythonThreat {
    threat_type: String,
    severity: String,
    description: String,
    file: String,
    line: Option<usize>,
    pattern: String,
    confidence: f32,
}

fn map_threat_type(s: &str) -> SecurityThreatType {
    match s {
        "command_injection" => SecurityThreatType::CommandInjection,
        "privilege_escalation" => SecurityThreatType::PrivilegeEscalation,
        "sandbox_escape" => SecurityThreatType::SandboxEscape,
        "shell_injection" => SecurityThreatType::ShellInjection,
        "destructive" => SecurityThreatType::Destructive,
        "resource_exhaustion" => SecurityThreatType::ResourceExhaustion,
        "remote_code_execution" => SecurityThreatType::RemoteCodeExecution,
        "data_exfiltration" => SecurityThreatType::DataExfiltration,
        other => SecurityThreatType::Custom(other.to_string()),
    }
}

fn map_severity(s: &str) -> ThreatSeverity {
    match s {
        "info" => ThreatSeverity::Info,
        "low" => ThreatSeverity::Low,
        "medium" => ThreatSeverity::Medium,
        "high" => ThreatSeverity::High,
        "critical" => ThreatSeverity::Critical,
        _ => ThreatSeverity::Medium,
    }
}

/// Lazily check if `python3` is available (once per process).
pub fn python3_available() -> bool {
    static OK: OnceLock<bool> = OnceLock::new();
    *OK.get_or_init(|| {
        Command::new("python3")
            .arg("-c")
            .arg("import ast")
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

impl AnalysisProvider for PythonAstAnalyzer {
    fn name(&self) -> &str {
        "python_ast"
    }

    fn analyze_capabilities(&self, files: &[FileToAnalyze]) -> CapabilityAnalysis {
        if !python3_available() {
            tracing::info!(target: "code_analysis", "python3 not available; using pattern analyzer");
            return self.fallback.analyze_capabilities(files);
        }
        match self.run_python_scan(files) {
            Some(scan) => self.into_capability_analysis(scan),
            None => self.fallback.analyze_capabilities(files),
        }
    }

    fn analyze_security(&self, files: &[FileToAnalyze]) -> SecurityAnalysis {
        if !python3_available() {
            return self.fallback.analyze_security(files);
        }
        match self.run_python_scan(files) {
            Some(scan) => self.into_security_analysis(scan),
            None => self.fallback.analyze_security(files),
        }
    }

    fn estimated_duration_ms(&self) -> u64 {
        80
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_file(path: &str, content: &str) -> FileToAnalyze {
        FileToAnalyze {
            path: path.to_string(),
            content: content.to_string(),
        }
    }

    #[test]
    fn python_ast_detects_import_and_subprocess() {
        if !python3_available() {
            return;
        }
        let analyzer = PythonAstAnalyzer::new();
        let files = vec![sample_file(
            "main.py",
            r#"
import urllib.request
import subprocess
subprocess.run(["echo", "hi"], check=True)
"#,
        )];
        let cap = analyzer.analyze_capabilities(&files);
        assert!(
            cap.inferred_types.contains(&"NetworkAccess".to_string()),
            "{cap:?}"
        );
        assert!(
            cap.inferred_types.contains(&"CodeExecution".to_string()),
            "{cap:?}"
        );
        assert_eq!(cap.provider, "python_ast");

        let sec = analyzer.analyze_security(&files);
        assert!(!sec.threats.is_empty() || sec.remote_access_detected);
    }
}
