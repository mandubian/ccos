// CCOS Marketplace Serialization Format Demo - RTFS Focus
//
// Shows how marketplace capabilities are serialized to RTFS format
// (JSON is just a portable snapshot; RTFS is the native language)

fn main() {
    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║       CCOS Marketplace Serialization - RTFS Format (Native)          ║");
    println!("║                                                                      ║");
    println!("║  Capabilities in CCOS are homoiconic - they ARE RTFS code           ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("1. HTTP Capability (OpenWeatherMap) - Serialized to RTFS");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let http_rtfs = r#":module
  :type "rtfs/capabilities/marketplace-snapshot"
  :version "1.0.0"
  :generated-at "2025-10-25T12:34:56Z"
  
  :capabilities [
    {:id "weather_api"
     :name "OpenWeatherMap API"
     :version "1.0.0"
     :description "Get current weather and forecasts"
     
     :provider :Http
     :provider-meta {
       :base-url "https://api.openweathermap.org"
       :timeout-ms 5000
       :auth-token "sk-weather-abc123def456"
     }
     
     :input-schema [:map [:city :string] [:units :string]]
     
     :output-schema [:map [:temperature :float] [:condition :string]]
     
     :metadata {
       :api-type "REST"
       :rate-limit "1000/day"
       :base-path "/v2.5"
     }
     
     :permissions ["read:weather"]
     :effects ["call:external_api"]
    }
  ]"#;

    for line in http_rtfs.lines() {
        println!("  {}", line);
    }

    println!("\n✓ HTTP Capability Features:");
    println!("  - Native RTFS format (not a secondary translation)");
    println!("  - Homoiconic: capability code = RTFS data structure");
    println!("  - :provider :Http indicates HTTP provider");
    println!("  - :provider-meta contains HTTP-specific config (base_url, timeout, auth)");
    println!("  - :input-schema and :output-schema defined as RTFS types (Map, String, Number)");
    println!("  - :metadata preserves API documentation and constraints");
    println!("  - All fields are loadable back into CapabilityManifest\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("2. MCP Capability (GitHub) - Serialized to RTFS with Session Metadata");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mcp_rtfs = r#":module
  :type "rtfs/capabilities/marketplace-snapshot"
  :version "1.0.0"
  :generated-at "2025-10-25T12:35:00Z"
  
  :capabilities [
    {:id "github_mcp"
     :name "GitHub MCP Server"
     :version "2.0.0"
     :description "Interact with GitHub repositories and issues"
     
     :provider :Mcp
     :provider-meta {
       :server-url "http://localhost:3001"
       :tool-name "github_operations"
       :timeout-ms 10000
     }
     
     :input-schema [:map [:action :string] [:repo :string] [:payload :any]]
     
     :output-schema [:map [:result :string] [:status :string] [:data :any]]
     
     :metadata {
       ;; Session Management Metadata (enables stateful MCP sessions)
       :mcp-requires-session "true"
       :mcp-server-url "http://localhost:3001"
       :mcp-tool-name "github_operations"
       :mcp-description "Perform GitHub API operations with session persistence"
       
       ;; Additional context
       :auth-type "oauth2"
       :rate-limit "5000/hour"
     }
     
     :permissions ["write:repo" "read:issues"]
     :effects ["call:github_api" "maintain:session"]
    }
  ]"#;

    for line in mcp_rtfs.lines() {
        println!("  {}", line);
    }

    println!("\n✓ MCP Capability Features:");
    println!("  - :provider :Mcp indicates MCP provider");
    println!("  - :provider-meta contains MCP server endpoint and tool configuration");
    println!("  - Session metadata (mcp_requires_session, mcp_server_url) preserved");
    println!("  - At runtime: marketplace detects 'mcp_' prefixed keys in metadata");
    println!("  - Routes to SessionPoolManager → MCPSessionHandler");
    println!("  - MCPSessionHandler maintains Mcp-Session-Id across tool calls");
    println!("  - Enables persistent authentication for GitHub API operations\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("3. Round-Trip Lifecycle");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("Step 1: Runtime Creation");
    println!(
        "  marketplace.register_capability_manifest(http_cap) → stored in Arc<RwLock<HashMap>>\n"
    );

    println!("Step 2: Export to RTFS");
    println!("  marketplace.export_capabilities_to_rtfs_dir(\"/capabilities\")");
    println!("  → Generates {{capability_id}}.rtfs files");
    println!("  → Each file contains complete CapabilityManifest as RTFS module\n");

    println!("Step 3: Human Inspection/Edit");
    println!("  User can read/edit .rtfs files (human-readable format)");
    println!("  Can modify metadata, schemas, provider config\n");

    println!("Step 4: Import into New Marketplace");
    println!("  new_marketplace.import_capabilities_from_rtfs_dir(\"/capabilities\")");
    println!("  → Scans for .rtfs files");
    println!("  → Parses RTFS modules using MCPDiscoveryProvider utilities");
    println!("  → Reconstructs CapabilityManifest objects");
    println!("  → Re-registers into marketplace\n");

    println!("Step 5: Session Routing at Runtime");
    println!("  user_request → marketplace.execute(cap_id, args)");
    println!("  marketplace checks metadata for '_requires_session' keys");
    println!("  If present → SessionPoolManager.execute_with_session()");
    println!("  SessionPoolManager detects provider (mcp_, graphql_, etc.)");
    println!("  Routes to MCPSessionHandler → maintains session across calls\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("4. Serializable Provider Types");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("✓ SUPPORTED (serialized to RTFS):");
    println!("  • Http        → :provider :Http, :provider-meta {{ :base-url, :timeout-ms, :auth-token }}");
    println!("  • Mcp         → :provider :Mcp, :provider-meta {{ :server-url, :tool-name, :timeout-ms }}");
    println!("  • A2A         → :provider :A2a, :provider-meta {{ :endpoint, :namespace }}");
    println!("  • RemoteRTFS  → :provider :RemoteRtfs, :provider-meta {{ :module-url }}\n");

    println!("✗ NOT SERIALIZABLE (gracefully skipped):");
    println!("  • Local       → Closures/lambdas (can't serialize function pointers)");
    println!("  • Stream      → Channels/async streams (runtime-specific)");
    println!("  • Registry    → Handles (external capability registry)");
    println!("  • Plugin      → Plugin handles (runtime-specific)\n");

    println!("  When exporting: non-serializable providers logged and skipped");
    println!("  Enables selective export of portable capabilities\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("5. API Usage Examples");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("Export all capabilities to RTFS files:");
    println!("  let count = marketplace.export_capabilities_to_rtfs_dir(\"/tmp/caps\").await?;");
    println!("  println!(\"Exported {{}} capabilities\", count);\n");

    println!("Import capabilities from RTFS directory:");
    println!("  let count = marketplace.import_capabilities_from_rtfs_dir(\"/tmp/caps\").await?;");
    println!("  println!(\"Loaded {{}} capabilities\", count);\n");

    println!("Export to JSON (portable snapshot):");
    println!("  marketplace.export_capabilities_to_file(\"caps.json\").await?;\n");

    println!("Import from JSON:");
    println!("  marketplace.import_capabilities_from_file(\"caps.json\").await?;\n");

    println!("Get capability after round-trip:");
    println!("  if let Some(cap) = marketplace.get_capability(\"weather_api\").await {{");
    println!("    println!(\"Reloaded: {{}}\", cap.name);");
    println!("    match &cap.provider {{");
    println!("      ProviderType::OpenApi(api) => {{");
    println!("        println!(\"Base URL: {{}} ({{}} operations)\", api.base_url, api.operations.len());");
    println!("      }}");
    println!("      ProviderType::Http(http) => println!(\"URL: {{}}\", http.base_url),");
    println!("      _ => (),");
    println!("    }}");
    println!("  }}\\n");

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("6. Implementation Details");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    println!("Files in marketplace.rs:");
    println!("  • export_capabilities_to_rtfs_dir()");
    println!("    - Iterates registered capabilities");
    println!("    - Skips non-serializable providers");
    println!("    - Generates .rtfs files with :provider blocks");
    println!("    - Uses type_expr_to_rtfs_pretty() for schema rendering\n");

    println!("  • import_capabilities_from_rtfs_dir()");
    println!("    - Scans directory for .rtfs files");
    println!("    - Tries MCPDiscoveryProvider.load_rtfs_capabilities() first (robust parser)");
    println!("    - Falls back to heuristic parsing if needed");
    println!("    - Reconstructs CapabilityManifest");
    println!("    - Registers into marketplace\n");

    println!("Files imported from:");
    println!("  • src/ccos/capability_marketplace/mcp_discovery.rs");
    println!("    - parse_rtfs_module()");
    println!("    - rtfs_to_capability_manifest()");
    println!("    - Reusable RTFS parsing utilities\n");

    println!("Session routing:");
    println!("  • src/ccos/capabilities/session_pool.rs");
    println!("    - SessionPoolManager.execute_with_session()");
    println!("    - Detects provider type from metadata keys");
    println!("    - Routes to provider-specific handler\n");

    println!("  • src/ccos/capabilities/mcp_session_handler.rs");
    println!("    - MCPSessionHandler implements SessionHandler trait");
    println!("    - Wraps MCPSessionManager");
    println!("    - Maintains session cache per (capability_id, server_url)\n");

    println!("╔══════════════════════════════════════════════════════════════════════╗");
    println!("║                    Key Principles                                   ║");
    println!("║                                                                      ║");
    println!("║  1. Homoiconicity: Capabilities ARE RTFS structures                 ║");
    println!("║     (code = data, fully introspectable and editable)                ║");
    println!("║                                                                      ║");
    println!("║  2. Native Format: RTFS serialization is the canonical form         ║");
    println!("║     (not a translation from internal structure)                     ║");
    println!("║                                                                      ║");
    println!("║  3. Portable Snapshot: JSON export for interop                      ║");
    println!("║     (not the primary format, just for convenience)                  ║");
    println!("║                                                                      ║");
    println!("║  4. Session Preservation: Metadata carries session requirements     ║");
    println!("║     (mcp_requires_session triggers stateful routing)                ║");
    println!("║                                                                      ║");
    println!("║  5. Round-Trip Integrity: Export→Edit→Import preserves everything   ║");
    println!("║     (provider config, schemas, permissions, session metadata)       ║");
    println!("╚══════════════════════════════════════════════════════════════════════╝");
}
