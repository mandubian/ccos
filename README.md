# RTFS & The Cognitive Computing Operating System (CCOS)

**Welcome to the future of computing.**

---

### Meta-Teaser: RTFS Has an Existential Conversation with Itself

What happens when a language is so self-aware it can contemplate its own syntax?
It gets a little meta, a little philosophical, and hopefully, a little better.
This is RTFS, using its own code-as-data nature to ponder its existence.

```rtfs
;; Step 1: Define Thyself (The data part of homoiconicity)
;; Here, we define the structure of an 'intent' object... as data.
;; It's like looking in a mirror, if the mirror was made of maps and keywords.
(def intent-spec
  :type :ccos:v1/language-construct
  :name "intent"
  :properties {
    :goal "The grand purpose. The 'why'. Required, obviously."
    :program "The 'how'. A plan to achieve the why. Also required."
    :success-criteria "How I know I haven't failed. Optional, but good for my self-esteem."
    :constraints "Things I shouldn't do. Like dividing by zero or starting a land war in Asia."
  })

;; Step 2: Create a Capability for Self-Critique
;; A function that lets the system look at its own navel and find lint.
(capability com.ccos:v1/critique-design
  :description "Takes a language construct specification and returns snarky but constructive feedback."
  :parameters {:spec :ccos:v1/language-construct}
  :returns {:critique :string}
  :executor
    (fn [params]
      (let [spec-name (get-in params [:spec :name])]
        (if (> (count (get-in params [:spec :properties])) 3)
          (str "The '" spec-name "' object is getting a bit chunky. Maybe a diet is in order?")
          (str "The '" spec-name "' object looks lean and mean. No notes. For now.")))))

;; Step 3: The Intent for Introspection (The Arbiter awakens)
;; Now, let's use the above to have a moment of pure, unadulterated navel-gazing.
(intent contemplate-own-complexity
  :goal "To analyze my own 'intent' structure and decide if I'm overcomplicating things."
  :program
    (plan
      :strategy :linear
      :steps [
        (action "fetch-own-spec"
          :description "Grabs the data that defines what an 'intent' is. Very meta."
          ;; In a real system, this would fetch the 'intent-spec' defined above.
          :capability :ccos:v1/get-resource
          :params {:resource-id "intent-spec"})

        (action "critique-the-spec"
          :description "Runs the self-critique capability on my own definition."
          :capability :com.ccos:v1/critique-design
          :params {:spec (result "fetch-own-spec")})

        (action "log-the-verdict"
          :description "The moment of truth. What does the Arbiter think of itself?"
          :capability :ccos:v1/log-message
          :params {:level :info
                   :message (get-in (result "critique-the-spec") [:critique])})
      ]))

```

---

## Vision: The Sentient Runtime

Our vision is to create a **Sentient Runtime**, an operating system that understands user *intent* and dynamically assembles and executes plans to achieve it. This system will move beyond the traditional paradigm of explicit commands and scripts, leveraging Large Language Models (LLMs) and a novel architecture to create a truly cognitive computing environment.

Key documents:
- **[The Vision](./docs/vision/SENTIENT_RUNTIME_VISION.md):** A deep dive into the concepts behind the CCOS, including the Arbiter, the Living Intent Graph, and the Capability Marketplace.
- **[The Roadmap](./docs/CCOS_ROADMAP.md):** The phased plan for realizing the CCOS vision.

## The Migration: From RTFS 1.0 to CCOS

We are currently in the process of evolving RTFS 1.0 into the CCOS. This is a significant undertaking that involves a complete reorganization of the language, architecture, and documentation.

- **[The Migration Plan](./docs/migration/RTFS_MIGRATION_PLAN.md):** This document tracks the step-by-step process of the migration, including documentation changes, language evolution, and implementation phases. It serves as the central hub for monitoring our progress.

### Migration Progress

| Phase | Description | Status |
| :--- | :--- | :--- |
| **Phase 1** | **Documentation & Project Reorganization** | ⏳ **In Progress** |
| Phase 2 | Language Evolution (RTFS 2.0 Specification) | ⬜ To Do |
| Phase 3 | CCOS Architecture Specification | ⬜ To Do |
| Phase 4 | Implementation & Code Migration | ⬜ To Do |

*(For detailed status, see the [full migration plan](./docs/migration/RTFS_MIGRATION_PLAN.md).)*

---

## Repository Structure

- **/docs**: Contains all documentation, organized by version and topic.
  - **/docs/rtfs-1.0**: Legacy documentation for the original RTFS.
  - **/docs/rtfs-2.0**: (Forthcoming) Specifications for the new CCOS/RTFS 2.0 architecture.
  - **/docs/vision**: The core vision document for the Sentient Runtime.
  - **/docs/roadmap**: The high-level CCOS roadmap.
  - **/docs/migration**: The detailed migration plan.
- **/rtfs_compiler**: The Rust-based reference implementation of the RTFS compiler and runtime.
- **/proposals**: Community and internal proposals for changes and enhancements.
- **/examples**: Example RTFS code.

## Contributing

We are actively seeking contributors to help us build the future of computing. Please see our (forthcoming) `CONTRIBUTING.md` for guidelines.

---

## License

This project is licensed under the Apache License, Version 2.0. See the [LICENSE](./LICENSE) file for details.

## Acknowledgements

A project of this scale is only possible with the support of a vibrant community. We would like to thank all our contributors. (Details forthcoming).
