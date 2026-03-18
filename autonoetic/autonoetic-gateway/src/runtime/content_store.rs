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
    /// Map of short alias (8 hex chars) → full handle for LLM-friendly lookup
    pub aliases: HashMap<String, ContentHandle>,
    /// Parent session ID for hierarchical content visibility.
    /// If set, content in this session is visible to the parent.
    #[serde(default)]
    pub parent_session_id: Option<String>,
}

/// Short alias prefix length (8 hex chars = 32 bits, collision probability < 1/4B)
pub const SHORT_ALIAS_LEN: usize = 8;

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

    /// Extracts short alias from a handle (first 8 hex chars after "sha256:").
    /// LLMs can reliably copy/reproduce this shorter identifier.
    pub fn handle_to_short_alias(handle: &ContentHandle) -> String {
        handle
            .strip_prefix("sha256:")
            .and_then(|h| h.get(..SHORT_ALIAS_LEN))
            .unwrap_or(handle)
            .to_string()
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
        if !manifests.contains_key(session_id) {
            let disk_manifest = self.load_manifest_from_disk_uncached(session_id)?;
            manifests.insert(session_id.to_string(), disk_manifest);
        }
        let manifest = manifests
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("Failed to load manifest for session '{}'", session_id))?;
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

        let manifest = self.load_manifest_from_disk_uncached(session_id)?;

        let mut manifests = self.manifests.lock().unwrap();
        manifests.insert(session_id.to_string(), manifest.clone());
        Ok(manifest)
    }

    fn load_manifest_from_disk_uncached(&self, session_id: &str) -> anyhow::Result<SessionManifest> {
        let path = self.manifest_path(session_id);
        if path.exists() {
            let json = std::fs::read_to_string(&path)?;
            let manifest: SessionManifest = serde_json::from_str(&json)?;
            Ok(manifest)
        } else {
            Ok(SessionManifest::default())
        }
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

    /// Registers a content name and short alias in a session manifest.
    /// The short alias (8 hex chars) is LLM-friendly for easy retrieval.
    pub fn register_name(
        &self,
        session_id: &str,
        name: &str,
        handle: &ContentHandle,
    ) -> anyhow::Result<()> {
        let mut manifests = self.manifests.lock().unwrap();
        if !manifests.contains_key(session_id) {
            let disk_manifest = self.load_manifest_from_disk_uncached(session_id)?;
            manifests.insert(session_id.to_string(), disk_manifest);
        }
        let manifest = manifests
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("Failed to load manifest for session '{}'", session_id))?;
        manifest.names.insert(name.to_string(), handle.clone());

        // Also register the short alias for LLM-friendly lookup
        let short_alias = Self::handle_to_short_alias(handle);
        manifest.aliases.insert(short_alias, handle.clone());

        self.save_manifest(session_id, manifest)?;
        Ok(())
    }

    /// Sets the parent session ID for hierarchical content visibility.
    /// Content written to this session will be visible to the parent.
    pub fn set_parent_session(
        &self,
        session_id: &str,
        parent_session_id: &str,
    ) -> anyhow::Result<()> {
        let mut manifests = self.manifests.lock().unwrap();
        if !manifests.contains_key(session_id) {
            let disk_manifest = self.load_manifest_from_disk_uncached(session_id)?;
            manifests.insert(session_id.to_string(), disk_manifest);
        }
        let manifest = manifests
            .get_mut(session_id)
            .ok_or_else(|| anyhow::anyhow!("Failed to load manifest for session '{}'", session_id))?;
        manifest.parent_session_id = Some(parent_session_id.to_string());
        self.save_manifest(session_id, manifest)?;
        Ok(())
    }

    /// Registers content in both the current session AND its parent session.
    /// This enables hierarchical content visibility where parent agents can
    /// read content written by their child agents.
    ///
    /// For example, if coder (session: "demo-session/coder-abc123") writes "weather.py":
    /// - It's registered in the child session under "weather.py"
    /// - It's registered in the parent session under "weather.py" (flat, last-writer-wins)
    /// - It's also registered in the parent under "demo-session/coder-abc123/weather.py"
    ///   for explicit disambiguation when multiple children write the same name.
    ///
    /// This allows the planner to do content.read("weather.py") after delegating to the coder.
    pub fn register_name_in_hierarchy(
        &self,
        session_id: &str,
        name: &str,
        handle: &ContentHandle,
    ) -> anyhow::Result<()> {
        // Always register in current session
        self.register_name(session_id, name, handle)?;

        // Load from disk — ContentStore is re-instantiated on every tool call so the
        // in-memory manifests cache is always empty; we must go to disk here.
        let manifest = self.load_manifest(session_id)?;

        if let Some(parent_session_id) = manifest.parent_session_id {
            // Register flat name in parent (last-writer-wins).
            // This lets the planner do content.read("weather_fetcher.py") naturally.
            self.register_name(&parent_session_id, name, handle)?;

            // Also register the namespaced name so the parent can disambiguate when
            // multiple children wrote files with the same flat name.
            let namespaced_name = format!("{}/{}", session_id, name);
            self.register_name(&parent_session_id, &namespaced_name, handle)?;

            tracing::debug!(
                target: "content_store",
                session_id = %session_id,
                parent_session_id = %parent_session_id,
                name = %name,
                namespaced_name = %namespaced_name,
                "Registered content in parent session (flat + namespaced)"
            );
        }

        Ok(())
    }

    /// Resolves a name by walking up the delegation chain.
    ///
    /// Resolution rules:
    /// 1. Try the flat name in the caller's own session.
    /// 2. Walk up to parent sessions, but only match the **namespaced** form
    ///    `"{child_session}/{name}"` — NOT flat names in the parent.
    ///
    /// This preserves sibling isolation: flat names registered in the parent by
    /// `register_name_in_hierarchy` are only visible to the parent itself, not to
    /// sibling child agents that also happen to walk up to the same parent.
    pub fn resolve_name_hierarchical(
        &self,
        session_id: &str,
        name: &str,
    ) -> anyhow::Result<ContentHandle> {
        // 1. Try the caller's own session directly (flat name).
        if let Ok(handle) = self.resolve_name(session_id, name) {
            return Ok(handle);
        }

        // 2. Walk up the delegation chain looking for the namespaced form only.
        //    A child searching for "weather.py" looks for
        //    "{child_session}/weather.py" in the parent's manifest.
        //    This means siblings can't accidentally access each other's work
        //    through the shared parent namespace.
        let mut current_session = session_id.to_string();
        loop {
            let manifest = self.load_manifest(&current_session)?;

            if let Some(parent_id) = manifest.parent_session_id.clone() {
                let namespaced_name = format!("{}/{}", current_session, name);
                let parent_manifest = self.load_manifest(&parent_id)?;
                if let Some(handle) = parent_manifest.names.get(&namespaced_name) {
                    return Ok(handle.clone());
                }
                current_session = parent_id;
            } else {
                break;
            }
        }

        Err(anyhow::anyhow!(
            "Content name '{}' not found in session '{}' or parent sessions",
            name,
            session_id
        ))
    }

    /// Reads content by name, handle, or short alias with hierarchical lookup.
    /// If not found in current session, walks up the parent chain.
    pub fn read_by_name_or_handle_hierarchical(
        &self,
        session_id: &str,
        name_or_handle: &str,
    ) -> anyhow::Result<Vec<u8>> {
        if name_or_handle.starts_with("sha256:") {
            // Full handle - read directly from content store
            self.read(&name_or_handle.to_string())
        } else if name_or_handle.len() == SHORT_ALIAS_LEN
            && name_or_handle.chars().all(|c| c.is_ascii_hexdigit())
        {
            // Short alias - try current session first, then walk up parent chain
            self.resolve_alias_hierarchical(session_id, name_or_handle)
                .and_then(|handle| self.read(&handle))
        } else {
            // Name lookup - use hierarchical resolution
            self.resolve_name_hierarchical(session_id, name_or_handle)
                .and_then(|handle| self.read(&handle))
        }
    }

    /// Resolves an alias by walking up the delegation chain.
    fn resolve_alias_hierarchical(
        &self,
        session_id: &str,
        alias: &str,
    ) -> anyhow::Result<ContentHandle> {
        let mut current_session = session_id.to_string();

        loop {
            let manifest = self.load_manifest(&current_session)?;

            // Check alias in current session
            if let Some(handle) = manifest.aliases.get(alias) {
                return Ok(handle.clone());
            }

            // Walk up to parent
            if let Some(parent_id) = manifest.parent_session_id.clone() {
                current_session = parent_id;
            } else {
                break;
            }
        }

        Err(anyhow::anyhow!(
            "Content alias '{}' not found in session '{}' or parent sessions",
            alias,
            session_id
        ))
    }

    /// Returns the short alias for a handle (for inclusion in API responses).
    pub fn get_short_alias(handle: &ContentHandle) -> String {
        Self::handle_to_short_alias(handle)
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

    /// Reads content by name, handle, or short alias.
    ///
    /// Resolution order:
    /// 1. If starts with "sha256:" → full handle lookup
    /// 2. If 8 hex chars → short alias lookup
    /// 3. Otherwise → session name lookup
    pub fn read_by_name_or_handle(
        &self,
        session_id: &str,
        name_or_handle: &str,
    ) -> anyhow::Result<Vec<u8>> {
        if name_or_handle.starts_with("sha256:") {
            // Full handle
            self.read(&name_or_handle.to_string())
        } else if name_or_handle.len() == SHORT_ALIAS_LEN
            && name_or_handle.chars().all(|c| c.is_ascii_hexdigit())
        {
            // Short alias (8 hex chars) - lookup in session manifest
            // Use load_manifest to ensure we load from disk if not in memory cache
            let manifest = self.load_manifest(session_id)?;
            let handle = manifest
                .aliases
                .get(name_or_handle)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Content alias '{}' not found in session '{}'",
                        name_or_handle,
                        session_id
                    )
                })?;
            self.read(&handle)
        } else {
            // Name lookup
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
    fn test_hierarchical_content_visibility() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        // Set up parent-child relationship
        // Parent session: "planner-session"
        // Child session: "planner-session/coder-abc123"
        let parent_session = "planner-session";
        let child_session = "planner-session/coder-abc123";

        store.set_parent_session(child_session, parent_session).unwrap();

        // Child writes content using hierarchical registration
        let content = b"print('Hello from coder')";
        let handle = store.write(content).unwrap();
        store.register_name_in_hierarchy(child_session, "weather.py", &handle).unwrap();

        // Child can read its own content directly by flat name
        let child_read = store.read_by_name_or_handle_hierarchical(child_session, "weather.py").unwrap();
        assert_eq!(child_read, content);

        // Parent can read child's content using the FLAT name (primary use-case)
        let parent_read_flat = store.read_by_name_or_handle_hierarchical(parent_session, "weather.py").unwrap();
        assert_eq!(parent_read_flat, content);

        // Parent can also read using the fully-qualified namespaced name for disambiguation
        let namespaced_name = format!("{}/{}", child_session, "weather.py");
        let parent_read_namespaced = store.read_by_name_or_handle_hierarchical(parent_session, &namespaced_name).unwrap();
        assert_eq!(parent_read_namespaced, content);

        // Short alias should also work for parent (aliases are global to content store)
        let short_alias = ContentStore::get_short_alias(&handle);
        let parent_read_alias = store.read_by_name_or_handle_hierarchical(parent_session, &short_alias).unwrap();
        assert_eq!(parent_read_alias, content);

        // Full handle should also work
        let parent_read_handle = store.read_by_name_or_handle_hierarchical(parent_session, &handle).unwrap();
        assert_eq!(parent_read_handle, content);
    }

    #[test]
    fn test_hierarchical_content_isolation() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        // Two sibling sessions with same parent
        let parent_session = "planner-session";
        let child1_session = "planner-session/coder-abc";
        let child2_session = "planner-session/coder-def";

        store.set_parent_session(child1_session, parent_session).unwrap();
        store.set_parent_session(child2_session, parent_session).unwrap();

        // Child1 writes content
        let content1 = b"child1 content";
        let handle1 = store.write(content1).unwrap();
        store.register_name_in_hierarchy(child1_session, "file1.py", &handle1).unwrap();

        // Child2 writes content
        let content2 = b"child2 content";
        let handle2 = store.write(content2).unwrap();
        store.register_name_in_hierarchy(child2_session, "file2.py", &handle2).unwrap();

        // Parent can read both children's content using hierarchical names
        let parent_read1 = store.read_by_name_or_handle_hierarchical(
            parent_session,
            &format!("{}/{}", child1_session, "file1.py")
        ).unwrap();
        assert_eq!(parent_read1, content1);

        let parent_read2 = store.read_by_name_or_handle_hierarchical(
            parent_session,
            &format!("{}/{}", child2_session, "file2.py")
        ).unwrap();
        assert_eq!(parent_read2, content2);

        // Each child can read its own content directly
        let child1_read = store.read_by_name_or_handle_hierarchical(child1_session, "file1.py").unwrap();
        assert_eq!(child1_read, content1);

        let child2_read = store.read_by_name_or_handle_hierarchical(child2_session, "file2.py").unwrap();
        assert_eq!(child2_read, content2);

        // Siblings cannot read each other's content by direct name (sibling isolation)
        let sibling_attempt = store.read_by_name_or_handle_hierarchical(child1_session, "file2.py");
        assert!(sibling_attempt.is_err());

        // Parent can read both via flat name (each child has a distinct flat name here)
        let parent_flat1 = store.read_by_name_or_handle_hierarchical(parent_session, "file1.py").unwrap();
        assert_eq!(parent_flat1, content1);
        let parent_flat2 = store.read_by_name_or_handle_hierarchical(parent_session, "file2.py").unwrap();
        assert_eq!(parent_flat2, content2);
    }

    #[test]
    fn test_hierarchical_flat_name_last_writer_wins() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        // Two sibling sessions both writing the same flat name
        let parent_session = "planner-session";
        let child1_session = "planner-session/coder-abc";
        let child2_session = "planner-session/coder-def";

        store.set_parent_session(child1_session, parent_session).unwrap();
        store.set_parent_session(child2_session, parent_session).unwrap();

        let content1 = b"first version";
        let handle1 = store.write(content1).unwrap();
        store.register_name_in_hierarchy(child1_session, "output.txt", &handle1).unwrap();

        let content2 = b"second version";
        let handle2 = store.write(content2).unwrap();
        store.register_name_in_hierarchy(child2_session, "output.txt", &handle2).unwrap();

        // Parent gets the last writer's content under the flat name
        let parent_flat = store.read_by_name_or_handle_hierarchical(parent_session, "output.txt").unwrap();
        assert_eq!(parent_flat, content2);

        // Both namespaced names are still independently accessible
        let ns1 = format!("{}/output.txt", child1_session);
        let ns2 = format!("{}/output.txt", child2_session);
        assert_eq!(store.read_by_name_or_handle_hierarchical(parent_session, &ns1).unwrap(), content1);
        assert_eq!(store.read_by_name_or_handle_hierarchical(parent_session, &ns2).unwrap(), content2);
    }

    #[test]
    fn test_content_store_short_alias() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let content = b"test content for alias";
        let handle = store.write(content).unwrap();
        store
            .register_name("session-1", "test.txt", &handle)
            .unwrap();

        // Get the short alias
        let short_alias = ContentStore::get_short_alias(&handle);
        assert_eq!(short_alias.len(), SHORT_ALIAS_LEN);
        assert!(short_alias.chars().all(|c| c.is_ascii_hexdigit()));

        // Read using short alias
        let result = store
            .read_by_name_or_handle("session-1", &short_alias)
            .unwrap();
        assert_eq!(result, content);

        // Verify full handle still works
        let result2 = store.read_by_name_or_handle("session-1", &handle).unwrap();
        assert_eq!(result2, content);
    }

    #[test]
    fn test_manifest_updates_merge_across_store_instances() {
        let temp = tempdir().unwrap();

        let store1 = ContentStore::new(temp.path()).unwrap();
        let h1 = store1.write(b"first").unwrap();
        store1.register_name("session-1", "a.txt", &h1).unwrap();

        // Simulate a later tool call with a fresh ContentStore instance.
        let store2 = ContentStore::new(temp.path()).unwrap();
        let h2 = store2.write(b"second").unwrap();
        store2.register_name("session-1", "b.txt", &h2).unwrap();

        let manifest = store2.load_manifest("session-1").unwrap();
        assert_eq!(manifest.names.get("a.txt"), Some(&h1));
        assert_eq!(manifest.names.get("b.txt"), Some(&h2));
    }

    #[test]
    fn test_register_name_preserves_parent_session_across_instances() {
        let temp = tempdir().unwrap();
        let child = "planner/coder-123";
        let parent = "planner";

        let store1 = ContentStore::new(temp.path()).unwrap();
        store1.set_parent_session(child, parent).unwrap();

        // Fresh instance should preserve parent_session_id when adding names.
        let store2 = ContentStore::new(temp.path()).unwrap();
        let h = store2.write(b"print('hi')").unwrap();
        store2.register_name(child, "weather.py", &h).unwrap();

        let manifest = store2.load_manifest(child).unwrap();
        assert_eq!(manifest.parent_session_id.as_deref(), Some(parent));
        assert_eq!(manifest.names.get("weather.py"), Some(&h));
    }
}
