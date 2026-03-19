//! Session-scoped resource budgets (config-driven, role-agnostic).
//!
//! Limits are defined in [`autonoetic_types::config::SessionBudgetConfig`] and enforced
//! for all agents sharing the same **session id**. Extend limits by adding optional
//! fields to that struct; [`SessionBudgetConfig::extensions`] is reserved for naming
//! future gateway modules without breaking existing configs.

use autonoetic_types::config::SessionBudgetConfig;
use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

#[derive(Debug, Default, Clone)]
struct ScopeCounters {
    llm_rounds: u64,
    tool_invocations: u64,
    llm_tokens: u64,
    /// Cumulative estimated spend (USD) when `estimated_cost_usd` is provided per completion.
    session_cost_usd: f64,
    clock_start: Option<Instant>,
}

/// Thread-safe registry of per-session counters checked against [`SessionBudgetConfig`].
#[derive(Debug)]
pub struct SessionBudgetRegistry {
    limits: SessionBudgetConfig,
    scopes: Mutex<HashMap<String, ScopeCounters>>,
}

impl SessionBudgetRegistry {
    pub fn new(limits: SessionBudgetConfig) -> Self {
        Self {
            limits,
            scopes: Mutex::new(HashMap::new()),
        }
    }

    /// True if any limit is set (skips locking when everything is unlimited).
    pub fn is_enabled(&self) -> bool {
        self.limits.max_llm_rounds.is_some()
            || self.limits.max_tool_invocations.is_some()
            || self.limits.max_llm_tokens.is_some()
            || self.limits.max_wall_clock_secs.is_some()
            || self.limits.max_session_price_usd.is_some()
    }

    /// Run before each LLM completion attempt (including retries).
    pub fn check_pre_llm(&self, scope: &str) -> anyhow::Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }
        let mut map = self
            .scopes
            .lock()
            .map_err(|e| anyhow::anyhow!("session budget lock poisoned: {e}"))?;
        let st = map.entry(scope.to_string()).or_default();
        if st.clock_start.is_none() {
            st.clock_start = Some(Instant::now());
        }

        if let Some(max_secs) = self.limits.max_wall_clock_secs {
            if let Some(started) = st.clock_start {
                if started.elapsed().as_secs() >= max_secs {
                    anyhow::bail!(
                        "Session budget exceeded: wall_clock_secs >= {} (scope: {})",
                        max_secs,
                        scope
                    );
                }
            }
        }

        if let Some(max_rounds) = self.limits.max_llm_rounds {
            if st.llm_rounds >= max_rounds {
                anyhow::bail!(
                    "Session budget exceeded: max_llm_rounds ({}) (scope: {})",
                    max_rounds,
                    scope
                );
            }
        }

        Ok(())
    }

    /// Record one LLM round and token usage after a provider response.
    /// `estimated_cost_usd` should be set for OpenRouter when catalog pricing is available.
    pub fn record_llm_completion(
        &self,
        scope: &str,
        input_tokens: u64,
        output_tokens: u64,
        estimated_cost_usd: Option<f64>,
    ) -> anyhow::Result<()> {
        if !self.is_enabled() {
            return Ok(());
        }
        let mut map = self
            .scopes
            .lock()
            .map_err(|e| anyhow::anyhow!("session budget lock poisoned: {e}"))?;
        let st = map.entry(scope.to_string()).or_default();
        if st.clock_start.is_none() {
            st.clock_start = Some(Instant::now());
        }
        st.llm_rounds = st.llm_rounds.saturating_add(1);
        let add = input_tokens.saturating_add(output_tokens);
        st.llm_tokens = st.llm_tokens.saturating_add(add);
        if let Some(c) = estimated_cost_usd {
            if c.is_finite() && c >= 0.0 {
                st.session_cost_usd += c;
            }
        }

        if let Some(max_tok) = self.limits.max_llm_tokens {
            if st.llm_tokens > max_tok {
                anyhow::bail!(
                    "Session budget exceeded: max_llm_tokens ({}, used {}) (scope: {})",
                    max_tok,
                    st.llm_tokens,
                    scope
                );
            }
        }

        if let Some(max_price) = self.limits.max_session_price_usd {
            if max_price >= 0.0 && st.session_cost_usd > max_price {
                anyhow::bail!(
                    "Session budget exceeded: max_session_price_usd ({:.6}, used {:.6}) (scope: {})",
                    max_price,
                    st.session_cost_usd,
                    scope
                );
            }
        }

        Ok(())
    }

    /// Reserve tool invocations before executing a batch.
    pub fn reserve_tool_invocations(&self, scope: &str, count: u64) -> anyhow::Result<()> {
        if !self.is_enabled() || count == 0 {
            return Ok(());
        }
        let Some(max_tools) = self.limits.max_tool_invocations else {
            return Ok(());
        };
        let mut map = self
            .scopes
            .lock()
            .map_err(|e| anyhow::anyhow!("session budget lock poisoned: {e}"))?;
        let st = map.entry(scope.to_string()).or_default();
        if st.clock_start.is_none() {
            st.clock_start = Some(Instant::now());
        }
        let next = st.tool_invocations.saturating_add(count);
        if next > max_tools {
            anyhow::bail!(
                "Session budget exceeded: max_tool_invocations ({}, would be {}) (scope: {})",
                max_tools,
                next,
                scope
            );
        }
        st.tool_invocations = next;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_llm_rounds_blocks() {
        let reg = SessionBudgetRegistry::new(SessionBudgetConfig {
            max_llm_rounds: Some(2),
            ..Default::default()
        });
        reg.check_pre_llm("s1").unwrap();
        reg.record_llm_completion("s1", 0, 0, None).unwrap();
        reg.check_pre_llm("s1").unwrap();
        reg.record_llm_completion("s1", 0, 0, None).unwrap();
        assert!(reg.check_pre_llm("s1").is_err());
    }

    #[test]
    fn max_tokens_blocks_after_record() {
        let reg = SessionBudgetRegistry::new(SessionBudgetConfig {
            max_llm_tokens: Some(100),
            ..Default::default()
        });
        reg.check_pre_llm("s1").unwrap();
        reg.record_llm_completion("s1", 60, 50, None)
            .unwrap_err(); // 110 > 100
    }

    #[test]
    fn max_tools_reserves() {
        let reg = SessionBudgetRegistry::new(SessionBudgetConfig {
            max_tool_invocations: Some(3),
            ..Default::default()
        });
        reg.reserve_tool_invocations("s1", 2).unwrap();
        reg.reserve_tool_invocations("s1", 1).unwrap();
        assert!(reg.reserve_tool_invocations("s1", 1).is_err());
    }

    #[test]
    fn max_session_price_usd_blocks() {
        let reg = SessionBudgetRegistry::new(SessionBudgetConfig {
            max_session_price_usd: Some(0.01),
            ..Default::default()
        });
        reg.check_pre_llm("s1").unwrap();
        reg.record_llm_completion("s1", 100, 100, Some(0.005)).unwrap();
        assert!(reg
            .record_llm_completion("s1", 100, 100, Some(0.02))
            .is_err());
    }
}
