# Application architecture and context boundaries

Read [Project structure](https://dovetaillang.org/book/project-structure.md), [Application layer](https://dovetaillang.org/book/application-layer.md), [Context mapping](https://dovetaillang.org/book/context-mapping.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

Apply the book proportionally: a small application need not have three projects.
Chapters 20–21 include design outlines, not runtime guarantees. There is no durable
execution runtime in Dovetail today.

## Configuration and execution facts

Separate what a definition requests from what an execution actually used. In an
agent harness, an agent can reference `SkillName`, `ToolName`, and `AgentName`;
the application resolves definitions when needed and records the versions or
content actually used in the run. Referenced capabilities can evolve without
forcing the parent definition to change. Require definition-time revision pinning
only when a concrete reproducibility or compatibility rule calls for it.

Ask when facts become available and which operation establishes them. A skill's
content revision may become known only after loading succeeds; it need not exist
on the agent definition. Discovery metadata, successfully loaded content, and
execution history serve different decisions. Capture accepted execution facts at
the appropriate stage rather than guessing them during configuration. Decide
whether later loads reuse captured content or resolve again from actual run rules.
The application obtains external facts; the domain decides what they mean.

Use the [modeling discussion](domain.md#collaborative-types-first-modeling) to
establish these rules before adding resolution registries or execution machinery.

## Review the actual boundary

Dependencies point toward policy; transport/storage remain at application edges.
Identify a concrete broken boundary or consequence rather than missing folder names.
Distinguish notifications from obligations, and account for retries, partial failure,
and persistence/publication gaps. Database rollback does not undo external effects.
Use a real infrastructure mechanism when durable execution is required. Test repeated
delivery, adapter mappings, and transactions where they affect correctness.
