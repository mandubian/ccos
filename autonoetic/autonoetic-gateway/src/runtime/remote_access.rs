//! Static analysis for detecting remote/network access in code.
//!
//! Analyzes code before execution to detect patterns that require
//! network access (HTTP requests, socket connections, etc.).
//! If detected, the code execution requires operator approval.

use regex::Regex;

/// Result of analyzing code for remote access patterns.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RemoteAccessAnalysis {
    /// Whether the code contains patterns that require network access.
    pub requires_approval: bool,
    /// List of detected patterns that triggered the flag.
    pub detected_patterns: Vec<DetectedPattern>,
    /// Summary of what was detected.
    pub summary: String,
}

/// A detected pattern indicating potential remote access.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DetectedPattern {
    /// Category of the pattern (import, function_call, url_literal, etc.)
    pub category: String,
    /// The specific pattern matched.
    pub pattern: String,
    /// Line number where the pattern was found (1-indexed, approximate).
    pub line_number: Option<usize>,
    /// Why this pattern indicates remote access.
    pub reason: String,
}

/// Static analyzer for detecting remote access patterns in code.
pub struct RemoteAccessAnalyzer;

impl RemoteAccessAnalyzer {
    /// Analyzes code for patterns that require network/remote access.
    ///
    /// Detection categories:
    /// 1. Import statements for network libraries
    /// 2. Function/method calls for network operations
    /// 3. URL literals (http://, https://, ftp://)
    /// 4. IP address literals
    pub fn analyze_code(code: &str) -> RemoteAccessAnalysis {
        let mut patterns = Vec::new();

        // Check for network-related imports
        patterns.extend(Self::detect_imports(code));

        // Check for network-related function calls
        patterns.extend(Self::detect_function_calls(code));

        // Check for URL literals
        patterns.extend(Self::detect_url_literals(code));

        // Check for IP address literals
        patterns.extend(Self::detect_ip_addresses(code));

        let requires_approval = !patterns.is_empty();

        let summary = if patterns.is_empty() {
            "No remote access patterns detected".to_string()
        } else {
            let categories: Vec<&str> = patterns.iter().map(|p| p.category.as_str()).collect();
            let unique_categories: Vec<&str> = categories
                .into_iter()
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();
            format!(
                "Detected {} remote access pattern(s) in categories: {}",
                patterns.len(),
                unique_categories.join(", ")
            )
        };

        RemoteAccessAnalysis {
            requires_approval,
            detected_patterns: patterns,
            summary,
        }
    }

    /// Detects import statements for network libraries.
    fn detect_imports(code: &str) -> Vec<DetectedPattern> {
        let mut patterns = Vec::new();

        // Python import patterns
        let import_patterns = vec![
            ("requests", "HTTP client library"),
            ("urllib", "URL handling library"),
            ("urllib.request", "URL request library"),
            ("urllib3", "HTTP client library"),
            ("httpx", "Async HTTP client library"),
            ("aiohttp", "Async HTTP client library"),
            ("socket", "Low-level networking library"),
            ("ftplib", "FTP client library"),
            ("smtplib", "SMTP client library"),
            ("paramiko", "SSH client library"),
            ("fabric", "SSH execution library"),
            ("websockets", "WebSocket client library"),
            ("redis", "Redis client library"),
            ("pymongo", "MongoDB client library"),
            ("mysql", "MySQL client library"),
            ("psycopg", "PostgreSQL client library"),
            ("sqlalchemy", "SQL toolkit (can connect to remote DBs)"),
            ("boto3", "AWS SDK (cloud access)"),
            ("google.cloud", "Google Cloud SDK"),
            ("azure", "Azure SDK"),
        ];

        for (lib, reason) in &import_patterns {
            // Match: import X, from X import, import X as Y
            let import_regex = format!(
                r"(?m)^\s*(?:import\s+{}|from\s+{}\s+import)",
                regex::escape(lib),
                regex::escape(lib)
            );
            if let Ok(re) = Regex::new(&import_regex) {
                for mat in re.find_iter(code) {
                    let line_num = code[..mat.start()].matches('\n').count() + 1;
                    patterns.push(DetectedPattern {
                        category: "import".to_string(),
                        pattern: mat.as_str().trim().to_string(),
                        line_number: Some(line_num),
                        reason: reason.to_string(),
                    });
                }
            }
        }

        patterns
    }

    /// Detects function/method calls for network operations.
    fn detect_function_calls(code: &str) -> Vec<DetectedPattern> {
        let mut patterns = Vec::new();

        let call_patterns = vec![
            (r"\.connect\s*\(", "Socket connection initiation"),
            (r"\.send\s*\(", "Sending data over network"),
            (r"\.recv\s*\(", "Receiving data from network"),
            (r"\.bind\s*\(", "Socket binding"),
            (r"\.listen\s*\(", "Socket listening"),
            (r"\.accept\s*\(", "Socket accept connection"),
            (r"urlopen\s*\(", "Opening URL connection"),
            (
                r"requests\.(get|post|put|delete|patch|head|options)\s*\(",
                "HTTP request",
            ),
            (
                r"httpx\.(get|post|put|delete|patch|head|options)\s*\(",
                "HTTP request",
            ),
            (r"\.get\s*\(.*http", "HTTP GET request"),
            (r"\.post\s*\(.*http", "HTTP POST request"),
            (r"fetch\s*\(", "Fetch API call"),
            (r"WebSocket\s*\(", "WebSocket connection"),
            (r"connect\s*\(.*ws://", "WebSocket connection"),
            (r"connect\s*\(.*wss://", "Secure WebSocket connection"),
        ];

        for (pattern, reason) in &call_patterns {
            if let Ok(re) = Regex::new(pattern) {
                for mat in re.find_iter(code) {
                    let line_num = code[..mat.start()].matches('\n').count() + 1;
                    // Avoid duplicate detection
                    let pat_str = mat.as_str().trim().to_string();
                    if !patterns.iter().any(|p: &DetectedPattern| {
                        p.pattern == pat_str && p.line_number == Some(line_num)
                    }) {
                        patterns.push(DetectedPattern {
                            category: "function_call".to_string(),
                            pattern: pat_str,
                            line_number: Some(line_num),
                            reason: reason.to_string(),
                        });
                    }
                }
            }
        }

        patterns
    }

    /// Detects URL literals in the code.
    fn detect_url_literals(code: &str) -> Vec<DetectedPattern> {
        let mut patterns = Vec::new();

        // Match http://, https://, ftp:// URLs
        let url_regex = r#"(https?|ftp)://[^\s"'`,;)}\]]+"#;
        if let Ok(re) = Regex::new(url_regex) {
            for mat in re.find_iter(code) {
                let line_num = code[..mat.start()].matches('\n').count() + 1;
                let url = mat.as_str();

                // Skip common false positives
                if url.contains("example.com") || url.contains("localhost") {
                    continue;
                }

                patterns.push(DetectedPattern {
                    category: "url_literal".to_string(),
                    pattern: url.to_string(),
                    line_number: Some(line_num),
                    reason: "URL literal indicates external resource access".to_string(),
                });
            }
        }

        patterns
    }

    /// Detects IP address literals in the code.
    fn detect_ip_addresses(code: &str) -> Vec<DetectedPattern> {
        let mut patterns = Vec::new();

        // Match IPv4 addresses (not 0.0.0.0 or 127.0.0.1 which are local)
        let ip_regex = r"\b(?:(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\.){3}(?:25[0-5]|2[0-4][0-9]|[01]?[0-9][0-9]?)\b";
        if let Ok(re) = Regex::new(ip_regex) {
            for mat in re.find_iter(code) {
                let line_num = code[..mat.start()].matches('\n').count() + 1;
                let ip = mat.as_str();

                // Skip local/loopback addresses
                if ip.starts_with("127.") || ip == "0.0.0.0" {
                    continue;
                }

                patterns.push(DetectedPattern {
                    category: "ip_address".to_string(),
                    pattern: ip.to_string(),
                    line_number: Some(line_num),
                    reason: "IP address literal indicates external network access".to_string(),
                });
            }
        }

        patterns
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_remote_access() {
        let code = r#"
import json
import math

def calculate(x, y):
    return math.sqrt(x**2 + y**2)

result = calculate(3, 4)
print(json.dumps({"result": result}))
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(!analysis.requires_approval);
        assert!(analysis.detected_patterns.is_empty());
    }

    #[test]
    fn test_http_import_detected() {
        let code = r#"
import requests

def get_data(url):
    return requests.get(url).json()
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(analysis.requires_approval);
        assert!(analysis
            .detected_patterns
            .iter()
            .any(|p| p.category == "import"));
    }

    #[test]
    fn test_urllib_import_detected() {
        let code = r#"
from urllib.request import urlopen

def fetch(url):
    with urlopen(url) as response:
        return response.read()
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(analysis.requires_approval);
        assert!(analysis
            .detected_patterns
            .iter()
            .any(|p| p.pattern.contains("urllib")));
    }

    #[test]
    fn test_socket_calls_detected() {
        let code = r#"
import socket

s = socket.socket()
s.connect(("example.com", 80))
s.send(b"GET / HTTP/1.1")
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(analysis.requires_approval);
        assert!(analysis
            .detected_patterns
            .iter()
            .any(|p| p.category == "function_call"));
    }

    #[test]
    fn test_url_literal_detected() {
        let code = r#"
import json

API_URL = "https://api.open-meteo.com/v1/forecast"
data = {"temp": 22}
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(analysis.requires_approval);
        assert!(analysis
            .detected_patterns
            .iter()
            .any(|p| p.category == "url_literal"));
    }

    #[test]
    fn test_ip_address_detected() {
        let code = r#"
SERVER_IP = "192.168.1.100"
PORT = 8080
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(analysis.requires_approval);
        assert!(analysis
            .detected_patterns
            .iter()
            .any(|p| p.category == "ip_address"));
    }

    #[test]
    fn test_local_ip_not_flagged() {
        let code = r#"
LOCAL_HOST = "127.0.0.1"
LOOPBACK = "127.0.0.1"
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(!analysis.requires_approval);
    }

    #[test]
    fn test_requests_get_detected() {
        let code = r#"
import requests

response = requests.get("https://api.example.com/data")
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(analysis.requires_approval);
        // Should have both import and function_call detections
        assert!(analysis.detected_patterns.len() >= 2);
    }

    #[test]
    fn test_httpx_detected() {
        let code = r#"
import httpx

async def fetch():
    async with httpx.AsyncClient() as client:
        return await client.get("https://example.com")
"#;
        let analysis = RemoteAccessAnalyzer::analyze_code(code);
        assert!(analysis.requires_approval);
    }
}
