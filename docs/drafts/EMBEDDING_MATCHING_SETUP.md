# Embedding-Based Semantic Matching Setup

This document explains how to configure and use embedding-based semantic matching for capability discovery.

## Overview

The embedding service provides **vector-based semantic similarity** calculation, which is more accurate than keyword-based matching. It supports:

1. **OpenRouter API** - Uses various embedding models via OpenRouter
2. **Local Models** - Uses local embedding models (e.g., Ollama)

## Configuration

### Option 1: OpenRouter (Recommended)

```bash
export OPENROUTER_API_KEY="your_api_key_here"
export EMBEDDING_MODEL="text-embedding-ada-002"  # Optional, defaults to text-embedding-ada-002
```

**Available Models:**
- `text-embedding-ada-002` (OpenAI, 1536 dimensions)
- `text-embedding-3-small` (OpenAI, 1536 dimensions)
- `text-embedding-3-large` (OpenAI, 3072 dimensions)
- `text-embedding-v3` (OpenAI, various sizes)

### Option 2: Local Model (e.g., Ollama)

```bash
export LOCAL_EMBEDDING_URL="http://localhost:11434/api"
export LOCAL_EMBEDDING_MODEL="nomic-embed-text"  # Optional, defaults to nomic-embed-text
```

**Setting up Ollama:**

```bash
# Install Ollama (if not installed)
curl -fsSL https://ollama.ai/install.sh | sh

# Pull an embedding model
ollama pull nomic-embed-text

# Start Ollama (if not running)
ollama serve
```

## How It Works

### Automatic Detection

The system automatically detects which embedding provider to use:

1. Checks for `OPENROUTER_API_KEY` → Uses OpenRouter
2. If not found, checks for `LOCAL_EMBEDDING_URL` → Uses local model
3. If neither found → Falls back to keyword-based matching

### Integration

Embedding-based matching is automatically integrated into the discovery pipeline:

```rust
// In discovery/engine.rs
let mut embedding_service = EmbeddingService::from_env();

for manifest in &capabilities {
    let desc_score = if let Some(ref mut emb_svc) = embedding_service {
        // Uses embedding similarity (more accurate)
        calculate_description_match_score_with_embedding_async(
            &need.rationale,
            &manifest.description,
            &manifest.name,
            Some(emb_svc),
        ).await
    } else {
        // Falls back to keyword-based matching
        calculate_description_match_score(
            &need.rationale,
            &manifest.description,
            &manifest.name,
        )
    };
}
```

## Benefits Over Keyword Matching

### 1. Semantic Understanding

**Keyword Matching:**
- "get issues" ≠ "retrieve issues" (different keywords)
- "list issues" = "list issues" (exact match)

**Embedding Matching:**
- "get issues" ≈ "retrieve issues" (semantically similar)
- "list issues" ≈ "show issues" (semantically similar)
- Handles synonyms and paraphrasing

### 2. Context Awareness

Embeddings capture:
- Word relationships (synonyms, antonyms)
- Semantic context (what words mean together)
- Domain-specific relationships

### 3. Better Accuracy

Example matching scenarios:

| Need | Capability Description | Keyword Score | Embedding Score |
|------|----------------------|---------------|-----------------|
| "List all open issues" | "List issues in a GitHub repository" | 0.67 | 0.89 |
| "Get repository issues" | "List issues in a GitHub repository" | 0.50 | 0.85 |
| "Retrieve GitHub issues" | "List issues in a GitHub repository" | 0.50 | 0.87 |
| "Fetch open issues" | "List issues in a GitHub repository" | 0.33 | 0.79 |

## Performance Considerations

### Caching

The embedding service includes an **in-memory cache** to avoid redundant API calls:

```rust
// Same text generates embedding once, then cached
service.embed("List issues")  // API call
service.embed("List issues")  // Cached (no API call)
```

### Cost

**OpenRouter:**
- Pay-per-use pricing
- Cache reduces costs
- ~$0.0001 per embedding (varies by model)

**Local Models:**
- No API costs
- Requires local GPU/CPU resources
- Slower but private

## Testing

### Test Local Embedding Service

```bash
# Set up environment (if not using defaults)
export LOCAL_EMBEDDING_URL="http://localhost:11434/api"
export LOCAL_EMBEDDING_MODEL="nomic-embed-text"

# Run the test
cargo run --example test_local_embedding
```

This will:
- Test embedding generation
- Calculate semantic similarity between texts
- Verify caching works
- Show performance metrics

### Test OpenRouter Embedding Service

```bash
# Set up environment
export OPENROUTER_API_KEY="your_key"

# Run test
cargo test --lib --package ccos embedding_service::tests::test_openrouter_embedding -- --nocapture
```

### Test Discovery with Embeddings

```bash
# Configure embedding service
export OPENROUTER_API_KEY="your_key"
export EMBEDDING_MODEL="text-embedding-ada-002"

# Run discovery example
cargo run --example test_mcp_discovery_direct
```

The example will automatically:
1. Detect embedding service from environment
2. Use embedding-based matching if available
3. Fall back to keyword matching if not

## Troubleshooting

### Embedding Service Not Found

**Symptom:** Logs show "⚠️ Embedding matching failed, falling back to keyword matching"

**Solutions:**
1. Check environment variables are set
2. Verify API key is valid (for OpenRouter)
3. Verify local model is running (for local models)
4. Check network connectivity

### Low Similarity Scores

**Symptom:** Embedding scores are unexpectedly low (< 0.5)

**Solutions:**
1. Try a different embedding model
2. Check if rationale and description are semantically related
3. Consider fine-tuning threshold (currently 0.5)

### Performance Issues

**Symptom:** Discovery is slow

**Solutions:**
1. Enable caching (automatic)
2. Use faster embedding model (e.g., `text-embedding-3-small`)
3. Consider batch embedding requests (future enhancement)

## Future Enhancements

- [ ] Batch embedding generation
- [ ] Persistent embedding cache (disk-based)
- [ ] Support for more embedding providers
- [ ] Hybrid scoring (keyword + embedding)
- [ ] Fine-tuning threshold per use case

