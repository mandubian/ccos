# MCP Introspection → CCOS Capability Demo (Guide)

This guide explains how to run the `mcp_introspection_demo` example that:
- Introspects an MCP server via JSON‑RPC `tools/list`
- Registers each discovered tool as a CCOS capability
- Executes a selected tool via the Capability Marketplace

Source: `rtfs_compiler/examples/mcp_introspection_demo.rs`

## Prerequisites
- Rust + Cargo installed
- An MCP server exposing a JSON‑RPC HTTP endpoint

## Build
From `rtfs_compiler/`:
- `cargo build --examples`

## Run
- List tools:
  - `cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --list`
- Execute a tool (example: `echo`):
  - `cargo run --example mcp_introspection_demo -- --server-url http://localhost:3000 --tool echo --args '{"text":"hello"}'`

Notes
- Tools are registered with ids like `mcp.demo.<toolName>`.
- The demo allows only `mcp.*` capabilities through its isolation policy.

## Minimal local MCP server (Node.js)
For quick testing:

```js
// server.js
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
        if (name === 'echo') result = { text: String(args.text ?? '') };
        else if (name === 'time') result = { now: new Date().toISOString() };
        else { res.statusCode = 400; return res.end(jsonResponse(id || 'call', null, { code: -32601, message: 'Unknown tool' })); }
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
- HTTP error from `tools/list`: verify URL, method=POST, and JSON‑RPC payload.
- “key must be a string”: ensure `--args` is valid JSON with string keys (the runtime now converts map keys correctly).
- No tools discovered: the server must return `{ result: { tools: [...] } }`.

## Next steps
- Point to a real MCP instance to exercise richer tools.
- Integrate discovery and adjust isolation policies in your apps.
