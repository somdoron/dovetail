# Part 21: Strategic Design and Context Mapping

> **Status: outline.** Bullets to be expanded into prose. Part 18 gave one context its shape,
> Part 19 filled it and Part 20 drove it; this part is about the space *between* contexts —
> which relationships exist, and what each one costs.

---

## 21.1 Subdomains: Core, Supporting, Generic

- **Core** — the reason the business wins. Model it carefully; spend your best effort here.
- **Supporting** — necessary, specific to you, not a differentiator. Model it adequately.
- **Generic** — solved the same way everywhere (auth, notifications, tax tables, geocoding). Buy it, or wrap something.
- This classification is not academic: **it is the single best predictor of how much translation to buy at a boundary** (21.4). Effort follows value.
- A context is not the same thing as a subdomain: a subdomain is a slice of the *problem*, a bounded context is a slice of the *solution*. Aim for one-to-one; accept that legacy rarely is.

## 21.2 Ubiquitous Language and Bounded Contexts

- A bounded context is the scope within which one language is consistent. Outside it, the same word means something else — and that is fine.
- The canonical example, worth using because it is unavoidable in practice: "customer" to Orders is a shipping address and a credit status; to Billing it is a tax jurisdiction and a payment instrument.
- **The language should be readable in the code.** In Dovetail, that means package names, type names, and method names all come from the business vocabulary of that context, and `primitives` is where the shared nouns get their types.
- Finding boundaries: follow the language, the data that changes together, and the teams. Where a term needs a qualifier to stay unambiguous ("billing customer"), you have found a seam.

## 21.3 Context Relationship Patterns

Evans' catalogue, each mapped onto Dovetail's project mechanics:

| Pattern | Leverage over upstream | Communication | In Dovetail |
|---|---|---|---|
| Shared Kernel | joint ownership | shared types, no translation | the `shared-kernel` project (18.8) |
| Partnership | mutual, coordinated releases | direct, both directions | mutual `depends` on each other's `contract` |
| Customer/Supplier | your needs are in their backlog | direct, or a thin ACL | `depends` on their `contract` + a mapping module |
| Conformist | none, but their model fits | direct contract use | `application` imports their contract types |
| Anticorruption Layer | none, or their model does not fit | translated | port in `application`, stub + translator in `infrastructure` |
| Open Host Service | you are one of many consumers | a published, general-purpose surface | the `contract` project's service interfaces |
| Published Language | — | a documented interchange vocabulary | the `contract` project's DTOs and events |
| Separate Ways | — | none | no `depends` edge at all |
| Big Ball of Mud | — | ACL, always | isolate behind one port; do not extend the model into it |

- **The `contract` project is Open Host Service and Published Language made into a build artifact.** The service interfaces are the OHS; the DTOs and integration events are the Published Language. Worth stating plainly — it is why Part 18's structure works.
- Partnership and Shared Kernel are the two that create *coupling between teams*, not just between codebases. They should be rare and deliberate.
- Separate Ways is a real answer. Duplicating a small capability often beats an integration nobody owns.

## 21.4 Direct Contract Use vs Anticorruption Layer

- The choice is a spectrum, not a binary. The chapter's job is to say where the line sits.
- **Conformist is right when:** the concept is in a *generic* or *supporting* subdomain; the consumed surface is small and read-only; upstream is a genuine Published Language; or you release together anyway.
- **ACL is right when:** the concept is in your **core domain** — the decisive criterion, because a foreign shape distorts the model you are paid to get right. Also: upstream is legacy or third-party, you have no release leverage, you may swap providers, or their model is far bigger than your need.
- **Costs, stated honestly.** ACL: a duplicated type, a mapping function, mapping tests, one more hop when debugging. Conformist: their vocabulary spreads through your code, their breaking change is your migration, their release schedule is yours.
- **The recommendation:** default to a *thin* ACL even when the mapping is nearly 1:1. Its value is not clever translation — it is that there is exactly one place to change when upstream renames a field. Choose Conformist where you genuinely do not care about the concept.
- **Consumer-driven contracts** as the middle path: the downstream declares what it needs and tests against the upstream, turning an accidental Conformist relationship into an explicit Customer/Supplier one.
- **Reading your real context map off `Dovetail.toml`** — the Domain-specific payoff:
  - `acme.orders.application` imports `acme.customers.contract.*` → you are a **Conformist**, whether you meant to be or not.
  - Only `acme.orders.infrastructure` imports it → you have an **ACL**.
  - One grep tells you which relationship you actually have, versus the one on the whiteboard.
  - It can be made structural: promote `infrastructure` to its own project and it becomes the only project that depends on foreign contracts — Conformist-by-accident is then a compile error. Cost: one more manifest entry (the 18.5 trade-off applied to a different wall).

## 21.5 Integration Events

- **Scope split with Part 20.** Publishing is 20.4 (which events to raise, the dual-write problem, the outbox); handler mechanics are 20.3 (idempotency, retries, dead-lettering). This section is about the *consumer's* strategic choice: whether a foreign event reaches your model as-is or translated, and what the schema contract between contexts has to guarantee.
- The instinct is to consume events as-is because each one looks small. **Argue the other way** — the case for translation is *stronger* for events than for RPC:
  1. **Zero say over granularity.** With RPC you at least choose which call to make; with events you get whatever they publish, at whatever grain they chose.
  2. **Already translated once — for the publisher.** Their domain event → their integration event was optimized for their model, not for any consumer.
  3. **The ACL is nearly free.** Events land in a subscriber adapter you had to write anyway; the translation is a function in code that already exists.
- **The recommendation:** translate at the subscriber into your own command or your own event type, before it reaches the application layer. Handlers take `MarkBuyerDelinquent`, not `CustomerDelinquencyChanged`. Then schema versions stay in the subscriber, two upstream events can collapse into one internal command, and handler tests need no foreign schema.
- **Consume as-is when** the event is a bare fact (`OrderCancelled { orderId }`) where translation is the identity function, or when you own both sides.
- **Notification vs event-carried state transfer.** A notification carries an ID and you call back for detail: low coupling, extra round trip. ECST carries the full payload: fast, works when upstream is down — but it bakes the upstream's shape into *your database*, which makes the ACL more important, not less, because the foreign shape is now durable.
- **Delivery concerns belong in the adapter** (mechanics in 20.3). The strategic point here: if an application handler contains "have I seen this ID before", a concern has leaked inward — and it leaked across a context boundary, which is worse.
- **Event schema evolution is stricter than RPC evolution.** No negotiation, and old events live in a log forever. Additive-only; version in the type name when you must break.
- **The subscription is still a compile-time dependency.** Even though the runtime arrow points from producer to consumer, the consumer `depends` on the producer's `contract`, and that edge counts in the workspace DAG.

## 21.6 Drawing and Maintaining the Context Map

- A context map is a picture of relationships, not of components. Each edge is labelled with a pattern from 21.3 and a direction (upstream/downstream).
- Keep it honest: the map is a claim about your code, and in Dovetail the code can be checked against it (21.4).
- When the map has an edge nobody can name, that is the finding.

## 21.7 Splitting and Merging Contexts

- **Splitting:** what Part 18's structure makes cheap — the consumer already depends only on a `contract`, so extracting a context is a deployment change plus a transport swap in one adapter.
- **Merging:** when two contexts have the same language, the same team, and always release together, the boundary is costing more than it returns.
- Migration shapes worth sketching: strangler-style extraction behind an ACL, and event-driven backfill.

---

## Summary (to write)

- Classify subdomains first — core/supporting/generic decides how much boundary discipline is worth buying.
- The `contract` project is your Open Host Service and Published Language; the ACL is how you consume someone else's.
- Default to a thin ACL, especially for anything touching the core domain, and especially for integration events.
- The relationship you actually have is visible in `Dovetail.toml` and in which package imports the foreign contract.
