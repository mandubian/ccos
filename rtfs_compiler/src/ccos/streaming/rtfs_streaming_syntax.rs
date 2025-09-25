use crate::ast::{Expression, Keyword, Literal, MapKey, Symbol};
use crate::ccos::capability_marketplace::CapabilityMarketplace;
use crate::runtime::error::RuntimeResult;
use crate::runtime::streaming::{StreamConfig, StreamHandle, StreamType, StreamingCapability};
use crate::runtime::values::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::mpsc;
use uuid::Uuid;

/// Minimal local streaming provider for tests
pub struct LocalStreamingProvider;

#[async_trait::async_trait]
impl StreamingCapability for LocalStreamingProvider {
    fn start_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        let (tx, _rx) = tokio::sync::mpsc::channel::<()>(1);
        Ok(StreamHandle {
            stream_id: Uuid::new_v4().to_string(),
            stop_tx: tx,
        })
    }
    fn stop_stream(&self, _handle: &StreamHandle) -> RuntimeResult<()> {
        // Signal shutdown if needed; ignore errors in tests
        let _ = _handle.stop_tx.clone().try_send(());
        Ok(())
    }
    async fn start_stream_with_config(
        &self,
        _params: &Value,
        _config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        let (tx, _rx) = tokio::sync::mpsc::channel::<()>(1);
        Ok(StreamHandle {
            stream_id: Uuid::new_v4().to_string(),
            stop_tx: tx,
        })
    }
    async fn send_to_stream(&self, _handle: &StreamHandle, _data: &Value) -> RuntimeResult<()> {
        Ok(())
    }
    fn start_bidirectional_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        let (tx, _rx) = tokio::sync::mpsc::channel::<()>(1);
        Ok(StreamHandle {
            stream_id: Uuid::new_v4().to_string(),
            stop_tx: tx,
        })
    }
    async fn start_bidirectional_stream_with_config(
        &self,
        _params: &Value,
        _config: &StreamConfig,
    ) -> RuntimeResult<StreamHandle> {
        let (tx, _rx) = tokio::sync::mpsc::channel::<()>(1);
        Ok(StreamHandle {
            stream_id: Uuid::new_v4().to_string(),
            stop_tx: tx,
        })
    }
}

// -------------------------------------------------------------------------------------------------
// Minimal macro lowering for (mcp-stream <endpoint> <processor-fn> <initial-state?>)
// Produces: (call :mcp.stream.start { :endpoint "..." :processor "..." :initial-state <value> })
// This is an initial ergonomic helper; in future we may move macro expansion earlier in parse.

/// Attempt to detect and lower the simple (mcp-stream ...) surface form into the canonical
/// capability call expression expected by the runtime. This keeps RTFS programs concise while
/// reusing the existing `(call :mcp.stream.start {...})` pathway described in the spec.
pub fn maybe_lower_mcp_stream_macro(expr: &Expression) -> Expression {
    // Internal helper to extract symbol name
    fn symbol_name(e: &Expression) -> Option<String> {
        if let Expression::Symbol(Symbol(s)) = e {
            Some(s.clone())
        } else {
            None
        }
    }
    // List structure is raw Vec<Expression>
    if let Expression::List(items) = expr {
        if items.is_empty() {
            return expr.clone();
        }
        if let Some(head) = items.get(0) {
            if let Some(sym) = symbol_name(head) {
                if sym == "mcp-stream" {
                    // Need at least endpoint and processor
                    if items.len() < 3 {
                        return expr.clone();
                    }
                    // Endpoint literal (symbol or string literal currently represented as Literal::String)
                    let endpoint = match &items[1] {
                        Expression::Literal(Literal::String(s)) => s.clone(),
                        Expression::Symbol(Symbol(s)) => s.clone(),
                        _ => return expr.clone(),
                    };
                    let processor = match &items[2] {
                        Expression::Symbol(Symbol(s)) => s.clone(),
                        Expression::Literal(Literal::String(s)) => s.clone(),
                        _ => return expr.clone(),
                    };
                    let initial_state = if items.len() > 3 {
                        items[3].clone()
                    } else {
                        Expression::Map(std::collections::HashMap::new())
                    };

                    // Build map with keyword keys (without leading ':') because MapKey::Keyword wraps raw value
                    let mut m = std::collections::HashMap::new();
                    m.insert(
                        MapKey::Keyword(Keyword("endpoint".to_string())),
                        Expression::Literal(Literal::String(endpoint)),
                    );
                    m.insert(
                        MapKey::Keyword(Keyword("processor".to_string())),
                        Expression::Literal(Literal::String(processor)),
                    );
                    m.insert(
                        MapKey::Keyword(Keyword("initial-state".to_string())),
                        initial_state,
                    );
                    let map_expr = Expression::Map(m);

                    // Form: (call :mcp.stream.start { ... })
                    let call_sym = Expression::Symbol(Symbol("call".to_string()));
                    let capability_kw = Expression::Literal(Literal::Keyword(Keyword(
                        "mcp.stream.start".to_string(),
                    )));
                    return Expression::List(vec![call_sym, capability_kw, map_expr]);
                }
            }
        }
    }
    expr.clone()
}
/// Minimal MCP-focused streaming executor
pub struct RtfsStreamingSyntaxExecutor {
    marketplace: Arc<CapabilityMarketplace>,
}

impl RtfsStreamingSyntaxExecutor {
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self { marketplace }
    }

    pub async fn register_mcp_stream(
        &self,
        capability_id: String,
        provider: Arc<dyn StreamingCapability + Send + Sync>,
        metadata: HashMap<String, String>,
    ) -> Result<(), crate::runtime::error::RuntimeError> {
        let name = metadata
            .get("name")
            .cloned()
            .unwrap_or_else(|| capability_id.clone());
        let description = metadata
            .get("description")
            .cloned()
            .unwrap_or_else(|| format!("MCP streaming capability {}", capability_id));

        self.marketplace
            .register_streaming_capability(
                capability_id,
                name,
                description,
                StreamType::Unidirectional,
                provider,
            )
            .await
            .map_err(|e| crate::runtime::error::RuntimeError::Generic(e.to_string()))
    }

    pub async fn start_mcp_stream(
        &self,
        capability_id: &str,
        params: Value,
        config: Option<StreamConfig>,
    ) -> Result<StreamHandle, crate::runtime::error::RuntimeError> {
        let final_config = config.unwrap_or(StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        });

        let handle = self
            .marketplace
            .start_stream_with_config(capability_id, &params, &final_config)
            .await
            .map_err(|e| crate::runtime::error::RuntimeError::Generic(e.to_string()))?;

        let (stop_tx, _stop_rx) = mpsc::channel(1);
        Ok(StreamHandle {
            stream_id: handle.stream_id,
            stop_tx,
        })
    }

    pub async fn stop_stream(
        &self,
        handle: StreamHandle,
    ) -> Result<(), crate::runtime::error::RuntimeError> {
        let mut stop_tx = handle.stop_tx;
        stop_tx.send(()).await.map_err(|e| {
            crate::runtime::error::RuntimeError::Generic(format!("failed to signal stop: {}", e))
        })
    }
}
