# Domain modeling

Apply this guidance to real business rules; a utility script does not need aggregates
or multiple projects. Prefer the domain's vocabulary over framework terminology.
These rules follow the book's domain-layer guidance, supporting both value-oriented
and object-oriented styles.

## Collaborative types-first modeling

For a new domain or substantial feature, suggest drafting a small `types.dove` and
discussing it with the user before implementing behavior. Treat it as a shared
discussion artifact, not a finished architecture. Start with the smallest model
supported by current requirements, label assumptions, and keep unresolved rules
visible rather than filling them with speculative infrastructure.

Invite feedback on meanings, ownership, identity, lifecycle, and invariants. For
each type and field, ask which requirement, invariant, or meaningful distinction
it represents. Explain it through behavior: what decision does it enable, and what
would break without it? If no current behavior needs it, consider removing it.
Discuss the consequential uncertainties in small rounds instead of presenting an
exhaustive questionnaire or implementing all possible answers.

Revise the declarations through user feedback, check that each iteration compiles,
and keep comments and related design documentation consistent. Mark intended
invariants separately from enforced ones. Introduce factories and transitions as
their rules become understood; follow the construction and testing guidance below.
Do not treat a deliberately unfinished types-only draft as an anemic implementation.

## Choosing vocabulary and distinctions

Prefer direct values when they express the current requirement. In an agent harness,
an agent's skill references can be `List<SkillName>` without a one-field `Skill`
wrapper; submitted input can be text until task/context structure has a purpose.
A tool descriptor can hold `inputSchema` directly, and a run can hold execution
fields directly without an extra record solely to group them.

Use domain names rather than configuration or provider mechanics. Names such as
`SkillBinding` or `CallableName` need a domain justification. Tools and subagents
can have independent `ToolName` and `AgentName` vocabularies; provider-level name
disambiguation belongs in infrastructure. Reuse standard-library concepts such as
`standard.time.Instant` and `standard.time.Duration` instead of equivalent wrappers;
query the project's dependency declarations before using their APIs.

Distinguish identity from role. A subagent may be an ordinary agent executing a
delegated run: represent the parent/delegation relationship on the run unless
requirements establish separate agent kinds. Ask when each fact becomes known;
configuration references and execution observations have different lifecycles (see
[application architecture](architecture.md#configuration-and-execution-facts)).

These are questions to guide modeling, not bans on wrappers, revisions, structured
context, or separate types. Preserve abstractions that protect real invariants or
express meaningful distinctions. Child-budget reservations, revision pinning,
transcript cursors, effect classifications, and extra registries need concrete
requirements before becoming domain concepts.

## Implementation and review

Read [The Domain Layer](https://dovetaillang.org/book/ddd.md) for value objects,
entities, aggregates, factories, and tests, and [Best Practices](https://dovetaillang.org/book/best-practices.md)
for language conventions. Use the [book access workflow](book.md).

Check that construction and transitions enforce the stated rules; private access
alone does not implement validation. Keep expected rejection typed, verify that
rejection leaves state unchanged, and check whether aliases bypass the boundary.
Accept both value-oriented and object-oriented modeling. An unfinished discussion
draft is not a completed implementation and should be reviewed in that context.
