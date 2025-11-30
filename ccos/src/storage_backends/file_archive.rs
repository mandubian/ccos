use crate::storage::{Archivable, ArchiveStats, ContentAddressableArchive};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

/// File-based archive that stores each entity as a JSON file named by its content hash.
#[derive(Debug, Clone)]
pub struct FileArchive {
    base_dir: PathBuf,
    // simple metadata store in-memory for stats; persisted as files on-disk
    metadata: Arc<Mutex<std::collections::HashMap<String, usize>>>,
    // index maps content-hash -> relative path (string)
    index: Arc<Mutex<std::collections::HashMap<String, String>>>,
}

// Implement the IndexableArchive trait if it is available in scope
impl super::super::storage::IndexableArchive for FileArchive {
    fn save_plan_intent_indices(
        &self,
        plan_index: &std::collections::HashMap<String, String>,
        intent_index: &std::collections::HashMap<String, Vec<String>>,
    ) -> Result<(), String> {
        self.save_plan_intent_indices(plan_index, intent_index)
    }

    fn load_plan_intent_indices(
        &self,
    ) -> Result<
        Option<(
            std::collections::HashMap<String, String>,
            std::collections::HashMap<String, Vec<String>>,
        )>,
        String,
    > {
        self.load_plan_intent_indices()
    }
}

impl FileArchive {
    pub fn new<P: AsRef<Path>>(base_dir: P) -> std::io::Result<Self> {
        let dir = base_dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        // Try to load existing index
        let index_path = dir.join("index.json");
        let index_map = if index_path.exists() {
            let content = fs::read_to_string(&index_path)?;
            serde_json::from_str(&content).unwrap_or_default()
        } else {
            std::collections::HashMap::new()
        };

        Ok(Self {
            base_dir: dir,
            metadata: Arc::new(Mutex::new(std::collections::HashMap::new())),
            index: Arc::new(Mutex::new(index_map)),
        })
    }

    fn default_rel_path(hash: &str) -> String {
        // Shard by first 4 hex chars for directory fan-out, store as <aa>/<bb>/<hash>.json
        let a = &hash[0..2];
        let b = &hash[2..4];
        format!("{}/{}/{}.json", a, b, hash)
    }

    fn path_for_hash(&self, hash: &str) -> PathBuf {
        // Look up in index for deterministic path
        let index = self
            .index
            .lock()
            .unwrap_or_else(|_| panic!("index lock poisoned"));
        if let Some(rel) = index.get(hash) {
            return self.base_dir.join(rel);
        }
        // Fallback to deterministic sharded scheme
        self.base_dir.join(Self::default_rel_path(hash))
    }

    fn save_index(&self) -> Result<(), String> {
        let idx = self
            .index
            .lock()
            .map_err(|_| "index lock poisoned".to_string())?;
        let content = serde_json::to_string_pretty(&*idx).map_err(|e| e.to_string())?;
        let path = self.base_dir.join("index.json");
        // Atomic write with archive lock to prevent concurrent writers
        self.atomic_write_with_lock(&path, content.as_bytes())?;
        Ok(())
    }

    /// Save per-domain indices (plans and intents) as sidecar files next to the archive.
    /// These are optional; callers may choose to persist whenever they update in-memory indices.
    pub fn save_plan_intent_indices(
        &self,
        plan_index: &std::collections::HashMap<String, String>,
        intent_index: &std::collections::HashMap<String, Vec<String>>,
    ) -> Result<(), String> {
        let plan_path = self.base_dir.join("plan_index.json");
        let intent_path = self.base_dir.join("intent_index.json");
        let plan_json = serde_json::to_string_pretty(plan_index).map_err(|e| e.to_string())?;
        let intent_json = serde_json::to_string_pretty(intent_index).map_err(|e| e.to_string())?;
        // Use locked atomic write so concurrent processes won't corrupt sidecars
        self.atomic_write_with_lock(&plan_path, plan_json.as_bytes())?;
        self.atomic_write_with_lock(&intent_path, intent_json.as_bytes())?;
        Ok(())
    }

    /// Attempt to load the plan/intent sidecar indices. Returns Ok(Some((plan_idx, intent_idx)))
    /// when both files were present and parsed, Ok(None) when the files weren't present.
    pub fn load_plan_intent_indices(
        &self,
    ) -> Result<
        Option<(
            std::collections::HashMap<String, String>,
            std::collections::HashMap<String, Vec<String>>,
        )>,
        String,
    > {
        let plan_path = self.base_dir.join("plan_index.json");
        let intent_path = self.base_dir.join("intent_index.json");
        if !plan_path.exists() || !intent_path.exists() {
            return Ok(None);
        }
        let plan_content = fs::read_to_string(&plan_path).map_err(|e| e.to_string())?;
        let intent_content = fs::read_to_string(&intent_path).map_err(|e| e.to_string())?;
        let plan_map: std::collections::HashMap<String, String> =
            serde_json::from_str(&plan_content).map_err(|e| e.to_string())?;
        let intent_map: std::collections::HashMap<String, Vec<String>> =
            serde_json::from_str(&intent_content).map_err(|e| e.to_string())?;
        Ok(Some((plan_map, intent_map)))
    }

    fn ensure_parent(path: &Path) -> Result<(), String> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    /// Acquire an exclusive archive-level lock (by creating a lock file) with timeout and
    /// a simple stale-lock detection. Returns a guard which removes the lock file when dropped.
    fn acquire_archive_lock(&self, timeout: Duration) -> Result<ArchiveLockGuard, String> {
        let lock_path = self.base_dir.join(".archive_lock");
        let start = std::time::Instant::now();
        let mut forced_cleanup_attempted = false;
        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock_path)
            {
                Ok(f) => {
                    return Ok(ArchiveLockGuard {
                        path: lock_path,
                        _file: f,
                    })
                }
                Err(e) => {
                    // If lock exists and looks stale, try removing it
                    if lock_path.exists() {
                        if let Ok(meta) = std::fs::metadata(&lock_path) {
                            if let Ok(mtime) = meta.modified() {
                                if let Ok(age) = SystemTime::now().duration_since(mtime) {
                                    if age > Duration::from_secs(10) {
                                        let _ = std::fs::remove_file(&lock_path);
                                        // try again immediately
                                        continue;
                                    }
                                }
                            }
                        }
                    }
                    if start.elapsed() > timeout {
                        // As a last resort, attempt a best-effort cleanup of a potentially stale lock
                        // and retry once before failing. This helps tests and crash-recovery scenarios.
                        if lock_path.exists() && !forced_cleanup_attempted {
                            let _ = std::fs::remove_file(&lock_path);
                            forced_cleanup_attempted = true;
                            // retry immediately after cleanup
                            continue;
                        }
                        return Err(format!("timeout acquiring archive lock: {}", e));
                    }
                    std::thread::sleep(Duration::from_millis(20));
                }
            }
        }
    }

    /// Atomic write that holds the archive-level lock for the duration of the write.
    fn atomic_write_with_lock(&self, path: &Path, data: &[u8]) -> Result<(), String> {
        let _guard = self.acquire_archive_lock(Duration::from_secs(5))?;
        Self::ensure_parent(path)?;
        let mut tmp = path.to_path_buf();
        let tmp_name = format!("{}.tmp", uuid::Uuid::new_v4());
        tmp.set_file_name(tmp_name);
        // Write temp in same directory
        if let Some(dir) = path.parent() {
            tmp = dir.join(tmp.file_name().unwrap());
        }
        {
            let mut f = fs::File::create(&tmp).map_err(|e| e.to_string())?;
            f.write_all(data).map_err(|e| e.to_string())?;
            f.sync_all().map_err(|e| e.to_string())?;
        }
        fs::rename(&tmp, path).map_err(|e| e.to_string())?;
        // Ensure directory entry is flushed to disk where supported (durability after rename)
        if let Some(dir) = path.parent() {
            if let Ok(dir_file) = fs::File::open(dir) {
                let _ = dir_file.sync_all().map_err(|e| e.to_string())?;
            }
        }
        Ok(())
    }
}

/// RAII guard for the archive-level lock file. When dropped the lock file is removed.
struct ArchiveLockGuard {
    path: PathBuf,
    _file: fs::File,
}

impl Drop for ArchiveLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

impl<T> ContentAddressableArchive<T> for FileArchive
where
    T: Archivable + Serialize + for<'de> Deserialize<'de> + Clone,
{
    fn store(&self, entity: T) -> Result<String, String> {
        let hash = entity.content_hash();
        println!(
            "🔍 FileArchive::store called for entity with hash: {}",
            hash
        );

        // Determine deterministic relative path; prefer existing index entry
        let rel = {
            let idx = self
                .index
                .lock()
                .map_err(|_| "index lock poisoned".to_string())?;
            idx.get(&hash)
                .cloned()
                .unwrap_or_else(|| Self::default_rel_path(&hash))
        };
        let path = self.base_dir.join(&rel);
        println!("🔍 FileArchive::store writing to path: {:?}", path);

        // Serialize to JSON
        let json = serde_json::to_string_pretty(&entity).map_err(|e| {
            println!("❌ FileArchive::store JSON serialization failed: {}", e);
            e.to_string()
        })?;
        println!(
            "🔍 FileArchive::store JSON serialized, length: {}",
            json.len()
        );

        // Ensure directories and atomically write file
        self.atomic_write_with_lock(&path, json.as_bytes())
            .map_err(|e| {
                println!("❌ FileArchive::store atomic_write_with_lock failed: {}", e);
                e
            })?;
        println!("✅ FileArchive::store atomic_write succeeded");

        // Update metadata
        let mut meta = self
            .metadata
            .lock()
            .map_err(|_| "metadata lock poisoned".to_string())?;
        meta.insert(hash.clone(), json.len());

        // Update index mapping and persist
        {
            let mut idx = self
                .index
                .lock()
                .map_err(|_| "index lock poisoned".to_string())?;
            idx.insert(hash.clone(), rel);
        }
        self.save_index()?;

        Ok(hash)
    }

    fn retrieve(&self, hash: &str) -> Result<Option<T>, String> {
        let path = self.path_for_hash(hash);
        if !path.exists() {
            return Ok(None);
        }
        let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
        let entity: T = serde_json::from_str(&content).map_err(|e| e.to_string())?;
        Ok(Some(entity))
    }

    fn exists(&self, hash: &str) -> bool {
        self.path_for_hash(hash).exists()
    }

    fn delete(&self, hash: &str) -> Result<(), String> {
        let path = self.path_for_hash(hash);
        if path.exists() {
            fs::remove_file(&path).map_err(|e| e.to_string())?;
        }

        // Remove from index
        let mut idx = self
            .index
            .lock()
            .map_err(|_| "index lock poisoned".to_string())?;
        idx.remove(hash);

        // Remove from metadata
        let mut meta = self
            .metadata
            .lock()
            .map_err(|_| "metadata lock poisoned".to_string())?;
        meta.remove(hash);

        Ok(())
    }

    fn stats(&self) -> ArchiveStats {
        let meta = self
            .metadata
            .lock()
            .unwrap_or_else(|_| panic!("metadata lock poisoned"));
        let total_entities = meta.len();
        let total_size_bytes = meta.values().copied().sum();
        ArchiveStats {
            total_entities,
            total_size_bytes,
            oldest_timestamp: None,
            newest_timestamp: None,
        }
    }

    fn verify_integrity(&self) -> Result<bool, String> {
        // Prefer index-based verification
        let idx = self
            .index
            .lock()
            .map_err(|_| "index lock poisoned".to_string())?;
        if !idx.is_empty() {
            for (hash, rel) in idx.iter() {
                let path = self.base_dir.join(rel);
                let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                let entity: T = serde_json::from_str(&content).map_err(|e| e.to_string())?;
                let computed = entity.content_hash();
                if &computed != hash {
                    return Ok(false);
                }
            }
            return Ok(true);
        }
        // Fallback: scan files ignoring index.json; compare file-stem to hash
        for entry in fs::read_dir(&self.base_dir).map_err(|e| e.to_string())? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if !path.is_file() {
                continue;
            }
            if path.file_name().and_then(|s| s.to_str()) == Some("index.json") {
                continue;
            }
            let content = fs::read_to_string(&path).map_err(|e| e.to_string())?;
            let entity: T = serde_json::from_str(&content).map_err(|e| e.to_string())?;
            let computed = entity.content_hash();
            let stem = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            if computed != stem {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn list_hashes(&self) -> Vec<String> {
        // Prefer index keys if available
        let idx = self
            .index
            .lock()
            .unwrap_or_else(|_| panic!("index lock poisoned"));
        if !idx.is_empty() {
            return idx.keys().cloned().collect();
        }
        // Fallback: list top-level files excluding index.json and directories
        let mut out = Vec::new();
        if let Ok(entries) = fs::read_dir(&self.base_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                if path.file_name().and_then(|s| s.to_str()) == Some("index.json") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    out.push(stem.to_string());
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestEntity {
        id: String,
        val: i32,
    }

    impl Archivable for TestEntity {
        fn entity_id(&self) -> String {
            self.id.clone()
        }
        fn entity_type(&self) -> &'static str {
            "TestEntity"
        }
    }

    #[test]
    fn test_file_archive_store_and_retrieve() {
        let dir = tempdir().unwrap();
        let archive = FileArchive::new(dir.path()).expect("create archive");

        let e = TestEntity {
            id: "a".to_string(),
            val: 1,
        };
        let hash = archive.store(e.clone()).expect("store");
        // `exists` is a trait method generic over T; call via fully-qualified syntax
        assert!(<FileArchive as ContentAddressableArchive<TestEntity>>::exists(&archive, &hash));
        let retrieved: TestEntity = archive.retrieve(&hash).expect("retrieve").unwrap();
        assert_eq!(retrieved.id, e.id);
        assert_eq!(retrieved.val, e.val);

        // Index should be persisted
        let index_path = dir.path().join("index.json");
        assert!(index_path.exists());
        let index_json = std::fs::read_to_string(index_path).unwrap();
        let v: serde_json::Value = serde_json::from_str(&index_json).unwrap();
        assert!(v.get(&hash).is_some());

        // New instance should be able to retrieve via index
        let archive2 = FileArchive::new(dir.path()).expect("reopen archive");
        let retrieved2: TestEntity = archive2.retrieve(&hash).expect("retrieve2").unwrap();
        assert_eq!(retrieved2.id, e.id);
    }

    #[test]
    fn test_acquire_lock_and_write() {
        let dir = tempdir().unwrap();
        let archive = FileArchive::new(dir.path()).expect("create archive");
        // Acquire lock explicitly and write a sidecar
        let plan_path = dir.path().join("plan_index.json");
        let data = b"{}";
        archive
            .atomic_write_with_lock(&plan_path, data)
            .expect("write with lock");
        assert!(plan_path.exists());
    }

    #[test]
    fn test_stale_lock_cleanup() {
        let dir = tempdir().unwrap();
        let archive = FileArchive::new(dir.path()).expect("create archive");
        let lock_path = dir.path().join(".archive_lock");
        // Create a stale lock file with old modified time
        std::fs::write(&lock_path, b"stale").unwrap();
        // Set modified time to far in the past (best-effort)
        // Set the mtime to the distant past so the lock is considered stale deterministically
        // Use the filetime crate (added as dev-dependency) to avoid platform-specific shell calls
        {
            use filetime::FileTime;
            let ft = FileTime::from_unix_time(0, 0);
            filetime::set_file_mtime(&lock_path, ft).unwrap();
        }
        // Now attempt to acquire lock; function should remove stale lock and succeed
        let guard = archive
            .acquire_archive_lock(Duration::from_secs(1))
            .expect("acquire after stale cleanup");
        drop(guard);
        // lock file should be removed after guard drop
        assert!(!lock_path.exists());
    }
}
