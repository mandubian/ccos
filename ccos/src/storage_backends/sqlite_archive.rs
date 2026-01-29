use crate::storage::{Archivable, ArchiveStats, ContentAddressableArchive};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct SqliteArchive {
    conn: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SqliteArchive {
    pub fn new<P: Into<PathBuf>>(path: P) -> Result<Self, String> {
        let db_path = path.into();
        let conn = Connection::open(&db_path).map_err(|e| e.to_string())?;
        conn.execute_batch(
            "BEGIN;CREATE TABLE IF NOT EXISTS objects(
                hash TEXT PRIMARY KEY,
                payload TEXT NOT NULL,
                stored_at INTEGER NOT NULL,
                size INTEGER NOT NULL
            );CREATE INDEX IF NOT EXISTS idx_objects_stored_at ON objects(stored_at);COMMIT;",
        )
        .map_err(|e| e.to_string())?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }
}

impl<T> ContentAddressableArchive<T> for SqliteArchive
where
    T: Archivable + Serialize + for<'de> Deserialize<'de> + Clone,
{
    fn store(&self, entity: T) -> Result<String, String> {
        let hash = entity.content_hash();
        let payload = serde_json::to_string(&entity).map_err(|e| e.to_string())?;
        let size = payload.len() as i64;
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_secs() as i64;

        // Insert or ignore to preserve immutability
        let conn_guard = self
            .conn
            .lock()
            .map_err(|_| "connection lock poisoned".to_string())?;
        conn_guard.execute(
            "INSERT OR IGNORE INTO objects(hash, payload, stored_at, size) VALUES (?1, ?2, ?3, ?4)",
            params![hash, payload, ts, size],
        ).map_err(|e| e.to_string())?;

        Ok(hash)
    }

    fn retrieve(&self, hash: &str) -> Result<Option<T>, String> {
        let conn_guard = self
            .conn
            .lock()
            .map_err(|_| "connection lock poisoned".to_string())?;
        let mut stmt = conn_guard
            .prepare("SELECT payload FROM objects WHERE hash = ?1")
            .map_err(|e| e.to_string())?;
        let payload: Option<String> = stmt
            .query_row(params![hash], |row| row.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        if let Some(p) = payload {
            let entity: T = serde_json::from_str(&p).map_err(|e| e.to_string())?;
            Ok(Some(entity))
        } else {
            Ok(None)
        }
    }

    fn exists(&self, hash: &str) -> bool {
        let conn_guard = match self.conn.lock() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let mut stmt = match conn_guard.prepare("SELECT 1 FROM objects WHERE hash = ?1 LIMIT 1") {
            Ok(s) => s,
            Err(_) => return false,
        };
        stmt.exists(params![hash]).unwrap_or(false)
    }

    fn delete(&self, hash: &str) -> Result<(), String> {
        let conn_guard = self
            .conn
            .lock()
            .map_err(|_| "connection lock poisoned".to_string())?;
        conn_guard
            .execute("DELETE FROM objects WHERE hash = ?1", params![hash])
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    fn stats(&self) -> ArchiveStats {
        let conn_guard = match self.conn.lock() {
            Ok(g) => g,
            Err(_) => {
                return ArchiveStats {
                    total_entities: 0,
                    total_size_bytes: 0,
                    oldest_timestamp: None,
                    newest_timestamp: None,
                }
            }
        };
        let total_entities: usize = conn_guard
            .query_row("SELECT COUNT(1) FROM objects", [], |r| r.get(0))
            .unwrap_or(0);
        let total_size_bytes: usize = conn_guard
            .query_row("SELECT COALESCE(SUM(size),0) FROM objects", [], |r| {
                r.get(0)
            })
            .unwrap_or(0);
        let oldest_ts: Option<u64> = conn_guard
            .query_row("SELECT MIN(stored_at) FROM objects", [], |r| r.get(0))
            .ok();
        let newest_ts: Option<u64> = conn_guard
            .query_row("SELECT MAX(stored_at) FROM objects", [], |r| r.get(0))
            .ok();

        ArchiveStats {
            total_entities,
            total_size_bytes,
            oldest_timestamp: oldest_ts,
            newest_timestamp: newest_ts,
        }
    }

    fn verify_integrity(&self) -> Result<bool, String> {
        let conn_guard = self
            .conn
            .lock()
            .map_err(|_| "connection lock poisoned".to_string())?;
        let mut stmt = conn_guard
            .prepare("SELECT hash, payload FROM objects")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows {
            let (hash, payload) = row.map_err(|e| e.to_string())?;
            let entity: T = serde_json::from_str(&payload).map_err(|e| e.to_string())?;
            if entity.content_hash() != hash {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn list_hashes(&self) -> Vec<String> {
        let mut out = Vec::new();
        let conn_guard = match self.conn.lock() {
            Ok(g) => g,
            Err(_) => return out,
        };
        if let Ok(mut stmt) = conn_guard.prepare("SELECT hash FROM objects") {
            let rows = stmt.query_map([], |row| row.get(0));
            if let Ok(iter) = rows {
                for r in iter.flatten() {
                    out.push(r);
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage_backends::file_archive::FileArchive;
    use tempfile::NamedTempFile;

    #[derive(Debug, Clone, Serialize, Deserialize)]
    struct TestEntity {
        id: String,
        n: i32,
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
    fn test_sqlite_store_and_retrieve() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let archive = SqliteArchive::new(path).expect("create sqlite archive");

        let e = TestEntity {
            id: "a".to_string(),
            n: 5,
        };
        let h = archive.store(e.clone()).expect("store");
        assert!(<SqliteArchive as ContentAddressableArchive<TestEntity>>::exists(&archive, &h));
        let got: TestEntity =
            <SqliteArchive as ContentAddressableArchive<TestEntity>>::retrieve(&archive, &h)
                .expect("retrieve")
                .unwrap();
        assert_eq!(got.id, e.id);

        // Verify integrity
        assert!(
            <SqliteArchive as ContentAddressableArchive<TestEntity>>::verify_integrity(&archive)
                .unwrap()
        );
    }

    #[test]
    fn test_sqlite_and_file_hash_stability() {
        let tmp = NamedTempFile::new().unwrap();
        let path = tmp.path().to_path_buf();
        let s = SqliteArchive::new(path).expect("sqlite");

        let e = TestEntity {
            id: "x".to_string(),
            n: 9,
        };
        let hs = <SqliteArchive as ContentAddressableArchive<TestEntity>>::store(&s, e.clone())
            .expect("store sqlite");

        let dir = tempfile::tempdir().unwrap();
        let f = FileArchive::new(dir.path()).expect("file archive");
        let hf = <FileArchive as ContentAddressableArchive<TestEntity>>::store(&f, e)
            .expect("store file");

        assert_eq!(hs, hf);
    }
}
