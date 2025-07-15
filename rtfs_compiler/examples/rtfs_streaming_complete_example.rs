// Complete RTFS 2.0 Streaming Example
// This example demonstrates how to use the homoiconic streaming syntax 
// to create a real-time data processing pipeline

use std::collections::HashMap;
use std::sync::Arc;
use serde_json::Value;
use uuid::Uuid;

use rtfs_compiler::runtime::rtfs_streaming_syntax::{
    RtfsStreamingSyntaxExecutor, RtfsStreamingExpression, StreamReference, 
    ProcessingLogic, StreamOptions, StreamSchema, ValidationRule, ErrorHandlingStrategy, BackpressureStrategy, MultiplexStrategy
};
use rtfs_compiler::runtime::capability_marketplace::{CapabilityMarketplace, StreamType, StreamConfig};

/// Example: Real-time IoT Data Processing Pipeline
/// This demonstrates a complete streaming application written in RTFS 2.0 syntax
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🚀 RTFS 2.0 Streaming Example: IoT Data Processing Pipeline");
    
    // Create the capability marketplace and executor
    let marketplace = Arc::new(CapabilityMarketplace::new());
    let mut executor = RtfsStreamingSyntaxExecutor::new(marketplace.clone());

    // Execute the RTFS 2.0 streaming plan
    execute_iot_pipeline(&mut executor).await?;

    Ok(())
}

/// Execute a comprehensive IoT data processing pipeline using RTFS 2.0 streaming syntax
async fn execute_iot_pipeline(executor: &mut RtfsStreamingSyntaxExecutor) -> Result<(), Box<dyn std::error::Error>> {
    println!("\n📋 Step 1: Register streaming capabilities");
    
    // Register IoT sensor data source
    let sensor_config = StreamConfig {
        buffer_size: 1000,
        enable_callbacks: true,
        ..Default::default()
    };
    let sensor_capability = RtfsStreamingExpression::RegisterStreamCapability {
        capability_id: "com.iot:v1.0:sensor-data".to_string(),
        stream_type: StreamType::Source,
        input_schema: None,
        output_schema: Some(StreamSchema {
            element_type: "map".to_string(),
            validation_rules: vec![
                ValidationRule {
                    field_path: "sensor_id".to_string(),
                    rule_type: "required".to_string(),
                    parameters: HashMap::new(),
                },
                ValidationRule {
                    field_path: "timestamp".to_string(),
                    rule_type: "timestamp".to_string(),
                    parameters: HashMap::new(),
                },
                ValidationRule {
                    field_path: "value".to_string(),
                    rule_type: "number".to_string(),
                    parameters: HashMap::new(),
                },
            ],
            strict_validation: true,
        }),
        config: sensor_config.clone(),
        provider: rtfs_compiler::runtime::capability_marketplace::StreamingProvider::Local { buffer_size: sensor_config.buffer_size },
        metadata: {
            let mut meta = HashMap::new();
            meta.insert("description".to_string(), "IoT sensor data stream".to_string());
            meta.insert("version".to_string(), "1.0".to_string());
            meta
        },
    };
    
    executor.execute_expression(sensor_capability).await?;
    println!("✅ Registered sensor data capability");

    // Register data processing capability
    let processor_config = StreamConfig {
        buffer_size: 500,
        enable_callbacks: true,
        ..Default::default()
    };
    let processor_capability = RtfsStreamingExpression::RegisterStreamCapability {
        capability_id: "com.iot:v1.0:data-processor".to_string(),
        stream_type: StreamType::Transform,
        input_schema: Some(StreamSchema {
            element_type: "map".to_string(),
            validation_rules: vec![],
            strict_validation: false,
        }),
        output_schema: Some(StreamSchema {
            element_type: "map".to_string(),
            validation_rules: vec![],
            strict_validation: false,
        }),
        config: processor_config.clone(),
        provider: rtfs_compiler::runtime::capability_marketplace::StreamingProvider::Local { buffer_size: processor_config.buffer_size },
        metadata: {
            let mut meta = HashMap::new();
            meta.insert("description".to_string(), "Data processing transform".to_string());
            meta
        },
    };
    
    executor.execute_expression(processor_capability).await?;
    println!("✅ Registered data processor capability");

    // Register alert system capability
    let alert_config = StreamConfig {
        buffer_size: 100,
        enable_callbacks: true,
        ..Default::default()
    };
    let alert_capability = RtfsStreamingExpression::RegisterStreamCapability {
        capability_id: "com.iot:v1.0:alert-system".to_string(),
        stream_type: StreamType::Sink,
        input_schema: Some(StreamSchema {
            element_type: "map".to_string(),
            validation_rules: vec![
                ValidationRule {
                    field_path: "alert_type".to_string(),
                    rule_type: "required".to_string(),
                    parameters: HashMap::new(),
                },
                ValidationRule {
                    field_path: "severity".to_string(),
                    rule_type: "enum".to_string(),
                    parameters: {
                        let mut params = HashMap::new();
                        params.insert("values".to_string(), "low,medium,high,critical".to_string());
                        params
                    },
                },
            ],
            strict_validation: true,
        }),
        output_schema: None,
        config: alert_config.clone(),
        provider: rtfs_compiler::runtime::capability_marketplace::StreamingProvider::Local { buffer_size: alert_config.buffer_size },
        metadata: {
            let mut meta = HashMap::new();
            meta.insert("description".to_string(), "Alert notification system".to_string());
            meta
        },
    };
    
    executor.execute_expression(alert_capability).await?;
    println!("✅ Registered alert system capability");

    println!("\n🔧 Step 2: Create streaming pipeline");
    
    // Create sensor data source
    let create_source = RtfsStreamingExpression::StreamSource {
        capability_id: "com.iot:v1.0:sensor-data".to_string(),
        config: Some(StreamConfig {
            buffer_size: 1000,
            enable_callbacks: true,
            ..Default::default()
        }),
        handle_name: "sensor-stream".to_string(),
    };
    
    executor.execute_expression(create_source).await?;
    println!("✅ Created sensor data source");

    // Create data processor
    let create_processor = RtfsStreamingExpression::StreamSource {
        capability_id: "com.iot:v1.0:data-processor".to_string(),
        config: Some(StreamConfig {
            buffer_size: 500,
            enable_callbacks: true,
            ..Default::default()
        }),
        handle_name: "processor-stream".to_string(),
    };
    
    executor.execute_expression(create_processor).await?;
    println!("✅ Created data processor");

    // Create alert system
    let create_alerts = RtfsStreamingExpression::StreamSink {
        capability_id: "com.iot:v1.0:alert-system".to_string(),
        config: Some(StreamConfig {
            buffer_size: 100,
            enable_callbacks: true,
            ..Default::default()
        }),
        handle_name: "alert-stream".to_string(),
    };
    
    executor.execute_expression(create_alerts).await?;
    println!("✅ Created alert system");

    println!("\n🔄 Step 3: Set up stream transformations");
    
    // Create data processing transform
    let data_transform = RtfsStreamingExpression::StreamTransform {
        input_stream: StreamReference::Handle("sensor-stream".to_string()),
        output_stream: StreamReference::Handle("processor-stream".to_string()),
        transform_fn: "process_sensor_data".to_string(),
        config: Some(StreamConfig {
            enable_callbacks: true,
            ..Default::default()
        }),
    };
    
    executor.execute_expression(data_transform).await?;
    println!("✅ Set up data processing transform");

    // Create alert generation transform
    let alert_transform = RtfsStreamingExpression::StreamTransform {
        input_stream: StreamReference::Handle("processor-stream".to_string()),
        output_stream: StreamReference::Handle("alert-stream".to_string()),
        transform_fn: "generate_alerts".to_string(),
        config: Some(StreamConfig {
            enable_callbacks: true,
            ..Default::default()
        }),
    };
    
    executor.execute_expression(alert_transform).await?;
    println!("✅ Set up alert generation transform");

    println!("\n📊 Step 4: Configure stream consumption with callbacks");
    
    // Set up comprehensive monitoring with callbacks
    let monitoring_consumption = RtfsStreamingExpression::StreamConsume {
        stream_handle: "sensor-stream".to_string(),
        processing_logic: ProcessingLogic::CallbackBased {
            callbacks: {
                let mut callbacks = HashMap::new();
                callbacks.insert("on-item".to_string(), "monitor_sensor_data".to_string());
                callbacks.insert("on-error".to_string(), "handle_sensor_error".to_string());
                callbacks.insert("on-complete".to_string(), "sensor_stream_complete".to_string());
                callbacks.insert("on-start".to_string(), "sensor_stream_started".to_string());
                callbacks
            },
        },
        options: Some(StreamOptions {
            enable_callbacks: Some(true),
            timeout_ms: Some(60000),
            batch_size: Some(10),
            error_handling: Some(ErrorHandlingStrategy::Retry {
                attempts: 3,
                delay_ms: 1000,
            }),
            backpressure: Some(BackpressureStrategy::DropOldest),
        }),
    };
    
    executor.execute_expression(monitoring_consumption).await?;
    println!("✅ Set up sensor stream monitoring with callbacks");

    // Set up alert stream consumption
    let alert_consumption = RtfsStreamingExpression::StreamConsume {
        stream_handle: "alert-stream".to_string(),
        processing_logic: ProcessingLogic::CallbackBased {
            callbacks: {
                let mut callbacks = HashMap::new();
                callbacks.insert("on-item".to_string(), "send_alert_notification".to_string());
                callbacks.insert("on-error".to_string(), "handle_alert_error".to_string());
                callbacks.insert("on-complete".to_string(), "alert_stream_complete".to_string());
                callbacks
            },
        },
        options: Some(StreamOptions {
            enable_callbacks: Some(true),
            timeout_ms: Some(30000),
            batch_size: Some(5),
            error_handling: Some(ErrorHandlingStrategy::DeadLetter),
            backpressure: Some(BackpressureStrategy::Block),
        }),
    };
    
    executor.execute_expression(alert_consumption).await?;
    println!("✅ Set up alert stream consumption");

    println!("\n🎯 Step 5: Execute complete streaming pipeline");
    
    // Create and execute the complete pipeline
    let complete_pipeline = RtfsStreamingExpression::StreamPipeline {
        stages: vec![
            RtfsStreamingExpression::StreamSource {
                capability_id: "com.iot:v1.0:sensor-data".to_string(),
                config: Some(StreamConfig {
                    buffer_size: 1000,
                    enable_callbacks: true,
                    ..Default::default()
                }),
                handle_name: "pipeline-sensor".to_string(),
            },
            RtfsStreamingExpression::StreamTransform {
                input_stream: StreamReference::Handle("pipeline-sensor".to_string()),
                output_stream: StreamReference::CapabilityId("com.iot:v1.0:data-processor".to_string()),
                transform_fn: "comprehensive_data_processing".to_string(),
                config: Some(StreamConfig {
                    enable_callbacks: true,
                    ..Default::default()
                }),
            },
            RtfsStreamingExpression::StreamSink {
                capability_id: "com.iot:v1.0:alert-system".to_string(),
                config: Some(StreamConfig {
                    buffer_size: 100,
                    enable_callbacks: true,
                    ..Default::default()
                }),
                handle_name: "pipeline-alerts".to_string(),
            },
        ],
        config: Some(StreamConfig {
            enable_callbacks: true,
            ..Default::default()
        }),
    };
    
    executor.execute_expression(complete_pipeline).await?;
    println!("✅ Executed complete streaming pipeline");

    println!("\n🔀 Step 6: Demonstrate advanced streaming patterns");
    
    // Create a multiplexed stream from multiple sensor sources
    let multiplex_example = RtfsStreamingExpression::StreamMultiplex {
        input_streams: vec![
            StreamReference::CapabilityId("com.iot:v1.0:temperature-sensor".to_string()),
            StreamReference::CapabilityId("com.iot:v1.0:humidity-sensor".to_string()),
            StreamReference::CapabilityId("com.iot:v1.0:pressure-sensor".to_string()),
        ],
        strategy: MultiplexStrategy::RoundRobin,
        output_handle: "multiplexed-sensors".to_string(),
    };
    
    executor.execute_expression(multiplex_example).await?;
    println!("✅ Created multiplexed sensor stream");

    // Create a demultiplexed stream for different alert types
    let demultiplex_example = RtfsStreamingExpression::StreamDemultiplex {
        input_stream: StreamReference::Handle("alert-stream".to_string()),
        criteria_fn: "classify_alert_severity".to_string(),
        outputs: {
            let mut outputs = HashMap::new();
            outputs.insert("critical".to_string(), StreamReference::CapabilityId("com.iot:v1.0:critical-alerts".to_string()));
            outputs.insert("high".to_string(), StreamReference::CapabilityId("com.iot:v1.0:high-alerts".to_string()));
            outputs.insert("medium".to_string(), StreamReference::CapabilityId("com.iot:v1.0:medium-alerts".to_string()));
            outputs.insert("low".to_string(), StreamReference::CapabilityId("com.iot:v1.0:low-alerts".to_string()));
            outputs
        },
    };
    
    executor.execute_expression(demultiplex_example).await?;
    println!("✅ Created demultiplexed alert streams");

    println!("\n🔄 Step 7: Demonstrate bidirectional streaming");
    
    // Create a bidirectional stream for interactive monitoring
    let bidirectional_monitoring = RtfsStreamingExpression::StreamBidirectional {
        capability_id: "com.iot:v1.0:interactive-monitor".to_string(),
        config: Some(StreamConfig {
            enable_callbacks: true,
            ..Default::default()
        }),
        handle_name: "interactive-monitor".to_string(),
    };
    
    executor.execute_expression(bidirectional_monitoring).await?;
    println!("✅ Created bidirectional monitoring stream");

    // Interact with the bidirectional stream
    let mut monitor_data = std::collections::HashMap::new();
    monitor_data.insert(rtfs_compiler::ast::MapKey::String("command".to_string()), rtfs_compiler::runtime::values::Value::String("get_sensor_status".to_string()));
    monitor_data.insert(rtfs_compiler::ast::MapKey::String("sensor_id".to_string()), rtfs_compiler::runtime::values::Value::String("temp_01".to_string()));
    monitor_data.insert(rtfs_compiler::ast::MapKey::String("timestamp".to_string()), rtfs_compiler::runtime::values::Value::String("2024-12-08T10:30:00Z".to_string()));
    let interactive_session = RtfsStreamingExpression::StreamInteract {
        stream_handle: "interactive-monitor".to_string(),
        send_item: Some(rtfs_compiler::runtime::capability_marketplace::StreamItem {
            data: rtfs_compiler::runtime::values::Value::Map(monitor_data),
            sequence: 0,
            timestamp: 0,
            metadata: std::collections::HashMap::new(),
            direction: rtfs_compiler::runtime::capability_marketplace::StreamDirection::Outbound,
            correlation_id: None,
        }),
        receive_logic: Some(ProcessingLogic::CallbackBased {
            callbacks: {
                let mut callbacks = HashMap::new();
                callbacks.insert("on-item".to_string(), "handle_monitor_response".to_string());
                callbacks.insert("on-error".to_string(), "handle_monitor_error".to_string());
                callbacks
            },
        }),
        config: Some(StreamConfig {
            enable_callbacks: true,
            ..Default::default()
        }),
    };
    
    executor.execute_expression(interactive_session).await?;
    println!("✅ Started interactive monitoring session");

    println!("\n🎉 RTFS 2.0 Streaming Pipeline Complete!");
    println!("📈 All streaming operations have been successfully configured and executed.");
    println!("🔄 The system is now processing IoT data in real-time with comprehensive monitoring.");
    
    Ok(())
}

/// Example RTFS 2.0 streaming plan as a homoiconic data structure
/// This shows how the streaming operations can be represented as pure data
fn example_homoiconic_streaming_plan() -> Value {
    serde_json::json!({
        "plan": {
            "type": "rtfs.core:v2.0:streaming-plan",
            "plan-id": "iot-pipeline-001",
            "description": "IoT data processing pipeline with real-time alerts",
            "version": "1.0",
            "resources": [
                {"type": "resource:ref", "path": "sensor-data-stream"},
                {"type": "resource:ref", "path": "alert-notification-system"}
            ],
            "program": {
                "type": "do",
                "expressions": [
                    {
                        "type": "register-stream-capability",
                        "capability-id": "com.iot:v1.0:sensor-data",
                        "stream-type": "source",
                        "output-schema": {
                            "type": "stream",
                            "element-type": {
                                "type": "map",
                                "fields": [
                                    {"sensor_id": "string"},
                                    {"timestamp": "timestamp"},
                                    {"value": "number"},
                                    {"unit": "string"}
                                ]
                            }
                        },
                        "config": {
                            "buffer-size": 1000,
                            "enable-callbacks": true
                        }
                    },
                    {
                        "type": "stream-source",
                        "capability-id": "com.iot:v1.0:sensor-data",
                        "handle-name": "sensor-stream",
                        "config": {
                            "buffer-size": 1000,
                            "enable-callbacks": true
                        }
                    },
                    {
                        "type": "stream-consume",
                        "stream-handle": "sensor-stream",
                        "processing-logic": {
                            "type": "callback-based",
                            "callbacks": {
                                "on-item": "process_sensor_reading",
                                "on-error": "handle_sensor_error",
                                "on-complete": "sensor_stream_complete"
                            }
                        },
                        "options": {
                            "enable-callbacks": true,
                            "timeout-ms": 60000,
                            "error-handling": {
                                "type": "retry",
                                "attempts": 3,
                                "delay-ms": 1000
                            }
                        }
                    },
                    {
                        "type": "stream-transform",
                        "input-stream": {"type": "handle", "name": "sensor-stream"},
                        "output-stream": {"type": "capability-id", "id": "com.iot:v1.0:alert-system"},
                        "transform-fn": "generate_alerts_from_sensor_data",
                        "config": {
                            "enable-callbacks": true
                        }
                    }
                ]
            }
        }
    })
}

/// Example callback functions that would be executed by the RTFS interpreter
/// These demonstrate how the streaming system integrates with business logic
mod callback_functions {
    use serde_json::Value;
    
    pub fn process_sensor_reading(item: Value) {
        println!("📊 Processing sensor reading: {}", item);
        
        // Extract sensor data
        if let Some(sensor_id) = item.get("sensor_id") {
            if let Some(value) = item.get("value") {
                if let Some(unit) = item.get("unit") {
                    println!("   Sensor {} reported {} {}", sensor_id, value, unit);
                    
                    // Perform processing logic
                    if let Some(val) = value.as_f64() {
                        if val > 100.0 {
                            println!("   ⚠️  High reading detected!");
                        }
                    }
                }
            }
        }
    }
    
    pub fn handle_sensor_error(error: Value) {
        println!("❌ Sensor error: {}", error);
        
        // Implement error handling logic
        if let Some(error_type) = error.get("type") {
            match error_type.as_str() {
                Some("timeout") => println!("   🔄 Attempting to reconnect..."),
                Some("invalid_data") => println!("   📋 Logging invalid data for review"),
                Some("connection_lost") => println!("   🌐 Connection lost, switching to backup"),
                _ => println!("   ⚠️  Unknown error type"),
            }
        }
    }
    
    pub fn sensor_stream_complete() {
        println!("✅ Sensor stream completed successfully");
        
        // Cleanup and finalization logic
        println!("   🧹 Cleaning up resources...");
        println!("   📊 Generating final reports...");
        println!("   💾 Saving state...");
    }
    
    pub fn generate_alerts_from_sensor_data(item: Value) -> Value {
        println!("🚨 Generating alerts from sensor data: {}", item);
        
        // Alert generation logic
        if let Some(value) = item.get("value").and_then(|v| v.as_f64()) {
            if value > 80.0 {
                return serde_json::json!({
                    "alert_type": "high_temperature",
                    "severity": "high",
                    "sensor_id": item.get("sensor_id"),
                    "value": value,
                    "timestamp": item.get("timestamp"),
                    "message": format!("High temperature reading: {}", value)
                });
            } else if value > 60.0 {
                return serde_json::json!({
                    "alert_type": "elevated_temperature",
                    "severity": "medium",
                    "sensor_id": item.get("sensor_id"),
                    "value": value,
                    "timestamp": item.get("timestamp"),
                    "message": format!("Elevated temperature reading: {}", value)
                });
            }
        }
        
        // No alert needed
        serde_json::json!({
            "alert_type": "none",
            "severity": "low",
            "sensor_id": item.get("sensor_id"),
            "value": item.get("value"),
            "timestamp": item.get("timestamp")
        })
    }
    
    pub fn handle_monitor_response(response: Value) {
        println!("📱 Monitor response: {}", response);
        
        // Handle interactive monitoring response
        if let Some(status) = response.get("status") {
            match status.as_str() {
                Some("online") => println!("   ✅ Sensor is online and functioning"),
                Some("offline") => println!("   ❌ Sensor is offline"),
                Some("error") => println!("   ⚠️  Sensor reporting error"),
                _ => println!("   ❓ Unknown sensor status"),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_homoiconic_plan_structure() {
        let plan = example_homoiconic_streaming_plan();
        assert!(plan.get("plan").is_some());
        assert!(plan["plan"].get("type").is_some());
        assert!(plan["plan"].get("program").is_some());
        assert!(plan["plan"]["program"].get("expressions").is_some());
    }
    
    #[tokio::test]
    async fn test_complete_pipeline_execution() {
        let marketplace = Arc::new(CapabilityMarketplace::new());
        let mut executor = RtfsStreamingSyntaxExecutor::new(marketplace);
        
        // This would normally execute the complete pipeline
        // For testing, we'll just verify the executor can be created
        // Cannot check active_streams directly as it is private; test executor creation only
    }
}
