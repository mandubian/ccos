# CCOS Technical Documentation

This directory contains **RTFS-specific implementation details** and technical references that complement both:
- The formal RTFS 2.0 specifications in `docs/rtfs-2.0/specs/`
- The CCOS cognitive architecture specifications in `docs/ccos/specs/`

This directory serves as the **technical implementation reference** for RTFS developers, while the CCOS specs focus on high-level cognitive architecture.

## Current Status

✅ **RTFS 2.0 Specifications Complete** - All formal specifications have been completed and are available in `docs/rtfs-2.0/specs/`  
✅ **Technical Documentation Cleaned Up** - Duplicate and outdated files removed, content consolidated

## Documents

### Core Implementation References
- **[01-core-objects.md](01-core-objects.md)** - Reference for correct RTFS 2.0 object grammar
- **[03-object-schemas.md](03-object-schemas.md)** - JSON schema definitions for RTFS 2.0 objects
- **[CAPABILITY_SYSTEM_SPEC.md](CAPABILITY_SYSTEM_SPEC.md)** - Detailed capability system implementation
- **[RTFS_CCOS_QUICK_REFERENCE.md](RTFS_CCOS_QUICK_REFERENCE.md)** - RTFS vs CCOS runtime reference
- **[TECHNICAL_IMPLEMENTATION_GUIDE.md](TECHNICAL_IMPLEMENTATION_GUIDE.md)** - Implementation architecture details

### Cleanup Status
- **[CLEANUP_PLAN.md](CLEANUP_PLAN.md)** - ✅ COMPLETED - Technical documentation consolidated

## Documentation Structure

The technical directory serves as a reference for RTFS implementation details, while the formal specifications are maintained in `docs/rtfs-2.0/specs/` and CCOS architecture specs in `docs/ccos/specs/`:

### CCOS Architecture Specs (High-Level)
- **Intent Graph**: `001-intent-graph.md`
- **Plans & Orchestration**: `002-plans-and-orchestration.md`
- **Causal Chain**: `003-causal-chain.md`
- **Capabilities & Marketplace**: `004-capabilities-and-marketplace.md`
- **Security & Context**: `005-security-and-context.md`
- **Arbiter & Cognitive Control**: `006-arbiter-and-cognitive-control.md`
- **Global Function Mesh**: `007-global-function-mesh.md`
- **Delegation Engine**: `008-delegation-engine.md`
- **Context Horizon**: `009-context-horizon.md`
- **Ethical Governance**: `010-ethical-governance.md`
- **Capability Attestation**: `011-capability-attestation.md`
- **Intent Sanitization**: `012-intent-sanitization.md`
- **Working Memory**: `013-working-memory.md`
- **Step Special Form Design**: `014-step-special-form-design.md`

### RTFS 2.0 Formal Specs (Language)

- **Philosophy**: `00-rtfs-2.0-philosophy.md`
- **Language Features**: `01-language-features.md`
- **Grammar Extensions**: `02-grammar-extensions.md`
- **Object Schemas**: `03-object-schemas.md`
- **Streaming Syntax**: `04-streaming-syntax.md`
- **Native Type System**: `05-native-type-system.md`
- **Capability System**: `06-capability-system.md`
- **Network Discovery**: `07-network-discovery.md`
- **Security & Attestation**: `08-security-attestation.md`
- **Secure Standard Library**: `09-secure-standard-library.md`
- **Formal Language Specification**: `10-formal-language-specification.md`
- **Specification Summary**: `11-specification-summary.md`
- **Capability Implementation**: `12-capability-system-implementation.md`
- **Integration Guide**: `13-rtfs-ccos-integration-guide.md`
- **Step Special Form**: `14-step-special-form.md`

### Technical Implementation Reference (This Directory)

## Implementation Status

- **RTFS 2.0 Specs**: ✅ Complete - All formal specifications finished
- **Capability System**: ✅ Implemented - Complete with security features
- **Object Schemas**: ✅ Complete - All five core object types defined
- **Grammar**: ✅ Corrected - Uses proper `(intent ...)` and `(plan ...)` syntax
- **Integration**: ✅ Complete - RTFS 2.0 and CCOS integration documented

## Contributing

Technical documentation should:
1. **Focus on RTFS Implementation**: Provide practical implementation details for RTFS developers
2. **Complement Both Specs**: Reference both RTFS 2.0 formal specs and CCOS architecture specs
3. **Include Code Examples**: Provide Rust code examples and implementation patterns
4. **Avoid Duplication**: Don't duplicate content from formal specifications
5. **Reference Authoritative Sources**: Link to formal specs for authoritative definitions
6. **Focus on Technical Details**: Provide technical guidance, not architectural decisions

## Related Issues

- [Issue #47: Write Formal RTFS Language Specification](https://github.com/mandubian/ccos/issues/47) - ✅ COMPLETED - All requirements met in RTFS 2.0 specs
- [Issue #50: Implement RTFS Native Type System](https://github.com/mandubian/ccos/issues/50) - ✅ COMPLETED
