# MCP Introspection → CCOS Capability Demo

This guide explains how to run the `mcp_introspection_demo` example that:
- Introspects an MCP server via JSON‑RPC `tools/list`
- Registers each discovered tool as a CCOS capability
- Executes a selected tool through the Capability Marketplace

The demo lives in `rtfs_compiler/examples/mcp_introspection_demo.rs`.

## Prerequisites
- Rust + Cargo installed
- A running MCP server exposing a JSON‑RPC HTTP endpoint
  - For local testing, you can use the minimal Node.js server below

## Build
- Build examples: `cargo build --examples` (from `rtfs_compiler/`)

## Run
- List available tools from a server:
  - `cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --list`
- Execute a tool (here: `echo`) with JSON args:
  - `cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --tool echo --args '{"text":"hello"}'`

Notes
- Discovered tools are registered as capabilities with ids like `mcp.demo.<toolName>`.
- The demo sets an isolation policy allowing only `mcp.*` capabilities.

## Minimal local MCP server (Node.js)
A tiny JSON‑RPC server that supports `tools/list` and `tools/call` for testing.

```js
// save as server.js and run: node server.js
const http = require('http');

const tools = [
  { name: 'echo', description: 'Echo back the input text' },
  { name: 'time', description: 'Return the current server time' },
];

function jsonResponse(id, result, error) {
  return JSON.stringify({ jsonrpc: '2.0', id, ...(error ? { error } : { result }) });
}

const server = http.createServer((req, res) => {
  if (req.method !== 'POST') { res.statusCode = 405; return res.end('Method Not Allowed'); }
  let body = '';
  req.on('data', chunk => body += chunk);
  req.on('end', () => {
    try {
      const { id, method, params } = JSON.parse(body || '{}');
      if (method === 'tools/list') {
        res.setHeader('Content-Type', 'application/json');
        return res.end(jsonResponse(id || 'tools_list', { tools }));
      }
      if (method === 'tools/call') {
        const name = params?.name;
        const args = params?.arguments || {};
        let result;
        if (name === 'echo') {
          result = { text: String(args.text ?? '') };
        } else if (name === 'time') {
          result = { now: new Date().toISOString() };
        } else {
          res.statusCode = 400;
          return res.end(jsonResponse(id || 'call', null, { code: -32601, message: 'Unknown tool' }));
        }
        res.setHeader('Content-Type', 'application/json');
        return res.end(jsonResponse(id || 'call', result));
      }
      res.statusCode = 400;
      res.end(jsonResponse(id || 'unknown', null, { code: -32601, message: 'Unknown method' }));
    } catch (e) {
      res.statusCode = 400;
      res.end(jsonResponse('parse_error', null, { code: -32700, message: 'Parse error', data: String(e) }));
    }
  });
});

server.listen(3000, () => console.log('MCP test server on http://localhost:3000'));
```

## Troubleshooting
- HTTP error from `tools/list`:
  - Verify the server URL and that it accepts POST `application/json` with JSON‑RPC payloads.
- “key must be a string” in inputs:
  - The demo uses a Value→JSON converter that ensures proper map key handling. Ensure your `--args` JSON object uses string keys (standard JSON requirement).
- No tools discovered:
  - Confirm your server returns `{ result: { tools: [...] } }` for `tools/list`.

## Next steps
- Point the demo at a real MCP server (with more tools) to exercise richer capabilities.
- Integrate discovery into your application and adjust isolation policies as needed.
