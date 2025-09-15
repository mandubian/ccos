//! Minimal, test-only Prometheus-like metrics exposition.
//!
//! Feature-gated behind `metrics_exporter` to avoid pulling server deps.
//! We expose text in Prometheus exposition format from in-memory metrics.

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::ccos::causal_chain::CausalChain;

fn escape_label_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            _ => out.push(ch),
        }
    }
    out
}

/// Render a minimal Prometheus-like text format from capability and function metrics.
pub fn render_prometheus_text(chain: &CausalChain) -> String {
    let mut out = String::new();
    out.push_str("# HELP ccos_total_cost Total aggregated cost across all actions\n");
    out.push_str("# TYPE ccos_total_cost gauge\n");
    out.push_str(&format!("ccos_total_cost {}\n", chain.get_total_cost()));

    out.push_str("# HELP ccos_capability_calls_total Total calls per capability id\n");
    out.push_str("# TYPE ccos_capability_calls_total counter\n");
    out.push_str("# HELP ccos_capability_avg_duration_ms Average duration per capability in milliseconds\n");
    out.push_str("# TYPE ccos_capability_avg_duration_ms gauge\n");
    out.push_str("# HELP ccos_capability_duration_ms Duration histogram per capability in milliseconds\n");
    out.push_str("# TYPE ccos_capability_duration_ms histogram\n");

    // We don't have an iterator; derive from all actions for portability
    // and then query per-capability metrics by name seen.
    let mut seen_caps: std::collections::HashSet<String> = std::collections::HashSet::new();
    for a in chain.get_all_actions() {
        if let Some(fn_name) = &a.function_name {
            seen_caps.insert(fn_name.clone());
        }
    }
    // Prepare per-capability duration histograms from actions
    let mut cap_durations: std::collections::HashMap<String, Vec<u64>> = std::collections::HashMap::new();
    for a in chain.get_all_actions() {
        if a.action_type == crate::ccos::types::ActionType::CapabilityCall {
            if let (Some(fn_name), Some(d)) = (&a.function_name, a.duration_ms) {
                cap_durations.entry(fn_name.clone()).or_default().push(d);
            }
        }
    }

    for cap in seen_caps {
        let cap_lbl = escape_label_value(&cap);
        if let Some(m) = chain.get_capability_metrics(&cap) {
            out.push_str(&format!(
                "ccos_capability_calls_total{{id=\"{}\"}} {}\n",
                cap_lbl, m.total_calls
            ));
            out.push_str(&format!(
                "ccos_capability_avg_duration_ms{{id=\"{}\"}} {}\n",
                cap_lbl, m.average_duration_ms
            ));
        }
        // Histogram exposition (cumulative buckets)
        if let Some(durs) = cap_durations.get(&cap) {
            let buckets: [u64; 11] = [5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000];
            let mut counts: Vec<u64> = Vec::with_capacity(buckets.len());
            for b in buckets.iter() {
                let c = durs.iter().filter(|v| **v <= *b).count() as u64;
                counts.push(c);
            }
            let sum: u64 = durs.iter().copied().sum();
            let count_total = durs.len() as u64;
            for (i, b) in buckets.iter().enumerate() {
                out.push_str(&format!(
                    "ccos_capability_duration_ms_bucket{{id=\"{}\",le=\"{}\"}} {}\n",
                    cap_lbl, b, counts[i]
                ));
            }
            // +Inf bucket equals total count
            out.push_str(&format!(
                "ccos_capability_duration_ms_bucket{{id=\"{}\",le=\"+Inf\"}} {}\n",
                cap_lbl, count_total
            ));
            out.push_str(&format!(
                "ccos_capability_duration_ms_sum{{id=\"{}\"}} {}\n",
                cap_lbl, sum
            ));
            out.push_str(&format!(
                "ccos_capability_duration_ms_count{{id=\"{}\"}} {}\n",
                cap_lbl, count_total
            ));
        }
    }

    out.push_str("# HELP ccos_function_calls_total Total calls per function name\n");
    out.push_str("# TYPE ccos_function_calls_total counter\n");
    out.push_str("# HELP ccos_function_avg_duration_ms Average duration per function in milliseconds\n");
    out.push_str("# TYPE ccos_function_avg_duration_ms gauge\n");
    out.push_str("# HELP ccos_function_duration_ms Duration histogram per function in milliseconds\n");
    out.push_str("# TYPE ccos_function_duration_ms histogram\n");

    // As above, iterate names from actions then fetch metrics
    let mut seen_funcs: std::collections::HashSet<String> = std::collections::HashSet::new();
    for a in chain.get_all_actions() {
        if let Some(fn_name) = &a.function_name {
            seen_funcs.insert(fn_name.clone());
        }
    }
    // Prepare per-function duration histograms from actions
    let mut fn_durations: std::collections::HashMap<String, Vec<u64>> = std::collections::HashMap::new();
    for a in chain.get_all_actions() {
        if let (Some(fn_name), Some(d)) = (&a.function_name, a.duration_ms) {
            fn_durations.entry(fn_name.clone()).or_default().push(d);
        }
    }

    for f in seen_funcs {
        let f_lbl = escape_label_value(&f);
        if let Some(m) = chain.get_function_metrics(&f) {
            out.push_str(&format!(
                "ccos_function_calls_total{{name=\"{}\"}} {}\n",
                f_lbl, m.total_calls
            ));
            out.push_str(&format!(
                "ccos_function_avg_duration_ms{{name=\"{}\"}} {}\n",
                f_lbl, m.average_duration_ms
            ));
        }
        // Histogram exposition
        if let Some(durs) = fn_durations.get(&f) {
            let buckets: [u64; 11] = [5, 10, 25, 50, 100, 250, 500, 1000, 2500, 5000, 10000];
            let mut counts: Vec<u64> = Vec::with_capacity(buckets.len());
            for b in buckets.iter() {
                let c = durs.iter().filter(|v| **v <= *b).count() as u64;
                counts.push(c);
            }
            let sum: u64 = durs.iter().copied().sum();
            let count_total = durs.len() as u64;
            for (i, b) in buckets.iter().enumerate() {
                out.push_str(&format!(
                    "ccos_function_duration_ms_bucket{{name=\"{}\",le=\"{}\"}} {}\n",
                    f_lbl, b, counts[i]
                ));
            }
            out.push_str(&format!(
                "ccos_function_duration_ms_bucket{{name=\"{}\",le=\"+Inf\"}} {}\n",
                f_lbl, count_total
            ));
            out.push_str(&format!(
                "ccos_function_duration_ms_sum{{name=\"{}\"}} {}\n",
                f_lbl, sum
            ));
            out.push_str(&format!(
                "ccos_function_duration_ms_count{{name=\"{}\"}} {}\n",
                f_lbl, count_total
            ));
        }
    }

    if !out.ends_with('\n') { out.push('\n'); }
    out
}

/// Start a tiny, blocking HTTP server serving `/metrics` with the text.
/// Returns a join handle; stop by dropping the listener thread (tests should run it briefly).
pub fn start_metrics_server(chain: Arc<Mutex<CausalChain>>, addr: &str) -> std::io::Result<thread::JoinHandle<()>> {
    let listener = TcpListener::bind(addr)?;
    let handle = thread::spawn(move || {
        // Handle a single request then exit (test-friendly)
        if let Ok((mut stream, _addr)) = listener.accept() {
            let _ = handle_client(&mut stream, &chain);
        }
    });
    Ok(handle)
}

fn handle_client(stream: &mut TcpStream, chain: &Arc<Mutex<CausalChain>>) -> std::io::Result<()> {
    let mut buf = [0u8; 512];
    let _ = stream.read(&mut buf)?;
    // Minimal parse: if request starts with GET /metrics
    let req = String::from_utf8_lossy(&buf);
    let is_metrics = req.starts_with("GET /metrics ");
    let (status, body) = if is_metrics {
        let text = if let Ok(guard) = chain.lock() { render_prometheus_text(&*guard) } else { String::new() };
        ("200 OK", text)
    } else {
        ("404 Not Found", String::from("not found"))
    };
    let resp = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        status,
        body.len(),
        body
    );
    stream.write_all(resp.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use crate::ccos::host::RuntimeHost;
    use crate::ccos::capability_marketplace::CapabilityMarketplace;
    use crate::runtime::security::RuntimeContext;
    use crate::runtime::host_interface::HostInterface;
    use crate::runtime::values::Value;

    #[test]
    fn test_render_and_server_smoke() {
        // Build a tiny environment: chain + host + minimal marketplace
        let chain = Arc::new(Mutex::new(CausalChain::new().unwrap()));
    let registry = Arc::new(tokio::sync::RwLock::new(crate::ccos::capabilities::registry::CapabilityRegistry::new()));
        let marketplace = Arc::new(CapabilityMarketplace::new(registry));
        let host = RuntimeHost::new(chain.clone(), marketplace, RuntimeContext::full());
        // Execute a capability that likely doesn't exist; ignore error but ensure chain has at least one action via delegation event
        let _ = host.record_delegation_event_for_test("intent-x", "approved", std::collections::HashMap::new());
        // Render text
        let text = {
            let c = chain.lock().unwrap();
            render_prometheus_text(&*c)
        };
    assert!(text.contains("ccos_total_cost"));
    // With at least one event recorded, histogram HELP lines should be present
    assert!(text.contains("ccos_function_duration_ms"));

        // Start server on ephemeral port
        let addr = "127.0.0.1:0";
        let listener = std::net::TcpListener::bind(addr).unwrap();
        let local_addr = listener.local_addr().unwrap();
        drop(listener); // free so our server can bind
        let handle = start_metrics_server(chain.clone(), &local_addr.to_string()).unwrap();
        // Give it a moment
        std::thread::sleep(Duration::from_millis(50));
        // Fetch metrics
        let mut stream = std::net::TcpStream::connect(local_addr).unwrap();
        stream.write_all(b"GET /metrics HTTP/1.1\r\nHost: localhost\r\n\r\n").unwrap();
        let mut resp = String::new();
        stream.read_to_string(&mut resp).unwrap();
        assert!(resp.contains("200 OK"));
        assert!(resp.contains("ccos_total_cost"));
        // Cleanup
        handle.thread().unpark();
    }
}
