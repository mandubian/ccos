//! Provider resolution — maps a raw `LlmConfig` into a concrete, resolved
//! endpoint + auth configuration.
//!
//! Drivers should never read environment variables directly; they receive a
//! `ResolvedProvider` already populated by this module.

/// Flags describing what a provider's API supports.
/// Drivers use these to decide which code paths to take.
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    /// Provider supports real SSE streaming (not just simulated).
    pub supports_streaming: bool,
    /// Provider streams individual tool-input JSON deltas during streaming.
    pub supports_tool_stream_deltas: bool,
    /// Provider requires system prompt at top level (not in the messages array).
    pub supports_system_top_level: bool,
    /// Provider includes token usage counts in stream chunks.
    pub supports_usage_in_stream: bool,
}

impl ProviderCapabilities {
    /// OpenAI-compatible endpoints (OpenAI, OpenRouter, Groq, etc.)
    pub fn openai_compatible() -> Self {
        Self {
            supports_streaming: true,
            supports_tool_stream_deltas: true,
            supports_system_top_level: false,
            supports_usage_in_stream: false,
        }
    }

    /// Anthropic Messages API
    pub fn anthropic() -> Self {
        Self {
            supports_streaming: true,
            supports_tool_stream_deltas: true,
            supports_system_top_level: true,
            supports_usage_in_stream: true,
        }
    }

    /// Google Gemini generateContent API
    pub fn gemini() -> Self {
        Self {
            supports_streaming: false, // we don't implement Gemini streaming yet
            supports_tool_stream_deltas: false,
            supports_system_top_level: true,
            supports_usage_in_stream: false,
        }
    }
}

/// Which authentication strategy to use.
#[derive(Debug, Clone)]
pub enum AuthStrategy {
    /// `Authorization: Bearer <key>` header (OpenAI-style)
    BearerToken(String),
    /// `x-api-key: <key>` header (Anthropic-style)
    XApiKey(String),
    /// `x-goog-api-key: <key>` header (Gemini-style)
    GoogleApiKey(String),
    /// No authentication required (e.g., local Ollama)
    None,
}

/// The family of wire protocol that a driver should use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DriverKind {
    OpenAi,
    Anthropic,
    Gemini,
}

/// Fully-resolved provider configuration ready for use by a driver.
/// No driver should ever call `std::env::var` directly.
#[derive(Debug, Clone)]
pub struct ResolvedProvider {
    pub kind: DriverKind,
    pub base_url: String,
    pub model: String,
    pub auth: AuthStrategy,
    pub capabilities: ProviderCapabilities,
    /// Extra HTTP headers to attach (e.g., OpenRouter attribution headers)
    pub extra_headers: Vec<(String, String)>,
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// Known provider defaults table
// ---------------------------------------------------------------------------

struct ProviderDefaults {
    base_url: &'static str,
    api_key_env: &'static str,
    kind: DriverKind,
    capabilities: fn() -> ProviderCapabilities,
}

fn provider_defaults(name: &str) -> Option<ProviderDefaults> {
    match name {
        "anthropic" | "claude" => Some(ProviderDefaults {
            base_url: "https://api.anthropic.com/v1/messages",
            api_key_env: "ANTHROPIC_API_KEY",
            kind: DriverKind::Anthropic,
            capabilities: ProviderCapabilities::anthropic,
        }),
        "gemini" | "google" => Some(ProviderDefaults {
            base_url: "https://generativelanguage.googleapis.com/v1beta",
            api_key_env: "GEMINI_API_KEY",
            kind: DriverKind::Gemini,
            capabilities: ProviderCapabilities::gemini,
        }),
        // ----------- OpenAI-compatible providers (single code path) -----------
        "openai" | "codex" => Some(ProviderDefaults {
            base_url: "https://api.openai.com/v1/chat/completions",
            api_key_env: "OPENAI_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "openrouter" => Some(ProviderDefaults {
            base_url: "https://openrouter.ai/api/v1/chat/completions",
            api_key_env: "OPENROUTER_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "groq" => Some(ProviderDefaults {
            base_url: "https://api.groq.com/openai/v1/chat/completions",
            api_key_env: "GROQ_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "together" => Some(ProviderDefaults {
            base_url: "https://api.together.xyz/v1/chat/completions",
            api_key_env: "TOGETHER_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "deepseek" => Some(ProviderDefaults {
            base_url: "https://api.deepseek.com/v1/chat/completions",
            api_key_env: "DEEPSEEK_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "mistral" => Some(ProviderDefaults {
            base_url: "https://api.mistral.ai/v1/chat/completions",
            api_key_env: "MISTRAL_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "fireworks" => Some(ProviderDefaults {
            base_url: "https://api.fireworks.ai/inference/v1/chat/completions",
            api_key_env: "FIREWORKS_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "perplexity" => Some(ProviderDefaults {
            base_url: "https://api.perplexity.ai/chat/completions",
            api_key_env: "PERPLEXITY_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "cohere" => Some(ProviderDefaults {
            base_url: "https://api.cohere.com/compatibility/v1/chat/completions",
            api_key_env: "COHERE_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "ai21" => Some(ProviderDefaults {
            base_url: "https://api.ai21.com/studio/v1/chat/completions",
            api_key_env: "AI21_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "cerebras" => Some(ProviderDefaults {
            base_url: "https://api.cerebras.ai/v1/chat/completions",
            api_key_env: "CEREBRAS_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "sambanova" => Some(ProviderDefaults {
            base_url: "https://api.sambanova.ai/v1/chat/completions",
            api_key_env: "SAMBANOVA_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "huggingface" => Some(ProviderDefaults {
            base_url: "https://api-inference.huggingface.co/v1/chat/completions",
            api_key_env: "HUGGINGFACE_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "xai" => Some(ProviderDefaults {
            base_url: "https://api.x.ai/v1/chat/completions",
            api_key_env: "XAI_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "replicate" => Some(ProviderDefaults {
            base_url: "https://api.replicate.com/v1/deployments",
            api_key_env: "REPLICATE_API_TOKEN",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "moonshot" | "kimi" => Some(ProviderDefaults {
            base_url: "https://api.moonshot.cn/v1/chat/completions",
            api_key_env: "MOONSHOT_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "qwen" | "dashscope" => Some(ProviderDefaults {
            base_url: "https://dashscope.aliyuncs.com/compatible-mode/v1/chat/completions",
            api_key_env: "DASHSCOPE_API_KEY",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        // Local providers — no API key needed
        "ollama" => Some(ProviderDefaults {
            base_url: "http://localhost:11434/v1/chat/completions",
            api_key_env: "",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "vllm" => Some(ProviderDefaults {
            base_url: "http://localhost:8000/v1/chat/completions",
            api_key_env: "",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        "lmstudio" => Some(ProviderDefaults {
            base_url: "http://localhost:1234/v1/chat/completions",
            api_key_env: "",
            kind: DriverKind::OpenAi,
            capabilities: ProviderCapabilities::openai_compatible,
        }),
        _ => None,
    }
}

/// Resolve a provider name + optional overrides into a `ResolvedProvider`.
/// Returns an error if an API key is required but missing from the environment.
pub fn resolve(
    provider: &str,
    model: &str,
    temperature: Option<f32>,
    max_tokens: Option<u32>,
    base_url_override: Option<&str>,
    api_key_override: Option<&str>,
) -> anyhow::Result<ResolvedProvider> {
    let defaults = provider_defaults(provider);

    let (kind, base_url, capabilities) = if let Some(ref d) = defaults {
        (
            d.kind.clone(),
            base_url_override.unwrap_or(d.base_url).to_string(),
            (d.capabilities)(),
        )
    } else if let Some(url) = base_url_override {
        // Unknown provider with a custom URL — treat as OpenAI-compatible
        (
            DriverKind::OpenAi,
            url.to_string(),
            ProviderCapabilities::openai_compatible(),
        )
    } else {
        anyhow::bail!(
            "Unknown provider '{}' and no base_url override provided",
            provider
        );
    };

    // Resolve auth
    let api_key = if let Some(k) = api_key_override {
        k.to_string()
    } else if let Some(ref d) = defaults {
        if d.api_key_env.is_empty() {
            String::new() // no key needed (Ollama)
        } else {
            std::env::var(d.api_key_env).map_err(|_| {
                anyhow::anyhow!(
                    "Missing {} environment variable for provider '{}'",
                    d.api_key_env,
                    provider
                )
            })?
        }
    } else {
        String::new()
    };

    let auth = match kind {
        DriverKind::Anthropic => AuthStrategy::XApiKey(api_key),
        DriverKind::Gemini => AuthStrategy::GoogleApiKey(api_key),
        DriverKind::OpenAi => {
            if api_key.is_empty() {
                AuthStrategy::None
            } else {
                AuthStrategy::BearerToken(api_key)
            }
        }
    };

    // Attach OpenRouter attribution headers
    let extra_headers = if provider == "openrouter" {
        vec![
            (
                "HTTP-Referer".to_string(),
                "https://autonoetic.ccos.local".to_string(),
            ),
            ("X-Title".to_string(), "Autonoetic Gateway".to_string()),
        ]
    } else {
        vec![]
    };

    Ok(ResolvedProvider {
        kind,
        base_url,
        model: model.to_string(),
        auth,
        capabilities,
        extra_headers,
        temperature,
        max_tokens,
    })
}
