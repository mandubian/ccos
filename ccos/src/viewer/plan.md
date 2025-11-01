# CCOS Viewer Implementation Plan

This document outlines the steps to create a web-based, real-time visualization dashboard for the CCOS execution flow.

## 1. Backend Setup (Rust)

-   [ ] **Integrate Web Server & WebSockets**:
    -   Add `axum`, `tokio`, and `tokio-tungstenite` to the `rtfs_compiler/Cargo.toml` dependencies.
    -   Create a new binary or example that launches a web server to serve the frontend assets and handle WebSocket connections.
    -   The server will have two main routes:
        -   `/`: Serves the main `index.html` and other static assets (CSS, JS).
        -   `/ws`: The WebSocket endpoint for real-time communication.

-   [ ] **Instrument CCOS Core for Event Emission**:
    -   Define a set of structured `Event` enums in Rust (e.g., `IntentCreated`, `PlanGenerated`, `ActionStatusChanged`, `CapabilityCalled`). These events will be serialized to JSON.
    -   Modify the `Orchestrator`, `CausalChain`, and other relevant CCOS components to broadcast these events through a shared channel (e.g., `tokio::sync::broadcast`).
    -   The WebSocket handler will subscribe to this broadcast channel and forward the JSON-serialized events to all connected web clients.

## 2. Frontend Development (HTML/CSS/JavaScript)

-   [ ] **Create the Main HTML Structure (`index.html`)**:
    -   Create a basic HTML file with three main sections:
        1.  A `div` for the interactive graph (`#graph-container`).
        2.  A `pre` and `code` block for displaying the RTFS plan (`#rtfs-code`).
        3.  A `div` for the real-time event log (`#log-container`).
    -   Include the necessary JavaScript libraries (D3.js/vis.js, Prism.js) from a CDN.

-   [ ] **Implement the Interactive Graph**:
    -   Write JavaScript to connect to the `/ws` WebSocket endpoint.
    -   On receiving events, parse the JSON data.
    -   Use D3.js or vis.js to:
        -   Add/update nodes and edges in the intent graph.
        -   Change node colors based on status (`pending`, `in-progress`, `success`, `failure`).
        -   Highlight the currently executing node.
        -   Add icons or special styling for capability calls and delegations.

-   [ ] **Implement the RTFS Code Viewer**:
    -   Write JavaScript to populate the `#rtfs-code` container with the generated plan.
    -   Integrate Prism.js with a custom grammar for RTFS (if needed) to apply syntax highlighting.
    -   Implement a feature where clicking a node in the graph highlights the corresponding code block.

-   [ ] **Implement the Event Log**:
    -   Write JavaScript to append formatted event messages to the `#log-container` as they are received from the WebSocket.

## 3. Integration and Refinement

-   [ ] **Create a New Demo Binary**:
    -   Create a new file `rtfs_compiler/src/viewer/main.rs` that ties everything together.
    -   This binary will initialize the CCOS components, start the web server, and run the demo logic.
-   [ ] **Styling**:
    -   Add a simple CSS file to style the dashboard for a clean and modern look.
-   [ ] **Testing and Refinement**:
    -   Run the new demo and test the full end-to-end functionality.
    -   Refine the visualizations and user interactions based on the results.
