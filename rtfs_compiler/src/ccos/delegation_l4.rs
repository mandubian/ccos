use std::sync::Arc;

use super::caching::l4_content_addressable::{L4CacheClient, RtfsModuleMetadata};
use super::delegation::{CallContext, DelegationEngine, ExecTarget};

/// Decorator that checks the L4 content-addressable cache before falling back
/// to the wrapped `DelegationEngine` implementation.
#[derive(Debug)]
pub struct L4AwareDelegationEngine<DE: DelegationEngine> {
    l4_client: Arc<L4CacheClient>,
    inner: DE,
    /// Minimum cosine similarity score required for a semantic hit
    similarity_threshold: f32,
}

impl<DE: DelegationEngine> L4AwareDelegationEngine<DE> {
    pub fn new(l4_client: L4CacheClient, inner: DE) -> Self {
        Self {
            l4_client: Arc::new(l4_client),
            inner,
            similarity_threshold: 0.95,
        }
    }

    /// Convenience constructor with custom threshold
    pub fn with_threshold(l4_client: L4CacheClient, inner: DE, threshold: f32) -> Self {
        Self {
            l4_client: Arc::new(l4_client),
            inner,
            similarity_threshold: threshold,
        }
    }
}

impl<DE: DelegationEngine> DelegationEngine for L4AwareDelegationEngine<DE> {
    fn decide(&self, ctx: &CallContext) -> ExecTarget {
        // Build interface hash (placeholder – a real implementation would hash the
        // function signature). For now we use the symbol name.
        let interface_hash = ctx.fn_symbol;
        let embedding_ref = ctx.semantic_hash.as_deref();

        if let Some(meta) = self
            .l4_client
            .query(interface_hash, embedding_ref, self.similarity_threshold)
        {
            return ExecTarget::L4CacheHit {
                storage_pointer: meta.storage_pointer,
                signature: meta.signature,
            };
        }

        // Fallback to the wrapped engine
        self.inner.decide(ctx)
    }
} 