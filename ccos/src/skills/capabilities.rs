//! RTFS skill capabilities
//!
//! Exposes RTFS-callable capabilities for loading and executing skills.

use crate::capability_marketplace::CapabilityMarketplace;
use crate::skills::PrimitiveMapper;
use crate::utils::value_conversion::{json_to_rtfs_value, rtfs_value_to_json};
use crate::skills::SkillMapper;
use rtfs::runtime::error::{RuntimeError, RuntimeResult};
use rtfs::runtime::values::Value;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::Mutex;

#[derive(Debug, Deserialize)]
struct SkillLoadInput {
    url: String,
}

#[derive(Debug, Serialize)]
struct SkillLoadOutput {
    skill_id: String,
    name: String,
    description: String,
    capabilities: Vec<String>,
    requires_approval: bool,
}

#[derive(Debug, Deserialize)]
struct SkillExecuteInput {
    skill_id: String,
    operation: String,
    #[serde(default)]
    params: Option<serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct PrimitiveMapInput {
    command: String,
}

#[derive(Debug, Serialize)]
struct PrimitiveMapOutput {
    capability_id: String,
    params: std::collections::HashMap<String, serde_json::Value>,
    confidence: f64,
    explanation: String,
}

#[derive(Debug, Serialize)]
struct PrimitiveMapFailure {
    success: bool,
    message: String,
}

pub async fn register_skill_capabilities(
    marketplace: Arc<CapabilityMarketplace>,
    skill_mapper: Arc<Mutex<SkillMapper>>,
) -> RuntimeResult<()> {
    // ccos.skill.load
    let skill_mapper_load = skill_mapper.clone();
    let load_handler = Arc::new(move |input: &Value| {
        let payload: SkillLoadInput = parse_payload("ccos.skill.load", input)?;
        let sm = skill_mapper_load.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let loaded = std::thread::spawn(move || {
            rt_handle.block_on(async {
                let mut mapper = sm.lock().await;
                mapper.load_and_register(&payload.url).await
            })
        })
        .join()
        .map_err(|_| RuntimeError::Generic("ccos.skill.load: thread join error".to_string()))?;

        let loaded = loaded.map_err(|err| {
            RuntimeError::Generic(format!("ccos.skill.load: {}", err))
        })?;

        if loaded.skill.id == "unnamed-skill" || loaded.capabilities_to_register.is_empty() {
            return Err(RuntimeError::Generic(
                "ccos.skill.load: skill is missing an id or has no capabilities".to_string(),
            ));
        }

        let output = SkillLoadOutput {
            skill_id: loaded.skill.id,
            name: loaded.skill.name,
            description: loaded.skill.description,
            capabilities: loaded.capabilities_to_register,
            requires_approval: loaded.requires_approval,
        };

        produce_value("ccos.skill.load", output)
    });

    marketplace
        .register_local_capability(
            "ccos.skill.load".to_string(),
            "Skill / Load".to_string(),
            "Load and register a skill from a URL (Markdown or YAML)".to_string(),
            load_handler,
        )
        .await?;

    // ccos.skill.execute
    let skill_mapper_exec = skill_mapper.clone();
    let marketplace_exec = marketplace.clone();
    let execute_handler = Arc::new(move |input: &Value| {
        let payload: SkillExecuteInput = parse_payload("ccos.skill.execute", input)?;
        let sm = skill_mapper_exec.clone();
        let marketplace = marketplace_exec.clone();
        let rt_handle = tokio::runtime::Handle::current();

        let result = std::thread::spawn(move || {
            rt_handle.block_on(async {
                // Ensure skill is registered (optional guard)
                let mapper = sm.lock().await;
                if mapper.get_skill(&payload.skill_id).is_none() {
                    return Err(RuntimeError::Generic(format!(
                        "Skill not loaded: {}",
                        payload.skill_id
                    )));
                }

                if !payload.operation.contains('.') {
                    let known_ops = mapper
                        .get_skill(&payload.skill_id)
                        .map(|skill| {
                            skill
                                .operations
                                .iter()
                                .map(|op| op.name.clone())
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                    if !known_ops.is_empty()
                        && !known_ops.iter().any(|op| op == &payload.operation)
                    {
                        return Err(RuntimeError::Generic(format!(
                            "Unknown operation for {}: {}. Available: {}",
                            payload.skill_id,
                            payload.operation,
                            known_ops.join(", ")
                        )));
                    }
                }

                let cap_id = if payload.operation.contains('.') {
                    payload.operation.clone()
                } else {
                    format!("{}.{}", payload.skill_id, payload.operation)
                };

                let params_json = payload.params.unwrap_or_else(|| serde_json::Value::Object(Default::default()));
                let rtfs_args = json_to_rtfs_value(&params_json)?;
                marketplace.execute_capability(&cap_id, &rtfs_args).await
            })
        })
        .join()
        .map_err(|_| RuntimeError::Generic("ccos.skill.execute: thread join error".to_string()))??;

        Ok(result)
    });

    marketplace
        .register_local_capability(
            "ccos.skill.execute".to_string(),
            "Skill / Execute".to_string(),
            "Execute a registered skill operation".to_string(),
            execute_handler,
        )
        .await?;

    // ccos.primitive.map
    let primitive_map_handler = Arc::new(move |input: &Value| {
        let payload: PrimitiveMapInput = parse_payload("ccos.primitive.map", input)?;
        let mapper = PrimitiveMapper::new();
        if let Some(mapped) = mapper.map_command(&payload.command) {
            let output = PrimitiveMapOutput {
                capability_id: mapped.capability_id,
                params: mapped.params,
                confidence: mapped.confidence,
                explanation: mapped.explanation,
            };
            produce_value("ccos.primitive.map", output)
        } else {
            let output = PrimitiveMapFailure {
                success: false,
                message: "No mapping found for command".to_string(),
            };
            produce_value("ccos.primitive.map", output)
        }
    });

    marketplace
        .register_local_capability(
            "ccos.primitive.map".to_string(),
            "Skill / Primitive Map".to_string(),
            "Map a shell command (curl, python, etc.) to a CCOS capability".to_string(),
            primitive_map_handler,
        )
        .await?;

    Ok(())
}

fn parse_payload<T: serde::de::DeserializeOwned>(
    capability: &str,
    value: &Value,
) -> RuntimeResult<T> {
    let serialized = rtfs_value_to_json(value)?;
    serde_json::from_value(serialized).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: input payload does not match schema: {}",
            capability, err
        ))
    })
}

fn produce_value<T: Serialize>(capability: &str, output: T) -> RuntimeResult<Value> {
    let json_value = serde_json::to_value(output).map_err(|err| {
        RuntimeError::Generic(format!(
            "{}: failed to serialize output: {}",
            capability, err
        ))
    })?;

    json_to_rtfs_value(&json_value)
}
