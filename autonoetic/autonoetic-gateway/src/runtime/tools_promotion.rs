//! Promotion Registry Tools.
//!
//! Provides tools for recording and querying content promotion status
//! (evaluator/auditor validation) per content handle.

use crate::causal_chain::CausalLogger;
use crate::llm::ToolDefinition;
use crate::policy::PolicyEngine;
use crate::runtime::promotion_store::PromotionStore;
use crate::runtime::tools::NativeTool;
use autonoetic_types::agent::AgentManifest;
use autonoetic_types::causal_chain::EntryStatus;
use autonoetic_types::promotion::{
    PromotionQueryArgs, PromotionQueryResponse, PromotionRecordArgs, PromotionRecordResponse,
};
use std::path::Path;

fn is_promotion_agent(manifest: &AgentManifest) -> bool {
    matches!(
        manifest.agent.id.as_str(),
        "evaluator.default" | "auditor.default"
    )
}

// ---------------------------------------------------------------------------
// Promotion Record Tool
// ---------------------------------------------------------------------------

pub struct PromotionRecordTool;

impl PromotionRecordTool {
    fn validate_content_handle(handle: &str) -> anyhow::Result<()> {
        anyhow::ensure!(
            handle.starts_with("sha256:"),
            "content_handle must be a valid SHA-256 content handle (sha256:...)"
        );
        Ok(())
    }
}

impl NativeTool for PromotionRecordTool {
    fn name(&self) -> &'static str {
        "promotion.record"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Records promotion status (evaluator or auditor validation result) for a content handle. Only evaluator.default and auditor.default agents can call this tool.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "content_handle": {
                        "type": "string",
                        "description": "SHA-256 content handle (e.g., 'sha256:abc123...') of the content being promoted"
                    },
                    "role": {
                        "type": "string",
                        "description": "Role recording this promotion: 'evaluator' or 'auditor'",
                        "enum": ["evaluator", "auditor"]
                    },
                    "pass": {
                        "type": "boolean",
                        "description": "Whether this validation passed (true) or failed (false)"
                    },
                    "findings": {
                        "type": "array",
                        "description": "Findings from this validation",
                        "items": {
                            "type": "object",
                            "properties": {
                                "severity": {
                                    "type": "string",
                                    "description": "Severity: 'info', 'warning', 'error', 'critical'",
                                    "enum": ["info", "warning", "error", "critical"]
                                },
                                "description": {
                                    "type": "string",
                                    "description": "Description of the finding"
                                },
                                "evidence": {
                                    "type": "string",
                                    "description": "Optional evidence supporting this finding"
                                }
                            },
                            "required": ["severity", "description"]
                        }
                    },
                    "summary": {
                        "type": "string",
                        "description": "Human-readable summary of the validation result"
                    }
                },
                "required": ["content_handle", "role", "pass"],
                "additionalProperties": false
            }),
        }
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        is_promotion_agent(manifest)
    }

    fn execute(
        &self,
        manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        session_id: Option<&str>,
        turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: PromotionRecordArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        Self::validate_content_handle(&args.content_handle)?;

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Promotion store requires gateway directory to be configured");
        };

        let store = PromotionStore::new(gw_dir)?;

        let record = store.record_promotion(
            args.content_handle.clone(),
            args.role.clone(),
            &manifest.agent.id,
            args.pass,
            args.findings.clone(),
            args.summary.clone(),
        )?;

        // Log to causal chain for tamper-evidence
        let causal_log_path = gw_dir.join("history").join("causal_chain.jsonl");
        if let Some(parent) = causal_log_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(logger) = CausalLogger::new(&causal_log_path) {
            let _ = logger.log(
                &manifest.agent.id,
                session_id.unwrap_or("unknown"),
                turn_id,
                0, // turn_index unknown in tool context
                "tool",
                "promotion.record",
                EntryStatus::Success,
                Some(serde_json::json!({
                    "arguments": {
                        "content_handle": args.content_handle,
                        "role": args.role.as_str(),
                        "pass": args.pass,
                    }
                })),
            );
        }

        let response = PromotionRecordResponse {
            ok: true,
            promotion_record: record,
        };

        serde_json::to_string(&response).map_err(Into::into)
    }
}

// ---------------------------------------------------------------------------
// Promotion Query Tool
// ---------------------------------------------------------------------------

pub struct PromotionQueryTool;

impl NativeTool for PromotionQueryTool {
    fn name(&self) -> &'static str {
        "promotion.query"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Queries the promotion status of a content handle. Returns evaluator and auditor validation results, or null if no promotion record exists.".to_string(),
            input_schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "content_handle": {
                        "type": "string",
                        "description": "SHA-256 content handle (e.g., 'sha256:abc123...') to query promotion status for"
                    }
                },
                "required": ["content_handle"],
                "additionalProperties": false
            }),
        }
    }

    fn is_available(&self, manifest: &AgentManifest) -> bool {
        manifest.capabilities.iter().any(|cap| {
            matches!(
                cap,
                autonoetic_types::capability::Capability::ReadAccess { .. }
            )
        })
    }

    fn execute(
        &self,
        _manifest: &AgentManifest,
        _policy: &PolicyEngine,
        _agent_dir: &Path,
        gateway_dir: Option<&Path>,
        arguments_json: &str,
        _session_id: Option<&str>,
        _turn_id: Option<&str>,
        _config: Option<&autonoetic_types::config::GatewayConfig>,
    ) -> anyhow::Result<String> {
        let args: PromotionQueryArgs = serde_json::from_str(arguments_json)
            .map_err(|e| anyhow::anyhow!("Invalid JSON arguments for '{}': {}", self.name(), e))?;

        anyhow::ensure!(
            args.content_handle.starts_with("sha256:"),
            "content_handle must be a valid SHA-256 content handle (sha256:...)"
        );

        let Some(gw_dir) = gateway_dir else {
            anyhow::bail!("Promotion store requires gateway directory to be configured");
        };

        let store = PromotionStore::new(gw_dir)?;

        let response = match store.get_promotion(&args.content_handle) {
            Some(record) => PromotionQueryResponse {
                content_handle: record.content_handle,
                evaluator_pass: Some(record.evaluator_pass),
                auditor_pass: Some(record.auditor_pass),
                evaluator_id: record.evaluator_id,
                auditor_id: record.auditor_id,
                evaluator_findings: record.evaluator_findings,
                auditor_findings: record.auditor_findings,
                evaluator_timestamp: record.evaluator_timestamp,
                auditor_timestamp: record.auditor_timestamp,
                promotion_gate_version: record.promotion_gate_version,
            },
            None => {
                return serde_json::to_string(&serde_json::json!({
                    "content_handle": args.content_handle,
                    "error": "No promotion record found for this content handle"
                }))
                .map_err(Into::into)
            }
        };

        serde_json::to_string(&response).map_err(Into::into)
    }
}
