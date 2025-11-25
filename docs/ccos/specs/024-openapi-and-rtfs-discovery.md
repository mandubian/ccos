# OpenAPI & Generic RTFS Discovery Guide

## 1. Overview

While **MCP** is the preferred protocol for dynamic tool discovery in CCOS, the system also supports two other critical discovery methods:
1.  **OpenAPI Import**: converting static REST API specifications (Swagger/OpenAPI 3.0) into native RTFS capabilities.
2.  **Local RTFS Discovery**: loading and validating manually written or synthesized `.rtfs` capability files from the local filesystem.

All discovery methods feed into the same **Capability Marketplace**, ensuring a unified interface for the CCOS runtime.

---

## 2. OpenAPI Discovery (`OpenAPIImporter`)

The `OpenAPIImporter` module bridges traditional REST APIs to the CCOS/RTFS world. It reads a standard OpenAPI JSON/YAML file and compiles it into an executable RTFS module.

### 2.1. Architecture

1.  **Spec Loading**: Fetches the OpenAPI spec from a URL or local path.
2.  **Operation Extraction**: Parses `paths` and `methods` (GET, POST, etc.).
3.  **Type Mapping**: Converts JSON Schema types to RTFS `TypeExpr` (e.g., `integer` → `:int`).
4.  **Auth Injection**: Detects security schemes (`ApiKey`, `Bearer`) and injects `:auth` calls into the generated code.
5.  **Code Generation**: Synthesizes an `.rtfs` file where each operation is a function wrapping an `:http.request`.

### 2.2. Generated Code Structure

A discovered OpenAPI capability looks like this in RTFS:

```clojure
(capability "openapi.github.get_repo"
  :provider :http
  :metadata { :openapi_source "https://api.github.com/openapi.json" }
  :implementation
    (fn [owner repo]
      "Get repository details"
      (call :http.request
        :method "GET"
        :url (str "https://api.github.com/repos/" owner "/" repo)
        :headers {"Accept" "application/vnd.github.v3+json"}
        :auth (call :ccos.auth.inject :service "github"))))
```

### 2.3. Usage (CLI)

You can manually trigger OpenAPI discovery using the `resolve-deps` tool (or similar entry points):

```bash
# Example (conceptual CLI usage)
cargo run --bin resolve-deps -- import-openapi \
  --url https://raw.githubusercontent.com/github/rest-api-description/main/descriptions/api.github.com/api.github.com.json \
  --namespace github
```

### 2.4. Configuration

*   **`CCOS_IMPORTERS_ENABLED`**: Must be set to `true` to enable OpenAPI discovery at runtime.
*   **`CCOS_HTTP_WRAPPER_ENABLED`**: Required to allow the generated code to execute generic HTTP requests.

---

## 3. Local RTFS Discovery

Local discovery is the foundational mechanism for loading capabilities that are already present on the filesystem.

### 3.1. The Process

1.  **Scanning**: The system recursively scans the `capabilities/` directory.
2.  **Parsing**: It parses every `.rtfs` file to extract the `(capability ...)` form.
3.  **Validation**:
    *   Checks schema syntax.
    *   Verifies the `:provider` type.
    *   Ensures unique `id`s.
4.  **Registration**: Valid capabilities are registered in the in-memory **Capability Marketplace**.

### 3.2. Metadata-Driven Loading

Local capabilities can declare metadata that influences how they are loaded:

*   `:lazy-load true`: The implementation code is not parsed until the capability is actually called.
*   `:hot-reload true`: The system watches the file for changes and reloads it automatically (dev mode).

---

## 4. Unified Discovery Pipeline

The **Missing Capability Resolver** orchestrates all these methods:

1.  **Runtime Error**: Code calls `(call :github.issues ...)` → Capability not found.
2.  **Resolver Check**:
    *   **Step 1 (Local)**: Is it in `capabilities/` but not loaded? (Load it).
    *   **Step 2 (MCP)**: Is it available in the MCP Registry? (Discover & Compile).
    *   **Step 3 (OpenAPI)**: Is there a known OpenAPI spec for this namespace? (Import & Compile).
3.  **Result**: A new `.rtfs` file is generated and hot-loaded. The execution resumes.

### Comparison

| Feature | MCP Discovery | OpenAPI Discovery | Local RTFS |
| :--- | :--- | :--- | :--- |
| **Source** | Dynamic Server (JSON-RPC) | Static Spec (JSON/YAML) | Filesystem |
| **Schema** | Introspected (`tools/list`) | Explicit in Spec | Explicit in File |
| **Output Schema** | Inferred (Probing) | Explicit (Response definitions) | Explicit |
| **Execution** | `(call :mcp ...)` | `(call :http ...)` | Native / Host |
| **Best For** | AI Agents, CLI Tools | Legacy REST APIs | Core Logic, Manual Code |



