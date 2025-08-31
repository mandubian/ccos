# Runtime Service (Embedding CCOS)

Purpose: keep UI/process responsive while CCOS orchestrates plans. Provides a channel-based command/event contract and an intent-event bridge to CausalChain.

Key pieces:
- start_service(ccos: Arc<CCOS>) -> RuntimeHandle
- RuntimeCommand: Start { goal, context }, Cancel { intent_id }, Shutdown
- RuntimeEvent: Started, Status, Step, Result, Error, Heartbeat, Stopped
- Default: current-thread Tokio runtime + LocalSet; `spawn_local` avoids Send bounds

Usage sketch (console):
- Build CCOS with `CCOS::new()`.
- Call `start_service(ccos)`; keep the returned handle.
- Subscribe to events: `let mut rx = handle.subscribe()`.
- Send commands: `handle.commands().send(RuntimeCommand::Start{..}).await`.
- Drain events in your loop without blocking your UI thread.

Notes:
- A timeout wraps `process_request` (25s) to avoid indefinite “Running…”.
- Default allowed capabilities in demos are offline-only (ccos.echo, ccos.math.add).
- Cancel is implemented as best-effort via aborting the in-flight task. It cancels the current run and emits an Error event with a short message.
