#!/usr/bin/env python3
"""
Migrate llm_provider.rs to use PromptManager for prompts.
This script handles the complex multi-line string replacements safely.
"""

import re

# Read the file
with open('rtfs_compiler/src/ccos/arbiter/llm_provider.rs', 'r') as f:
    content = f.read()

# Step 1: Add imports
old_imports = """use crate::ccos::types::{
    GenerationContext, IntentStatus, Plan, PlanBody, PlanLanguage, StorableIntent, TriggerSource,
};"""

new_imports = """use crate::ccos::arbiter::prompt::{FilePromptStore, PromptManager};
use crate::ccos::types::{
    GenerationContext, IntentStatus, Plan, PlanBody, PlanLanguage, StorableIntent, TriggerSource,
};"""

content = content.replace(old_imports, new_imports)

# Step 2: Add prompt_manager field to struct
old_struct = """pub struct OpenAILlmProvider {
    config: LlmProviderConfig,
    client: reqwest::Client,
    metrics: RetryMetrics,
}"""

new_struct = """pub struct OpenAILlmProvider {
    config: LlmProviderConfig,
    client: reqwest::Client,
    metrics: RetryMetrics,
    prompt_manager: PromptManager<FilePromptStore>,
}"""

content = content.replace(old_struct, new_struct)

# Step 3: Initialize PromptManager in constructor
old_constructor = """        Ok(Self { 
            config, 
            client,
            metrics: RetryMetrics::new(),
        })
    }

    /// Get current retry metrics summary"""

new_constructor = """        let prompt_store = FilePromptStore::new("assets/prompts/arbiter");
        let prompt_manager = PromptManager::new(prompt_store);

        Ok(Self { 
            config, 
            client,
            metrics: RetryMetrics::new(),
            prompt_manager,
        })
    }

    /// Get current retry metrics summary"""

content = content.replace(old_constructor, new_constructor)

# Step 4: Migrate generate_intent method
# Find the start of generate_intent
intent_start = content.find("    async fn generate_intent(")
if intent_start == -1:
    print("ERROR: Could not find generate_intent method")
    exit(1)

# Find the system_message assignment
system_msg_start = content.find('        let system_message = r#"You are an AI assistant', intent_start)
system_msg_end = content.find('Only respond with valid JSON."#;', system_msg_start) + len('Only respond with valid JSON."#;')

old_intent_prompt = content[system_msg_start:system_msg_end]

new_intent_prompt = """        // Load prompt from assets with fallback
        let vars = HashMap::from([
            ("user_request", prompt.to_string()),
        ]);
        
        let system_message = self.prompt_manager
            .render("intent_generation", "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load intent_generation prompt from assets: {}. Using fallback.", e);
                r#"You are an AI assistant that converts natural language requests into structured intents for a cognitive computing system.

Generate a JSON response with the following structure:
{
  "name": "descriptive_name_for_intent",
  "goal": "clear_description_of_what_should_be_achieved",
  "constraints": {
    "constraint_name": "constraint_value_as_string"
  },
  "preferences": {
    "preference_name": "preference_value_as_string"
  },
  "success_criteria": "how_to_determine_if_intent_was_successful"
}

IMPORTANT: All values in constraints and preferences must be strings, not numbers or arrays.
Examples:
- "max_cost": "100" (not 100)
- "priority": "high" (not ["high"])
- "timeout": "30_seconds" (not 30)

Only respond with valid JSON."#.to_string()
            });"""

content = content.replace(old_intent_prompt, new_intent_prompt)

# Step 5: Migrate generate_plan method - this is the complex one
# Find where both prompts are defined
plan_start = content.find("    async fn generate_plan(", intent_start + 100)
reduced_start = content.find('        // Reduced RTFS grammar prompt', plan_start)
full_start = content.find('        // Full plan prompt', reduced_start)
user_msg_start = content.find('        let user_message = if full_plan_mode {', full_start)

# Extract the section from reduced_start to just before user_message
old_plan_prompts = content[reduced_start:user_msg_start]

new_plan_prompts = """        // Prepare variables for prompt rendering
        let vars = HashMap::from([
            ("goal", intent.goal.clone()),
            ("constraints", format!("{:?}", intent.constraints)),
            ("preferences", format!("{:?}", intent.preferences)),
        ]);

        // Load appropriate prompt based on mode
        let prompt_id = if full_plan_mode {
            "plan_generation_full"
        } else {
            "plan_generation_reduced"
        };

        let system_message = self.prompt_manager
            .render(prompt_id, "v1", &vars)
            .unwrap_or_else(|e| {
                eprintln!("Warning: Failed to load {} prompt from assets: {}. Using fallback.", prompt_id, e);
                // Fallback to original hard-coded prompts based on mode
                if full_plan_mode {
                    r#"You translate an RTFS intent into a concrete RTFS plan using a constrained schema.
Output format: ONLY a single well-formed RTFS s-expression starting with (plan ...). No prose, no JSON, no fences."#.to_string()
                } else {
                    r#"You translate an RTFS intent into a concrete RTFS execution body using a reduced grammar.
Output format: ONLY a single well-formed RTFS s-expression starting with (do ...). No prose, no JSON, no fences."#.to_string()
                }
            });

        """

content = content.replace(old_plan_prompts, new_plan_prompts)

# Step 6: Update the messages vec construction
old_messages = """        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: if full_plan_mode {
                    full_plan_system_message.to_string()
                } else {
                    reduced_system_message.to_string()
                },
            },"""

new_messages = """        let messages = vec![
            OpenAIMessage {
                role: "system".to_string(),
                content: system_message,
            },"""

content = content.replace(old_messages, new_messages)

# Step 7: Update show_prompts section
old_show = """        if show_prompts {
            let system_msg = if full_plan_mode {
                full_plan_system_message
            } else {
                reduced_system_message
            };
            println!(
                "\\n=== LLM Plan Generation Prompt ===\\n[system]\\n{}\\n\\n[user]\\n{}\\n=== END PROMPT ===\\n",
                system_msg,
                user_message
            );
        }"""

new_show = """        if show_prompts {
            println!(
                "\\n=== LLM Plan Generation Prompt ===\\n[system]\\n{}\\n\\n[user]\\n{}\\n=== END PROMPT ===\\n",
                system_message,
                user_message
            );
        }"""

content = content.replace(old_show, new_show)

# Write the result
with open('rtfs_compiler/src/ccos/arbiter/llm_provider.rs', 'w') as f:
    f.write(content)

print("✓ Migration complete!")
print("  - Added PromptManager imports")
print("  - Added prompt_manager field to OpenAILlmProvider")
print("  - Initialized PromptManager in constructor")
print("  - Migrated generate_intent to use prompt assets")
print("  - Migrated generate_plan to use prompt assets (both modes)")
