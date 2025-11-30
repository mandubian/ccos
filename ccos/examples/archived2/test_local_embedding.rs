//! Test local embedding service
//!
//! This example tests the embedding service with a local model (e.g., Ollama).
//! It demonstrates embedding-based semantic similarity calculation.

use ccos::discovery::embedding_service::{EmbeddingProvider, EmbeddingService};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🧪 Testing Local Embedding Service\n");
    println!("{}", "═".repeat(80));

    // Get configuration from environment
    let base_url = std::env::var("LOCAL_EMBEDDING_URL")
        .unwrap_or_else(|_| "http://localhost:11434/api".to_string());
    let model =
        std::env::var("LOCAL_EMBEDDING_MODEL").unwrap_or_else(|_| "nomic-embed-text".to_string());

    println!("📋 Configuration:");
    println!("  Base URL: {}", base_url);
    println!("  Model: {}", model);
    println!();

    // Create embedding service
    let provider = EmbeddingProvider::Local {
        base_url: base_url.clone(),
        model: model.clone(),
    };

    let mut service = EmbeddingService::new(provider);

    println!("🔍 Testing Embedding Generation\n");

    // Test 1: Generate embedding for a simple text
    println!("Test 1: Simple text embedding");
    let text1 = "List all open issues in a GitHub repository";
    let embedding1 = match service.embed(text1).await {
        Ok(embedding) => {
            println!(
                "  ✓ Success! Generated embedding of {} dimensions",
                embedding.len()
            );
            println!(
                "  First 5 values: {:?}",
                &embedding[..embedding.len().min(5)]
            );
            embedding
        }
        Err(e) => {
            println!("  ✗ Failed: {}", e);
            println!();
            println!("💡 Troubleshooting:");
            println!("  • Check if Ollama is running: ollama serve");
            println!("  • Verify model is installed: ollama pull {}", model);
            println!("  • Check if base URL is correct: {}", base_url);
            return Err(format!("Failed to generate embedding: {}", e).into());
        }
    };
    println!();

    // Test 2: Generate embedding for another text
    println!("Test 2: Another text embedding");
    let text2 = "List issues in a GitHub repository";
    let embedding2 = match service.embed(text2).await {
        Ok(embedding) => {
            println!(
                "  ✓ Success! Generated embedding of {} dimensions",
                embedding.len()
            );
            embedding
        }
        Err(e) => {
            println!("  ✗ Failed: {}", e);
            return Err(format!("Failed to generate embedding: {}", e).into());
        }
    };
    println!();

    // Test 3: Calculate similarity between two texts
    println!("Test 3: Semantic similarity calculation");

    let similarity = EmbeddingService::cosine_similarity(&embedding1, &embedding2);
    println!("  Text 1: '{}'", text1);
    println!("  Text 2: '{}'", text2);
    println!("  Similarity: {:.4}", similarity);

    if similarity > 0.8 {
        println!("  ✓ High similarity (semantically very similar)");
    } else if similarity > 0.6 {
        println!("  ✓ Good similarity (semantically related)");
    } else if similarity > 0.4 {
        println!("  ⚠ Moderate similarity (somewhat related)");
    } else {
        println!("  ✗ Low similarity (not very related)");
    }
    println!();

    // Test 4: Compare with different texts
    println!("Test 4: Comparison with different text");
    let text3 = "Delete a file from the filesystem";
    let embedding3 = match service.embed(text3).await {
        Ok(emb) => emb,
        Err(e) => {
            println!("  ✗ Failed to generate embedding: {}", e);
            return Err(format!("Failed to generate embedding: {}", e).into());
        }
    };

    let similarity1_3 = EmbeddingService::cosine_similarity(&embedding1, &embedding3);
    let similarity2_3 = EmbeddingService::cosine_similarity(&embedding2, &embedding3);

    println!("  Text 1: '{}'", text1);
    println!("  Text 3: '{}'", text3);
    println!("  Similarity: {:.4}", similarity1_3);
    println!();
    println!("  Text 2: '{}'", text2);
    println!("  Text 3: '{}'", text3);
    println!("  Similarity: {:.4}", similarity2_3);
    println!();

    if similarity > similarity1_3 && similarity > similarity2_3 {
        println!("  ✓ Similar texts (1 & 2) have higher similarity than different texts");
    }
    println!();

    // Test 5: Cache test
    println!("Test 5: Caching test");
    let start = std::time::Instant::now();
    let _ = match service.embed(text1).await {
        Ok(emb) => emb,
        Err(e) => {
            println!("  ✗ Failed: {}", e);
            return Err(format!("Failed to generate embedding: {}", e).into());
        }
    };
    let first_call = start.elapsed();

    let start = std::time::Instant::now();
    let _ = match service.embed(text1).await {
        Ok(emb) => emb,
        Err(e) => {
            println!("  ✗ Failed: {}", e);
            return Err(format!("Failed to generate embedding: {}", e).into());
        }
    };
    let second_call = start.elapsed();

    println!("  First call: {:?}", first_call);
    println!("  Second call (cached): {:?}", second_call);
    if second_call < first_call {
        println!("  ✓ Cache is working (second call faster)");
    }
    println!();

    println!("{}", "═".repeat(80));
    println!("✅ All tests passed!");
    println!();
    println!("💡 Next steps:");
    println!(
        "  • Use this in capability matching: export LOCAL_EMBEDDING_URL=\"{}\"",
        base_url
    );
    println!("  • Run discovery examples with: cargo run --example test_mcp_discovery_direct");

    Ok(())
}
