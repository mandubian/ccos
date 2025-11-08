//! Demo: Execute a generated capability from RTFS file
//!
//! This demo demonstrates:
//! 1. Loading a capability from an RTFS file
//! 2. Registering it in the marketplace
//! 3. Executing it with dummy data
//! 4. Creating and executing a plan that calls the capability

use ccos::environment::CCOSBuilder;
use rtfs::ast::MapKey;
use rtfs::runtime::values::Value;
use std::collections::HashMap;
use std::error::Error;
use std::path::Path;

fn main() -> Result<(), Box<dyn Error>> {
    println!("🚀 CCOS Capability Execution Demo\n");
    println!("{}", "=".repeat(80));

    // Enable fallback execution context for direct capability calls
    // This avoids "Host method called without a valid execution context" for direct executions
    std::env::set_var("CCOS_TEST_FALLBACK_CONTEXT", "1");

    // Build CCOS environment
    println!("\n📦 Initializing CCOS environment...");
    let env = CCOSBuilder::new()
        .verbose(true)
        .build()
        .map_err(|e| format!("Failed to init CCOS environment: {}", e))?;
    println!("✅ CCOS environment initialized\n");

    // Load the capability from RTFS file
    let capability_path = "capabilities/generated/text.filter.by-topic/capability.rtfs";
    println!("📖 Loading capability from: {}", capability_path);

    if !Path::new(capability_path).exists() {
        return Err(format!("Capability file not found: {}", capability_path).into());
    }

    // First register the capability using execute_file (required for it to work)
    env.execute_file(capability_path)
        .map_err(|e| format!("Failed to load capability file: {}", e))?;
    println!("✅ Capability registered via execute_file\n");

    // Now test load-capability prelude function (returns ID, doesn't register)
    println!("🔧 Testing load-capability prelude function...");
    let load_capability_code = format!(r#"(load-capability "{}")"#, capability_path);
    let load_result = env
        .execute_code(&load_capability_code)
        .map_err(|e| format!("load-capability failed: {}", e))?;

    let capability_id = match load_result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(Value::String(id)) => id,
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(v) => {
            println!("📊 load-capability returned: {:?}", v);
            return Err(format!("Expected capability ID string, got: {:?}", v).into());
        }
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(_) => {
            return Err("load-capability requires host interaction".into());
        }
    };
    println!(
        "✅ load-capability returned capability ID: {}\n",
        capability_id
    );

    // Create dummy data for testing
    println!("📝 Creating dummy test data...");
    let dummy_issues = vec![
        create_issue(
            "Fix bug in authentication",
            "The login system has a security issue",
            "security",
        ),
        create_issue(
            "Add user profile page",
            "Users need to see their profile information",
            "feature",
        ),
        create_issue(
            "Improve performance",
            "The database queries are too slow",
            "performance",
        ),
        create_issue(
            "Security audit required",
            "We need to review all authentication endpoints",
            "security",
        ),
    ];

    let test_input = create_test_input_map(&dummy_issues, "security");
    println!("✅ Test data created: {} issues\n", dummy_issues.len());

    // Execute the capability directly using RTFS code
    // This ensures execution context is set properly
    println!("🔧 Executing capability directly...");
    let capability_call_code = format!(
        r#"(call "text.filter.by-topic" {})"#,
        format_test_input_rtfs(&test_input)
    );

    let execution_result = env
        .execute_code(&capability_call_code)
        .map_err(|e| format!("Capability execution failed: {}", e))?;

    let result = match execution_result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(v) => v,
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(hc) => {
            return Err(format!("Capability requires host interaction: {:?}", hc).into());
        }
    };

    println!("✅ Capability executed successfully!");
    println!("📊 Result:");
    print_value_pretty(&result, 0);
    println!();

    // Now create a plan that loads the capability and then calls it
    println!("📋 Creating a plan that loads and calls the capability...");
    let plan_rtfs = format!(
        r#"
(plan "filter-issues-plan"
  :body (do
    (println "Starting plan execution...")
    
    ;; Load the capability using load-capability
    (let [cap-id (load-capability "{}")]
      (do
        (println "Loaded capability:" cap-id)
        
        ;; Create test data
        (let [test-issues [
          {{:title "Fix bug in authentication" :body "The login system has a security issue" :topic "security"}}
          {{:title "Add user profile page" :body "Users need to see their profile information" :topic "feature"}}
          {{:title "Improve performance" :body "The database queries are too slow" :topic "performance"}}
          {{:title "Security audit required" :body "We need to review all authentication endpoints" :topic "security"}}
        ]]
          
          (let [input {{
            :raw-issues-list test-issues
            :filter_topic "security"
            :filter-type "contains"
          }}]
            
            (let [result (call "text.filter.by-topic" input)]
              (do
                (println "Plan execution completed!")
                (println "Filtered issues count:")
                (println (count (get result :filtered-issues)))
                result
              )
            )
          )
        )
      )
    )
  )
)
"#,
        capability_path
    );
    println!("📝 Plan RTFS:");
    println!("{}", plan_rtfs);
    println!();

    println!("🚀 Executing plan...");
    let plan_result = env
        .execute_code(&plan_rtfs)
        .map_err(|e| format!("Plan execution failed: {}", e))?;

    println!("✅ Plan executed successfully!");
    println!("📊 Plan result:");
    match plan_result {
        rtfs::runtime::execution_outcome::ExecutionOutcome::Complete(v) => {
            print_value_pretty(&v, 0);
        }
        rtfs::runtime::execution_outcome::ExecutionOutcome::RequiresHost(hc) => {
            println!("⚠️  Plan requires host interaction: {:?}", hc);
        }
    }
    println!();

    println!("{}", "=".repeat(80));
    println!("✅ Demo completed successfully!");

    Ok(())
}

fn create_issue(title: &str, body: &str, topic: &str) -> Value {
    let mut issue = HashMap::new();
    issue.insert(
        MapKey::String("title".to_string()),
        Value::String(title.to_string()),
    );
    issue.insert(
        MapKey::String("body".to_string()),
        Value::String(body.to_string()),
    );
    issue.insert(
        MapKey::String("topic".to_string()),
        Value::String(topic.to_string()),
    );
    Value::Map(issue)
}

fn create_test_input_map(issues: &[Value], filter_topic: &str) -> HashMap<MapKey, Value> {
    let mut input = HashMap::new();
    input.insert(
        MapKey::Keyword(rtfs::ast::Keyword("raw-issues-list".to_string())),
        Value::Vector(issues.to_vec()),
    );
    input.insert(
        MapKey::Keyword(rtfs::ast::Keyword("filter_topic".to_string())),
        Value::String(filter_topic.to_string()),
    );
    input.insert(
        MapKey::Keyword(rtfs::ast::Keyword("filter-type".to_string())),
        Value::String("contains".to_string()),
    );
    input
}

fn format_test_input_rtfs(input: &HashMap<MapKey, Value>) -> String {
    let mut parts = Vec::new();
    for (k, v) in input {
        let key_str = match k {
            MapKey::Keyword(kw) => format!(":{}", kw.0),
            MapKey::String(s) => format!("\"{}\"", s),
            MapKey::Integer(i) => format!("{}", i),
        };
        let val_str = format_value_rtfs(v);
        parts.push(format!("{} {}", key_str, val_str));
    }
    format!("{{{}}}", parts.join(" "))
}

fn format_value_rtfs(v: &Value) -> String {
    match v {
        Value::Nil => "nil".to_string(),
        Value::Boolean(b) => format!("{}", b),
        Value::Integer(i) => format!("{}", i),
        Value::Float(f) => format!("{}", f),
        Value::String(s) => format!("\"{}\"", s.replace("\"", "\\\"")),
        Value::Vector(vec) => {
            let items: Vec<String> = vec.iter().map(format_value_rtfs).collect();
            format!("[{}]", items.join(" "))
        }
        Value::Map(m) => {
            let mut parts = Vec::new();
            for (k, v) in m {
                let key_str = match k {
                    MapKey::Keyword(kw) => format!(":{}", kw.0),
                    MapKey::String(s) => format!("\"{}\"", s),
                    MapKey::Integer(i) => format!("{}", i),
                };
                parts.push(format!("{} {}", key_str, format_value_rtfs(v)));
            }
            format!("{{{}}}", parts.join(" "))
        }
        _ => format!("{:?}", v),
    }
}

fn create_plan_rtfs() -> String {
    r#"
;; Plan that calls the text.filter.by-topic capability
;; This demonstrates loading a capability and using it in a plan
(plan
  :name "Filter Issues Plan"
  :description "Demonstrates calling a loaded capability from a plan"
  :body
    (do
      (println "Starting plan execution...")
      
      ;; Note: The capability should already be loaded via execute_file
      ;; If we wanted to load it in the plan, we could use: (load-capability "capabilities/generated/text.filter.by-topic/capability.rtfs")
      
      ;; Create test data
      (let [test-issues [
        {:title "Fix bug in authentication" :body "The login system has a security issue" :topic "security"}
        {:title "Add user profile page" :body "Users need to see their profile information" :topic "feature"}
        {:title "Improve performance" :body "The database queries are too slow" :topic "performance"}
        {:title "Security audit required" :body "We need to review all authentication endpoints" :topic "security"}
      ]]
        
        (let [input {
          :raw-issues-list test-issues
          :filter_topic "security"
          :filter-type "contains"
        }]
          
          (let [result (call "text.filter.by-topic" input)]
            (do
              (println "Plan execution completed!")
              (println "Filtered issues count:")
              (println (count (get result :filtered-issues)))
              result
            )
          )
        )
      )
    )
)
"#.to_string()
}

fn print_value_pretty(v: &Value, indent: usize) {
    let indent_str = "  ".repeat(indent);
    match v {
        Value::Nil => println!("{}nil", indent_str),
        Value::Boolean(b) => println!("{}{}", indent_str, b),
        Value::Integer(i) => println!("{}{}", indent_str, i),
        Value::Float(f) => println!("{}{}", indent_str, f),
        Value::String(s) => println!("{}\"{}\"", indent_str, s),
        Value::Vector(vec) => {
            println!("{}[", indent_str);
            for item in vec {
                print_value_pretty(item, indent + 1);
            }
            println!("{}]", indent_str);
        }
        Value::Map(m) => {
            println!("{}{{", indent_str);
            for (k, v) in m {
                let key_str = match k {
                    MapKey::String(s) => format!("\"{}\"", s),
                    MapKey::Keyword(kw) => format!(":{}", kw.0),
                    MapKey::Integer(i) => format!("{}", i),
                };
                print!("{}{}: ", indent_str, key_str);
                print_value_pretty(v, indent + 1);
            }
            println!("{}}}", indent_str);
        }
        _ => println!("{}{:?}", indent_str, v),
    }
}
