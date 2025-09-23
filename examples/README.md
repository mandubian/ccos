# Examples

This folder contains small RTFS snippets you can compile and run with the CCOS runtime.

## mcp_and_fs_plan.rtfs

Shows a typed plan that:
- Calls an MCP tool to fetch data (Host boundary 1)
- Writes to a local file via filesystem capability (Host boundary 2)

Entrypoint: `examples.mcp-and-fs/run`

Inputs (map):
- `:city` (string): which city to query
- `:outfile` (string): target file path to write

Output (map):
- `:status` (string)
- `:path` (string)

Notes:
- Each `(call …)` is the explicit Host boundary. The runtime yields `RequiresHost`; the Host executes the capability and resumes execution.
- MCP capability id used: `"mcp.default_mcp_server.get-weather"`. Adjust to match your discovered MCP tools. The ID pattern is `mcp.{server}.{tool}`.
- Filesystem capability used: `:fs.write`. If your host uses a different name (e.g., `:ccos.file.write`), update the example accordingly.

Running:
- Ensure your Host registers the MCP tool and filesystem write capabilities.
- Provide an input map like `{ :city "Paris" :outfile "/tmp/weather.txt" }` when invoking the entrypoint.

Troubleshooting:
- If you hit an error like "unknown capability", check your Host capability marketplace registration.
- Verify permissions/policies allow `:fs.write`.