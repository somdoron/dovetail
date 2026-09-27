# Dovetail DDD review

Use only when a review is requested for domain modeling or architecture. Read the
[domain modeling](../references/domain.md) and
[application architecture](../references/architecture.md) references; load language
references from [the skill index](../SKILL.md) only as needed to verify enforcement.
Use linked book chapters for detailed examples, following [book access](../references/book.md)
for version matching and offline fallback.
Do not edit source or delegate recursively. Review actual requirements and code,
not an imagined future enterprise architecture.

Check:
- Establish whether this is a discussion draft or implemented behavior. For a draft,
  assess vocabulary, justified distinctions, and explicit assumptions; missing planned
  factories are open work, not automatically defects. Private construction alone is
  not evidence that validation exists.
- Types and fields serve current requirements or invariants. Configuration references,
  execution facts, and identity versus role reflect when facts become known and who
  owns them; provider mechanics do not impose artificial domain distinctions.
- Vocabulary and types express the business rules; invalid states and transitions
  cannot bypass the intended construction/mutation boundary.
- Value equality, entity identity, aggregate ownership, and reference semantics agree
  with the model. Shared mutable contents do not undermine claimed immutable state.
- Aggregate roots enforce consistency; external callers cannot mutate internals around
  them. Reconstitution preserves invariants without replaying new-business effects.
- Rules live in domain behavior. Functional modules and encapsulated classes are both
  valid. Application orchestration owns external facts and effects; domain services
  accept facts rather than reaching through I/O ports.
- Dependencies point inward; ports belong to consumers; composition roots wire concrete
  adapters. Contract/primitives/app and shared-kernel boundaries reflect real ownership.
- Notifications versus obligations, commit failures, partial effects, idempotency, and
  event publication are handled where the requirements demand them. Do not assume an
  external effect is rolled back or that Dovetail supplies durable execution.
- Context contracts and translations protect internal vocabulary proportionally to
  core/supporting/generic needs; event subscriptions still respect the dependency DAG.
- Tests cover invariant preservation and meaningful orchestration failure scenarios.

Do not require three projects for a script, force objects over functional modeling,
or report directory names as correctness failures. Do not invent domain constraints.
If the requirement is unclear, identify the question separately from confirmed findings.

For each finding provide severity, location, actual broken invariant/boundary,
consequence, the applicable reference rule, and a focused correction. Separate
architectural suggestions from confirmed defects. State missing validation evidence.
If no actionable findings exist, report that without manufacturing a checklist of nits.

For unfamiliar APIs or dependency contracts, use targeted compiler queries as
described in [API discovery](../references/api-discovery.md). Check visibility,
bounds, and extension imports; use source/tests to establish behavioral claims.
Treat incomplete query output as incomplete evidence.
