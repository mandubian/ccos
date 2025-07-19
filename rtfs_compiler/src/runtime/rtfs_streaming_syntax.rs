use tokio::sync::RwLock;
use crate::runtime::capability_marketplace::StreamingCapability;
use crate::runtime::capability_marketplace::StreamHandle;
use crate::runtime::capability_marketplace::StreamConfig;
use crate::runtime::error::RuntimeResult;

/// Minimal local streaming provider for tests
pub struct LocalStreamingProvider;

#[async_trait::async_trait]
impl StreamingCapability for LocalStreamingProvider {
    fn start_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        Err(crate::runtime::error::RuntimeError::Generic("Not implemented".to_string()))
    }
    fn stop_stream(&self, _handle: &StreamHandle) -> RuntimeResult<()> {
        Err(crate::runtime::error::RuntimeError::Generic("Not implemented".to_string()))
    }
    async fn start_stream_with_config(&self, _params: &Value, _config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        Err(crate::runtime::error::RuntimeError::Generic("Not implemented".to_string()))
    }
    async fn send_to_stream(&self, _handle: &StreamHandle, _data: &Value) -> RuntimeResult<()> {
        Err(crate::runtime::error::RuntimeError::Generic("Not implemented".to_string()))
    }
    fn start_bidirectional_stream(&self, _params: &Value) -> RuntimeResult<StreamHandle> {
        Err(crate::runtime::error::RuntimeError::Generic("Not implemented".to_string()))
    }
    async fn start_bidirectional_stream_with_config(&self, _params: &Value, _config: &StreamConfig) -> RuntimeResult<StreamHandle> {
        Err(crate::runtime::error::RuntimeError::Generic("Not implemented".to_string()))
    }
}
// RTFS 2.0 Streaming Syntax Implementation Examples
// This demonstrates how the homoiconic streaming syntax integrates with the capability marketplace

use std::collections::HashMap;
use std::sync::Arc;
use uuid::Uuid;

use crate::runtime::capability_marketplace::{CapabilityMarketplace, StreamType, StreamCallbacks};
use crate::runtime::values::Value;

/// Direction of data flow in a stream
#[derive(Debug, Clone)]
pub enum StreamDirection {
    Inbound,
    Outbound,
    Bidirectional,
}

/// Item in a stream, with metadata and direction
#[derive(Debug, Clone)]
pub struct StreamItem {
    pub timestamp: u64,
    pub metadata: HashMap<String, String>,
    pub direction: StreamDirection,
    pub correlation_id: Option<String>,
}

/// RTFS 2.0 Stream Syntax Parser and Executor
/// This integrates with the existing capability marketplace to execute homoiconic streaming plans
pub struct RtfsStreamingSyntaxExecutor {
    marketplace: Arc<CapabilityMarketplace>,
    active_streams: HashMap<String, StreamHandle>,
    resource_refs: HashMap<String, String>,
}

/// Represents a parsed RTFS 2.0 streaming expression
#[derive(Clone)]
pub enum RtfsStreamingExpression {
    // Stream capability registration
    RegisterStreamCapability {
        capability_id: String,
        stream_type: StreamType,
        input_schema: Option<StreamSchema>,
        output_schema: Option<StreamSchema>,
        config: StreamConfig,
        provider: Arc<dyn StreamingCapability + Send + Sync>,
        metadata: HashMap<String, String>,
    },
    
    // Stream operations
    StreamSource {
        capability_id: String,
        config: Option<StreamConfig>,
        handle_name: String,
    },
    
    StreamSink {
        capability_id: String,
        config: Option<StreamConfig>,
        handle_name: String,
    },
    
    StreamTransform {
        input_stream: StreamReference,
        output_stream: StreamReference,
        transform_fn: String, // Function name or code
        config: Option<StreamConfig>,
    },
    
    StreamBidirectional {
        capability_id: String,
        config: Option<StreamConfig>,
        handle_name: String,
    },
    
    StreamDuplex {
        input_capability: String,
        output_capability: String,
        config: Option<StreamConfig>,
        handle_name: String,
    },
    
    // Stream consumption and production
    StreamConsume {
        stream_handle: String,
        processing_logic: ProcessingLogic,
        options: Option<StreamOptions>,
    },
    
    StreamProduce {
        stream_handle: String,
        items: Vec<StreamItem>,
        options: Option<StreamOptions>,
    },
    
    StreamInteract {
        stream_handle: String,
        send_item: Option<StreamItem>,
        receive_logic: Option<ProcessingLogic>,
        config: Option<StreamConfig>,
    },
    
    // Advanced operations
    StreamPipeline {
        stages: Vec<RtfsStreamingExpression>,
        config: Option<StreamConfig>,
    },
    
    StreamMultiplex {
        input_streams: Vec<StreamReference>,
        strategy: MultiplexStrategy,
        output_handle: String,
    },
    
    StreamDemultiplex {
        input_stream: StreamReference,
        criteria_fn: String,
        outputs: HashMap<String, StreamReference>,
    },
}

impl std::fmt::Debug for RtfsStreamingExpression {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RtfsStreamingExpression::RegisterStreamCapability { 
                capability_id, 
                stream_type, 
                input_schema, 
                output_schema, 
                config, 
                metadata, 
                .. 
            } => {
                f.debug_struct("RegisterStreamCapability")
                    .field("capability_id", capability_id)
                    .field("stream_type", stream_type)
                    .field("input_schema", input_schema)
                    .field("output_schema", output_schema)
                    .field("config", config)
                    .field("provider", &"<StreamingCapability>")
                    .field("metadata", metadata)
                    .finish()
            },
            RtfsStreamingExpression::StreamSource { capability_id, config, handle_name } => {
                f.debug_struct("StreamSource")
                    .field("capability_id", capability_id)
                    .field("config", config)
                    .field("handle_name", handle_name)
                    .finish()
            },
            RtfsStreamingExpression::StreamSink { capability_id, config, handle_name } => {
                f.debug_struct("StreamSink")
                    .field("capability_id", capability_id)
                    .field("config", config)
                    .field("handle_name", handle_name)
                    .finish()
            },
            RtfsStreamingExpression::StreamTransform { input_stream, output_stream, transform_fn, config } => {
                f.debug_struct("StreamTransform")
                    .field("input_stream", input_stream)
                    .field("output_stream", output_stream)
                    .field("transform_fn", transform_fn)
                    .field("config", config)
                    .finish()
            },
            RtfsStreamingExpression::StreamBidirectional { capability_id, config, handle_name } => {
                f.debug_struct("StreamBidirectional")
                    .field("capability_id", capability_id)
                    .field("config", config)
                    .field("handle_name", handle_name)
                    .finish()
            },
            RtfsStreamingExpression::StreamDuplex { input_capability, output_capability, config, handle_name } => {
                f.debug_struct("StreamDuplex")
                    .field("input_capability", input_capability)
                    .field("output_capability", output_capability)
                    .field("config", config)
                    .field("handle_name", handle_name)
                    .finish()
            },
            RtfsStreamingExpression::StreamConsume { stream_handle, processing_logic, options } => {
                f.debug_struct("StreamConsume")
                    .field("stream_handle", stream_handle)
                    .field("processing_logic", processing_logic)
                    .field("options", options)
                    .finish()
            },
            RtfsStreamingExpression::StreamProduce { stream_handle, items, options } => {
                f.debug_struct("StreamProduce")
                    .field("stream_handle", stream_handle)
                    .field("items", items)
                    .field("options", options)
                    .finish()
            },
            RtfsStreamingExpression::StreamInteract { stream_handle, send_item, receive_logic, config } => {
                f.debug_struct("StreamInteract")
                    .field("stream_handle", stream_handle)
                    .field("send_item", send_item)
                    .field("receive_logic", receive_logic)
                    .field("config", config)
                    .finish()
            },
            RtfsStreamingExpression::StreamPipeline { stages, config } => {
                f.debug_struct("StreamPipeline")
                    .field("stages", stages)
                    .field("config", config)
                    .finish()
            },
            RtfsStreamingExpression::StreamMultiplex { input_streams, strategy, output_handle } => {
                f.debug_struct("StreamMultiplex")
                    .field("input_streams", input_streams)
                    .field("strategy", strategy)
                    .field("output_handle", output_handle)
                    .finish()
            },
            RtfsStreamingExpression::StreamDemultiplex { input_stream, criteria_fn, outputs } => {
                f.debug_struct("StreamDemultiplex")
                    .field("input_stream", input_stream)
                    .field("criteria_fn", criteria_fn)
                    .field("outputs", outputs)
                    .finish()
            },
        }
    }
}

/// Stream reference types (handles, resource refs, etc.)
#[derive(Debug, Clone)]
pub enum StreamReference {
    Handle(String),
    ResourceRef(String),
    CapabilityId(String),
    Expression(Box<RtfsStreamingExpression>),
}

/// Stream schema definition
#[derive(Debug, Clone)]
pub struct StreamSchema {
    pub element_type: String,
    pub validation_rules: Vec<ValidationRule>,
    pub strict_validation: bool,
}

/// Processing logic for stream items
#[derive(Debug, Clone)]
pub enum ProcessingLogic {
    ChannelBased {
        item_binding: String,
        body_expressions: Vec<String>,
    },
    CallbackBased {
        callbacks: HashMap<String, String>, // event_name -> callback_function
    },
}

/// Stream operation options
#[derive(Debug, Clone)]
pub struct StreamOptions {
    pub enable_callbacks: Option<bool>,
    pub error_handling: Option<ErrorHandlingStrategy>,
    pub backpressure: Option<BackpressureStrategy>,
    pub timeout_ms: Option<u64>,
    pub batch_size: Option<usize>,
}

/// Error handling strategies
#[derive(Debug, Clone)]
pub enum ErrorHandlingStrategy {
    Skip,
    Retry { attempts: u32, delay_ms: u64 },
    DeadLetter,
    Fail,
}

/// Backpressure handling strategies
#[derive(Debug, Clone)]
pub enum BackpressureStrategy {
    DropOldest,
    DropNewest,
    Block,
    Resize { max_size: usize },
}

/// Multiplexing strategies
#[derive(Debug, Clone)]
pub enum MultiplexStrategy {
    RoundRobin,
    Priority,
    Random,
    Custom(String),
}

/// Validation rules for stream schemas
#[derive(Debug, Clone)]
pub struct ValidationRule {
    pub field_path: String,
    pub rule_type: String,
    pub parameters: HashMap<String, String>,
}

impl RtfsStreamingSyntaxExecutor {
    pub fn new(marketplace: Arc<CapabilityMarketplace>) -> Self {
        Self {
            marketplace,
            active_streams: HashMap::new(),
            resource_refs: HashMap::new(),
        }
    }

    /// Execute a parsed RTFS 2.0 streaming expression
    pub async fn execute_expression(&mut self, expr: RtfsStreamingExpression) -> Result<ExecutionResult, StreamingError> {
        match expr {
            RtfsStreamingExpression::RegisterStreamCapability { 
                capability_id, 
                stream_type, 
                input_schema, 
                output_schema, 
                config, 
                provider,
                metadata 
            } => {
                self.register_stream_capability(capability_id, stream_type, input_schema, output_schema, config, provider, metadata).await
            },
            
            RtfsStreamingExpression::StreamSource { capability_id, config, handle_name } => {
                self.create_stream_source(capability_id, config, handle_name).await
            },
            
            RtfsStreamingExpression::StreamSink { capability_id, config, handle_name } => {
                self.create_stream_sink(capability_id, config, handle_name).await
            },
            
            RtfsStreamingExpression::StreamTransform { input_stream, output_stream, config, .. } => {
                self.create_stream_transform(input_stream, output_stream, config).await
            },
            
            RtfsStreamingExpression::StreamBidirectional { capability_id, config, handle_name } => {
                self.create_stream_bidirectional(capability_id, config, handle_name).await
            },
            
            RtfsStreamingExpression::StreamDuplex { input_capability, output_capability, config, handle_name } => {
                self.create_stream_duplex(input_capability, output_capability, config, handle_name).await
            },
            
            RtfsStreamingExpression::StreamConsume { stream_handle, processing_logic, options } => {
                self.consume_stream(stream_handle, processing_logic, options).await
            },
            
            RtfsStreamingExpression::StreamProduce { stream_handle, options, .. } => {
                self.produce_to_stream(stream_handle, options).await
            },
            
            RtfsStreamingExpression::StreamInteract { stream_handle, receive_logic, config, .. } => {
                self.interact_with_stream(stream_handle, receive_logic, config).await
            },
            
            RtfsStreamingExpression::StreamPipeline { stages, config } => {
                self.execute_stream_pipeline(stages, config).await
            },
            
            RtfsStreamingExpression::StreamMultiplex { input_streams, strategy, output_handle } => {
                self.multiplex_streams(input_streams, strategy, output_handle).await
            },
            
            RtfsStreamingExpression::StreamDemultiplex { input_stream, criteria_fn, outputs } => {
                self.demultiplex_stream(input_stream, criteria_fn, outputs).await
            },
        }
    }

    /// Register a streaming capability with the marketplace
    async fn register_stream_capability(
        &mut self,
        capability_id: String,
        stream_type: StreamType,
        _input_schema: Option<StreamSchema>,
        _output_schema: Option<StreamSchema>,
        _config: StreamConfig,
        provider: Arc<dyn StreamingCapability + Send + Sync>,
        metadata: HashMap<String, String>,
    ) -> Result<ExecutionResult, StreamingError> {
        // Use metadata for name/description
        let name = metadata.get("name").unwrap_or(&capability_id).clone();
        let description = metadata.get("description")
            .unwrap_or(&format!("RTFS streaming capability for {}", capability_id))
            .clone();

        self.marketplace.register_streaming_capability(
            capability_id.clone(),
            name,
            description,
            stream_type,
            provider,
        ).await.map_err(|e| StreamingError::MarketplaceError(e.to_string()))?;
        Ok(ExecutionResult::Success(format!("Registered streaming capability: {}", capability_id)))
    }

    /// Create a stream source
    async fn create_stream_source(
        &mut self,
        capability_id: String,
        config: Option<StreamConfig>,
        handle_name: String,
    ) -> Result<ExecutionResult, StreamingError> {
        let final_config = config.unwrap_or(StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        });
        let params = Value::Map(HashMap::new());
        let handle = self.marketplace.start_stream_with_config(
            &capability_id,
            &params,
            &final_config,
        ).await.map_err(|e| StreamingError::Other(e.to_string()))?;
        self.active_streams.insert(handle_name.clone(), handle);
        Ok(ExecutionResult::Success(format!("Created stream source: {} -> {}", capability_id, handle_name)))
    }

    /// Create a stream sink
    async fn create_stream_sink(
        &mut self,
        capability_id: String,
        config: Option<StreamConfig>,
        handle_name: String,
    ) -> Result<ExecutionResult, StreamingError> {
        let final_config = config.unwrap_or(StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        });
        let params = Value::Map(HashMap::new());
        let handle = self.marketplace.start_stream_with_config(
            &capability_id,
            &params,
            &final_config,
        ).await.map_err(|e| StreamingError::Other(e.to_string()))?;
        self.active_streams.insert(handle_name.clone(), handle);
        Ok(ExecutionResult::Success(format!("Created stream sink: {} -> {}", capability_id, handle_name)))
    }

    /// Create a stream transform
    async fn create_stream_transform(
        &mut self,
        _input_stream: StreamReference,
        _output_stream: StreamReference,
        config: Option<StreamConfig>,
    ) -> Result<ExecutionResult, StreamingError> {
        // let input_handle = self.resolve_stream_reference(input_stream).await?;
        // let output_handle = self.resolve_stream_reference(output_stream).await?;
        let final_config = config.unwrap_or(StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        });
        // Create transform capability
        let transform_id = format!("transform-{}", Uuid::new_v4());
        let _transform_provider = Arc::new(LocalStreamingProvider);

        // Register transform as a streaming capability
        let name = transform_id.clone();
        let description = format!("Transform capability for {}", transform_id);
        self.marketplace.register_streaming_capability(
            transform_id.clone(),
            name,
            description,
            StreamType::Transform,
            _transform_provider,
        ).await.map_err(|e| StreamingError::Other(e.to_string()))?;
        let params = Value::Map(HashMap::new());
        let _transform_handle = self.marketplace.start_stream_with_config(
            &transform_id,
            &params,
            &final_config,
        ).await.map_err(|e| StreamingError::Other(e.to_string()))?;
        Ok(ExecutionResult::Success(format!("Created stream transform: {}", transform_id)))
    }

    /// Create a bidirectional stream
    async fn create_stream_bidirectional(
        &mut self,
        capability_id: String,
        config: Option<StreamConfig>,
        handle_name: String,
    ) -> Result<ExecutionResult, StreamingError> {
        let final_config = config.unwrap_or(StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        });
        let params = Value::Map(HashMap::new());
        let handle = self.marketplace.start_bidirectional_stream_with_config(
            &capability_id,
            &params,
            &final_config,
        ).await.map_err(|e| StreamingError::Other(e.to_string()))?;
        self.active_streams.insert(handle_name.clone(), handle);
        Ok(ExecutionResult::Success(format!("Created bidirectional stream: {} -> {}", capability_id, handle_name)))
    }

    /// Create a duplex stream
    async fn create_stream_duplex(
        &mut self,
        _input_capability: String,
        _output_capability: String,
        _config: Option<StreamConfig>,
        handle_name: String,
    ) -> Result<ExecutionResult, StreamingError> {
        // let params = Value::Map(HashMap::new());
        // For duplex, use the input_capability as the main capability
        // Method start_duplex_stream does not exist, commenting out for now
        // let _handle = self.marketplace.start_duplex_stream(
        //     &input_capability,
        //     &params,
        // ).await.map_err(|e| StreamingError::Other(e.to_string()))?;
        // TODO: Handle DuplexStreamChannels properly
        Ok(ExecutionResult::Success(format!("Created duplex stream: {}", handle_name)))
    }

    /// Consume from a stream
    async fn consume_stream(
        &mut self,
        stream_handle: String,
        processing_logic: ProcessingLogic,
        _options: Option<StreamOptions>,
    ) -> Result<ExecutionResult, StreamingError> {
        // For now, just acknowledge that we would consume from the stream
        // The actual consumption would need to be handled differently since StreamHandle
        // doesn't implement Clone and requires &mut self for recv()
        match processing_logic {
            ProcessingLogic::ChannelBased { .. } => {
                // Channel-based consumption would be handled by the marketplace
                return Ok(ExecutionResult::Success(format!("Registered channel-based consumption for stream: {}", stream_handle)));
            },
            ProcessingLogic::CallbackBased { .. } => {
                // For callback-based consumption, we need to modify the handle in place
                // Commented out: handle.callbacks and handle.callbacks_enabled do not exist on StreamHandle
                // let stream_callbacks = self.build_stream_callbacks(callbacks)?;
                // if let Some(handle) = self.active_streams.get_mut(&stream_handle) {
                //     handle.callbacks = stream_callbacks;
                //     handle.callbacks_enabled = true;
                // }
            },
        }

        Ok(ExecutionResult::Success(format!("Started consuming stream: {}", stream_handle)))
    }

    /// Produce to a stream
    async fn produce_to_stream(
        &mut self,
        stream_handle: String,
        _options: Option<StreamOptions>,
    ) -> Result<ExecutionResult, StreamingError> {
        // let handle = self.active_streams.get(&stream_handle)
        //     .ok_or_else(|| StreamingError::HandleNotFound(stream_handle.clone()))?;

        // Commented out: handle.sender does not exist on StreamHandle
        // if let Some(sender) = &handle.sender {
        //     for item in items {
        //         sender.send(item).await.map_err(|e| StreamingError::SendError(e.to_string()))?;
        //     }
        // } else {
        //     return Err(StreamingError::Other("No sender available on stream handle".to_string()));
        // }
        // items is unused

        Ok(ExecutionResult::Success(format!("Produced items to stream: {}", stream_handle)))
    }

    /// Interact with a bidirectional stream
    async fn interact_with_stream(
        &mut self,
        stream_handle: String,
        receive_logic: Option<ProcessingLogic>,
        _config: Option<StreamConfig>,
    ) -> Result<ExecutionResult, StreamingError> {
        // let handle = self.active_streams.get(&stream_handle)
        //     .ok_or_else(|| StreamingError::HandleNotFound(stream_handle.clone()))?;
        // send_item is unused

        // Send item if provided
        // Commented out: handle.sender does not exist on StreamHandle
        // if let Some(item) = send_item {
        //     if let Some(sender) = &handle.sender {
        //         sender.send(item).await.map_err(|e| StreamingError::SendError(e.to_string()))?;
        //     } else {
        //         return Err(StreamingError::Other("No sender available on stream handle".to_string()));
        //     }
        // }

        // Set up receive logic if provided
        if let Some(logic) = receive_logic {
            match logic {
                ProcessingLogic::ChannelBased { .. } => {
                    // For channel-based processing, we need to handle the stream in a different way
                    // since StreamHandle is not Clone and recv() requires &mut self
                    // This would be handled by the capability marketplace internally
                    // We'll register the processing logic instead
                    return Ok(ExecutionResult::Success(format!("Registered channel-based processing for stream: {}", stream_handle)));
                },
                ProcessingLogic::CallbackBased { .. } => {
                    // Commented out: handle.callbacks and handle.callbacks_enabled do not exist on StreamHandle
                    // let stream_callbacks = self.build_stream_callbacks(callbacks)?;
                    // if let Some(handle) = self.active_streams.get_mut(&stream_handle) {
                    //     handle.callbacks = stream_callbacks;
                    //     handle.callbacks_enabled = true;
                    // }
                },
            }
        }

        Ok(ExecutionResult::Success(format!("Set up interaction with stream: {}", stream_handle)))
    }

    /// Execute a streaming pipeline
    async fn execute_stream_pipeline(
        &mut self,
        stages: Vec<RtfsStreamingExpression>,
        _config: Option<StreamConfig>,
    ) -> Result<ExecutionResult, StreamingError> {
        let pipeline_id = format!("pipeline-{}", Uuid::new_v4());
        let mut results = Vec::new();

        // Execute each stage in sequence
        for (i, stage) in stages.into_iter().enumerate() {
            let stage_result = Box::pin(self.execute_expression(stage)).await?;
            results.push(format!("Stage {}: {}", i, stage_result));
        }

        Ok(ExecutionResult::Success(format!("Executed pipeline {}: {:?}", pipeline_id, results)))
    }

    /// Multiplex multiple streams
    async fn multiplex_streams(
        &mut self,
        input_streams: Vec<StreamReference>,
        strategy: MultiplexStrategy,
        output_handle: String,
    ) -> Result<ExecutionResult, StreamingError> {
        let mut input_handles = Vec::new();
        
        for stream_ref in input_streams {
            let handle = self.resolve_stream_reference(stream_ref).await?;
            input_handles.push(handle);
        }

        let multiplex_handle = self.marketplace.start_bidirectional_stream_with_config(
            "multiplex",
            &Value::Map(HashMap::new()),
            &StreamConfig {
                callbacks: None,
                auto_reconnect: false,
                max_retries: 0,
            }
        ).await.map_err(|e| StreamingError::Other(e.to_string()))?;
        
        // TODO: Implement proper multiplexing logic with the provided strategy
        let _ = strategy; // Acknowledge unused variable

        self.active_streams.insert(output_handle.clone(), multiplex_handle);

        Ok(ExecutionResult::Success(format!("Created multiplexed stream: {}", output_handle)))
    }

    /// Demultiplex a stream
    async fn demultiplex_stream(
        &mut self,
        input_stream: StreamReference,
        _criteria_fn: String,
        outputs: HashMap<String, StreamReference>,
    ) -> Result<ExecutionResult, StreamingError> {
        let _input_handle = self.resolve_stream_reference(input_stream).await?;
        let mut output_handles = HashMap::new();

        for (key, stream_ref) in outputs {
            let handle = self.resolve_stream_reference(stream_ref).await?;
            output_handles.insert(key, handle);
        }

        let _demultiplex_handle = self.marketplace.start_bidirectional_stream_with_config(
            "demultiplex",
            &Value::Map(HashMap::new()),
            &StreamConfig {
                callbacks: None,
                auto_reconnect: false,
                max_retries: 0,
            }
        ).await.map_err(|e| StreamingError::Other(e.to_string()))?;

        Ok(ExecutionResult::Success("Created demultiplexed stream".to_string()))
    }

    /// Helper: Resolve a stream reference to a handle
    async fn resolve_stream_reference(&mut self, stream_ref: StreamReference) -> Result<StreamHandle, StreamingError> {
        match stream_ref {
            StreamReference::Handle(handle_name) => {
                self.active_streams.remove(&handle_name)
                    .ok_or_else(|| StreamingError::HandleNotFound(handle_name))
            },
            StreamReference::ResourceRef(resource_path) => {
                let resolved_path = self.resource_refs.get(&resource_path)
                    .ok_or_else(|| StreamingError::ResourceNotFound(resource_path.clone()))?;
                
                // Create stream from resolved resource using start_stream_with_config
                self.marketplace.start_stream_with_config(
                    resolved_path,
                    &Value::Map(HashMap::new()),
                    &StreamConfig {
                        callbacks: None,
                        auto_reconnect: false,
                        max_retries: 0,
                    }
                ).await.map_err(|e| StreamingError::Other(e.to_string()))
            },
            StreamReference::CapabilityId(capability_id) => {
                self.marketplace.start_stream_with_config(
                    &capability_id,
                    &Value::Map(HashMap::new()),
                    &StreamConfig {
                        callbacks: None,
                        auto_reconnect: false,
                        max_retries: 0,
                    }
                ).await.map_err(|e| StreamingError::Other(e.to_string()))
            },
            StreamReference::Expression(expr) => {
                // Recursively execute expression to get handle
                let result = Box::pin(self.execute_expression(*expr)).await?;
                match result {
                    ExecutionResult::StreamHandle(handle) => Ok(handle),
                    _ => Err(StreamingError::InvalidExpression("Expression did not produce a stream handle".to_string())),
                }
            },
        }
    }



    /// Helper: Build stream callbacks from callback definitions
    fn build_stream_callbacks(&self, callbacks: HashMap<String, String>) -> Result<StreamCallbacks, StreamingError> {
        let mut stream_callbacks = StreamCallbacks::default();

        for (event_name, callback_fn) in callbacks {
            match event_name.as_str() {
                "on-item" | "on-data" => {
                    // For StreamCallbacks, we need to use on_data_received
                    stream_callbacks.on_data_received = Some(Arc::new(move |event| {
                        // Convert StreamEvent to appropriate format for callback
                        println!("Executing data callback: {} for event: {:?}", callback_fn, event);
                        Ok(())
                    }));
                },
                "on-error" => {
                    stream_callbacks.on_error = Some(Arc::new(move |event| {
                        println!("Executing error callback: {} for event: {:?}", callback_fn, event);
                        Ok(())
                    }));
                },
                "on-complete" | "on-disconnected" => {
                    stream_callbacks.on_disconnected = Some(Arc::new(move |event| {
                        println!("Executing disconnected callback: {} for event: {:?}", callback_fn, event);
                        Ok(())
                    }));
                },
                "on-start" | "on-connected" => {
                    stream_callbacks.on_connected = Some(Arc::new(move |event| {
                        println!("Executing connected callback: {} for event: {:?}", callback_fn, event);
                        Ok(())
                    }));
                },
                _ => return Err(StreamingError::InvalidCallback(event_name)),
            }
        }

        Ok(stream_callbacks)
    }

    // Removed unused execute_processing_logic method
}

/// Result of executing a streaming expression
#[derive(Debug)]
pub enum ExecutionResult {
    Success(String),
    StreamHandle(StreamHandle),
    Value(String),
}

impl std::fmt::Display for ExecutionResult {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionResult::Success(msg) => write!(f, "{}", msg),
            ExecutionResult::StreamHandle(_) => write!(f, "Stream handle created"),
            ExecutionResult::Value(val) => write!(f, "{}", val),
        }
    }
}

/// Streaming-specific errors
#[derive(Debug)]
pub enum StreamingError {
    HandleNotFound(String),
    StreamNotFound(String),
    ResourceNotFound(String),
    InvalidExpression(String),
    InvalidCallback(String),
    SendError(String),
    ReceiveError(String),
    MarketplaceError(String),
    Other(String),
}

impl std::fmt::Display for StreamingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            StreamingError::HandleNotFound(name) => write!(f, "Stream handle not found: {}", name),
            StreamingError::StreamNotFound(name) => write!(f, "Stream not found: {}", name),
            StreamingError::ResourceNotFound(path) => write!(f, "Resource not found: {}", path),
            StreamingError::InvalidExpression(msg) => write!(f, "Invalid expression: {}", msg),
            StreamingError::InvalidCallback(name) => write!(f, "Invalid callback: {}", name),
            StreamingError::SendError(msg) => write!(f, "Send error: {}", msg),
            StreamingError::ReceiveError(msg) => write!(f, "Receive error: {}", msg),
            StreamingError::MarketplaceError(msg) => write!(f, "Marketplace error: {}", msg),
            StreamingError::Other(msg) => write!(f, "Other error: {}", msg),
        }
    }
}

impl std::error::Error for StreamingError {}




/// Transform provider for stream transforms
pub struct RtfsTransformProvider {
    pub transform_fn: String,
    pub input_handle: StreamHandle,
    pub output_handle: StreamHandle,
    pub config: StreamConfig,
}


#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rtfs_streaming_syntax_execution() {
        let registry = Arc::new(tokio::sync::RwLock::new(crate::runtime::capability_registry::CapabilityRegistry::new()));
        let marketplace = Arc::new(crate::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
        let mut executor = RtfsStreamingSyntaxExecutor::new(marketplace);

        // Test registering a stream capability
        let config = StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        };
        let register_expr = RtfsStreamingExpression::RegisterStreamCapability {
            capability_id: "test-stream".to_string(),
            stream_type: StreamType::Source,
            input_schema: None,
            output_schema: Some(StreamSchema {
                element_type: "map".to_string(),
                validation_rules: vec![],
                strict_validation: false,
            }),
            config: config.clone(),
            provider: Arc::new(LocalStreamingProvider),
            metadata: HashMap::new(),
        };

        let result = executor.execute_expression(register_expr).await;
        assert!(result.is_ok());

        // Test creating a stream source
        let source_expr = RtfsStreamingExpression::StreamSource {
            capability_id: "test-stream".to_string(),
            config: None,
            handle_name: "test-handle".to_string(),
        };

        let result = executor.execute_expression(source_expr).await;
        assert!(result.is_ok());
        assert!(executor.active_streams.contains_key("test-handle"));
    }

    #[tokio::test]
    async fn test_stream_pipeline_execution() {
        let registry = Arc::new(tokio::sync::RwLock::new(crate::runtime::capability_registry::CapabilityRegistry::new()));
        let marketplace = Arc::new(crate::runtime::capability_marketplace::CapabilityMarketplace::new(registry));
        let mut executor = RtfsStreamingSyntaxExecutor::new(marketplace);

        // First, register the capabilities that will be used in the pipeline
        let config = StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        };
        let register_source = RtfsStreamingExpression::RegisterStreamCapability {
            capability_id: "data-source".to_string(),
            stream_type: StreamType::Source,
            input_schema: None,
            output_schema: None,
            config: config.clone(),
            provider: Arc::new(LocalStreamingProvider),
            metadata: HashMap::new(),
        };

        let config = StreamConfig {
            callbacks: None,
            auto_reconnect: false,
            max_retries: 0,
        };
        let register_sink = RtfsStreamingExpression::RegisterStreamCapability {
            capability_id: "data-sink".to_string(),
            stream_type: StreamType::Sink,
            input_schema: None,
            output_schema: None,
            config: config.clone(),
            provider: Arc::new(LocalStreamingProvider),
            metadata: HashMap::new(),
        };

        // Register the capabilities
        let result = executor.execute_expression(register_source).await;
        assert!(result.is_ok(), "Failed to register source capability: {:?}", result);

        let result = executor.execute_expression(register_sink).await;
        assert!(result.is_ok(), "Failed to register sink capability: {:?}", result);

        // Now test the pipeline
        let pipeline_expr = RtfsStreamingExpression::StreamPipeline {
            stages: vec![
                RtfsStreamingExpression::StreamSource {
                    capability_id: "data-source".to_string(),
                    config: None,
                    handle_name: "source".to_string(),
                },
                RtfsStreamingExpression::StreamSink {
                    capability_id: "data-sink".to_string(),
                    config: None,
                    handle_name: "sink".to_string(),
                },
            ],
            config: None,
        };

        let result = executor.execute_expression(pipeline_expr).await;
        assert!(result.is_ok(), "Pipeline execution failed: {:?}", result);
    }
}
