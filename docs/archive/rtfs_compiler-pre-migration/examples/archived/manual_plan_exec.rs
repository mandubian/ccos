use std::error::Error;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    println!("Manual plan execution test: start");

    // Initialize CCOS (this will register default local capabilities)
    let ccos = rtfs_compiler::ccos::CCOS::new().await.expect("init CCOS");

    // Build a tiny RTFS plan that calls the math capability and echoes the result
    let rtfs_body = "(do
  (step \"add-numbers\"
    (set! :sum (call :ccos.math.add 2 3)))
  (step \"display\"
    (call :ccos.echo (get :sum))))"
        .to_string();

    let plan =
        rtfs_compiler::ccos::types::Plan::new_rtfs(rtfs_body, vec!["intent-manual-1".to_string()]);

    // Persist a minimal StorableIntent so validate_and_execute_plan can look it up
    let mut st =
        rtfs_compiler::ccos::types::StorableIntent::new("Manual intent for test".to_string());
    // Ensure the stored intent id matches the plan's referenced id
    st.intent_id = "intent-manual-1".to_string();
    st.name = Some("manual-test".to_string());
    // Store into CCOS's IntentGraph
    if let Ok(mut graph_lock) = ccos.get_intent_graph().lock() {
        graph_lock.store_intent(st).expect("store manual intent");
    } else {
        eprintln!("Failed to lock intent graph to store manual intent");
    }

    // Use the controlled demo context which allows the offline math/echo capabilities
    let ctx = rtfs_compiler::ccos::runtime_service::default_controlled_context();

    println!("Calling validate_and_execute_plan...");
    match ccos.validate_and_execute_plan(plan, &ctx).await {
        Ok(exec) => {
            println!("Execution success: {}", exec.success);
            println!("Execution value: {:?}", exec.value);
        }
        Err(e) => {
            eprintln!("Execution error: {}", e);
        }
    }

    println!("Manual plan execution test: done");
    Ok(())
}
