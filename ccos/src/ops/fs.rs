use rtfs::runtime::{RuntimeError, RuntimeResult};
use std::fs;
use std::path::Path;

/// Options for filesystem operations
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct FsOptions {
    pub recursive: Option<bool>,
    pub force: Option<bool>,
}

/// Directory entry information
#[derive(Debug, serde::Serialize, serde::Deserialize)]
pub struct DirEntry {
    pub name: String,
    pub path: String,
    pub is_dir: bool,
    pub is_file: bool,
    pub size: u64,
}

/// List directory contents
pub fn list_dir(path: &str) -> RuntimeResult<Vec<DirEntry>> {
    let path = Path::new(path);
    if !path.exists() {
        return Err(RuntimeError::Generic(format!(
            "Path does not exist: {}",
            path.display()
        )));
    }
    if !path.is_dir() {
        return Err(RuntimeError::Generic(format!(
            "Path is not a directory: {}",
            path.display()
        )));
    }

    let entries = fs::read_dir(path).map_err(|e| RuntimeError::IoError(e.to_string()))?;
    let mut result = Vec::new();

    for entry in entries {
        let entry = entry.map_err(|e| RuntimeError::IoError(e.to_string()))?;
        let metadata = entry
            .metadata()
            .map_err(|e| RuntimeError::IoError(e.to_string()))?;
        result.push(DirEntry {
            name: entry.file_name().to_string_lossy().into_owned(),
            path: entry.path().to_string_lossy().into_owned(),
            is_dir: metadata.is_dir(),
            is_file: metadata.is_file(),
            size: metadata.len(),
        });
    }

    Ok(result)
}

/// Read file content
pub fn read_file(path: &str) -> RuntimeResult<String> {
    fs::read_to_string(path).map_err(|e| RuntimeError::IoError(e.to_string()))
}

/// Write file content
pub fn write_file(path: &str, content: &str) -> RuntimeResult<()> {
    fs::write(path, content).map_err(|e| RuntimeError::IoError(e.to_string()))
}

/// Read file content as bytes
pub fn read_file_bytes(path: &str) -> RuntimeResult<Vec<u8>> {
    fs::read(path).map_err(|e| RuntimeError::IoError(e.to_string()))
}

/// Write bytes to file
pub fn write_file_bytes(path: &str, content: &[u8]) -> RuntimeResult<()> {
    fs::write(path, content).map_err(|e| RuntimeError::IoError(e.to_string()))
}

/// Delete file or directory
pub fn delete(path: &str, recursive: bool) -> RuntimeResult<bool> {
    let path_ref = Path::new(path);
    if !path_ref.exists() {
        return Ok(false);
    }

    if path_ref.is_dir() {
        if recursive {
            fs::remove_dir_all(path_ref).map_err(|e| RuntimeError::IoError(e.to_string()))?;
        } else {
            fs::remove_dir(path_ref).map_err(|e| RuntimeError::IoError(e.to_string()))?;
        }
    } else {
        fs::remove_file(path_ref).map_err(|e| RuntimeError::IoError(e.to_string()))?;
    }
    Ok(true)
}

/// Create directory
pub fn mkdir(path: &str, recursive: bool) -> RuntimeResult<()> {
    if recursive {
        fs::create_dir_all(path).map_err(|e| RuntimeError::IoError(e.to_string()))
    } else {
        fs::create_dir(path).map_err(|e| RuntimeError::IoError(e.to_string()))
    }
}

/// Check if path exists
pub fn exists(path: &str) -> bool {
    Path::new(path).exists()
}
