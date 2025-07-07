use std::io::Write;
use std::rc::Rc;
use std::sync::Arc;

use flate2::{write::GzEncoder, Compression};
use rtfs_compiler::ccos::caching::l4_content_addressable::{L4CacheClient, RtfsModuleMetadata};
use rtfs_compiler::parser::parse_expression;
use rtfs_compiler::runtime::{Evaluator, ModuleRegistry, RuntimeResult, Value};
use rtfs_compiler::ccos::delegation::{DelegationEngine, StaticDelegationEngine};
use rtfs_compiler::ccos::delegation_l4::L4AwareDelegationEngine;
use rtfs_compiler::bytecode::WasmBackend; // not used but ModuleRegistry requires a backend for publishing

#[test]
fn test_l4_cache_compressed_rtfs_roundtrip() -> RuntimeResult<()> {
    // RTFS source that defines `add`
    let src_code = "(defn add [x y] (+ x y))";

    // Compress with gzip
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(src_code.as_bytes()).unwrap();
    let compressed = encoder.finish().unwrap();

    // Create cache and store blob
    let cache = L4CacheClient::new();
    let blob_hash = cache.store_blob(compressed.clone()).expect("store blob");

    // Register metadata so DelegationEngine can find it
    let metadata = RtfsModuleMetadata::new(Vec::new(), "add-src".to_string(), blob_hash.clone());
    // No bytecode for now; pass empty vec
    cache.publish_module(Vec::new(), metadata).expect("publish meta");

    // Build runtime components
    let backend = Arc::new(WasmBackend::default());
    let module_registry = ModuleRegistry::new()
        .with_l4_cache(Arc::new(cache.clone()))
        .with_bytecode_backend(backend);

    let inner = StaticDelegationEngine::new(Default::default());
    let de: Arc<dyn DelegationEngine> = Arc::new(L4AwareDelegationEngine::new(cache.clone(), inner));

    let mut evaluator = Evaluator::new(Rc::new(module_registry), de);

    // Manually retrieve, decompress, parse, and evaluate the defn to populate env
    let compressed_blob = cache.get_blob(&blob_hash).expect("blob exists");
    let mut decoder = flate2::read::GzDecoder::new(&compressed_blob[..]);
    let mut decompressed = String::new();
    use std::io::Read;
    decoder.read_to_string(&mut decompressed).unwrap();

    let do_code = format!("(do {} (add 4 5))", decompressed);
    let expr = parse_expression(&do_code).unwrap();
    let result = evaluator.evaluate(&expr)?;
    assert_eq!(result, Value::Integer(9));

    Ok(())
} 