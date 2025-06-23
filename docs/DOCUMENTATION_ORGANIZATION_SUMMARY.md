# Documentation Organization Summary

**Status:** Updated as of June 23, 2025

## Overview

This document summarizes the current, reorganized structure of the project documentation. The reorganization is a key part of **Phase 1** of the [RTFS 1.0 to CCOS Migration Plan](./migration/RTFS_MIGRATION_PLAN.md) and is designed to create a clean, scalable, and intuitive layout that separates the legacy RTFS 1.0 materials from the forward-looking CCOS/RTFS 2.0 vision.

---

## New Directory Structure

```
docs/
├── CCOS_ROADMAP.md
├── DOCUMENTATION_ORGANIZATION_SUMMARY.md
├── README.md
├── migration/
│   └── RTFS_MIGRATION_PLAN.md
├── rtfs-1.0/
│   ├── implementation/
│   └── specs/
├── rtfs-2.0/         # (Forthcoming)
└── vision/
    └── SENTIENT_RUNTIME_VISION.md
```

---

## Directory Guide

### Root (`/`)

- **`README.md`**: The main project README. It serves as the primary entry point, outlining the CCOS vision and linking to key documentation like the roadmap and migration plan.

### `/docs`

- **`CCOS_ROADMAP.md`**: The high-level, strategic roadmap for the evolution of RTFS into the Cognitive Computing Operating System.
- **`DOCUMENTATION_ORGANIZATION_SUMMARY.md`**: This file.
- **`README.md`**: An overview of the documentation structure.

### `/docs/vision/`

- **Purpose**: To house the foundational, long-term vision for the project.
- **`SENTIENT_RUNTIME_VISION.md`**: The core document describing the CCOS, its architecture (Arbiter, Intent Graph, etc.), and its philosophical underpinnings.

### `/docs/migration/`

- **Purpose**: To manage and track the migration process.
- **`RTFS_MIGRATION_PLAN.md`**: The detailed, step-by-step plan for migrating from RTFS 1.0 to CCOS, including tasks, status, and timelines.

### `/docs/rtfs-1.0/`

- **Purpose**: To archive the legacy documentation for the original RTFS 1.0.
- **/specs/**: The original language specifications, grammar, and data model for RTFS 1.0.
- **/implementation/**: Technical documentation related to the implementation of the RTFS 1.0 compiler and runtime.

### `/docs/rtfs-2.0/`

- **Purpose**: A forward-looking directory that will contain all new specifications for the CCOS and RTFS 2.0.
- **Content (Forthcoming)**: This will include detailed specs for the new core objects (`Intent`, `Plan`, `Action`), namespacing, the Arbiter, the Intent Graph, and all other components of the new architecture.

## Benefits of this Structure

- **Clarity**: A clean separation between past, present, and future.
- **Focus**: Developers and contributors can easily find the documentation relevant to their area of interest.
- **Scalability**: The structure can easily accommodate new documents as the project grows.
- **Historical Context**: Preserves the original RTFS 1.0 documentation for reference and backward-compatibility analysis.
