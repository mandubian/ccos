//! Artifact Generator
//!
//! Generates RTFS capability s-expressions from parameter schemas.
//! Uses (call ...) primitive for host delegation. Implements spec section 21.3-21.5.

use super::schema_builder::{ParamSchema, ParamTypeInfo};
use super::skill_extractor::{
    extract_constraints, extract_skills_advanced, ExtractedSkill, SkillCategory,
};
use crate::synthesis::status::{STATUS_PROCESSING, STATUS_READY_FOR_EXECUTION};
use crate::synthesis::InteractionTurn;
use crate::capability_marketplace::types::CapabilityManifest;
use crate::rtfs_bridge::extractors::capability_def_to_rtfs_string;
use rtfs::ast::{
    CapabilityDefinition, DoExpr, Expression, Keyword, LetBinding, LetExpr, Literal, MapKey,
    Property, Symbol,
};
use std::collections::HashMap;

/// Generate a collector capability that asks sequential questions.
pub fn generate_collector(schema: &ParamSchema, domain: &str) -> String {
    // (parameters -> built below using AST types)

    // Build Let bindings as AST LetBinding entries
    let mut bindings: Vec<LetBinding> = Vec::new();
    for (i, (_k, _meta)) in schema.params.iter().enumerate() {
        let var = format!("p{}", i + 1);
        let prompt = if _meta.prompt.is_empty() {
            _meta.key.clone()
        } else {
            _meta.prompt.clone()
        };
        let prompt_sane = prompt.replace('"', "'");
        // (call ccos.user.ask "prompt")
        let call = Expression::FunctionCall {
            callee: Box::new(Expression::Symbol(Symbol("call".into()))),
            arguments: vec![
                Expression::Symbol(Symbol("ccos.user.ask".into())),
                Expression::Literal(Literal::String(prompt_sane)),
            ],
        };
        bindings.push(LetBinding {
            pattern: rtfs::ast::Pattern::Symbol(Symbol(var.clone())),
            type_annotation: None,
            value: Box::new(call),
        });
    }

    // Build context map: {:key p1 :key2 p2}
    let mut context_map: HashMap<MapKey, Expression> = HashMap::new();
    for (i, (_k, meta)) in schema.params.iter().enumerate() {
        let var = format!("p{}", i + 1);
        context_map.insert(
            MapKey::Keyword(Keyword(meta.key.clone())),
            Expression::Symbol(Symbol(var)),
        );
    }

    // inner let: (let [context { ... }] {:status "..." :context context})
    let inner_let = LetExpr {
        bindings: vec![LetBinding {
            pattern: rtfs::ast::Pattern::Symbol(Symbol("context".into())),
            type_annotation: None,
            value: Box::new(Expression::Map(context_map)),
        }],
        body: vec![Expression::Map({
            let mut m = HashMap::new();
            m.insert(
                MapKey::Keyword(Keyword("status".into())),
                Expression::Literal(Literal::String(STATUS_READY_FOR_EXECUTION.into())),
            );
            m.insert(
                MapKey::Keyword(Keyword("context".into())),
                Expression::Symbol(Symbol("context".into())),
            );
            m
        })],
    };

    let impl_do = Expression::Let(LetExpr {
        bindings,
        body: vec![Expression::Let(inner_let)],
    });

    // Build CapabilityDefinition using AST types
    let cap_def = CapabilityDefinition {
        name: Symbol(format!("{}.collector.v1", domain)),
        properties: vec![
            Property {
                key: Keyword("description".into()),
                value: Expression::Literal(Literal::String("AUTO-GENERATED COLLECTOR".into())),
            },
            Property {
                key: Keyword("parameters".into()),
                value: Expression::List(vec![]),
            },
            Property {
                key: Keyword("implementation".into()),
                value: impl_do,
            },
        ],
    };

    capability_def_to_rtfs_string(&cap_def)
}

/// Generate a planner capability that embeds a synthesized plan using captured context.
pub fn generate_planner(schema: &ParamSchema, history: &[InteractionTurn], domain: &str) -> String {
    let mut ordered_params: Vec<(&String, &super::schema_builder::ParamMeta)> =
        schema.params.iter().collect();
    ordered_params.sort_by(|a, b| a.0.cmp(b.0));

    // Build expects expression capturing required context keys (sorted for determinism)
    let mut expects_keys: Vec<Expression> = Vec::new();
    for (key, _meta) in ordered_params.iter() {
        expects_keys.push(Expression::Symbol(Symbol(format!(":{}", key))));
    }
    let expects_expr = if expects_keys.is_empty() {
        Expression::List(vec![
            Expression::Symbol(Symbol(":expects".into())),
            Expression::List(vec![]),
        ])
    } else {
        Expression::List(vec![
            Expression::Symbol(Symbol(":expects".into())),
            Expression::List(vec![
                Expression::Symbol(Symbol(":context/keys".into())),
                Expression::List(expects_keys),
            ]),
        ])
    };

    let required_keys_vec = Expression::Vector(
        ordered_params
            .iter()
            .map(|(key, _)| Expression::Literal(Literal::Keyword(Keyword((*key).clone()))))
            .collect(),
    );

    let plan_id = format!("{}.synthesized.plan.v1", domain);
    let capability_id = format!("{}.generated.capability.v1", domain);

    let required_keys_literal = if ordered_params.is_empty() {
        "[]".to_string()
    } else {
        let joined = ordered_params
            .iter()
            .map(|(key, _)| format!(":{}", key))
            .collect::<Vec<_>>()
            .join(" ");
        format!("[{}]", joined)
    };

    let plan_body_unescaped = format!(
        "(do\n  {{:status \"requires_agent\"\n    :message \"Implement capability :{capability_id} or register an equivalent agent.\"\n    :generated-capability :{capability_id}\n    :required-keys {required_keys_literal}\n    :context context}})"
    );
    let plan_body_literal = sanitize_literal_string(&plan_body_unescaped);

    // Capture parameter details as structured metadata for downstream refinement
    let inputs_vector = Expression::Vector(
        ordered_params
            .iter()
            .map(|(key, meta)| {
                let mut param_map = HashMap::new();
                param_map.insert(
                    MapKey::Keyword(Keyword("key".into())),
                    Expression::Literal(Literal::Keyword(Keyword((*key).clone()))),
                );
                let prompt = if meta.prompt.is_empty() {
                    (*key).clone()
                } else {
                    meta.prompt.clone()
                };
                param_map.insert(
                    MapKey::Keyword(Keyword("prompt".into())),
                    Expression::Literal(Literal::String(sanitize_literal_string(&prompt))),
                );
                let answer_expr = match &meta.answer {
                    Some(ans) => Expression::Literal(Literal::String(sanitize_literal_string(ans))),
                    None => Expression::Literal(Literal::Nil),
                };
                param_map.insert(MapKey::Keyword(Keyword("answer".into())), answer_expr);
                param_map.insert(
                    MapKey::Keyword(Keyword("required".into())),
                    Expression::Literal(Literal::Boolean(meta.required)),
                );
                param_map.insert(
                    MapKey::Keyword(Keyword("source-turn".into())),
                    Expression::Literal(Literal::Integer(meta.source_turn as i64)),
                );
                Expression::Map(param_map)
            })
            .collect(),
    );

    // Preserve the raw conversation turns as structured metadata
    let conversation_vector = Expression::Vector(
        history
            .iter()
            .map(|turn| {
                let mut turn_map = HashMap::new();
                turn_map.insert(
                    MapKey::Keyword(Keyword("turn-index".into())),
                    Expression::Literal(Literal::Integer(turn.turn_index as i64)),
                );
                turn_map.insert(
                    MapKey::Keyword(Keyword("prompt".into())),
                    Expression::Literal(Literal::String(sanitize_literal_string(&turn.prompt))),
                );
                let ans_expr = match &turn.answer {
                    Some(ans) => Expression::Literal(Literal::String(sanitize_literal_string(ans))),
                    None => Expression::Literal(Literal::Nil),
                };
                turn_map.insert(MapKey::Keyword(Keyword("answer".into())), ans_expr);
                Expression::Map(turn_map)
            })
            .collect(),
    );

    let mut plan_map = HashMap::new();
    plan_map.insert(
        MapKey::Keyword(Keyword("plan-id".into())),
        Expression::Literal(Literal::String(plan_id.clone())),
    );
    plan_map.insert(
        MapKey::Keyword(Keyword("language".into())),
        Expression::Literal(Literal::String("rtfs20".into())),
    );
    plan_map.insert(
        MapKey::Keyword(Keyword("plan-body".into())),
        Expression::Literal(Literal::String(plan_body_literal)),
    );
    plan_map.insert(
        MapKey::Keyword(Keyword("generated-capability".into())),
        Expression::Literal(Literal::Keyword(Keyword(capability_id.clone()))),
    );
    plan_map.insert(
        MapKey::Keyword(Keyword("required-keys".into())),
        required_keys_vec.clone(),
    );
    plan_map.insert(MapKey::Keyword(Keyword("inputs".into())), inputs_vector);
    plan_map.insert(
        MapKey::Keyword(Keyword("conversation".into())),
        conversation_vector,
    );
    plan_map.insert(
        MapKey::Keyword(Keyword("turns-total".into())),
        Expression::Literal(Literal::Integer(history.len() as i64)),
    );

    let diagnostics = sanitize_literal_string(&format!(
        "synthesized planner captured {} parameters across {} turns",
        ordered_params.len(),
        history.len()
    ));

    let mut result_map = HashMap::new();
    result_map.insert(
        MapKey::Keyword(Keyword("status".into())),
        Expression::Literal(Literal::String(STATUS_PROCESSING.into())),
    );
    result_map.insert(
        MapKey::Keyword(Keyword("context".into())),
        Expression::Symbol(Symbol("context".into())),
    );
    result_map.insert(
        MapKey::Keyword(Keyword("result".into())),
        Expression::Map(plan_map),
    );
    result_map.insert(
        MapKey::Keyword(Keyword("source".into())),
        Expression::Literal(Literal::String("ccos.synthesis".into())),
    );
    result_map.insert(
        MapKey::Keyword(Keyword("diagnostics".into())),
        Expression::Literal(Literal::String(diagnostics)),
    );

    let impl_do = Expression::Do(DoExpr {
        expressions: vec![Expression::Map(result_map)],
    });

    let mut properties = vec![
        Property {
            key: Keyword("description".into()),
            value: Expression::Literal(Literal::String(
                "AUTO-GENERATED PLANNER (embedded synthesis plan)".into(),
            )),
        },
        Property {
            key: Keyword("parameters".into()),
            value: Expression::Map({
                let mut m = HashMap::new();
                m.insert(
                    MapKey::Keyword(Keyword("context".into())),
                    Expression::Literal(Literal::String("map".into())),
                );
                m
            }),
        },
    ];

    properties.push(Property {
        key: Keyword("expects".into()),
        value: expects_expr,
    });
    properties.push(Property {
        key: Keyword("implementation".into()),
        value: impl_do,
    });

    let cap_def = CapabilityDefinition {
        name: Symbol(format!("{}.planner.v1", domain)),
        properties,
    };

    capability_def_to_rtfs_string(&cap_def)
}

/// Generate a planner prompt that asks the delegating arbiter to synthesize a fresh capability
/// and plan using the collector output as grounding context. The prompt captures the schema,
/// collected answers, and conversation history so LLM-side delegation has the same inputs the
/// runtime would observe.
pub fn generate_planner_via_arbiter(
    schema: &ParamSchema,
    history: &[InteractionTurn],
    domain: &str,
) -> String {
    let collector_rtfs = generate_collector(schema, domain);

    let mut ordered_params: Vec<(&String, &super::schema_builder::ParamMeta)> =
        schema.params.iter().collect();
    ordered_params.sort_by(|a, b| a.0.cmp(b.0));

    let params_section = if ordered_params.is_empty() {
        "- (no parameters declared)".to_string()
    } else {
        ordered_params
            .iter()
            .map(|(key, meta)| {
                let prompt = if meta.prompt.is_empty() {
                    (*key).clone()
                } else {
                    meta.prompt.clone()
                };
                let answer = meta
                    .answer
                    .as_ref()
                    .map(|a| sanitize_literal_string(a))
                    .unwrap_or_else(|| "<no answer>".to_string());
                format!(
                    "- :{} (required: {}, prompt: \"{}\", answer: {})",
                    key,
                    meta.required,
                    sanitize_literal_string(&prompt),
                    answer
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let history_section = if history.is_empty() {
        "- (no prior interaction turns captured)".to_string()
    } else {
        history
            .iter()
            .map(|turn| {
                let answer = turn
                    .answer
                    .as_ref()
                    .map(|a| sanitize_literal_string(a))
                    .unwrap_or_else(|| "<no answer>".to_string());
                format!(
                    "- turn {} :: prompt=\"{}\" :: answer={}",
                    turn.turn_index,
                    sanitize_literal_string(&turn.prompt),
                    answer
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    let required_keys = if ordered_params.is_empty() {
        "[]".to_string()
    } else {
        let joined = ordered_params
            .iter()
            .map(|(key, _)| format!(":{}", key))
            .collect::<Vec<_>>()
            .join(" ");
        format!("[{}]", joined)
    };

    // Build a concise goal hint from available answers
    let goal_hints = if ordered_params.is_empty() {
        "- (no answers collected)".to_string()
    } else {
        ordered_params
            .iter()
            .map(|(key, meta)| {
                let ans = meta
                    .answer
                    .as_ref()
                    .map(|s| sanitize_literal_string(s))
                    .unwrap_or_else(|| "<no answer>".to_string());
                format!("- {} => {}", key, ans)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    // Load RTFS grammar and anti-patterns to steer the LLM output toward valid RTFS
    let grammar = std::fs::read_to_string("assets/prompts/arbiter/plan_generation/v1/grammar.md")
        .or_else(|_| {
            std::fs::read_to_string("../assets/prompts/arbiter/plan_generation/v1/grammar.md")
        })
        .unwrap_or_else(|_| "(grammar unavailable)".to_string());
    let anti_patterns =
        std::fs::read_to_string("assets/prompts/arbiter/plan_generation_full/v1/anti_patterns.md")
            .or_else(|_| {
                std::fs::read_to_string(
                    "../assets/prompts/arbiter/plan_generation_full/v1/anti_patterns.md",
                )
            })
            .unwrap_or_else(|_| "(anti-patterns unavailable)".to_string());

    format!(
        concat!(
            "You are the Delegating Arbiter LLM tasked with synthesizing a new RTFS capability",
            " that can execute a fully-specified plan for the domain `{domain}`.",
            "\n\n",
            "## Grounding Context\n",
            "### Collector Capability Prototype\n",
            "```rtfs\n{collector}\n```\n",
            "### Parameter Schema\n",
            "Required keys: {required_keys}\n",
            "{params_section}\n\n",
            "### Conversation History\n",
            "{history_section}\n\n",
            "### Goal Hints (derived from answers)\n",
            "{goal_hints}\n\n",
            "## RTFS Plan Grammar (Reference)\n",
            "````markdown\n{grammar}\n````\n\n",
            "## Common Anti-Patterns to Avoid\n",
            "````markdown\n{anti_patterns}\n````\n\n",
            "## Strict Output Contract\n",
            "- Return ONLY a single top-level RTFS `(capability ...)` form.\n",
            "- Do NOT wrap the output in `(do ...)`.\n",
            "- Do NOT use host language constructs (no `fn`, no `clojure.*`, no comments).\n",
            "- The `:implementation` body MUST be valid RTFS using only RTFS special forms,\n",
            "  and MUST consume the collected context or declared parameters.\n",
            "- Start the response with `(capability` on the first line. No prose before or after.\n",
            "- Optionally include `:expects` and `:needs_capabilities`.\n\n",
            "## HTTP Capability Usage\n",
            "- For HTTP requests, use: (call \\\"ccos.network.http-fetch\\\" ...)\n",
            "- Map format: (call \\\"ccos.network.http-fetch\\\" {{:url \\\"https://...\\\" :method \\\"GET\\\" :headers {{...}} :body \\\"...\\\"}})\n",
            "- List format: (call \\\"ccos.network.http-fetch\\\" :url \\\"https://...\\\" :method \\\"GET\\\" :headers {{...}} :body \\\"...\\\")\n",
            "- Simple format: (call \\\"ccos.network.http-fetch\\\" \\\"https://...\\\")  ; for GET requests\n",
            "- Response format: {{:status 200 :body \\\"...\\\" :headers {{...}}}}\n"
        ),
        domain = domain,
        collector = collector_rtfs,
        required_keys = required_keys,
        params_section = params_section,
        history_section = history_section,
        goal_hints = goal_hints,
        grammar = grammar,
        anti_patterns = anti_patterns
    )
}

// Stub generation removed - replaced with deferred execution approach

/// Convert our local `Elem` builder into the repository `ast::Expression`.
// All generators now construct AST types directly; no builder placeholder needed.

/// Extract capability requirements from synthesized RTFS code.
pub fn extract_capability_requirements(rtfs_code: &str) -> Vec<String> {
    let mut reqs = Vec::new();
    for line in rtfs_code.lines() {
        if let Some(pos) = line.find("(call ccos.") {
            let start = pos + "(call ".len();
            // find end of token
            if let Some(end_rel) = line[start..].find(|c: char| c.is_whitespace() || c == ')') {
                let cap = line[start..start + end_rel].trim().to_string();
                reqs.push(cap);
            }
        }
    }
    reqs
}

/// Enhanced agent synthesis with intelligent parameter mapping and skill-based plan generation
pub fn generate_agent_with_intelligent_mapping(
    schema: &ParamSchema,
    history: &[InteractionTurn],
    domain: &str,
) -> String {
    // Extract skills and constraints from interaction history
    let skills = extract_skills_advanced(history);
    let constraints = extract_constraints(history);

    // Analyze parameter flow patterns
    let param_flow = analyze_parameter_flow(schema, &skills);

    // Generate RTFS plan based on skills and parameter flow
    let rtfs_plan = generate_skill_based_rtfs_plan(&param_flow, &skills, &constraints, domain);

    // Create agent descriptor with intelligent mappings
    let agent_skills: Vec<String> = skills
        .into_iter()
        .filter(|s| s.confidence > 0.6)
        .map(|s| s.skill)
        .collect();

    // Render skills and constraints in a human-friendly way instead of Debug output
    let skills_repr = format!("[{}]", agent_skills.join(", "));
    let constraints_repr = format!(
        "[{}]",
        constraints
            .iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );

    format!(
        r#"
AgentDescriptor {{
    agent_id: "{}.agent.v1",
    execution_mode: AgentExecutionMode::RTFS {{
        plan: "{}"
    }},
    skills: {},
    supported_constraints: {},
    trust_tier: TrustTier::T1Trusted,
    // ... other metadata
}}
"#,
        domain,
        rtfs_plan.replace('"', "\\\"").replace('\n', "\\n"),
        skills_repr,
        constraints_repr
    )
}

/// v0.1: Registry-first planner generator.
///
/// - Looks for a capability in `marketplace_snapshot` whose `metadata["context/keys"]`
///   (comma-separated) covers all required keys from `schema`.
/// - If a perfect match is found, emit a direct-call RTFS planner that calls that capability.
/// - Otherwise, emit a requires-agent stub using `generate_stub`.
pub fn generate_planner_generic_v0_1(
    schema: &ParamSchema,
    history: &[InteractionTurn],
    domain: &str,
    marketplace_snapshot: &[CapabilityManifest],
) -> String {
    // Collect required keys from schema
    let mut required_keys: Vec<String> = Vec::new();
    for (k, meta) in &schema.params {
        if meta.required {
            required_keys.push(k.clone());
        }
    }

    // Try to find a perfect capability match
    for manifest in marketplace_snapshot {
        if let Some(ctx_keys_csv) = manifest.metadata.get("context/keys") {
            let candidate_keys: Vec<String> = ctx_keys_csv
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            let mut all_present = true;
            for rk in &required_keys {
                if !candidate_keys.iter().any(|c| c == rk) {
                    all_present = false;
                    break;
                }
            }

            if all_present {
                // Emit direct-call planner RTFS using the capability id
                let cap_id = &manifest.id;
                // Simple RTFS: call the capability with the provided context and forward result
                let rtfs = format!(
                    "(do\n  (let [result (call :{} {{:context context}})]\n    {{:status \"{}\" :result result :context context}}))",
                    cap_id, STATUS_PROCESSING
                );
                return rtfs;
            }
        }
    }

    // No perfect match -> synthesize a planner embedding a new capability proposal
    generate_planner(schema, history, domain)
}

/// Analyze how parameters should flow through RTFS plans based on skills
fn analyze_parameter_flow(schema: &ParamSchema, skills: &[ExtractedSkill]) -> ParameterFlow {
    let mut flow = ParameterFlow {
        input_params: Vec::new(),
        intermediate_params: Vec::new(),
        output_params: Vec::new(),
        capability_calls: Vec::new(),
    };

    // Classify parameters based on skills and typical usage patterns
    for (key, meta) in &schema.params {
        if skills.iter().any(|s| s.category == SkillCategory::Analysis) {
            // Analysis skills suggest parameters flow through processing steps
            if key.contains("input") || key.contains("source") {
                flow.input_params.push(key.clone());
            } else if key.contains("result") || key.contains("output") {
                flow.output_params.push(key.clone());
            } else {
                flow.intermediate_params.push(key.clone());
            }
        } else {
            // Default classification
            flow.input_params.push(key.clone());
        }
    }

    // Generate capability calls based on skills
    for skill in skills {
        if skill.confidence > 0.7 {
            let capability_name = skill_to_capability_name(&skill.skill);
            flow.capability_calls.push(capability_name);
        }
    }

    flow
}

/// Convert skill name to capability name
fn skill_to_capability_name(skill: &str) -> String {
    skill.replace("-", ".").replace("_", ".")
}

/// Generate RTFS plan based on skills and parameter flow
fn generate_skill_based_rtfs_plan(
    flow: &ParameterFlow,
    skills: &[ExtractedSkill],
    constraints: &[String],
    domain: &str,
) -> String {
    let mut plan_parts = Vec::new();

    // Start with input parameter binding
    if !flow.input_params.is_empty() {
        plan_parts.push(format!(
            "(let [{}]",
            flow.input_params
                .iter()
                .enumerate()
                .map(|(i, param)| format!("{} {}", param.replace("/", "_"), param))
                .collect::<Vec<_>>()
                .join(" ")
        ));
    }

    // Add capability calls based on skills
    for capability in &flow.capability_calls {
        let args = flow
            .input_params
            .iter()
            .take(2) // Limit to first 2 params for simplicity
            .map(|p| p.replace("/", "_"))
            .collect::<Vec<_>>()
            .join(" ");

        if !args.is_empty() {
            plan_parts.push(format!("(call {}.{} {})", domain, capability, args));
        } else {
            plan_parts.push(format!("(call {}.{} {{}})", domain, capability));
        }
    }

    // Add final result construction
    let result_parts: Vec<String> = flow
        .output_params
        .iter()
        .enumerate()
        .map(|(i, param)| format!(":{} {}", param.replace("/", "_"), param.replace("/", "_")))
        .collect();

    // Serialize constraints in an RTFS-friendly form. If constraints are expressions, print
    // using expression RTFS printer; otherwise fall back to JSON for readability.
    let constraints_str = match serde_json::to_string(&constraints) {
        Ok(js) => js,
        Err(_) => format!("{:?}", constraints),
    };

    if !result_parts.is_empty() {
        plan_parts.push(format!(
            "{{:status \"task_ready\" {} :constraints {}}}",
            result_parts.join(" "),
            constraints_str
        ));
    } else {
        plan_parts.push(format!(
            "{{:status \"task_ready\" :constraints {}}}",
            constraints_str
        ));
    }

    // Close let if we opened one
    if !flow.input_params.is_empty() {
        plan_parts.insert(0, "(do".to_string());
        plan_parts.push(")".to_string());
    } else {
        plan_parts.insert(0, "(do".to_string());
        plan_parts.push(")".to_string());
    }

    plan_parts.join("\n  ")
}

/// Parameter flow analysis result
#[derive(Debug)]
struct ParameterFlow {
    input_params: Vec<String>,
    intermediate_params: Vec<String>,
    output_params: Vec<String>,
    capability_calls: Vec<String>,
}

/// Helper: format a parameter type for RTFS :parameters map
fn _format_param_type(param_type: &ParamTypeInfo) -> String {
    match param_type {
        ParamTypeInfo::String => "\"string\"".to_string(),
        ParamTypeInfo::Enum { values } => {
            format!(
                "[:enum {}]",
                values
                    .iter()
                    .map(|v| format!("\"{}\"", v))
                    .collect::<Vec<_>>()
                    .join(" ")
            )
        }
    }
}

// ---- RTFS serializer helpers (small and pragmatic) ----
fn rtfs_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

fn rtfs_str(s: &str) -> String {
    format!("\"{}\"", rtfs_escape(s))
}

fn sanitize_literal_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}
