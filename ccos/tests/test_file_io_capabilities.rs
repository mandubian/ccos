use ccos::capabilities::{CapabilityExecutionPolicy, CapabilityRegistry};
use rtfs::runtime::security::RuntimeContext;
use rtfs::runtime::values::Value;

#[test]
fn file_io_capabilities_roundtrip() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file_path = dir.path().join("sample.txt");
    std::fs::write(&file_path, "initial").expect("write");

    let mut registry = CapabilityRegistry::new();
    registry.set_execution_policy(CapabilityExecutionPolicy::InlineDev);

    let context = RuntimeContext::controlled(vec![
        "ccos.io.file-exists".to_string(),
        "ccos.io.read-file".to_string(),
        "ccos.io.write-file".to_string(),
        "ccos.io.delete-file".to_string(),
    ]);

    let path_value = Value::String(file_path.to_string_lossy().to_string());

    let exists = registry
        .execute_capability_with_microvm(
            "ccos.io.file-exists",
            vec![path_value.clone()],
            Some(&context),
        )
        .expect("file-exists");
    assert_eq!(exists, Value::Boolean(true));

    let read_value = registry
        .execute_capability_with_microvm(
            "ccos.io.read-file",
            vec![path_value.clone()],
            Some(&context),
        )
        .expect("read-file");
    assert_eq!(read_value, Value::String("initial".to_string()));

    let write_result = registry
        .execute_capability_with_microvm(
            "ccos.io.write-file",
            vec![path_value.clone(), Value::String("updated".to_string())],
            Some(&context),
        )
        .expect("write-file");
    assert_eq!(write_result, Value::Boolean(true));
    assert_eq!(
        std::fs::read_to_string(&file_path).expect("read"),
        "updated"
    );

    let delete_result = registry
        .execute_capability_with_microvm(
            "ccos.io.delete-file",
            vec![path_value.clone()],
            Some(&context),
        )
        .expect("delete-file");
    assert_eq!(delete_result, Value::Boolean(true));
    assert!(!file_path.exists());
}

#[test]
fn file_io_base64_capabilities_roundtrip() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file_path = dir.path().join("binary.bin");

    // "Hello World" in bytes: [72, 101, 108, 108, 111, 32, 87, 111, 114, 108, 100]
    // Base64 of "Hello World": "SGVsbG8gV29ybGQ="

    let mut registry = CapabilityRegistry::new();
    registry.set_execution_policy(CapabilityExecutionPolicy::InlineDev);

    let context = RuntimeContext::controlled(vec![
        "ccos.io.read-file-base64".to_string(),
        "ccos.io.write-file-base64".to_string(),
    ]);

    let path_value = Value::String(file_path.to_string_lossy().to_string());
    let base64_content = Value::String("SGVsbG8gV29ybGQ=".to_string());

    // Write Base64
    let write_result = registry
        .execute_capability_with_microvm(
            "ccos.io.write-file-base64",
            vec![path_value.clone(), base64_content.clone()],
            Some(&context),
        )
        .expect("write-file-base64");
    assert_eq!(write_result, Value::Boolean(true));

    // Verify file content on disk is raw bytes, not base64 string
    let file_content = std::fs::read(&file_path).expect("read");
    assert_eq!(file_content, b"Hello World");

    // Read Base64
    let read_value = registry
        .execute_capability_with_microvm(
            "ccos.io.read-file-base64",
            vec![path_value.clone()],
            Some(&context),
        )
        .expect("read-file-base64");
    assert_eq!(read_value, base64_content);
}

#[test]
fn file_io_capabilities_require_permission() {
    let dir = tempfile::tempdir().expect("temp dir");
    let file_path = dir.path().join("guard.txt");
    std::fs::write(&file_path, "secure").expect("write");

    let mut registry = CapabilityRegistry::new();
    registry.set_execution_policy(CapabilityExecutionPolicy::InlineDev);

    let restricted_context = RuntimeContext::controlled(vec![]);

    let result = registry.execute_capability_with_microvm(
        "ccos.io.read-file",
        vec![Value::String(file_path.to_string_lossy().to_string())],
        Some(&restricted_context),
    );

    assert!(result.is_err());
}
