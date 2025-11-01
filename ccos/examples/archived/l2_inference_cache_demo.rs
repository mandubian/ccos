use ccos::caching::l2_inference::{InferenceResult, L2InferenceCache};
use std::time::Duration;

fn main() {
    println!("🚀 L2 Inference Cache Demo");
    println!("==========================\n");

    // Initialize the L2 Inference Cache with default configuration
    let cache = L2InferenceCache::with_default_config();

    // Scenario: A user calls a function like (analyze-sentiment "I love this product!")
    // The delegation engine decides to use GPT-4 for sentiment analysis

    let model_id = "gpt-4o";

    // Test Case 1: First call - cache miss, expensive LLM call
    println!("🔍 Test Case 1: First call to analyze sentiment");
    let prompt1 = "Analyze the sentiment of this text: 'I love this product! It's amazing and works perfectly.'";

    // Simulate expensive LLM call (150ms inference time)
    let result1 = InferenceResult::new(
        "Positive sentiment (0.95 confidence). The text expresses strong positive emotions with words like 'love', 'amazing', and 'perfectly'.".to_string(),
        0.95,
        model_id.to_string(),
        150, // 150ms inference time
    );

    // Cache the result
    cache
        .put_inference(model_id, prompt1, result1.clone())
        .unwrap();

    let stats1 = cache.get_stats();
    println!(
        "  Cache Stats: Hits={}, Misses={}, Puts={}, Hit Rate={:.2}%",
        stats1.hits,
        stats1.misses,
        stats1.puts,
        stats1.hit_rate * 100.0
    );

    // Test Case 2: Identical call - cache hit, instant response
    println!("\n🔍 Test Case 2: Identical call - should hit cache");
    let prompt2 = "Analyze the sentiment of this text: 'I love this product! It's amazing and works perfectly.'";

    let start_time = std::time::Instant::now();
    let cached_result = cache.get_inference(model_id, prompt2).unwrap();
    let cache_lookup_time = start_time.elapsed();

    assert_eq!(cached_result.output, result1.output);
    assert_eq!(cached_result.confidence, 0.95);
    println!("  Cache Hit! Lookup time: {:?}", cache_lookup_time);
    println!("  Response: {}", cached_result.output);

    let stats2 = cache.get_stats();
    println!(
        "  Cache Stats: Hits={}, Misses={}, Puts={}, Hit Rate={:.2}%",
        stats2.hits,
        stats2.misses,
        stats2.puts,
        stats2.hit_rate * 100.0
    );

    // Test Case 3: Similar but different prompt - cache miss
    println!("\n🔍 Test Case 3: Similar but different prompt - should miss cache");
    let prompt3 = "Analyze the sentiment of this text: 'I really like this product! It's great and works well.'";

    // Simulate another expensive LLM call
    let result3 = InferenceResult::new(
        "Positive sentiment (0.92 confidence). The text expresses positive emotions with words like 'like', 'great', and 'well'.".to_string(),
        0.92,
        model_id.to_string(),
        145,
    );

    cache
        .put_inference(model_id, prompt3, result3.clone())
        .unwrap();

    let stats3 = cache.get_stats();
    println!(
        "  Cache Stats: Hits={}, Misses={}, Puts={}, Hit Rate={:.2}%",
        stats3.hits,
        stats3.misses,
        stats3.puts,
        stats3.hit_rate * 100.0
    );

    // Test Case 4: Different model for same prompt - cache miss
    println!("\n🔍 Test Case 4: Different model for same prompt - should miss cache");
    let model_id_2 = "claude-3-opus";

    // Simulate Claude giving a different response
    let result4 = InferenceResult::new(
        "Positive sentiment (0.98 confidence). The language is clearly enthusiastic and favorable."
            .to_string(),
        0.98,
        model_id_2.to_string(),
        180, // Claude might be slower
    );

    cache
        .put_inference(model_id_2, prompt1, result4.clone())
        .unwrap();

    let stats4 = cache.get_stats();
    println!(
        "  Cache Stats: Hits={}, Misses={}, Puts={}, Hit Rate={:.2}%",
        stats4.hits,
        stats4.misses,
        stats4.puts,
        stats4.hit_rate * 100.0
    );

    // Test Case 5: Show model-specific results
    println!("\n🔍 Test Case 5: Model-specific caching");
    let gpt_results = cache.get_model_results(model_id);
    let claude_results = cache.get_model_results(model_id_2);

    println!("  GPT-4 results: {} cached inferences", gpt_results.len());
    println!(
        "  Claude results: {} cached inferences",
        claude_results.len()
    );

    // Test Case 6: Performance comparison
    println!("\n🔍 Test Case 6: Performance comparison");

    // Simulate expensive LLM call (200ms)
    let expensive_start = std::time::Instant::now();
    std::thread::sleep(Duration::from_millis(200)); // Simulate LLM call
    let expensive_time = expensive_start.elapsed();

    // Simulate cache lookup (should be much faster)
    let cache_start = std::time::Instant::now();
    let _ = cache.get_inference(model_id, prompt1);
    let cache_time = cache_start.elapsed();

    println!("  LLM call simulation: {:?}", expensive_time);
    println!("  Cache lookup: {:?}", cache_time);
    println!(
        "  Speedup: {:.1}x faster",
        expensive_time.as_micros() as f64 / cache_time.as_micros() as f64
    );

    // Test Case 7: Cache invalidation scenario
    println!("\n🔍 Test Case 7: Cache invalidation");

    // Simulate a scenario where we need to invalidate a cached result
    // (e.g., model was updated, or we want fresh results)
    cache.invalidate_inference(model_id, prompt1).unwrap();

    // Now the same call should miss cache again
    let invalidated_result = cache.get_inference(model_id, prompt1);
    assert!(invalidated_result.is_none());

    let final_stats = cache.get_stats();
    println!("  After invalidation - Cache Stats: Hits={}, Misses={}, Puts={}, Invalidations={}, Hit Rate={:.2}%", 
             final_stats.hits, final_stats.misses, final_stats.puts, final_stats.invalidations, final_stats.hit_rate * 100.0);

    // Test Case 8: Demonstrate confidence-based filtering
    println!("\n🔍 Test Case 8: Confidence-based filtering");

    // Add a low-confidence result
    let low_confidence_result = InferenceResult::new(
        "Uncertain sentiment (0.45 confidence). The text is ambiguous.".to_string(),
        0.45,
        model_id.to_string(),
        120,
    );

    cache
        .put_inference(model_id, "ambiguous text", low_confidence_result.clone())
        .unwrap();

    // Check if it's considered low confidence
    let retrieved_low = cache.get_inference(model_id, "ambiguous text").unwrap();
    if retrieved_low.is_low_confidence(0.5) {
        println!("  Low confidence result detected and flagged");
    }

    // Test Case 9: Demonstrate TTL (Time To Live) functionality
    println!("\n🔍 Test Case 9: TTL (Time To Live) functionality");

    // Add a result and check if it becomes stale
    let fresh_result =
        InferenceResult::new("Fresh result".to_string(), 0.9, model_id.to_string(), 100);

    cache
        .put_inference(model_id, "fresh input", fresh_result.clone())
        .unwrap();

    // Check if it's stale after a short duration
    let retrieved_fresh = cache.get_inference(model_id, "fresh input").unwrap();
    if !retrieved_fresh.is_stale(Duration::from_secs(1)) {
        println!("  Result is still fresh (not stale)");
    }

    // Test Case 10: Show comprehensive statistics
    println!("\n🔍 Test Case 10: Comprehensive statistics");
    let comprehensive_stats = cache.get_stats();
    println!("  Final Cache Statistics:");
    println!("    - Total hits: {}", comprehensive_stats.hits);
    println!("    - Total misses: {}", comprehensive_stats.misses);
    println!("    - Total puts: {}", comprehensive_stats.puts);
    println!(
        "    - Total invalidations: {}",
        comprehensive_stats.invalidations
    );
    println!(
        "    - Hit rate: {:.2}%",
        comprehensive_stats.hit_rate * 100.0
    );
    println!("    - Cache size: {}", comprehensive_stats.size);

    println!("\n✅ L2 Inference Cache demo completed successfully!");
    println!("   The L2 Inference Cache demonstrates:");
    println!("   - Exact match caching for identical prompts");
    println!("   - Model-specific caching (different models, different results)");
    println!("   - Performance improvements through caching");
    println!("   - Proper cache invalidation for fresh results");
    println!("   - Confidence-based result filtering");
    println!("   - TTL-based staleness detection");
    println!("   - Comprehensive statistics tracking");
}
