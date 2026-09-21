# Application architecture and context boundaries

Apply the book's project/layer guidance proportionally to the application. Chapters
20–21 contain outline material; their design advice is not implemented runtime
functionality or a compiler guarantee. Start with one project when no real boundary
requires more. Do not force a prescribed folder count onto trivial changes.

## Projects and dependency direction

For a substantial bounded context, the book organizes `primitives` (value vocabulary),
`contract` (public data/service wire surface), and `app` (implementation). Within app,
separate domain, application, and infrastructure packages; a separate domain project
is an optional stronger boundary. Dependencies point inward toward policy, never
from domain into persistence/transport. Keep the workspace DAG acyclic.

Put ports where their consumers own the contract; adapters implement them at the
infrastructure edge. Assemble concrete dependencies in the composition root. A shared
kernel contains deliberately governed shared concepts, not a dumping ground for
entities and implementation details. External consumers depend on a context's contract,
not its app internals. Compiler dependency/visibility checks enforce declared boundaries,
but cannot discover business ownership or guarantee pure domain code by themselves.

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

## Application responsibilities

Use cases coordinate requests, event handlers, and jobs: acquire facts, ask the domain
to decide, persist outcomes, and fulfill obligations. Prefer I/O around pure decisions;
when later decisions require earlier effects, use explicit alternating stages rather
than pretending the whole workflow is one pure operation.

Distinguish notifications (a fact others may observe) from obligations (effects this
use case must ensure happen). A database transaction cannot undo an HTTP request,
email, or money transfer. Plan commit failures, retries, partial completion, duplicate
delivery, and idempotency. Do not apply "one change, one transaction, one aggregate"
as an unconditional rule that hides required consequences.

Repositories typically serve aggregate roots, not arbitrary internal entities. Translate
storage errors at the boundary while preserving actionable failure information. Keep
domain failures distinguishable from infrastructure failures in application results.
Publishing integration events must account for persistence/publication gaps; use an
explicit reliable mechanism when the requirement needs one, not an unawaited send.

Sagas may recover backward via compensation or forward by completing obligations.
Irreversible actions cannot be honestly described as rollbackable. Dovetail has no
durable execution runtime today: record progress/retry requirements explicitly and
use an actual chosen infrastructure mechanism; do not invent durable APIs.

## Context mapping

Identify upstream/downstream ownership and core/supporting/generic subdomains.
Choose boundaries by language and ownership, not merely database tables. Public
contracts provide a published language/open host service; an anticorruption layer
protects the consumer's vocabulary. Direct contract use can be reasonable for simple
supporting/generic integration; core-domain crossings generally justify translation.

Integration event handlers should translate external event types before entering
internal application/domain code. Consumer subscriptions still create compile-time
contract dependencies, so cycles remain invalid even if runtime traffic is asynchronous.
Do not share entities across contexts or mechanically mirror external schemas internally.

Test use-case orchestration with controlled ports, including repeated delivery and
partial failure when relevant. Test adapter mappings and transactions separately.
Architectural findings must identify an actual broken boundary or consequence;
missing folder names alone are not defects.
