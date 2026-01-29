use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimits {
    pub cpu_shares: u32,
    pub memory_mb: u64,
    pub timeout_ms: u64,
    pub network_egress_bytes: u64,
}

impl Default for ResourceLimits {
    fn default() -> Self {
        Self {
            cpu_shares: 0,
            memory_mb: 0,
            timeout_ms: 0,
            network_egress_bytes: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ResourceMetrics {
    pub cpu_time_ms: u64,
    pub memory_peak_mb: u64,
    pub wall_clock_ms: u64,
    pub network_egress_bytes: u64,
    pub storage_write_bytes: u64,
}

impl ResourceMetrics {
    pub fn from_microvm_metadata(
        metadata: &rtfs::runtime::microvm::ExecutionMetadata,
    ) -> Self {
        let network_egress_bytes = metadata
            .network_requests
            .iter()
            .map(|req| req.bytes_sent)
            .sum();

        let storage_write_bytes = metadata
            .file_operations
            .iter()
            .filter(|op| is_write_operation(&op.operation))
            .map(|op| op.bytes_processed)
            .sum();

        Self {
            cpu_time_ms: duration_to_ms(metadata.cpu_time),
            memory_peak_mb: metadata.memory_used_mb,
            wall_clock_ms: duration_to_ms(metadata.duration),
            network_egress_bytes,
            storage_write_bytes,
        }
    }
}

fn duration_to_ms(duration: Duration) -> u64 {
    duration.as_millis() as u64
}

fn is_write_operation(operation: &str) -> bool {
    matches!(operation, "write" | "create" | "append" | "truncate")
        || operation.starts_with("write-")
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtfs::runtime::microvm::{ExecutionMetadata, FileOperation, NetworkRequest};

    #[test]
    fn test_metrics_from_microvm_metadata() {
        let metadata = ExecutionMetadata {
            duration: Duration::from_millis(150),
            memory_used_mb: 64,
            cpu_time: Duration::from_millis(42),
            network_requests: vec![
                NetworkRequest {
                    url: "https://example.com".to_string(),
                    method: "GET".to_string(),
                    status_code: Some(200),
                    bytes_sent: 120,
                    bytes_received: 640,
                },
                NetworkRequest {
                    url: "https://example.net".to_string(),
                    method: "POST".to_string(),
                    status_code: Some(201),
                    bytes_sent: 80,
                    bytes_received: 512,
                },
            ],
            file_operations: vec![
                FileOperation {
                    path: "/tmp/out.txt".to_string(),
                    operation: "write".to_string(),
                    bytes_processed: 256,
                },
                FileOperation {
                    path: "/tmp/in.txt".to_string(),
                    operation: "read".to_string(),
                    bytes_processed: 512,
                },
                FileOperation {
                    path: "/tmp/append.txt".to_string(),
                    operation: "append".to_string(),
                    bytes_processed: 128,
                },
            ],
        };

        let metrics = ResourceMetrics::from_microvm_metadata(&metadata);
        assert_eq!(metrics.cpu_time_ms, 42);
        assert_eq!(metrics.memory_peak_mb, 64);
        assert_eq!(metrics.wall_clock_ms, 150);
        assert_eq!(metrics.network_egress_bytes, 200);
        assert_eq!(metrics.storage_write_bytes, 384);
    }
}
