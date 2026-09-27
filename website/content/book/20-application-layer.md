# Part 20: The Application Layer

> **Status: outline.** Bullets to be expanded into prose. Part 19 built the model;
> this part is about the layer that drives it — the use cases, and the three kinds
> of thing that trigger them. Part 21 zooms out to other contexts.

---

## 20.1 What a Use Case Owns

- A use case takes **one intent** — place this order, close this card — and owns **everything that intent entails**. That is a larger job than the usual formula admits, because only some of what an intent entails is a write to your own database.
- **The shape, from 18.3: load, decide, persist, announce.** The decision is a domain call and is one line; everything around it is plumbing.
- **It holds no business rules.** The review test: an `if` in an application service is either a guard (fine), a mapping (fine), or a business rule (a bug — it belongs on the aggregate).
- **The transaction is a tool, not the definition.** Where a consequence can be made atomic with the decision, make it atomic. Where it cannot — an HTTP call, another context, a broker — it does not stop being yours, and 20.6 is how you discharge it.
- The application layer is where the **unit of work** begins and ends. Who owns the transaction — the repository, an explicit `Transaction` port, or the adapter — and why the domain must not know about it.
- **The only IO inside a database transaction is database IO.** No HTTP calls, no queue publishes, no calls into another bounded context. Two independent reasons, and both bite in production:
  - **Transactions get retried.** At repeatable-read or serializable isolation, serialization failures are routine rather than exceptional, and the documented correct response is to retry the whole transaction. Every external call inside it is then re-issued — once per attempt.
  - **A commit can fail after the call succeeded**, and you cannot un-send an HTTP request. The card is closed at the network and open in your database.
  - This is the sandwich again, stated for storage: the transaction is part of the bottom slice, and it writes to exactly one place.

### The "one change, one transaction, one aggregate" rules

You will meet these everywhere, usually stated as laws:

> One use case, one transaction. One transaction, one aggregate. One change at a time.

As **aspirations** they are sound: they push you to notice coupling and to ask whether two things really are separate. As **laws** they are wrong, and each one fails on the first example anybody reaches for.

**"One transaction, one aggregate"** fails on a transfer between two accounts. It touches two `Account` aggregates *and* writes a `JournalEntry` — its own aggregate, because an account cannot carry its entire history — and all three must land together or none of them. Turning that into a saga, with compensating actions and an intermediate state where the money is in neither account, is a great deal of machinery to avoid a database transaction that would have done it correctly. **Atomicity is a storage decision, not a modelling one.** Write as many aggregates in one transaction as correctness requires and your store can give you. What 19.6's third rule forbids is stricter and different: not "two aggregates in one transaction", but "two aggregates that cannot be reasoned about separately".

**"One change"** fails because a single intent routinely has several consequences, and only some of them are writes you control. Closing a card updates a row, tells the card network, notifies the customer, and may release a spending limit held in another context. Insisting that the use case does one thing does not make the other three disappear — it means nobody has written down whose job they are.

What survives is the pair of questions underneath: *does this really have to be atomic?* and *is this second thing genuinely mine?* Both deserve an honest answer, and sometimes the honest answer is yes.

### Consequences: notification or obligation

When an intent has a consequence that cannot be in the transaction, there are two genuinely different situations. Conflating them is where systems quietly go wrong.

**A notification.** *"This happened; react if it concerns you."* The consequence belongs to whoever is listening. Publish an integration event (20.5) and you are finished: if a subscriber fails, that is the subscriber's problem to retry, and your use case was correct the moment it committed.

**An obligation.** *"This must also happen, and it is my job to see that it does."* Closing a card at the card network is not a courtesy extended to a listener — the card is not closed until the network says so. If nothing ever makes that call, your context is **wrong**, not merely unobserved.

The test is one question: **if this never happens, whose bug is it?** If it is yours, it is an obligation, and publishing an event does not discharge it. It only makes the failure harder to see, because the outstanding work is now invisible in a broker instead of pending in your own state, where you could have found it.

This is worth being deliberate about, because the prevailing fashion pushes everything toward notification — it makes each service look simple in isolation. The cost arrives later and lands somewhere else: causality becomes something you reconstruct from logs, and a step nobody owns is a step nobody is paged for. **When a consequence is an obligation, keep it.** 20.6 is how you make one you own survive a crash.

### The sandwich: IO at the edges

That shape has a name — **impure, pure, impure** — and it is the target for every use case in the layer:

```
    load       impure   queries, the clock, a call to another context
    decide      pure    the domain layer (Part 19)
    save       impure   persist, publish, commit
```

- **Only the middle has business rules**, and it needs no adapters to test — it is the domain layer, so 19.11's tests already cover it.
- **The edges have nothing worth testing.** They fetch and they write. No branching, no rules.
- **This is what makes "pass the fact, not the port" (19.8) practical.** The top slice is where facts get gathered, so the domain never needs a port to go looking. The two ideas are the same idea from different ends: the domain refuses to do IO, and the sandwich is the shape that lets it.
- **The transaction wraps the bottom slice** — or the whole thing, if a read has to be consistent with the write.

A use case that reads as one sandwich is almost certainly right. When one does not, that is information.

### When the sandwich does not fit: the layered cake

Sometimes you cannot gather everything up front, because **what to load depends on what the model decided**. Placing an order picks a fulfilment centre; only then do you know whose stock to check. Pricing picks a tax regime; only then do you know which rate table to read.

```
    load       impure
    decide      pure
    load       impure   ← could not have known to fetch this any earlier
    decide      pure
    save       impure
```

That is a **layered cake**, and it is normal — not a design failure, and not something to contort the model to avoid. What matters is that the layers stay honest:

- **Each pure slice stays pure.** No port sneaks into the domain because there are now two calls instead of one. 19.8 does not relax.
- **Each impure slice stays dumb.** It fetches what it was told to fetch. If choosing *what* to fetch involves an `if` about the business, that `if` is a rule and belongs in the slice above it.

Three ways to keep the cake thin, in order of preference:

1. **Over-fetch and stay a sandwich.** Load everything the use case might need up front. Costs a sometimes-wasted query; buys a shape anyone can read at a glance. For a two-branch case this is usually the right trade, and people reach for it far too rarely.
2. **Accept two layers.** Entirely fine. Name the intermediate value after the decision that produced it — `chosenWarehouse`, not `data2` — so the alternation stays legible.
3. **Let the model ask.** The domain function returns what it needs as a **value** — an enum of required facts — and the application fetches it and re-enters. The branching stays in the domain where it can be tested purely; the application becomes a small loop that services requests. Costs a protocol, buys the purity back.

**The warning sign is height.** A five-layer cake means the *sequence itself* has become business logic, and it now lives in the application layer — where it cannot be unit-tested without adapters and cannot be pointed at (19.1). That is the moment for (3), or for asking whether this use case is really a saga (20.6).

**And note what every layer above quietly assumes: that the process survives to the end.** Sandwich and cake alike are shapes for a single run. If the machine dies between two impure slices, everything after the last commit simply did not happen — and if the model already decided something that the outside world was told about, or was about to be told about, you are now inconsistent with it. That is a different problem, it has its own name, and it is 20.6.

## 20.2 Commands, Queries, and Results

- **Commands are types, not parameter lists.** `PlaceOrder` as a record beats six positional arguments: named, validated, loggable, and replayable.
- Where the command type lives: `application`, built by the driving adapter from a contract DTO. The DTO is the wire; the command is the intent. They diverge sooner than you expect.
- **Validation happens twice, and that is correct.** The adapter checks shape ("is this valid JSON with a non-empty id"); the dovetail checks rules ("can this order be placed"). Neither can do the other's job.
- **Never return an aggregate from a use case.** Return the ID, or a small result type. An aggregate handed to a caller is an aggregate mutated outside a transaction.
- **Queries are not use cases.** Read models bypass the domain and return contract types (18.6). Do not route a screen through an aggregate.
- **Error mapping is a three-hop chain,** and each hop belongs to a different layer:
  - domain error (`OrderError.NotDraft`) — the rule that refused
  - application error (`PlaceOrderError.Rejected`) — plus the failures only the use case can have: not found, storage, upstream
  - contract error (`OrderServiceError`) — wire-stable, what a caller can act on
  - Collapsing these is tempting and costs you the ability to change any of the three independently.

### Errors from the domain

The first hop is the one you write dozens of times, so it is worth having an idiom for.

An aggregate refuses with `Result<T, OrderError>`. A use case returns `Async<T, PlaceOrderError>`. Those error types are deliberately different, because **the use case can fail in ways the domain cannot**: the order was not found, the database was unreachable, another context said no. The application error is the wider type, and the usual shape is an enum that wraps the domain's and adds its own:

```dovetail
public enum PlaceOrderError =
    Rejected(OrderError)      // the model said no
    NotFound
    Storage(RepositoryError)
    Upstream(LookupError)
```

`Rejected(OrderError)` rather than a flattened copy of the domain's variants: the domain owns that list, it will grow (19.12), and copying it means editing two enums forever.

**Write the conversion once, as a `From` implementation:**

```dovetail
implement From<OrderError> for PlaceOrderError =
    public function from(value: OrderError): PlaceOrderError =
        PlaceOrderError.Rejected(value)
```

**Then use it as a method reference at the call site:**

```dovetail
async function handle(self, command: PlaceOrder): Async<OrderId, PlaceOrderError> =
    let order: Order = ...
    let (placed, event) = order.place(now, creditApproved)
        .mapError(PlaceOrderError.from)
        .orReturn
    ...
```

`.mapError(PlaceOrderError.from).orReturn` is the idiom. It reads as one hop, the conversion lives in exactly one place, and adding a variant to `OrderError` does not touch any call site.

**Where the conversion is automatic, and where it is not.** This is worth knowing precisely, because the two look similar and behave differently:

- **At a `use` site**, the compiler inserts the conversion for you. `use fileResource` inside a function returning `Async<_, AppError>` works when `From<FileError> for AppError` exists — no `.mapError` at all (Part 13).
- **At a `try`/`orReturn` site**, it does not. The lift that exists in the standard library carries the error type through unchanged (`Result<T, E>` into `Async<T, E>`), so `orReturn` only works directly when the two error types already agree. Crossing from `OrderError` to `PlaceOrderError` needs the explicit `.mapError`.

That asymmetry is a wrinkle in the language rather than a rule with a reason behind it — but it is what compiles today, so write the `.mapError`. The `From` implementation is still worth defining: it is what makes `use` sites clean, and it gives the explicit hop a name instead of an inline lambda repeated at twenty call sites.

**The third hop happens at the driving adapter** (18.3), not here. The use case returns `PlaceOrderError`; the HTTP handler turns it into a status code and a contract error. That keeps wire compatibility a concern of the layer that owns the wire.


## 20.3 The Three Triggers

- A request, an event, and a clock. **Same shape, different door** — which is the point of the layer: one use case, reachable three ways.

### Requests

- Driving adapters (18.3) decode, build a command, call the service, encode the result. No logic in between.

### Event handlers

- **Two kinds, and they are not the same pattern:**
  - **Internal** — a domain event from this context (19.7) drives a second aggregate. Reach for this when the second change may genuinely lag — not to avoid writing two aggregates in one transaction (20.1), but because they are not required to be correct at the same instant.
  - **External** — an integration event from another context, translated at the subscriber into *your* command before it reaches the handler (Part 21.5). The handler never sees a foreign type.
- **Idempotency is the handler's defining constraint.** At-least-once delivery is the norm, so a handler must be safe to run twice. Where the dedup lives — the adapter, a processed-events table, or a naturally idempotent operation — and why "naturally idempotent" is the one worth designing for.
- Failure handling: retry, dead-letter, and the fact that a handler cannot signal failure back to the publisher.

### Jobs

- Scheduled or periodic work: expiry sweeps, reconciliation, reminders, retries of an outbox.
- **The clock is a port.** A job that reads the wall clock directly is untestable; inject it and time becomes a value you control.
- **Commit per item, not per batch.** Iterate and commit each item's work as it goes, rather than widening one transaction over the whole sweep. Consequences: partial progress, resumability, and why that is better than a batch that half-fails.
- A job is a use case with a timer for a caller — so it holds no more logic than one, and should usually call the same application service a request would.

## 20.4 Repositories

- **Why the port lives here and not in the domain layer.** Evans places the repository interface in the domain layer; Part 18 places it in `application`. That is forced by a language fact, and the result is the more honest boundary:
  - A repository signature returns `Async`. `Async` is an IO concept, and the domain layer has none — in the four-project layout (18.5) putting it there is a compile error, not a style debate.
  - Evans' placement is a consequence of Java and C# being unable to say "returns a value" versus "performs an effect" in a type. There was no seam there to land on. Dovetail has one, so the split falls where the effects actually change.
  - What stays in `domain` is what is genuinely domain: the aggregate, its invariants, its events (Part 19). What lives here is a *persistence* concept expressed in domain terms. The aggregate does not know it is stored, and now it cannot pretend otherwise.
- **One repository per aggregate root** (19.6). No repository for a part of an aggregate — that is a hole in the boundary.
- Repositories are a *collection* illusion: `findById`, `save`, and a small number of domain-meaningful finders. Not a query language.
- **Methods speak aggregates**, never rows and never DTOs. A method returning `OrderSummaryDto` is a query service, not a repository — give it its own port (18.6, 20.2).
- **Reconstitution.** The adapter must rebuild an aggregate without re-running its creation rules; the options and their costs are in 19.9. The choice is the adapter's to consume but the domain's to offer.
- **The `version` check on save** is where optimistic concurrency actually happens — a storage concern the domain layer knows nothing about. A save that finds a changed version fails the use case, and the caller retries the whole thing — not the write.

## 20.5 Publishing Integration Events

- The application layer decides which domain events become integration events (19.7) and publishes them through a port; infrastructure delivers them. (The *consumer's* side of the same wire — translate or conform — is 21.5.)
- **The dual-write problem:** you cannot atomically commit the database and publish to a broker. Say this plainly rather than letting readers discover it in production.
- **The transactional outbox** as the standard answer: write the event to the same database, in the same transaction; a relay publishes it after. Where each piece lives in the layout of Part 18.
- Translation direction: domain event → integration event is a *narrowing*, chosen deliberately. Not everything the model notices is anyone else's business.

## 20.6 Sagas, Compensation, and Durable Execution

For work that spans several aggregates, several contexts, or a long stretch of time — where one transaction is impossible and a distributed one would be worse.

### Two shapes, and the word only covers one of them

The [1987 Sagas paper](https://www.cs.cornell.edu/andru/cs711/2002fa/reading/sagas.pdf) describes two recovery strategies. In current usage "saga" almost always means the first, which is a shame, because the second is the one most systems actually need.

- **Backward recovery.** Each step has a **compensating action**; on failure you unwind. Compensation is a business decision — refund, cancel, apologise — not a rollback, which is exactly why no framework can generate one for you.
- **Forward recovery.** The workflow is checkpointed and driven relentlessly to completion. On interruption it resumes from the last **save point** rather than unwinding. Today this generally goes by the name **durable execution**.

Which you need is a question about the work, not about taste. **If a step cannot be undone — money moved, an email sent, a card closed at the card network — forward recovery is the only honest answer**, because there is no compensation to write.

### The problem durable execution solves

This is the failure you will actually hit, and 20.1 has already ruled out the easy way round it.

The model decides something, you commit it, and now you must tell an external service or another bounded context:

- Put the external call **inside** the transaction and a retry re-issues it — and serialization retries are routine (20.1, 20.4). Retry the transaction a hundred times and you have closed the card a hundred times.
- Put it **after** the commit, which 20.1 says you must, and a crash in the gap leaves your database saying *closed* and the other system saying *open*.

There is no arrangement of one process and one transaction that removes this. **The commit and the call cannot be made atomic**, so the only remaining move is to make the pair *recoverable*: record that the call still has to happen, durably, in the same transaction as the decision — and then make sure something eventually performs it.

### What a durable runtime gives you, and what it demands back

- **Save points** after each step, and resumption from the last one.
- Because a crash can land between "the step finished" and "the save point was written", steps are **at-least-once**. So **every step must be idempotent** — the same requirement an event handler owes you (20.3), generalized to the whole workflow. For an external call that usually means an idempotency key the far side honours.
- **Deterministic replay.** Any value the workflow generates — a UUID, a timestamp, a random choice — must be generated once and persisted, so a resumed run sees the same value. This is what makes an idempotency key survive a restart: a freshly generated key on the second attempt is not an idempotency key at all, it is a second request.

**Part 19 already did the hardest part of this.** Aggregates that take `now` and their identifiers as parameters rather than reading a clock or a generator (19.4, 19.8) are replayable by construction: feed the same inputs and you get the same decision. A domain layer that read the clock itself could not be replayed deterministically at all. Whatever you build on top, that discipline is the prerequisite — and it is one more reason the rule was worth insisting on.

### Dovetail has no durable execution runtime today

Say this plainly rather than implying otherwise: there is no `Saga` type, no save-point store, no workflow coordinator in the language or its standard library. What you do in the meantime, cheapest first:

1. **Reach for the outbox (20.5) first.** It *is* durable execution, for exactly one step: the publish. If the only thing that must follow the commit is "tell someone", the outbox is the entire answer and you need none of the machinery below. This covers far more cases than people expect, because "call the other context" can usually be turned into "publish a fact and let them react".
2. **For a genuine multi-step workflow, hand-roll forward recovery.** Persist the workflow's state as its own aggregate, advance it one step per transaction, and drive it with a job (20.3) that picks up unfinished work. Generate idempotency keys **at the moment the workflow state is written**, not at the moment of the call, so a resumed run reuses them. This is well-trodden and a few hundred lines; it is not research.
3. **Or delegate to an external coordinator** — Temporal, Restate, and similar — with your Dovetail services as the workers. You buy save points, retries, and visibility, and you pay in an extra piece of infrastructure and a programming model that wants to own your control flow.

This is the honest extension of the layered cake (20.1): the cake alternates pure and impure slices within one run, and durable execution is what you need when the run itself may not survive to the end.

*(An aside, not a plan: `Async` in Dovetail is a reified tree of effects rather than an opaque continuation, which happens to be the shape a checkpointing runtime needs. Whether that ever becomes durable execution in the language is an open question.)*

### Structuring one, whichever recovery you use

- Where it lives: `application`, since it orchestrates rather than decides. Its persisted state is its own aggregate — a workflow has an identity and a lifecycle like anything else, and 19.2 applies to its states as much as to an order's.
- **Choreography vs orchestration.** Choreography (each context reacts to events) is loosely coupled and very hard to see; orchestration (one coordinator drives) is visible and centralizing. Prefer orchestration past about three steps, purely because you can debug it.
- **Timeouts are first-class.** Most saga bugs are the branch nobody wrote for "the reply never came."

## 20.7 Cross-Cutting Concerns

- **Authorization belongs here.** The use case is the permission unit — "may this actor place this order" — not the entity and not the HTTP route. The actor arrives as part of the command, not from an ambient context.
- **Logging and tracing** wrap the use case; the domain layer stays silent. A span per use case is the natural granularity.
- **Retries and timeouts** belong to the adapter, not the service. A use case that retries is a use case whose transaction boundary is unclear.
- The general rule: if a concern is about *how the program runs* rather than *what the business decided*, it wraps the use case rather than living inside it.

## 20.8 Testing the Application Layer

- In-memory adapters, not mocks: ports are [interfaces](06-type-system.md#611-interfaces-and-interface-types), so a fake repository backed by a `List` is a few lines (18.10, 20.4).
- What these tests assert is **orchestration**, not rules: was the aggregate saved, was the event published, was the right error returned. The rule itself is already covered by a dovetail test.
- **Test the handler's idempotency explicitly** — deliver the same event twice and assert the second is a no-op. This is the bug that reaches production otherwise.
- A controlled clock makes job tests deterministic and fast.

## 20.9 Worked Example

- Continue Part 19's `Order`: the `PlaceOrder` use case end to end, then the same aggregate driven by an event handler and by an expiry job — showing the three triggers over one unchanged domain model.
- Then a saga: place order → reserve stock → take payment, with compensation on each failure branch.

---

## Summary (to write)

- A use case owns **one intent and all of its consequences**, and holds no business rules. "One change, one transaction, one aggregate" are useful questions and bad laws.
- A consequence outside the transaction is a **notification** (someone else's to react to) or an **obligation** (yours to complete). If its never happening is your bug, an event does not discharge it.
- Commands are types; aggregates never leave the layer; errors are mapped in three hops.
- **Use cases are sandwiches:** IO at the edges, decisions in the pure middle. That shape is the other half of "pass the fact, not the port".
- When you cannot gather everything up front, a **layered cake** is honest — but height is a warning that the sequence has become a business rule.
- Requests, events, and jobs are three doors to the same use case — handlers additionally owe you idempotency.
- Dual writes are a real problem with a boring solution: the outbox.
- Sagas coordinate what a transaction cannot. **Backward recovery** compensates and unwinds; **forward recovery** (durable execution) checkpoints and completes — and it is the only option once a step cannot be undone.
- Dovetail has **no durable execution runtime today**. The outbox covers the one-step case; beyond that you hand-roll forward recovery or delegate to a coordinator. Part 19's rule that clocks and IDs are parameters is what makes any of it replayable.
