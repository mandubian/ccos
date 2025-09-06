# CCOS Viewer Server

Single lightweight Axum-based server that:

- Initializes CCOS runtime and starts the runtime service
- Serves the demo frontend assets directly from `../rtfs_compiler/src/viewer/web`
- Exposes a WebSocket endpoint at `/ws` streaming `ViewerEvent` JSON messages translated from `RuntimeEvent`

## Run

From this directory:

```
cargo run
```

It will bind on `127.0.0.1:3001` (or next free port up to +9). Console will show something like:

```
viewer_server listening on http://127.0.0.1:3001
```

## Use

Open the reported URL in a browser (e.g. http://127.0.0.1:3001 ). The page loads `index.html`, `app.js`, and `style.css` from the canonical location inside `rtfs_compiler`.

WebSocket URL used by `app.js` should point to the same host+port you opened, e.g.

```
const socket = new WebSocket(`ws://${window.location.host}/ws`);
```

(If you hardcoded a different host earlier for the Python server, revert that in the canonical `app.js` at `rtfs_compiler/src/viewer/web/app.js`).

## Why only one server now?

We previously had:
- Python simple HTTP server on :8080 (static only)
- Rust `viewer_server` on :3001 (WebSocket + CCOS)

This created duplicate copies of the web assets and confusion. Now only the Rust server remains; the Python server is unnecessary.

## Development Notes

- Modify frontend in `rtfs_compiler/src/viewer/web/*` only.
- No sync/copy step required anymore.
- Adding assets: either add explicit routes (like existing ones) or implement a small static file service if needed later.

## Next Ideas

- Add endpoint to submit new goals via POST -> broadcast
- Add incremental graph building (edges between intents)
- Add heartbeat / status badge in UI

Enjoy the simplified stack.


