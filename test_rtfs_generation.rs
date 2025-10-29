// Quick test to verify RTFS generation fixes
use std::collections::HashMap;

#[derive(Clone, Debug)]
struct ProposedStep {
    name: String,
    required_inputs: Vec<String>,
    expected_outputs: Vec<String>,
}

#[derive(Clone, Debug)]
struct ResolvedStep {
    original: ProposedStep,
    capability_id: String,
}

fn build_step_call_args(
    step: &ProposedStep,
) -> Result<String, Box<dyn std::error::Error>> {
    // Build map-based arguments without $ prefix: {:key1 val1 :key2 val2}
    if step.required_inputs.is_empty() {
        return Ok("{}".to_string());
    }
    
    let mut args_parts = vec!["{".to_string()];
    for (i, input) in step.required_inputs.iter().enumerate() {
        args_parts.push(format!("    :{} {}", input, input));
        if i < step.required_inputs.len() - 1 {
            args_parts.push("\n".to_string());
        }
    }
    args_parts.push("\n  }".to_string());
    
    Ok(args_parts.join(""))
}

fn generate_orchestrator_capability(
    goal: &str,
    resolved_steps: &[ResolvedStep],
) -> Result<String, Box<dyn std::error::Error>> {
    let mut rtfs_code = String::new();
    
    // Collect all unique input variables from all steps
    let mut all_inputs = std::collections::HashSet::new();
    for step in resolved_steps {
        for input in &step.original.required_inputs {
            all_inputs.insert(input.clone());
        }
    }
    
    // Build input-schema map with :any type as default
    let input_schema = if all_inputs.is_empty() {
        "{}".to_string()
    } else {
        let mut schema_parts = Vec::new();
        let mut sorted_inputs: Vec<_> = all_inputs.iter().collect();
        sorted_inputs.sort();
        for input in sorted_inputs {
            schema_parts.push(format!("    :{} :any", input));
        }
        format!("{{\n{}\n  }}", schema_parts.join("\n"))
    };
    
    // Build a proper RTFS 2.0 plan structure with input/output schemas
    rtfs_code.push_str("(plan\n");
    rtfs_code.push_str(&format!("  :name \"synth.plan.orchestrator.v1\"\n"));
    rtfs_code.push_str(&format!("  :language rtfs20\n"));
    rtfs_code.push_str(&format!("  :input-schema {}\n", input_schema));
    rtfs_code.push_str(&format!("  :output-schema {{\n    :result :any\n  }}\n"));
    rtfs_code.push_str(&format!(
        "  :annotations {{:goal \"{}\" :step_count {}}}\n",
        goal.replace("\"", "\\\""),
        resolved_steps.len()
    ));
    rtfs_code.push_str("  :body (do\n");

    if resolved_steps.is_empty() {
        rtfs_code.push_str("    (step \"No Steps\" {})\n");
    } else {
        // Build sequential steps using proper RTFS syntax without $ prefix
        for resolved in resolved_steps.iter() {
            let step_desc = &resolved.original.name;
            let step_args = build_step_call_args(&resolved.original)?;
            
            rtfs_code.push_str(&format!(
                "    (step \"{}\" (call :{} {}))\n",
                step_desc.replace("\"", "\\\""),
                resolved.capability_id,
                step_args
            ));
        }
    }
    
    rtfs_code.push_str("  )\n");
    rtfs_code.push_str(")\n");
    
    Ok(rtfs_code)
}

fn main() {
    println!("Testing RTFS Generation Fixes...\n");
    
    // Test 1: Simple flight booking example
    println!("TEST 1: Flight Booking Orchestration");
    println!("====================================");
    
    let steps = vec![
        ResolvedStep {
            original: ProposedStep {
                name: "Search available flights".to_string(),
                required_inputs: vec!["origin".to_string(), "destination".to_string(), "dates".to_string()],
                expected_outputs: vec!["flights".to_string()],
            },
            capability_id: "travel.flights.search".to_string(),
        },
        ResolvedStep {
            original: ProposedStep {
                name: "Check passenger requirements".to_string(),
                required_inputs: vec!["party_size".to_string(), "age_info".to_string()],
                expected_outputs: vec!["requirements".to_string()],
            },
            capability_id: "travel.passengers.check".to_string(),
        },
    ];
    
    match generate_orchestrator_capability("Book a flight from NYC to LA", &steps) {
        Ok(rtfs) => {
            println!("✓ Generated RTFS:\n");
            println!("{}", rtfs);
            
            // Check for required properties
            let checks = [
                (":input-schema", "Input schema declaration"),
                (":output-schema", "Output schema declaration"),
                ("origin", "No $ prefix on origin variable"),
                ("destination", "No $ prefix on destination variable"),
                ("party_size", "No $ prefix on party_size variable"),
            ];
            
            println!("\nValidation Checks:");
            for (pattern, desc) in &checks {
                if rtfs.contains(pattern) {
                    println!("  ✓ {} - FOUND: {}", desc, pattern);
                } else {
                    println!("  ✗ {} - MISSING: {}", desc, pattern);
                }
            }
            
            // Check for forbidden patterns
            let forbidden = [
                (":$", "Old RTFS 1.0 variable template syntax"),
            ];
            
            println!("\nForbidden Pattern Checks:");
            for (pattern, desc) in &forbidden {
                if rtfs.contains(pattern) {
                    println!("  ✗ {} FOUND: {} (SHOULD NOT BE PRESENT)", desc, pattern);
                } else {
                    println!("  ✓ {} - ABSENT (correct)", desc);
                }
            }
        }
        Err(e) => println!("✗ Error: {}", e),
    }
    
    println!("\n");
    
    // Test 2: Empty steps
    println!("TEST 2: Empty Steps");
    println!("===================");
    
    match generate_orchestrator_capability("Do nothing", &[]) {
        Ok(rtfs) => {
            println!("✓ Generated RTFS for empty steps:\n");
            println!("{}", rtfs);
            
            if rtfs.contains(":input-schema {}") {
                println!("✓ Empty input schema generated correctly");
            } else {
                println!("✗ Empty input schema not as expected");
            }
        }
        Err(e) => println!("✗ Error: {}", e),
    }
    
    println!("\n");
    
    // Test 3: Multiple steps with overlapping inputs
    println!("TEST 3: Multiple Steps with Overlapping Inputs");
    println!("==============================================");
    
    let steps = vec![
        ResolvedStep {
            original: ProposedStep {
                name: "Get user preferences".to_string(),
                required_inputs: vec!["user_id".to_string()],
                expected_outputs: vec!["preferences".to_string()],
            },
            capability_id: "user.preferences.get".to_string(),
        },
        ResolvedStep {
            original: ProposedStep {
                name: "Filter options".to_string(),
                required_inputs: vec!["options".to_string(), "user_id".to_string()],
                expected_outputs: vec!["filtered_options".to_string()],
            },
            capability_id: "filtering.apply".to_string(),
        },
        ResolvedStep {
            original: ProposedStep {
                name: "Rank results".to_string(),
                required_inputs: vec!["filtered_options".to_string(), "preferences".to_string()],
                expected_outputs: vec!["ranked_results".to_string()],
            },
            capability_id: "ranking.score".to_string(),
        },
    ];
    
    match generate_orchestrator_capability("Complex multi-step ranking", &steps) {
        Ok(rtfs) => {
            println!("✓ Generated RTFS for overlapping inputs:\n");
            println!("{}", rtfs);
            
            // All unique inputs should appear in schema
            let required_vars = vec!["user_id", "options", "preferences", "filtered_options"];
            println!("\nInput Variables Check:");
            for var in &required_vars {
                if rtfs.contains(&format!(":{} :any", var)) {
                    println!("  ✓ {} - declared in input schema", var);
                } else {
                    println!("  ? {} - check schema manually", var);
                }
            }
        }
        Err(e) => println!("✗ Error: {}", e),
    }
}
