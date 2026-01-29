use crate::sandbox::network_proxy::NetworkRequest;
use crate::secrets::SecretStore;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct SecretMount {
    pub mount_point: String,
    pub files: HashMap<String, String>,
}

#[derive(Clone)]
pub struct SecretInjector {
    secret_store: Arc<SecretStore>,
}

impl SecretInjector {
    pub fn new(secret_store: Arc<SecretStore>) -> Self {
        Self { secret_store }
    }

    pub fn inject_for_sandbox(
        &self,
        _capability_id: &str,
        required_secrets: &[String],
    ) -> RuntimeResult<SecretMount> {
        let mount_point = "/run/secrets".to_string();
        let mut files = HashMap::new();

        for secret_name in required_secrets {
            let value = self.secret_store.get(secret_name).ok_or_else(|| {
                RuntimeError::Generic(format!("Secret not available: {}", secret_name))
            })?;
            files.insert(secret_name.clone(), value);
        }

        Ok(SecretMount { mount_point, files })
    }

    pub fn inject_headers(
        &self,
        mut request: NetworkRequest,
        _capability_id: &str,
        required_secrets: &[String],
    ) -> RuntimeResult<NetworkRequest> {
        for secret_name in required_secrets {
            if let Some(value) = self.secret_store.get(secret_name) {
                request
                    .headers
                    .insert(format!("X-Secret-{}", secret_name), value);
            }
        }

        Ok(request)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::utils::fs::get_workspace_root;
    use std::fs;

    struct SecretsGuard {
        path: std::path::PathBuf,
        original: Option<String>,
    }

    impl SecretsGuard {
        fn new(path: std::path::PathBuf) -> Self {
            let original = fs::read_to_string(&path).ok();
            Self { path, original }
        }
    }

    impl Drop for SecretsGuard {
        fn drop(&mut self) {
            if let Some(original) = self.original.as_ref() {
                let _ = fs::write(&self.path, original);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    #[test]
    fn test_inject_headers_from_secret_store() {
        let root = get_workspace_root();
        let secrets_dir = root.join(".ccos");
        let secrets_path = secrets_dir.join("secrets.toml");
        let _guard = SecretsGuard::new(secrets_path.clone());

        let _ = fs::create_dir_all(&secrets_dir);
        let content = r#"[secrets]
TEST_SECRET = "dummy"
"#;
        fs::write(&secrets_path, content).expect("write secrets.toml");

        let store = SecretStore::new(Some(root)).expect("secret store");
        let injector = SecretInjector::new(Arc::new(store));

        let request = NetworkRequest {
            method: "GET".to_string(),
            url: "https://example.com".to_string(),
            host: "example.com".to_string(),
            port: 443,
            headers: HashMap::new(),
            body: None,
        };

        let enriched = injector
            .inject_headers(request, "ccos.network.http-fetch", &["TEST_SECRET".to_string()])
            .expect("inject headers");

        assert_eq!(
            enriched.headers.get("X-Secret-TEST_SECRET"),
            Some(&"dummy".to_string())
        );
    }
}
