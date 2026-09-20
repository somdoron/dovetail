# Domain modeling

Apply this guidance to real business rules; a utility script does not need aggregates
or multiple projects. Prefer the domain's vocabulary over framework terminology.
These rules follow the book's domain-layer guidance, supporting both value-oriented
and object-oriented styles.

## Valid states and value objects

Start with types expressing the rules. Use enums whose variants carry exactly the
data valid for that state rather than flags plus unrelated optional fields. Use
nominal newtypes for IDs/units that must not mix. A value object earns its place by
protecting a rule or expressing meaningful operations, not by wrapping every scalar.

Use private construction and validated `make`/`parse` functions returning typed errors
for untrusted input. Private newtypes also hide raw inspection; expose named queries.
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
