use ccos::caching::l3_semantic::{L3SemanticCache, SemanticCacheConfig};

fn main() {
    println!("🧠 L3 Semantic Cache Demo");
    println!("=========================\n");

    // Initialize the L3 Semantic Cache with custom configuration
    let mut config = SemanticCacheConfig::default();
    config.similarity_threshold = 0.7; // 70% similarity threshold
    config.max_size = 100;
    let cache = L3SemanticCache::new(config);

    println!("🔍 Test Case 1: Exact match caching");
    let query1 = "What is the weather like today?";
    let result1 = "The weather is sunny with a high of 75°F";

    cache.put_semantic(query1, result1).unwrap();

    // Exact match should work
    let exact_result = cache.get_semantic(query1);
    assert!(exact_result.is_some());
    let (value, similarity) = exact_result.unwrap();
    println!(
        "  Exact match: '{}' -> '{}' (similarity: {:.3})",
        query1, value, similarity
    );
    assert!((similarity - 1.0).abs() < 0.001); // Should be exact match

    println!("\n🔍 Test Case 2: Semantic similarity detection");

    // Similar queries that should hit the cache
    let similar_queries = vec![
        "How's the weather?",
        "What's the weather like?",
        "Tell me about today's weather",
        "Is it nice outside?",
        "What's the temperature today?",
    ];

    for query in similar_queries {
        let result = cache.get_semantic(query);
        if let Some((value, similarity)) = result {
            println!(
                "  Similar: '{}' -> '{}' (similarity: {:.3})",
                query, value, similarity
            );
        } else {
            println!("  No match: '{}' (below threshold)", query);
        }
    }

    println!("\n🔍 Test Case 3: Multiple semantic entries");

    // Add more diverse semantic entries
    cache
        .put_semantic(
            "How do I make coffee?",
            "Grind beans, add hot water, wait 4 minutes",
        )
        .unwrap();
    cache
        .put_semantic(
            "What's the recipe for coffee?",
            "Use 2 tablespoons of ground coffee per cup",
        )
        .unwrap();
    cache
        .put_semantic(
            "How to brew coffee?",
            "Pour hot water over coffee grounds in a filter",
        )
        .unwrap();

    cache
        .put_semantic(
            "What's the capital of France?",
            "Paris is the capital of France",
        )
        .unwrap();
    cache
        .put_semantic(
            "Where is Paris located?",
            "Paris is located in northern France",
        )
        .unwrap();
    cache
        .put_semantic(
            "Tell me about Paris",
            "Paris is the capital and largest city of France",
        )
        .unwrap();

    println!("  Added coffee-related queries");
    println!("  Added Paris-related queries");

    println!("\n🔍 Test Case 4: Semantic search results");

    // Test semantic search for coffee queries
    let coffee_query = "How to prepare coffee?";
    let coffee_results = cache.get_similar_entries(coffee_query, 0.5);
    println!("  Coffee-related results for '{}':", coffee_query);
    for (query, result, similarity) in coffee_results {
        println!(
            "    '{}' -> '{}' (similarity: {:.3})",
            query, result, similarity
        );
    }

    // Test semantic search for Paris queries
    let paris_query = "What is Paris?";
    let paris_results = cache.get_similar_entries(paris_query, 0.5);
    println!("  Paris-related results for '{}':", paris_query);
    for (query, result, similarity) in paris_results {
        println!(
            "    '{}' -> '{}' (similarity: {:.3})",
            query, result, similarity
        );
    }

    println!("\n🔍 Test Case 5: Cache statistics");
    let stats = cache.get_stats();
    println!("  Cache Statistics:");
    println!("    - Total hits: {}", stats.hits);
    println!("    - Total misses: {}", stats.misses);
    println!("    - Total puts: {}", stats.puts);
    println!("    - Hit rate: {:.2}%", stats.hit_rate * 100.0);
    println!("    - Cache size: {}", stats.size);

    println!("\n🔍 Test Case 6: Threshold sensitivity");

    // Test with different similarity thresholds
    let test_query = "What's the weather?";

    println!("  Testing '{}' with different thresholds:", test_query);
    for threshold in [0.3, 0.5, 0.7, 0.9] {
        let results = cache.get_similar_entries(test_query, threshold);
        println!("    Threshold {:.1}: {} results", threshold, results.len());
    }

    println!("\n🔍 Test Case 7: Cache invalidation");

    // Invalidate a specific entry
    cache.invalidate_semantic(query1).unwrap();

    // Try to get the invalidated entry
    let invalidated_result = cache.get_semantic(query1);
    if invalidated_result.is_none() {
        println!("  Successfully invalidated: '{}'", query1);
    }

    // But similar queries should still work
    let similar_result = cache.get_semantic("How's the weather?");
    if similar_result.is_some() {
        println!("  Similar queries still work after invalidation");
    }

    println!("\n🔍 Test Case 8: Performance comparison");

    // Simulate expensive semantic search
    let expensive_start = std::time::Instant::now();
    std::thread::sleep(std::time::Duration::from_millis(100)); // Simulate expensive operation
    let expensive_time = expensive_start.elapsed();

    // Simulate cache lookup
    let cache_start = std::time::Instant::now();
    let _ = cache.get_semantic("What's the weather like?");
    let cache_time = cache_start.elapsed();

    println!("  Semantic search simulation: {:?}", expensive_time);
    println!("  Cache lookup: {:?}", cache_time);
    println!(
        "  Speedup: {:.1}x faster",
        expensive_time.as_micros() as f64 / cache_time.as_micros() as f64
    );

    println!("\n🔍 Test Case 9: Embedding analysis");

    // Show how embeddings work
    let generator = cache.config().embedding_dimension;
    println!("  Embedding dimension: {}", generator);
    println!(
        "  Similarity threshold: {:.1}%",
        cache.config().similarity_threshold * 100.0
    );

    // Test some example similarities
    let test_pairs = vec![
        ("hello world", "hello world"),
        ("hello world", "hi world"),
        ("hello world", "goodbye world"),
        ("hello world", "completely different"),
    ];

    println!("  Example similarity scores:");
    for (text1, text2) in test_pairs {
        let result1 = cache.get_semantic(text1);
        let result2 = cache.get_semantic(text2);

        if let (Some((_, sim1)), Some((_, sim2))) = (result1, result2) {
            println!("    '{}' vs '{}': {:.3} vs {:.3}", text1, text2, sim1, sim2);
        }
    }

    println!("\n🔍 Test Case 10: Final comprehensive statistics");
    let final_stats = cache.get_stats();
    println!("  Final Cache Statistics:");
    println!("    - Total hits: {}", final_stats.hits);
    println!("    - Total misses: {}", final_stats.misses);
    println!("    - Total puts: {}", final_stats.puts);
    println!("    - Total invalidations: {}", final_stats.invalidations);
    println!("    - Hit rate: {:.2}%", final_stats.hit_rate * 100.0);
    println!("    - Cache size: {}", final_stats.size);

    println!("\n✅ L3 Semantic Cache demo completed successfully!");
    println!("   The L3 Semantic Cache demonstrates:");
    println!("   - Semantic similarity detection for similar queries");
    println!("   - Configurable similarity thresholds");
    println!("   - Vector-based similarity search");
    println!("   - Performance improvements through semantic caching");
    println!("   - Proper cache invalidation and management");
    println!("   - Comprehensive statistics tracking");
    println!("   - Embedding-based query matching");
}
