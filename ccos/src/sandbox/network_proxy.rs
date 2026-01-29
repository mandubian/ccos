use crate::sandbox::secret_injection::SecretInjector;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct NetworkRequest {
    pub method: String,
    pub url: String,
    pub host: String,
    pub port: u16,
    pub headers: HashMap<String, String>,
    pub body: Option<Vec<u8>>,
}

#[derive(Debug, Clone)]
pub struct NetworkResponse {
    pub status: u16,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

#[derive(Clone)]
pub struct NetworkProxy {
    allowed_hosts: HashSet<String>,
    allowed_ports: HashSet<u16>,
    secret_injector: Arc<SecretInjector>,
}

impl NetworkProxy {
    pub fn new(
        allowed_hosts: HashSet<String>,
        allowed_ports: HashSet<u16>,
        secret_injector: Arc<SecretInjector>,
    ) -> Self {
        Self {
            allowed_hosts,
            allowed_ports,
            secret_injector,
        }
    }

    pub async fn forward_request(
        &self,
        request: NetworkRequest,
        capability_id: &str,
        required_secrets: &[String],
    ) -> RuntimeResult<NetworkResponse> {
        if !self.allowed_hosts.is_empty() && !self.allowed_hosts.contains(&request.host) {
            return Err(RuntimeError::Generic(format!(
                "Host not allowed: {}",
                request.host
            )));
        }
        if !self.allowed_ports.is_empty() && !self.allowed_ports.contains(&request.port) {
            return Err(RuntimeError::Generic(format!(
                "Port not allowed: {}",
                request.port
            )));
        }

        let enriched = self
            .secret_injector
            .inject_headers(request, capability_id, required_secrets)?;

        let method = enriched.method.parse().map_err(|e| {
            RuntimeError::Generic(format!("Invalid HTTP method {}: {}", enriched.method, e))
        })?;

        let client = reqwest::Client::new();
        let mut builder = client.request(method, &enriched.url);
        for (key, value) in enriched.headers.iter() {
            builder = builder.header(key, value);
        }
        if let Some(body) = enriched.body {
            builder = builder.body(body);
        }

        let response = builder
            .send()
            .await
            .map_err(|e| RuntimeError::NetworkError(e.to_string()))?;

        let status = response.status().as_u16();
        let mut headers = HashMap::new();
        for (key, value) in response.headers().iter() {
            if let Ok(value_str) = value.to_str() {
                headers.insert(key.to_string(), value_str.to_string());
            }
        }
        let body = response
            .bytes()
            .await
            .map_err(|e| RuntimeError::NetworkError(e.to_string()))?
            .to_vec();

        Ok(NetworkResponse {
            status,
            headers,
            body,
        })
    }
}
