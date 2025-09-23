# Caching Architecture: L1 Delegation Cache

_Status: **SPECIFICATION** – Ready for implementation_

---

## 1. Purpose

This document outlines the architecture and implementation plan for the **Layer 1 Delegation Cache**. The primary goal of this layer is to accelerate the CCOS by memoizing the decisions made by the `DelegationEngine`. This avoids the performance cost of re-evaluating the same `CallContext` repeatedly.

## 2. Core Architecture

The L1 cache is a high-speed, distributed key-value store that sits between the `RTFS Evaluator` and the `DelegationEngine`.

-   **Key:** The 64-bit hash of the `CallContext` struct. This includes the function symbol, a fingerprint of its argument types, and a hash of the ambient runtime context (permissions, etc.).
-   **Value:** The `ExecTarget` enum returned by the `DelegationEngine`.
-   **Technology:** The cache will be implemented using a distributed, in-memory data store like **Redis** or **Memcached** to ensure low latency and shareability across the entire CCOS.

### Architectural Diagram

+----------------+ 1. Build CallContext +-----------------+ | RTFS Evaluator | -----------------------> | L1 Cache Client | +----------------+ +-----------------+ | 2. Cache Lookup (by hash) | +----------------v----------------+ | L1 Cache (Redis) | +----------------^----------------+ | 5. Cache Write +--------------------+ 4. Return Target +------------------+ | Delegation Engine | <------------------- | L1 Cache Client | +--------------------+ 3. Cache Miss +------------------+## 3. Implementation Plan

### Milestone 1: Local, In-Memory Cache (Reference Implementation)

**Goal:** Implement a basic, thread-safe, in-memory LRU cache within the `DelegationEngine` itself.

1.  **Modify `DelegationEngine` Trait:**
    *   Add a `cache` field to the `DelegationEngine` struct, typed as `Arc<Mutex<LruCache<u64, ExecTarget>>>`.
2.  **Update `decide` Method:**
    *   Before calling the core decision logic, acquire a lock on the cache and perform a lookup using the `CallContext` hash.
    *   On a cache hit, return the cached `ExecTarget` immediately.
    *   On a cache miss, proceed with the decision logic, and before returning the result, write the new `(hash, ExecTarget)` pair to the cache.
3.  **Add Metrics:**
    *   Implement Prometheus/OpenTelemetry counters for `l1_cache_hits` and `l1_cache_misses`.

### Milestone 2: Distributed Cache Integration (Production)

**Goal:** Replace the local LRU cache with a production-grade, distributed Redis client.

1.  **Introduce Redis Client:**
    *   Add the `redis-rs` crate as a dependency.
    *   Create a `RedisCacheProvider` struct that connects to a Redis instance specified in the CCOS configuration.
2.  **Abstract Cache Backend:**
    *   Define a `DelegationCache` trait with `get(key)` and `set(key, value)` methods.
    *   Implement this trait for both the local `LruCache` (from M1) and the new `RedisCacheProvider`.
3.  **Update `DelegationEngine`:**
    *   Change the `cache` field to be a `Box<dyn DelegationCache>`.
    *   The CCOS will instantiate either the local or Redis provider at startup based on configuration, making the backend pluggable.

### Milestone 3: Cache Invalidation and TTL

**Goal:** Implement a strategy for cache invalidation to handle changes in the system.

1.  **Time-to-Live (TTL):**
    *   When setting a key in Redis, apply a default TTL (e.g., 1 hour). This ensures that stale decisions are eventually purged.
2.  **Event-Based Invalidation:**
    *   The `DelegationEngine` will subscribe to a system-wide event bus (e.g., NATS).
    *   When a relevant event occurs (e.g., `policy-updated`, `capability-added`), the engine will issue a `FLUSHDB` command or selectively invalidate keys related to the updated component.

## 4. Performance Goals

-   **P99 Latency (Cache Hit):** < 1ms
-   **P99 Latency (Cache Miss):** < 5ms (excluding core decision logic)
-   **Target Hit Rate:** > 95% for common workloads.
