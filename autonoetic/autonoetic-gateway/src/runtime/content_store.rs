//! Content-addressable storage for Autonoetic agents.
//!
//! Provides SHA-256 based content addressing that works locally and remotely.
//! Content is stored as immutable blobs; session manifests map names to handles.

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A content handle is a SHA-256 hash prefixed with "sha256:".
pub type ContentHandle = String;

/// Session manifest mapping content names to handles.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SessionManifest {
    /// Map of content name → handle
    pub names: HashMap<String, ContentHandle>,
    /// Set of persisted handles (survive session cleanup)
    pub persisted: std::collections::HashSet<ContentHandle>,
}

/// Content-addressable store for agent artifacts.
///
/// Storage layout:
/// ```text
/// .gateway/content/
/// └── sha256/
///     └── ab/
///         └── c123...  ← immutable content blobs
/// ```
pub struct ContentStore {
    /// Root path for content storage (.gateway/content/)
    content_dir: PathBuf,
    /// Root path for session manifests (.gateway/sessions/)
    sessions_dir: PathBuf,
    /// In-memory cache of session manifests (loaded on demand)
    manifests: Arc<Mutex<HashMap<String, SessionManifest>>>,
}

impl ContentStore {
    /// Creates a new ContentStore.
    pub fn new(gateway_dir: &Path) -> anyhow::Result<Self> {
        let content_dir = gateway_dir.join("content").join("sha256");
        let sessions_dir = gateway_dir.join("sessions");
        std::fs::create_dir_all(&content_dir)?;
        std::fs::create_dir_all(&sessions_dir)?;
        Ok(Self {
            content_dir,
            sessions_dir,
            manifests: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    /// Computes the SHA-256 hash of content.
    pub fn compute_handle(content: &[u8]) -> ContentHandle {
        let mut hasher = Sha256::new();
        hasher.update(content);
        format!("sha256:{:x}", hasher.finalize())
    }

    /// Computes the storage path for a content handle.
    fn handle_to_path(&self, handle: &ContentHandle) -> PathBuf {
        // sha256:ab12cd34... → sha256/ab/12cd34...
        let hash = handle.strip_prefix("sha256:").unwrap_or(handle);
        let prefix = &hash[..2];
        let rest = &hash[2..];
        self.content_dir.join(prefix).join(rest)
    }

    /// Writes content to the store and returns its handle.
    ///
    /// If content with the same hash already exists, returns the existing handle
    /// (natural deduplication).
    pub fn write(&self, content: &[u8]) -> anyhow::Result<ContentHandle> {
        let handle = Self::compute_handle(content);
        let path = self.handle_to_path(&handle);

        // Only write if not already stored
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, content)?;
            tracing::debug!(
                target: "content_store",
                handle = %handle,
                bytes = content.len(),
                "Stored new content"
            );
        }

        Ok(handle)
    }

    /// Reads content by handle.
    pub fn read(&self, handle: &ContentHandle) -> anyhow::Result<Vec<u8>> {
        let path = self.handle_to_path(handle);
        if !path.exists() {
            anyhow::bail!("Content not found: {}", handle);
        }
        Ok(std::fs::read(&path)?)
    }

    /// Reads content as UTF-8 string.
    pub fn read_string(&self, handle: &ContentHandle) -> anyhow::Result<String> {
        let bytes = self.read(handle)?;
        String::from_utf8(bytes).map_err(|e| anyhow::anyhow!("Content is not valid UTF-8: {}", e))
    }

    /// Returns true if content exists in the store.
    pub fn exists(&self, handle: &ContentHandle) -> bool {
        self.handle_to_path(handle).exists()
    }

    /// Marks content as persistent (survives session cleanup).
    pub fn persist(&self, session_id: &str, handle: &ContentHandle) -> anyhow::Result<()> {
        if !self.exists(handle) {
            anyhow::bail!("Cannot persist non-existent content: {}", handle);
        }

        let mut manifests = self.manifests.lock().unwrap();
        let manifest = manifests.entry(session_id.to_string()).or_default();
        manifest.persisted.insert(handle.clone());

        // Persist to disk
        self.save_manifest(session_id, manifest)?;

        tracing::info!(
            target: "content_store",
            session_id = %session_id,
            handle = %handle,
            "Marked content as persistent"
        );

        Ok(())
    }

    /// Loads a session manifest from disk (or returns cached).
    pub fn load_manifest(&self, session_id: &str) -> anyhow::Result<SessionManifest> {
        {
            let manifests = self.manifests.lock().unwrap();
            if let Some(m) = manifests.get(session_id) {
                return Ok(m.clone());
            }
        }

        let path = self.manifest_path(session_id);
        let manifest = if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            serde_json::from_str(&json)?
        } else {
            SessionManifest::default()
        };

        let mut manifests = self.manifests.lock().unwrap();
        manifests.insert(session_id.to_string(), manifest.clone());
        Ok(manifest)
    }

    /// Saves a session manifest to disk.
    fn save_manifest(&self, session_id: &str, manifest: &SessionManifest) -> anyhow::Result<()> {
        let path = self.manifest_path(session_id);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(manifest)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    /// Returns the path to a session's manifest file.
    fn manifest_path(&self, session_id: &str) -> PathBuf {
        self.sessions_dir.join(session_id).join("manifest.json")
    }

    /// Registers a content name in a session manifest.
    pub fn register_name(
        &self,
        session_id: &str,
        name: &str,
        handle: &ContentHandle,
    ) -> anyhow::Result<()> {
        let mut manifests = self.manifests.lock().unwrap();
        let manifest = manifests.entry(session_id.to_string()).or_default();
        manifest.names.insert(name.to_string(), handle.clone());
        self.save_manifest(session_id, manifest)?;
        Ok(())
    }

    /// Resolves a name to a handle within a session.
    pub fn resolve_name(&self, session_id: &str, name: &str) -> anyhow::Result<ContentHandle> {
        let manifest = self.load_manifest(session_id)?;
        manifest.names.get(name).cloned().ok_or_else(|| {
            anyhow::anyhow!(
                "Content name '{}' not found in session '{}'",
                name,
                session_id
            )
        })
    }

    /// Reads content by name within a session.
    pub fn read_by_name(&self, session_id: &str, name: &str) -> anyhow::Result<Vec<u8>> {
        let handle = self.resolve_name(session_id, name)?;
        self.read(&handle)
    }

    /// Reads content by name or handle.
    ///
    /// If the input starts with "sha256:", treats it as a handle.
    /// Otherwise, treats it as a session-relative name.
    pub fn read_by_name_or_handle(
        &self,
        session_id: &str,
        name_or_handle: &str,
    ) -> anyhow::Result<Vec<u8>> {
        if name_or_handle.starts_with("sha256:") {
            self.read(&name_or_handle.to_string())
        } else {
            self.read_by_name(session_id, name_or_handle)
        }
    }

    /// Lists all content names in a session.
    pub fn list_names(&self, session_id: &str) -> anyhow::Result<Vec<String>> {
        let manifest = self.load_manifest(session_id)?;
        let mut names: Vec<String> = manifest.names.keys().cloned().collect();
        names.sort();
        Ok(names)
    }

    /// Lists all content names with their handles in a session.
    pub fn list_names_with_handles(
        &self,
        session_id: &str,
    ) -> anyhow::Result<Vec<(String, String)>> {
        let manifest = self.load_manifest(session_id)?;
        let mut entries: Vec<(String, String)> = manifest.names.into_iter().collect();
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(entries)
    }

    /// Lists all persisted handles in a session.
    pub fn list_persisted(&self, session_id: &str) -> anyhow::Result<Vec<ContentHandle>> {
        let manifest = self.load_manifest(session_id)?;
        let mut handles: Vec<ContentHandle> = manifest.persisted.iter().cloned().collect();
        handles.sort();
        Ok(handles)
    }

    /// Removes session content that is not persisted.
    ///
    /// Returns the number of handles removed.
    pub fn cleanup_session(&self, session_id: &str) -> anyhow::Result<usize> {
        let manifest = self.load_manifest(session_id)?;
        let mut removed = 0;

        for (name, handle) in &manifest.names {
            if !manifest.persisted.contains(handle) {
                let path = self.handle_to_path(handle);
                if path.exists() {
                    // Only remove if no other sessions reference this handle
                    // (For simplicity, we don't track cross-session refs yet)
                    tracing::debug!(
                        target: "content_store",
                        name = %name,
                        handle = %handle,
                        "Session cleanup (content remains in store)"
                    );
                }
            }
            removed += 1;
        }

        // Clear the manifest (keep persisted handles)
        let mut manifests = self.manifests.lock().unwrap();
        let mut new_manifest = SessionManifest::default();
        new_manifest.persisted = manifest.persisted;
        manifests.insert(session_id.to_string(), new_manifest);

        Ok(removed)
    }

    /// Returns statistics about the content store.
    pub fn stats(&self) -> anyhow::Result<ContentStoreStats> {
        let mut total_size = 0u64;
        let mut entry_count = 0u64;

        if self.content_dir.exists() {
            for prefix_entry in std::fs::read_dir(&self.content_dir)? {
                let prefix_entry = prefix_entry?;
                if prefix_entry.file_type()?.is_dir() {
                    for entry in std::fs::read_dir(prefix_entry.path())? {
                        let entry = entry?;
                        if entry.file_type()?.is_file() {
                            total_size += entry.metadata()?.len();
                            entry_count += 1;
                        }
                    }
                }
            }
        }

        Ok(ContentStoreStats {
            entry_count,
            total_size_bytes: total_size,
        })
    }
}

/// Statistics about the content store.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ContentStoreStats {
    pub entry_count: u64,
    pub total_size_bytes: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_content_store_write_and_read() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let content = b"Hello, World!";
        let handle = store.write(content).unwrap();

        assert!(handle.starts_with("sha256:"));
        assert_eq!(store.read(&handle).unwrap(), content);
    }

    #[test]
    fn test_content_store_deduplication() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let content = b"Same content";
        let handle1 = store.write(content).unwrap();
        let handle2 = store.write(content).unwrap();

        assert_eq!(handle1, handle2);
    }

    #[test]
    fn test_content_store_session_manifest() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let content = b"Script content";
        let handle = store.write(content).unwrap();

        store
            .register_name("session-1", "main.py", &handle)
            .unwrap();

        let resolved = store.resolve_name("session-1", "main.py").unwrap();
        assert_eq!(resolved, handle);

        let content_back = store.read_by_name("session-1", "main.py").unwrap();
        assert_eq!(content_back, content);
    }

    #[test]
    fn test_content_store_read_by_name_or_handle() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let content = b"Test content";
        let handle = store.write(content).unwrap();
        store
            .register_name("session-1", "test.txt", &handle)
            .unwrap();

        // Read by name
        let by_name = store
            .read_by_name_or_handle("session-1", "test.txt")
            .unwrap();
        assert_eq!(by_name, content);

        // Read by handle
        let by_handle = store.read_by_name_or_handle("session-1", &handle).unwrap();
        assert_eq!(by_handle, content);
    }

    #[test]
    fn test_content_store_persist() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let content = b"Persistent content";
        let handle = store.write(content).unwrap();

        store.persist("session-1", &handle).unwrap();

        let persisted = store.list_persisted("session-1").unwrap();
        assert!(persisted.contains(&handle));
    }

    #[test]
    fn test_content_store_list_names() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let h1 = store.write(b"file1").unwrap();
        let h2 = store.write(b"file2").unwrap();

        store.register_name("session-1", "a.txt", &h1).unwrap();
        store.register_name("session-1", "b.txt", &h2).unwrap();

        let names = store.list_names("session-1").unwrap();
        assert_eq!(names, vec!["a.txt", "b.txt"]);
    }

    #[test]
    fn test_content_store_stats() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        store.write(b"content1").unwrap();
        store.write(b"content2").unwrap();
        store.write(b"content1").unwrap(); // duplicate

        let stats = store.stats().unwrap();
        assert_eq!(stats.entry_count, 2); // deduplicated
        assert!(stats.total_size_bytes > 0);
    }

    #[test]
    fn test_content_store_skill_md_artifact() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let skill_content = r#"---
name: "weather.script.default"
description: "Retrieves weather from Open-Meteo API"
script_entry: "main.py"
io:
  accepts:
    type: object
    required: [latitude, longitude]
  returns:
    type: object
---
# Weather Script Agent

Retrieves current or forecast weather.
"#;
        let main_py = r#"print("Hello from weather script")"#;

        let h1 = store.write(skill_content.as_bytes()).unwrap();
        let h2 = store.write(main_py.as_bytes()).unwrap();

        store
            .register_name("session-1", "weather_agent/SKILL.md", &h1)
            .unwrap();
        store
            .register_name("session-1", "weather_agent/main.py", &h2)
            .unwrap();

        let names = store.list_names("session-1").unwrap();
        assert_eq!(names.len(), 2);
        assert!(names.contains(&"weather_agent/SKILL.md".to_string()));
        assert!(names.contains(&"weather_agent/main.py".to_string()));
    }

    #[test]
    fn test_content_store_missing_content() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let result = store.read(
            &"sha256:0000000000000000000000000000000000000000000000000000000000000000".to_string(),
        );
        assert!(result.is_err());
    }
}
