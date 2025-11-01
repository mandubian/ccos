//! CCOS Security Policies
//!
//! Predefined security policies for common use cases with CCOS-specific capability IDs.
//! These policies use RTFS RuntimeContext but contain CCOS-specific capability ID mappings.

use rtfs::runtime::security::RuntimeContext;

/// Predefined security policies for common use cases
/// 
/// These policies provide presets for common execution scenarios in CCOS.
/// They configure RuntimeContext with appropriate capability allowlists for CCOS capabilities.
pub struct SecurityPolicies;

impl SecurityPolicies {
    /// Policy for running user-provided RTFS code
    pub fn user_code() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            // Allow safe LLM calls in user code
            "ccos.ai.llm-execute".to_string(),
        ])
    }

    /// Policy for running system management code
    pub fn system_management() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.io.print".to_string(),
            "ccos.io.println".to_string(),
            "ccos.io.file-exists".to_string(),
            "ccos.io.read-file".to_string(),
            "ccos.io.write-file".to_string(),
            "ccos.io.delete-file".to_string(),
            "ccos.system.current-time".to_string(),
            "ccos.system.current-timestamp-ms".to_string(),
            // Allow LLM calls for system prompts (audited)
            "ccos.ai.llm-execute".to_string(),
        ])
    }

    /// Policy for running data processing code
    pub fn data_processing() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
            "ccos.ask-human".to_string(),
            // Allow LLM calls for summarization/extraction
            "ccos.ai.llm-execute".to_string(),
        ])
    }

    /// Policy for running agent coordination code
    pub fn agent_coordination() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.agent.discover-agents".to_string(),
            "ccos.agent.task-coordination".to_string(),
            "ccos.agent.ask-human".to_string(),
            "ccos.agent.discover-and-assess-agents".to_string(),
            "ccos.agent.establish-system-baseline".to_string(),
            // Allow LLM calls for negotiation/coordination
            "ccos.ai.llm-execute".to_string(),
        ])
    }

    /// Policy for running file operations (high security)
    pub fn file_operations() -> RuntimeContext {
        let mut ctx = RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.io.file-exists".to_string(),
            "ccos.io.read-file".to_string(),
            "ccos.io.write-file".to_string(),
            "ccos.io.delete-file".to_string(),
            "ccos.io.open-file".to_string(),
            "ccos.io.read-line".to_string(),
            "ccos.io.write-line".to_string(),
            "ccos.io.close-file".to_string(),
            // LLM execution disabled here by default for tighter isolation
        ]);

        // Force microVM for all file operations
        ctx.use_microvm = true;
        ctx.max_execution_time = Some(10000); // 10 seconds
        ctx.max_memory_usage = Some(32 * 1024 * 1024); // 32MB

        ctx
    }

    /// Policy for testing capabilities (includes all test capabilities)
    pub fn test_capabilities() -> RuntimeContext {
        RuntimeContext::controlled(vec![
            "ccos.echo".to_string(),
            "ccos.math.add".to_string(),
            "ccos.ask-human".to_string(),
            "ccos.io.log".to_string(),
            // Enable LLM for tests
            "ccos.ai.llm-execute".to_string(),
        ])
    }

    /// Policy for running networking operations (HTTP, etc.) under isolation
    pub fn networking() -> RuntimeContext {
        let mut ctx = RuntimeContext::controlled(vec![
            "ccos.io.log".to_string(),
            "ccos.network.http-fetch".to_string(),
        ]);

        // Enforce microVM for network operations per validator policy
        ctx.use_microvm = true;
        // Reasonable defaults for network tasks
        ctx.max_execution_time = Some(10000); // 10 seconds
        ctx.max_memory_usage = Some(32 * 1024 * 1024); // 32MB

        ctx
    }
}

