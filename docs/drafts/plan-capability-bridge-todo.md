## Implementation Plan: Plan/Capability Canonicalization and Plan-as-Capability Adapter

Date: 2025-11-05
Owner: CCOS core

### Goals
- Clear contracts: canonical map schemas for Plan and Capability
- Deterministic parsing: normalize function-call sugar to canonical maps
- Easier validation/tooling: validators, CLI/REPL utilities, migration
- Expose a plan as a capability via a thin adapter (reuse orchestration)

### Tasks

#### ✅ Completed
- ✅ Define canonical Plan/Capability map schemas in CCOS (fields, types)
  - Created `ccos/src/rtfs_bridge/canonical_schemas.rs`
  - Defined `CanonicalPlanSchema` with required/optional fields and validation
  - Defined `CanonicalCapabilitySchema` with required/optional fields and validation
  - Added basic validation logic and tests

- ✅ Normalize function-call syntax to canonical maps with deprecation warnings
  - Created `ccos/src/rtfs_bridge/normalizer.rs`
  - Implemented `normalize_plan_to_map()` - converts `(plan ...)` to `{:type "plan" ...}`
  - Implemented `normalize_capability_to_map()` - converts `(capability ...)` to `{:type "capability" ...}`
  - Added deprecation warnings when function-call syntax is detected
  - Supports both old-style (keyword-value pairs) and new-style (map argument) function calls
  - Validates normalized maps against canonical schemas
  - Handles maps that are already in canonical format

#### ✅ Completed (continued)
- ✅ Implement strict validators for Plan/Capability schemas (TypeExpr-aware)
  - Enhanced validators with `validate_type_expr_schema()` function
  - Validates that schemas contain valid TypeExpr structures
  - Integrated into `validate_plan()` and extraction pipeline
  - Added tests for schema validation

- ✅ Align rtfs_bridge extractors/converters to canonical schema
  - Integrated normalization into extraction pipeline
  - All Plans now validate schemas automatically

- ✅ Add plan-as-capability adapter (inherit schemas, wrap :body)
  - Implemented `plan_to_capability_map()` conversion
  - Wraps plan body as capability implementation
  - Inherits schemas and metadata

#### 🔄 Next Steps
- Propagate effects/permissions for wrapper caps via static analysis (conservative)
- Support :language on local capability implementations; provider-meta for others
- Update marketplace import/export to preserve schemas, language, metadata
- Add backward-compat layer + migration script for existing RTFS files
- Tooling: CLI/REPL commands for validate/normalize/convert (plan/cap)
- Tests: unit/integration/golden samples; e2e plan-as-cap execution
- Docs: update specs (002, 004), guides, examples, LLM prompts
- Observability: audit logs/metrics for normalization and adapter usage
- Governance: policy checks for wrappers; attestation/version rules
- Versioning: add :schema-version and semantic version flow
- LLM prompt updates to emit canonical maps with schemas

### Notes
- Keep Plan and Capability distinct (governance, marketplace routing, provenance)
- Allow adapter to expose a Plan as a callable Capability when needed
- Maintain RTFS/CCOS separation: bridges live in CCOS; RTFS remains language/runtime only

### Progress Log

**2025-11-05**: Started implementation

**Phase 1: Canonical Schemas** ✅
- Created canonical schema definitions module
- Defined Plan schema: `:type`, `:name`, `:body` (required), `:language`, `:input-schema`, `:output-schema`, `:capabilities-required`, `:annotations`, `:intent-ids`, `:policies` (optional)
- Defined Capability schema: `:id`, `:name`, `:version`, `:description` (required), `:input-schema`, `:output-schema`, `:implementation`, `:language`, `:provider`, `:provider-meta`, `:permissions`, `:effects`, `:metadata`, `:attestation`, `:provenance` (optional)
- Added validation functions for both schemas

**Phase 2: Normalization** ✅
- Created normalization module with `NormalizationConfig` for behavior control
- Implemented function-call to canonical map conversion for Plans
- Implemented function-call to canonical map conversion for Capabilities
- Added deprecation warnings when old syntax is detected
- Supports validation after normalization
- Handles both keyword-value pair and map-argument function call styles

**Phase 3: Integration** ✅
- Integrated normalization into `extract_plan_from_rtfs()` - all Plans now normalize to canonical maps first
- Updated converters: `plan_to_rtfs_function_call()` marked as deprecated, `plan_to_rtfs_map()` is canonical
- All extraction now happens through normalized canonical maps

**Phase 4: Plan-as-Capability Adapter** ✅
- Created `plan_as_capability.rs` module
- Implemented `plan_to_capability_map()` - converts Plan to Capability RTFS map
- Wraps plan `:body` as capability `:implementation` function
- Inherits plan's `:input-schema` and `:output-schema`
- Adds metadata, provider info, and effects/permissions
- Implemented `prepare_plan_as_capability()` - prepares plan for marketplace registration
- Added `PlanAsCapabilityConfig` for flexible configuration
- Supports all PlanLanguage variants (Rtfs20, Wasm, Python, GraphJson, Other)

**Phase 5: TypeExpr-Aware Validators** ✅
- Enhanced `validators.rs` with TypeExpr-aware schema validation
- Implemented `validate_type_expr_schema()` - validates schemas are valid TypeExpr structures
- Validates keywords, strings, nested maps, and vectors as type expressions
- Integrated into `validate_plan()` - Plans now validate schemas automatically
- Added `validate_capability_schemas()` helper for capability validation
- Integrated validation into `extract_plan_from_rtfs()` pipeline
- Added tests for schema validation

**Phase 6: Effects/Permissions Propagation** ✅
- Created `effects_propagation.rs` module for static analysis
- Implemented `propagate_effects_from_plan()` - analyzes plan body to extract capability calls
- Uses regex and AST parsing to find all `(call :capability.id ...)` patterns
- Recursively walks RTFS expression tree to find nested capability calls
- Supports flexible capability lookup via closure (allows marketplace, cache, etc.)
- Integrated into `prepare_plan_as_capability_with_propagation()`
- Conservatively unions all effects and permissions from used capabilities
- Added tests for capability ID extraction

**Phase 7: Language Tagging Support** ✅
- Created `language_utils.rs` module for language validation and normalization
- Implemented `plan_language_to_string()` - converts PlanLanguage to canonical string
- Implemented `parse_language_string()` - parses language strings to PlanLanguage
- Added `validate_language_string()` - validates language string format
- Added `validate_local_capability_has_language()` - ensures local capabilities have language
- Added `ensure_language_for_local_capability()` - auto-sets language if missing
- Integrated language validation into canonical schema validation
- Enhanced `plan_to_capability_map()` to use language utilities
- Added canonical language constants (rtfs20, wasm, python, graphjson)
- Distinction between `:language` (for local) and `:provider-meta` (for remote) clarified
- Added comprehensive tests for language utilities


