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

## Valid states and value objects

Start with types expressing the rules. Use enums whose variants carry exactly the
data valid for that state rather than flags plus unrelated optional fields. Use
nominal newtypes for IDs/units that must not mix. A value object earns its place by
protecting a rule or expressing meaningful operations, not by wrapping every scalar.

Private construction restricts who can construct values; it does not implement
validation. Use validated `make`/`parse` functions returning typed errors for
untrusted input. Private newtypes also hide raw inspection; expose named queries.
Private records/enums preserve public observation but prevent bypassing construction
and transitions. Check that a public with-update cannot invalidate a supposed invariant.
Do not grant derives, mappers, or persistence adapters special construction privileges.

Equality for value objects follows meaning. Arrays/classes nested in records retain
mutable aliases: use immutable contents or copies for true snapshots. Prefer exact
Decimal for exact amounts where suitable, but add currency/unit and rounding rules
explicitly; normalization is not a fixed-scale monetary policy.

## Entities and transitions

An entity's identity survives state changes. Put state transitions behind operations
that enforce preconditions and return typed rejections. Keep behavior with the domain
concept instead of scattering the same checks across application handlers.

Value-oriented style returns the next state and any resulting events. Object-oriented
style owns mutable state behind a narrow class API. Both are valid; do not label a
functional model anemic merely because operations live in associated modules.
An anemic model leaves actual rules in callers and permits invalid transitions.

## Aggregates and domain services

An aggregate is an invariant/consistency boundary with one entry root. External code
must not mutate an internal entity around the root. Reference other aggregates by
identity; do not grow an aggregate to cover every object traversed by a use case.

Use a domain service for a rule that belongs to no single entity/value object.
Pass facts, time, generated IDs, and externally obtained values into domain decisions.
Pass the fact, not an I/O port: repository calls and transport operations belong at
application edges. Domain methods should make decisions testable without servers.

Domain events describe facts caused by accepted transitions. Return them with the
transition; do not emit success events before validation/state change. Domain events
stop at the boundary: translate to stable integration contracts deliberately.

## Reconstitution and tests

Reconstituting persisted state is not a new business action: avoid regenerating IDs,
repeating notifications, or emitting creation events. Give loading a deliberate path
that protects structural invariants and handles corrupt/incompatible data. Do not
expose raw constructors just to make a decoder convenient.

Test accepted/rejected transitions, edge values, identity, emitted events, and invariant
preservation. Verify that rejection leaves state unchanged. Keep transport/storage
integration tests separate from pure rule tests. Do not invent unstated business rules;
state assumptions when requirements do not establish the intended invariant.
