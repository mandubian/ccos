// This test file intentionally disabled. The previous HTTP endpoint test
// required Axum Router state to be Send + Sync, which the current CCOS
// architecture does not satisfy due to non-Send components (IntentGraph,
// runtime, dynamic trait objects). The real HTTP integration test was
// replaced by a simulation test 'architecture_endpoint_sim.rs' that invokes
// the snapshot builder directly without spinning up a server.
//
// Keeping this placeholder avoids accidental re-introduction of the old code.
// If in future CCOS internals become Send + Sync, an actual HTTP-level test
// can be reinstated here.

#[test]
#[ignore]
fn http_endpoint_architecture_placeholder() {
    // Intentionally empty.
}
