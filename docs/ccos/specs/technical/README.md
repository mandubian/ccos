# CCOS Technical Documentation

This directory contains detailed technical specifications and implementation guides for CCOS (Cognitive Computing Operating System) and RTFS (Reason about The Fucking Spec).

## Documents

### Type System
- [`rtfs-native-type-system.md`](./rtfs-native-type-system.md) - Comprehensive specification for RTFS 2.0 native type system, including array shapes, type refinements, predicates, and migration from JSON Schema

### Future Technical Specs
- Security and governance implementation details
- Runtime performance optimization guides  
- Inter-agent communication protocols
- Capability marketplace architecture
- RTFS compiler implementation guides

## Implementation Status

- **RTFS Native Types**: ⚠️ In Progress (Issue #50) - Basic TypeExpr exists, needs full type system with predicates and validation
- **Capability System**: ✅ Implemented - Basic capability marketplace with security features
- **Governance Kernel**: 🔄 Partial - Security policies implemented, full governance in development

## Contributing

Technical specifications should:
1. Include complete implementation architecture with Rust code examples
2. Provide migration strategies for existing code
3. Demonstrate real-world usage examples
4. Consider AI-friendliness and performance implications
5. Reference related GitHub issues for tracking

## Related Issues

- [Issue #50: Implement RTFS Native Type System for Capability Schemas](https://github.com/mandubian/ccos/issues/50)
