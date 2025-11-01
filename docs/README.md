# CCOS + RTFS Documentation

This directory contains all documentation for the CCOS (Cognitive Computing Operating System) and RTFS (Runtime for Trustworthy Systems) project.

## 📚 Essential Reading

### RTFS Type System & Tools (NEW)
- **[Type System Formal Specification](./rtfs-2.0/specs/13-type-system.md)** (903 lines) - Complete theoretical foundation with soundness proofs, subtyping rules, and formal semantics
- **[Type Checking Quick Guide](./rtfs-2.0/guides/type-checking-guide.md)** (523 lines) - Practical guide for developers with examples and best practices
- **[REPL Interactive Guide](./rtfs-2.0/guides/repl-guide.md)** (664 lines) - User-friendly REPL with visual feedback, plain-language explanations, and instant type/security analysis

### Core Documentation
- **[RTFS 2.0 Specifications](./rtfs-2.0/specs/)** - Language specifications
- **[CCOS Specifications](./ccos/specs/)** - CCOS architecture and design

## Directory Structure

### `/implementation/`
Detailed technical documentation about RTFS implementation:
- Runtime system implementation summaries
- Architecture deep-dives and design decisions
- Performance analysis and optimization guides
- Testing documentation and validation approaches

### Related Documentation (Outside `/docs/`)

#### `/specs/` - Language and System Specifications
- Core language specifications and grammar
- Type system and semantics documentation
- Standard library specifications
- Security and resource management models

#### Root-level Documentation
- **`README.md`** - Project overview and getting started
- **`NEXT_STEPS.md`** / **`NEXT_STEPS_UPDATED.md`** - Development roadmap and priorities
- **`IMPLEMENTATION_SUMMARY.md`** - High-level implementation status

## Documentation Types

### For Users
- Getting started guides
- Language tutorials and examples
- API reference documentation

### For Developers
- Implementation details and architecture
- Contributing guidelines
- Development setup and workflow

### For Researchers
- Language design rationale
- Performance characteristics
- Formal specifications and proofs

## Maintenance Guidelines

- Keep documentation in sync with implementation
- Use clear, consistent formatting and structure
- Include examples and practical guidance
- Update cross-references when moving or renaming files
- Tag documentation with version information when relevant

## Contributing

When adding new documentation:
1. Choose the appropriate directory based on content type
2. Follow existing naming conventions
3. Update relevant README files to reference new content
4. Ensure links and cross-references are accurate
5. Review for clarity and completeness
