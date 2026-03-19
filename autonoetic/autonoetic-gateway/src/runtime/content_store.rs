//! Content-addressable storage for Autonoetic agents.
//!
//! Provides SHA-256 based content addressing that works locally and remotely.
//! Content is stored as immutable blobs; session manifests map names to handles.
//!
//! Visibility model:
//! - `private`: visible only to the writing session
//! - `session`: visible to all sessions under the same root_session_id (default)
//! - `global`: durable and cross-session readable

use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A content handle is a SHA-256 hash prefixed with "sha256:".
pub type ContentHandle = String;

/// Visibility scope for content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ContentVisibility {
    /// Visible only to the writing session.
    Private,
    /// Visible to all sessions under the same root_session_id.
    /// This is the default and matches the collaboration model.
    #[default]
    Session,
    /// Durable and cross-session readable.
    Global,
}

/// Returns the root session id — the portion before the first `/`.
///
/// `"demo-session/coder.default-abc"` → `"demo-session"`
/// `"demo-session"` → `"demo-session"`
pub fn root_session_id(session_id: &str) -> &str {
    session_id.split('/').next().unwrap_or(session_id)
}

/// Session manifest mapping content names to handles.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct SessionManifest {
    /// Map of content name → handle
    pub names: HashMap<String, ContentHandle>,
    /// Map of short alias (8 hex chars) → full handle for LLM-friendly lookup
    pub aliases: HashMap<String, ContentHandle>,
    /// Root session ID for content visibility.
    /// All sessions sharing the same root_session_id can read each other's
    /// session-visible content.
    #[serde(default)]
    pub root_session_id: Option<String>,
    /// Per-handle visibility tracking.
    #[serde(default)]
    pub visibility: HashMap<ContentHandle, ContentVisibility>,
}

/// Session ID used for the global content manifest.
const GLOBAL_SESSION_ID: &str = "__global__";

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

    fn load_manifest_from_disk_uncached(
        &self,
        session_id: &str,
    ) -> anyhow::Result<SessionManifest> {
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
        let manifest = manifests.get_mut(session_id).ok_or_else(|| {
            anyhow::anyhow!("Failed to load manifest for session '{}'", session_id)
        })?;
        manifest.names.insert(name.to_string(), handle.clone());

        // Also register the short alias for LLM-friendly lookup
        let short_alias = Self::handle_to_short_alias(handle);
        manifest.aliases.insert(short_alias, handle.clone());

        self.save_manifest(session_id, manifest)?;
        Ok(())
    }

    /// Sets the root session ID for content visibility.
    /// All sessions sharing the same root_session_id can read each other's
    /// session-visible content.
    pub fn set_root_session(&self, session_id: &str, root: &str) -> anyhow::Result<()> {
        let mut manifests = self.manifests.lock().unwrap();
        if !manifests.contains_key(session_id) {
            let disk_manifest = self.load_manifest_from_disk_uncached(session_id)?;
            manifests.insert(session_id.to_string(), disk_manifest);
        }
        let manifest = manifests.get_mut(session_id).ok_or_else(|| {
            anyhow::anyhow!("Failed to load manifest for session '{}'", session_id)
        })?;
        manifest.root_session_id = Some(root.to_string());
        self.save_manifest(session_id, manifest)?;
        Ok(())
    }

    /// Registers content with the given visibility.
    ///
    /// - `Private`: only registers in the current session's manifest.
    /// - `Session`: registers in both the current session AND the root session's manifest.
    /// - `Global`: registers in the current session, root session, AND a global index.
    pub fn register_name_with_visibility(
        &self,
        session_id: &str,
        name: &str,
        handle: &ContentHandle,
        visibility: ContentVisibility,
    ) -> anyhow::Result<()> {
        // Always register in current session
        self.register_name(session_id, name, handle)?;

        // Track visibility
        {
            let mut manifests = self.manifests.lock().unwrap();
            if !manifests.contains_key(session_id) {
                let disk_manifest = self.load_manifest_from_disk_uncached(session_id)?;
                manifests.insert(session_id.to_string(), disk_manifest);
            }
            if let Some(manifest) = manifests.get_mut(session_id) {
                manifest.visibility.insert(handle.clone(), visibility);
                self.save_manifest(session_id, manifest)?;
            }
        }

        // For session/global visibility, also register in root session
        if visibility != ContentVisibility::Private {
            let manifest = self.load_manifest(session_id)?;
            if let Some(root_id) = manifest.root_session_id {
                if root_id != session_id {
                    self.register_name(&root_id, name, handle)?;
                }
            }

            // For global visibility, also register in the global manifest
            if visibility == ContentVisibility::Global {
                self.register_name(GLOBAL_SESSION_ID, name, handle)?;

                tracing::debug!(
                    target: "content_store",
                    session_id = %session_id,
                    name = %name,
                    "Registered content in global manifest"
                );
            }
        }

        Ok(())
    }

    /// Resolves a name by checking current session, then root session, then global manifest.
    ///
    /// This enables session-visible and global content to be read by any session.
    pub fn resolve_name_with_root(
        &self,
        session_id: &str,
        name: &str,
    ) -> anyhow::Result<ContentHandle> {
        // 1. Try the caller's own session
        if let Ok(handle) = self.resolve_name(session_id, name) {
            return Ok(handle);
        }

        // 2. Try the root session (for session-visible content)
        let manifest = self.load_manifest(session_id)?;
        if let Some(root_id) = manifest.root_session_id {
            if root_id != session_id {
                if let Ok(handle) = self.resolve_name(&root_id, name) {
                    return Ok(handle);
                }
            }
        }

        // 3. Try the global manifest (for global-visible content)
        if session_id != GLOBAL_SESSION_ID {
            if let Ok(handle) = self.resolve_name(GLOBAL_SESSION_ID, name) {
                return Ok(handle);
            }
        }

        Err(anyhow::anyhow!(
            "Content name '{}' not found in session '{}', root session, or global",
            name,
            session_id
        ))
    }

    /// Resolves an alias by checking current session, then root session, then global.
    fn resolve_alias_with_root(
        &self,
        session_id: &str,
        alias: &str,
    ) -> anyhow::Result<ContentHandle> {
        // Check current session
        let manifest = self.load_manifest(session_id)?;
        if let Some(handle) = manifest.aliases.get(alias) {
            return Ok(handle.clone());
        }

        // Check root session
        if let Some(root_id) = manifest.root_session_id {
            if root_id != session_id {
                let root_manifest = self.load_manifest(&root_id)?;
                if let Some(handle) = root_manifest.aliases.get(alias) {
                    return Ok(handle.clone());
                }
            }
        }

        // Check global manifest
        if session_id != GLOBAL_SESSION_ID {
            let global_manifest = self.load_manifest(GLOBAL_SESSION_ID)?;
            if let Some(handle) = global_manifest.aliases.get(alias) {
                return Ok(handle.clone());
            }
        }

        Err(anyhow::anyhow!(
            "Content alias '{}' not found in session '{}', root session, or global",
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

    /// Checks whether a handle is visible in the current session, its root session, or global.
    ///
    /// A handle is visible if it is registered (by name or alias) in:
    /// - The current session's manifest
    /// - The root session's manifest
    /// - The global manifest
    pub fn is_handle_visible(&self, session_id: &str, handle: &str) -> anyhow::Result<bool> {
        // Check current session
        let manifest = self.load_manifest(session_id)?;
        if manifest.names.values().any(|h| h == handle) {
            return Ok(true);
        }
        if manifest.aliases.values().any(|h| h == handle) {
            return Ok(true);
        }

        // Check root session
        if let Some(root_id) = manifest.root_session_id {
            if root_id != session_id {
                let root_manifest = self.load_manifest(&root_id)?;
                if root_manifest.names.values().any(|h| h == handle) {
                    return Ok(true);
                }
                if root_manifest.aliases.values().any(|h| h == handle) {
                    return Ok(true);
                }
            }
        }

        // Check global manifest
        if session_id != GLOBAL_SESSION_ID {
            let global_manifest = self.load_manifest(GLOBAL_SESSION_ID)?;
            if global_manifest.names.values().any(|h| h == handle) {
                return Ok(true);
            }
            if global_manifest.aliases.values().any(|h| h == handle) {
                return Ok(true);
            }
        }

        Ok(false)
    }

    /// Resolves a handle with visibility check — reads only if handle is visible
    /// in the current or root session.
    fn resolve_handle_with_visibility(
        &self,
        session_id: &str,
        handle: &str,
    ) -> anyhow::Result<Vec<u8>> {
        if self.is_handle_visible(session_id, handle)? {
            self.read(&handle.to_string())
        } else {
            Err(anyhow::anyhow!(
                "Content handle '{}' is not visible in session '{}' or its root session",
                handle,
                session_id
            ))
        }
    }

    /// Reads content by name, handle, or short alias with root-based lookup.
    ///
    /// Resolution order:
    /// 1. If starts with "sha256:" → check visibility, then read
    /// 2. If 8 hex chars → short alias lookup (session, then root)
    /// 3. Otherwise → name lookup (session, then root)
    pub fn read_by_name_or_handle(
        &self,
        session_id: &str,
        name_or_handle: &str,
    ) -> anyhow::Result<Vec<u8>> {
        if name_or_handle.starts_with("sha256:") {
            self.resolve_handle_with_visibility(session_id, name_or_handle)
        } else if name_or_handle.len() == SHORT_ALIAS_LEN
            && name_or_handle.chars().all(|c| c.is_ascii_hexdigit())
        {
            self.resolve_alias_with_root(session_id, name_or_handle)
                .and_then(|handle| self.read(&handle))
        } else {
            self.resolve_name_with_root(session_id, name_or_handle)
                .and_then(|handle| self.read(&handle))
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

    /// Clears a session manifest.
    pub fn cleanup_session(&self, session_id: &str) -> anyhow::Result<usize> {
        let manifest = self.load_manifest(session_id)?;
        let removed = manifest.names.len();

        tracing::debug!(
            target: "content_store",
            session_id = %session_id,
            names_removed = removed,
            "Session cleanup"
        );

        // Clear the manifest
        let mut manifests = self.manifests.lock().unwrap();
        manifests.insert(session_id.to_string(), SessionManifest::default());

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
    fn test_root_session_visibility() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let parent_session = "demo-session";
        let child_session = "demo-session/coder-abc123";

        store
            .set_root_session(child_session, parent_session)
            .unwrap();

        // Child writes content with session visibility
        let content = b"print('Hello from coder')";
        let handle = store.write(content).unwrap();
        store
            .register_name_with_visibility(
                child_session,
                "weather.py",
                &handle,
                ContentVisibility::Session,
            )
            .unwrap();

        // Child can read its own content
        let child_read = store
            .read_by_name_or_handle(child_session, "weather.py")
            .unwrap();
        assert_eq!(child_read, content);

        // Parent (root session) can read child's content
        let parent_read = store
            .read_by_name_or_handle(parent_session, "weather.py")
            .unwrap();
        assert_eq!(parent_read, content);

        // Full handle also works
        let parent_read_handle = store
            .read_by_name_or_handle(parent_session, &handle)
            .unwrap();
        assert_eq!(parent_read_handle, content);

        // Short alias also works
        let short_alias = ContentStore::get_short_alias(&handle);
        let parent_read_alias = store
            .read_by_name_or_handle(parent_session, &short_alias)
            .unwrap();
        assert_eq!(parent_read_alias, content);
    }

    #[test]
    fn test_private_visibility_isolates_from_root() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let parent_session = "demo-session";
        let child_session = "demo-session/coder-abc123";

        store
            .set_root_session(child_session, parent_session)
            .unwrap();

        // Child writes private content
        let content = b"private scratchpad";
        let handle = store.write(content).unwrap();
        store
            .register_name_with_visibility(
                child_session,
                "scratch.txt",
                &handle,
                ContentVisibility::Private,
            )
            .unwrap();

        // Child can read its own content
        let child_read = store
            .read_by_name_or_handle(child_session, "scratch.txt")
            .unwrap();
        assert_eq!(child_read, content);

        // Parent CANNOT read child's private content by name
        let parent_attempt = store.read_by_name_or_handle(parent_session, "scratch.txt");
        assert!(parent_attempt.is_err());
    }

    #[test]
    fn test_sibling_session_visibility() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let parent_session = "demo-session";
        let child1_session = "demo-session/coder-abc";
        let child2_session = "demo-session/coder-def";

        store
            .set_root_session(child1_session, parent_session)
            .unwrap();
        store
            .set_root_session(child2_session, parent_session)
            .unwrap();

        // Child1 writes session-visible content
        let content1 = b"child1 output";
        let handle1 = store.write(content1).unwrap();
        store
            .register_name_with_visibility(
                child1_session,
                "output.py",
                &handle1,
                ContentVisibility::Session,
            )
            .unwrap();

        // Child2 can read sibling's session-visible content via root
        let child2_read = store
            .read_by_name_or_handle(child2_session, "output.py")
            .unwrap();
        assert_eq!(child2_read, content1);

        // Child1 writes private content
        let content2 = b"child1 private";
        let handle2 = store.write(content2).unwrap();
        store
            .register_name_with_visibility(
                child1_session,
                "draft.py",
                &handle2,
                ContentVisibility::Private,
            )
            .unwrap();

        // Child2 cannot read sibling's private content
        let child2_attempt = store.read_by_name_or_handle(child2_session, "draft.py");
        assert!(child2_attempt.is_err());
    }

    #[test]
    fn test_root_session_last_writer_wins() {
        let temp = tempdir().unwrap();
        let store = ContentStore::new(temp.path()).unwrap();

        let parent_session = "demo-session";
        let child1_session = "demo-session/coder-abc";
        let child2_session = "demo-session/coder-def";

        store
            .set_root_session(child1_session, parent_session)
            .unwrap();
        store
            .set_root_session(child2_session, parent_session)
            .unwrap();

        // Both write to same filename
        let content1 = b"first version";
        let handle1 = store.write(content1).unwrap();
        store
            .register_name_with_visibility(
                child1_session,
                "output.txt",
                &handle1,
                ContentVisibility::Session,
            )
            .unwrap();

        let content2 = b"second version";
        let handle2 = store.write(content2).unwrap();
        store
            .register_name_with_visibility(
                child2_session,
                "output.txt",
                &handle2,
                ContentVisibility::Session,
            )
            .unwrap();

        // Root session gets the last writer's content
        let root_read = store
            .read_by_name_or_handle(parent_session, "output.txt")
            .unwrap();
        assert_eq!(root_read, content2);

        // Each child can still read its own version
        let child1_read = store
            .read_by_name_or_handle(child1_session, "output.txt")
            .unwrap();
        assert_eq!(child1_read, content1);

        let child2_read = store
            .read_by_name_or_handle(child2_session, "output.txt")
            .unwrap();
        assert_eq!(child2_read, content2);
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
    fn test_root_session_preserved_across_instances() {
        let temp = tempdir().unwrap();
        let child = "demo-session/coder-123";
        let parent = "demo-session";

        let store1 = ContentStore::new(temp.path()).unwrap();
        store1.set_root_session(child, parent).unwrap();

        let store2 = ContentStore::new(temp.path()).unwrap();
        let h = store2.write(b"print('hi')").unwrap();
        store2.register_name(child, "weather.py", &h).unwrap();

        let manifest = store2.load_manifest(child).unwrap();
        assert_eq!(manifest.root_session_id.as_deref(), Some(parent));
        assert_eq!(manifest.names.get("weather.py"), Some(&h));
    }

    #[test]
    fn test_root_session_id_helper() {
        assert_eq!(root_session_id("demo-session"), "demo-session");
        assert_eq!(
            root_session_id("demo-session/coder.default-abc"),
            "demo-session"
        );
        assert_eq!(root_session_id("a/b/c"), "a");
    }
}
