# Part 19: The Domain Layer

Part 18 built the walls. This part is about what goes inside the innermost one: the model — the types that carry your business rules.

Everything here lives in the `domain` package (or the `orders-domain` project, if you took the four-project layout of 18.5). That means everything here is **pure**: no `Async`, no `await`, no `use`, no repository, no logging. A domain function takes values and returns values. If it cannot decide with what it was handed, it says so by returning an error.

Part 20 covers the layer that drives this one — use cases, event handlers, jobs, and the repository ports that load and store what you build here. Part 21 zooms out to other contexts.

> **The one idea:** model with types until most of your rules *cannot* be broken, then give the rules that remain exactly one home.

Those two halves are in that order deliberately. A rule enforced by a type is checked everywhere, by the compiler, forever. A rule enforced by a function is checked wherever someone remembers to call it. Dovetail has a strong type system; the whole point of this chapter is to spend it.

---

## 19.1 Start With the Types

Dovetail separates a type's **declaration** from its **behavior**: a record says what the shape is, and a `module` of the same name says what it does. Lean into that. Put the entire model's vocabulary in one file, and the behavior in a file per concept:

```
orders-dovetail/src/
├── types.dove        every record, enum, and newtype in the model
├── Order.dove        module Order — the aggregate's behavior
├── Shipment.dove     module Shipment — a second aggregate
└── policy.dove       domain services and policies
```

This is the first recommendation in the chapter because it is the one that changes how you work, not just how your files are arranged.

It does assume the **functional style** of 19.5, which is what the rest of this chapter is written in. The layout works because a record separates shape from behavior, so the shapes can be collected without dragging the functions along. A class does not separate them, and is not meant to — see 19.5 for what an object-oriented model organizes by instead.

### Why one types file

**Because the types file *is* the model.** Everything else in the domain layer is consequence. When the shapes are collected in one place, you can read the entire model in one sitting — states, parts, errors, events — and that turns it into an artifact you can actually use: something to review, to walk through with a domain expert, to hand a new joiner, and to diff when the business changes. A model change *should* be a visible edit in one file, not a shift scattered across twelve.

**Because it is where the modelling happens.** The habit this layout encourages is to write the types first and the behavior second — to spend your thinking on what states exist and what each one carries, before writing a single function. That is the work. 19.2 is entirely about doing it well, and it is much easier to do when you are looking at the whole vocabulary at once rather than at one record in one file.

**Because mutual references are free.** All files in a package are typechecked together, so `Order` may mention `OrderLine`, `OrderStatus`, and `OrderPlaced` in any order, with no forward declarations and no dependency ordering to maintain. There is nothing pushing you to split.

**Because the behavior files stay about behavior.** `Order.dove` reads as a list of the things an order can do, uninterrupted by field declarations.

---

## 19.2 Make Illegal States Unrepresentable

This is the section the rest of the chapter rests on.

Most "business rules" are not really rules. They are statements about which combinations of data make sense — and a statement about which combinations make sense is a **type**. Every one you can express as a type is a rule that cannot be violated, cannot be forgotten at a call site, and needs no test.

### The model most people write first

```dovetail
public enum OrderStatus =
    Draft
    Placed
    Cancelled

public record Order =
    id: OrderId
    customerId: CustomerId
    status: OrderStatus
    lines: List<OrderLine>
    placedAt: Option<Instant>
    total: Option<Money>
    cancelledAt: Option<Instant>
    cancellationReason: Option<CancellationReason>
```

This looks reasonable, and it is what a database table would suggest. Count what it permits: three statuses times four independent optional fields is **48 representable combinations**, of which perhaps three make sense. The rest are all constructible, and the compiler is happy with every one of them:

- a `Draft` with a `placedAt`
- a `Placed` order with no `total`
- an order that is `Cancelled` but has a `placedAt` and no `cancelledAt`
- an order with both a cancellation reason and a `Draft` status

Worse than the nonsense states is what they do to every function you write afterwards. `total` is an `Option<Money>` in all three states, so *every* caller must handle `None` — including callers that only ever see placed orders, where `None` is impossible. You end up with `.require` sprinkled through the model, or a defaulted zero that quietly becomes a wrong invoice.

The status enum here is not modelling anything. It is a label sitting next to the data, and keeping them consistent is now your job, forever, in every function.

### The model to write instead

Put the data **inside the state that gives it meaning**:

```dovetail
public record Placement =
    placedAt: Instant
    total: Money

public record Cancellation =
    cancelledAt: Instant
    reason: CancellationReason

public enum OrderStatus =
    Draft
    Placed(Placement)
    Cancelled(Cancellation)

public record Order private =
    id: OrderId
    customerId: CustomerId
    status: OrderStatus
    lines: List<OrderLine>
```

Now there are **exactly three representable states**, and each carries precisely what that state means:

- A `Draft` has no placement. Not an empty one, not a `None` — there is no field to be wrong about.
- A `Placed` order *always* has a `placedAt` and a `total`. Both are plain values, not `Option`, because in this state their absence is not a possibility.
- A `Cancelled` order always has a reason.

The nonsense states are gone — not caught, *gone*. There is no code that rejects them because there is no way to write them down. Try to read a placement off a draft and the compiler stops you before you finish the thought:

```dovetail
match order.status with
    case OrderStatus.Draft(placement) => ...
```
```
error: variant 'OrderStatus.Draft' has no fields;
       use 'OrderStatus.Draft' without parentheses
```

And reading the value requires proving you are in the state that has it:

```dovetail
    public function placedAt(self): Option<Instant> =
        match self.status with
            case OrderStatus.Placed(placement) => Some(placement.placedAt)
            case _ => None
```

The `Option` appears exactly once, at the boundary where it is honest — asking a possibly-unplaced order when it was placed — instead of infecting the four fields underneath.

### Naming the payload, or inlining it

A variant can carry its fields directly, with no separate record:

```dovetail
public enum OrderStatus =
    Draft
    Placed { placedAt: Instant, total: Money }
    Cancelled { cancelledAt: Instant, reason: CancellationReason }
```

built with `OrderStatus.Placed { placedAt = now; total = total }` and read by naming the fields in the pattern:

```dovetail
    public function placedAt(self): Option<Instant> =
        match self.status with
            case OrderStatus.Placed { placedAt = at; total = _ } => Some(at)
            case _ => None
```

Both forms model exactly the same three states — this is a question of whether the payload deserves a name, not of how much the type can prove.

**Giving the payload a name preserves the brace syntax.** With the named declaration `Placed(Placement)`, the same construction and field pattern still work:

```dovetail
let status = OrderStatus.Placed { placedAt = now; total = total }

match status with
    case OrderStatus.Placed { placedAt = at; total = _ } => Some(at)
    case _ => None
```

The construction means `OrderStatus.Placed(Placement { placedAt = now; total = total })`. The pattern means `OrderStatus.Placed(Placement { placedAt = at, total = _ })`. This lets you extract an inline payload into a named record without rewriting callers that construct or inspect its fields.

When a caller needs the concept as a whole, the positional form binds the actual record:

```dovetail
match status with
    case OrderStatus.Placed(placement) => Some(placement)
    case _ => None
```

Both forms remain available. Braces inspect the record's fields; parentheses bind or pass the `Placement` value. The shorthand applies to exactly one record payload, including a generic record whose type is known. It does not unwrap newtypes, classes, or multiple positional payloads. Patterns may omit fields, so `case OrderStatus.Placed { placedAt = at }` is sufficient when only the timestamp matters; there is no need for a rest marker.

**Inline** when the fields only ever exist as part of that state. It is two fewer types in `types.dove`, and the enum reads as the whole story of the state machine.

**A named record** when the payload is a concept in its own right — something you pass to a function, return, store, or want to say in a signature. `Placement` is one: 19.7 builds it as a value before putting the order into the state that holds it, and a function can take a `Placement` without taking an `Order`. Once you find yourself writing the same field list in two places, it has already earned the name.

Apply that test **per variant, not per enum** — one state's payload earning a name says nothing about its neighbours'. `OrderStatus` in 19.12 ends up mixed for exactly this reason: `AwaitingApproval(Money)` carries a bare total that means nothing outside the state, while `Approved(Approval)` names a payload the approval rules pass around. The rest of this chapter uses named payloads because `Placement` and `Cancellation` both earn one.

### Rules of thumb

**A field that only means something in one state belongs inside that state.** This is the whole technique, and `placedAt` is the archetype: it exists once an order is placed, and asking for it before then is not a missing value, it is a meaningless question.

**`Option` means "optional in every state", not "filled in later".** If a field becomes populated at a transition, it belongs to the state the transition produces. A middle name is genuinely optional; a shipment date is a state.

**Two booleans that cannot both be true are an enum.** `isPlaced` and `isCancelled` gives you four combinations for three states, and one of them is a bug you will eventually ship.

**A collection with a minimum size is not a plain list.** If a placed order must have at least one line, the honest type says so; `List<OrderLine>` cannot. So the rule ends up in `place` as a check that can refuse (19.7) — which is fine, but notice it is the type system telling you where a rule leaked out into a function.

### Errors are types too

The same discipline applies to refusals. An error enum whose variants are bare names throws away everything the code knew at the moment it refused:

```dovetail
public enum OrderError =
    NoLines
    NotDraft(OrderStatus)
    TooManyLines(Int32)
    MixedCurrency(Currency, Currency)
    CreditRefused
    ApprovalRequired(Money)
```

`NotDraft(OrderStatus)` carries what the order *actually* was, so the caller can say "this order was cancelled on the 3rd" instead of "not a draft". `MixedCurrency(Currency, Currency)` names both offenders. `ApprovalRequired(Money)` reports the total that tripped the threshold, so the message writes itself and the application layer can route the approval without going back to look.

The rule: **an error should carry the evidence for itself.** If reporting it well requires re-deriving what happened, the variant is missing a field.

### The compiler as reviewer

The payoff arrives on the day the business adds a state. Add `Refunded(Refund)` to `OrderStatus` and every exhaustive `match` in the codebase stops compiling — each one a place where somebody has to decide what a refunded order means. That is a review checklist generated by the compiler, complete and free.

This is why a bare `case _ =>` catch-all is worth being suspicious of in the domain layer. It buys you a little brevity today and costs you exactly the compile errors you would have wanted tomorrow. Match the states explicitly, and let the additions find you.

### How far to take it

You can go further. **Typestate** gives each state its own type — `DraftOrder`, `PlacedOrder` — so that `place` only exists on a draft and calling it twice is not a runtime error but a nonexistent function.

That is genuinely stronger, and for most models it is too much. You get a type per state, conversions at every boundary, and a repository that has to return a sum type anyway, since a row's state is not known until it is read. The enum-with-payloads is the sweet spot: it eliminates the illegal *combinations*, which is where the bugs actually live, while keeping one type you can load, store, and pass around.

Reach for typestate when a wrong transition is catastrophic rather than merely wrong — a payment capture, a compliance hold, a released deployment — and accept the ceremony for those.

---

## 19.3 Value Objects

A **value object** is defined by its value, not by identity. Two amounts of 12.50 USD are the same amount; there is no "which one." Value objects have no lifecycle, and they never change.

### Where they live

**Default to the domain package.** Most value objects are nobody else's business — an `OrderLine`, a `DiscountBand`, a `PickingPriority`. They go in `types.dove` with everything else.

**Promote one to `primitives` when a second layer needs to speak it** — in practice, when it appears on the wire. That is what `primitives` is for (18.2): it exists so `contract` and `domain` can share a vocabulary without either depending on the other. A type only the model uses creates no such pressure, so putting it there buys nothing and widens a surface other code compiles against.

`OrderId`, `Sku`, `Money`, and `Quantity` are in `primitives` below because the contract carries them. Had they been internal to the model, they would live in the domain package and nothing else would change.

That is the same rule at every boundary: **put a type at the innermost place that needs it, and promote only when a second consumer appears.** Domain-only stays in `domain`; shared with the contract moves to `primitives`; shared with another context moves to the shared kernel (18.8). Promotion is cheap — move the file, change the `package` line — which is precisely why starting inward costs nothing.

### Make invalid values unconstructable

19.2 removed illegal *combinations*. The same idea one level down removes illegal *values*: a private newtype and a module function that can refuse construction.

```dovetail
package acme.orders.primitives

public newtype OrderId private = String

module OrderId =
    public function parse(raw: String): Result<OrderId, String> =
        if raw.isEmpty then
            Error("order id must not be empty")
        else
            Ok(OrderId(raw))

    public function text(self): String = self.value

public newtype Sku private = String

module Sku =
    public function parse(raw: String): Result<Sku, String> =
        if raw.length != 8 then
            Error("sku must be exactly 8 characters")
        else
            Ok(Sku(raw))

    public function text(self): String = self.value
```

`private` after the newtype name is what makes this real. Outside `module Sku` in the defining package, even elsewhere in that same package, `Sku("nope")` is a compile error — *cannot construct private newtype 'Sku' outside its associated module*. Only that module can read `.value` or unwrap a constructor pattern; callers use `text()`. Trait implementations and extensions must use these module functions too. The only public construction path here is `parse`, so **once you hold a `Sku` it is valid**, and no code downstream ever checks again.

### Push behavior onto the value

A value object with no methods is a tooltip. Behavior that belongs to the value goes on the value:

```dovetail
@derive(Equatable)
public enum Currency =
    Usd
    Eur

public enum MoneyError =
    CurrencyMismatch

public record Money =
    minorUnits: Int64
    currency: Currency

module Money =
    public function of(minorUnits: Int64, currency: Currency): Money =
        Money { minorUnits = minorUnits; currency = currency }

    public function add(self, other: Money): Result<Money, MoneyError> =
        if self.currency != other.currency then
            Error(MoneyError.CurrencyMismatch)
        else
            Ok(Money {
                minorUnits = self.minorUnits + other.minorUnits
                currency = self.currency
            })

    public function times(self, quantity: Quantity): Money =
        Money {
            minorUnits = self.minorUnits * quantity.count().toInt64()
            currency = self.currency
        }
```

`add` returns a `Result` because adding USD to EUR is not an arithmetic operation, it is a bug — and the type says so. When money is an `Int64`, every `+` in the codebase is a place where the currencies could have differed and nobody checked.

`minorUnits` is the other decision worth making once: money as a floating-point number is a rounding bug waiting for a large enough invoice.

### When to introduce one

**Default to yes.** A bare `String` or `Int32` in a domain signature is a smell — not an error, but worth a second look every time, because it is the shape a rule takes before anyone has noticed it is a rule. `Quantity` not `Int32`, because quantities are positive. `Sku` not `String`, because SKUs have a shape. `Money` not `Int64`, because 500 of what?

The immediate payoff is that argument-order mistakes stop compiling. Given `function priceFor(sku: Sku, quantity: Quantity): Money` you cannot swap the arguments, which you very much can when they are `String` and `Int32`.

**The delayed payoff is the bigger one, and it is why this is a smell rather than a preference.** The rule usually arrives *after* the field does. Nobody says "SKUs are eight characters" on day one; it turns up in the third sprint, in a bug report. If `Sku` is a `String`, that rule has nowhere to live, so it lands in whichever function noticed first — and then in a second one, written by someone who did not know about the first, and the two drift. If `Sku` is a type, there is exactly one place the rule can go, and the day it goes there every value in the system is already validated.

By then the primitive is also **too late to tighten**. `String` is in two hundred signatures and nothing distinguishes the ones that meant a SKU, so the change you want is not a change you can make. The type costs six lines on the day you have no rules to put in it, which is precisely the day it is cheap.

**The exceptions are narrow.** Inside a value object's own representation — `Money.minorUnits` is an `Int64`, and the wrapper is the whole point. And genuinely free text with no unit and no rule, such as a customer's note. Everything else earns a type.

Conventions: `parse` when construction can fail, `make` when it cannot, never `new` or `create`.

---

## 19.4 Entities

An **entity** is defined by identity over time. An order that has had ten lines added and one removed is still that order. Two orders with identical contents are still two different orders.

- **The identity is a value object** — `OrderId`, not `String` — so it cannot be confused with a `CustomerId`.
- **The ID comes from outside.** Generating it inside the entity means randomness or a clock, and both are IO. An adapter supplies it, so the domain stays pure and deterministic. The same goes for timestamps: `place` takes `now` as a parameter (19.8).
- **Every change goes through a function that can refuse.** No setters, and no assembling a new version by hand from outside the module.
- **An entity is not automatically an aggregate root.** `OrderLine` has identity within its order but no independent existence; only roots get repositories (19.6).

### Make the transition boundary enforceable

The recommended declaration in 19.2 is `public record Order private = ...`. The type and its fields remain visible, so a caller can read `order.status`, inspect `order.lines`, and match the record. Construction and `with` updates belong exclusively to `module Order` in the defining package. The type can stay in `types.dove` and its module in `Order.dove`; file placement does not change that ownership.

This makes the entity rule enforceable: callers must use `Order.draft`, `order.addLine(...)`, and `order.place(...)`. Writing `order with { status = OrderStatus.Placed(...) }` from application code is a compile error. Trait implementations and extensions must use those functions as well.

`OrderStatus` remains publicly constructible in this model. Building a status value does not let a caller install it in a private `Order`. If the status needs its own construction rules, declare `enum OrderStatus private = ...` and put its factories in `module OrderStatus`; even `module Order` must then call those factories. Enum patterns remain available either way. Private records and enums expose inspection; private newtypes also hide their wrapped value.

The same boundary applies to named record payload shorthand. If `Placement` has a private constructor, `OrderStatus.Placed { placedAt = now; total = total }` must be inside `module Placement`, because it constructs a `Placement`. `OrderStatus.Placed(placement)` can wrap an existing value without constructing another record. Either pattern form remains available for inspection. If `OrderStatus` also has private construction, its permission is checked independently: being inside one type's module does not grant construction access to the other.

---

## 19.5 Two Styles: Values or Objects

**Record fields cannot be reassigned.** Referenced arrays or classes can still
contain mutable state; record immutability is shallow. Classes can carry mutable state. So this is not a per-type decision — it is a choice about what the whole domain layer is written in:

- **Functional.** Aggregates are records. A rule is a function from a value to a new value.
- **Object-oriented.** Aggregates are classes. A rule is a method that changes the object in place.

Pick one for the entire layer. A model where half the aggregates are values and half are objects is the worst of both, because every reader has to check which kind they are holding.

**This chapter recommends the functional style**, and is written in it.

### The functional model

```dovetail
module Order =
    public function draft(id: OrderId, customerId: CustomerId): Order =
        Order {
            id = id
            customerId = customerId
            status = OrderStatus.Draft
            lines = List<OrderLine>.Nil
        }

    public function addLine(self, line: OrderLine): Result<Order, OrderError> =
        match self.status with
            case OrderStatus.Draft =>
                if self.lines.length >= maxLines then
                    Error(OrderError.TooManyLines(maxLines))
                else
                    Ok(self with { lines = self.lines.prepend(line) })
            case other => Error(OrderError.NotDraft(other))
```

A rule that refuses returns `Error`; a rule that succeeds returns the next `Order`. The `with` expression keeps that from being a wall of field copies.

Note the shape of that `match`: because the state machine is an enum (19.2), the guard *is* the state check, and the failure case gets the actual status to report. In a model with a bare status field this would be `if self.status != OrderStatus.Draft`, and the error would have nothing to say.

Why this is the better default in Dovetail:

- **It matches everything else about the language.** No exceptions, rules returning `Result`, a layer with no IO, modules that already separate shape from behavior. `Result<Order, OrderError>` is the same shape as every other fallible operation and composes with `andThen` and `orReturn` like they all do.
- **Immutable contents make sharing safe.** An `Order` made entirely of immutable
  values can be shared without another function changing it. An array or mutable
  class stored in a record can still be changed through an alias.
- **The previous value is still there** — for comparing before and after, auditing what changed, retrying from a known state, or replaying history. Event sourcing stops being an architecture and becomes the natural reading of the model.
- **The tests are stronger.** You can assert both that the operation produced what it should and that its input was left alone (19.11).

The cost, named honestly: an operation that both advances the aggregate and reports a fact must return both, as a tuple (19.7).

### The object-oriented model

```dovetail
public class Order private (
    public id: OrderId,
    public customerId: CustomerId,
    mutable status: OrderStatus,
    mutable lines: List<OrderLine>
) =
    public function draft(id: OrderId, customerId: CustomerId): Order =
        Order(id, customerId, OrderStatus.Draft, List<OrderLine>.Nil)

    public function addLine(self, line: OrderLine): Result<Unit, OrderError> =
        match self.status with
            case OrderStatus.Draft =>
                self.lines = self.lines.prepend(line)
                Ok(())
            case other => Error(OrderError.NotDraft(other))
```

Two details are worth pointing out: `private` on the constructor means only `Order.draft` can build one, and class members are private unless marked `public`, so `status` and `lines` are not readable from outside at all.

Everything in 19.2 applies unchanged here — the state machine is still an enum carrying its data. The style choice is about mutation, not about how well you model.

### It changes the file layout

**19.1's `types.dove` does not apply to this style, and should not.** A class declares its fields and its methods in one body: data and behavior travel together, which is the entire point of reaching for a class. So an object-oriented model organizes **one file per aggregate** — `Order.dove` holds the `Order` class whole — with the value objects, enums, and errors it coordinates in a shared `types.dove` alongside it, since those are still records and enums.

```
orders-dovetail/src/
├── Order.dove        class Order — fields and behavior together
├── Shipment.dove     class Shipment
├── types.dove        value objects, state enums, errors, events
└── policy.dove       domain services
```

Be honest about what that costs. 19.1's real argument was not tidiness, it was that one readable file *is* the model — something to review, to walk through with a domain expert, to diff when the business changes. Split across aggregate files, the model stops being a single artifact and you get it back only by reading several files at once. The state enums and value objects still collect in `types.dove`, so a good deal survives; the aggregate shapes do not.

Do not try to recover the split with an extension or a module hanging off the class. Even where the language allows it, it is two patterns fighting: a class that has handed its behavior to somewhere else is a record wearing a class's syntax, and you would be better off with the functional style.

### Choosing

| | Functional (record) | Object-oriented (class) |
|---|---|---|
| Mutation | none — produce the next value | in place |
| Aliasing bugs | impossible | possible; a reference handed out can change |
| Updating | `self with { status = ... }` | `self.status = ...` |
| State *and* an event | returns a tuple | event only; state already changed |
| Event sourcing, audit, replay | natural | awkward |
| File layout | `types.dove` + a file per concept (19.1) | a file per aggregate; shape and behavior together |
| Fits Dovetail's `Result`-based, IO-free layer | closely | fine |
| Reads like the blue book | takes adjustment | yes |

**The recommendation: the functional style**, because it agrees with the rest of the language. Choose the object-oriented style if your team thinks natively in objects, or if you are porting an existing Java or C# model and want the shapes to line up. It is a coherent choice, not a lesser one. What you should not do is mix them.

---

## 19.6 Aggregates

An **aggregate** is a consistency boundary: a cluster of entities and value objects that must be correct *together*, with one entity — the **root** — as the only way in. `Order` is a root; `OrderLine` is inside its boundary and has no independent existence.

This is the most consequential modelling decision you make, because it decides what has to be correct together — and so what a unit of work looks like (20.1).

### The three rules

**1. Protect true invariants inside the boundary.** The test is temporal: if a rule must hold *at every instant*, everything it mentions belongs in one aggregate. If it may lag by a second without harm, it does not.

*"An order's total must equal the sum of its lines"* must never be observably false, so lines are inside `Order`. *"A customer's lifetime spend must reflect their orders"* can lag, so that is a handler (20.3), not an aggregate. And a *screen* that shows things together is not evidence of an invariant — that is a read model (20.2), and it can be assembled from anywhere.

**2. Reference other aggregates by identity.** An `Order` holds a `CustomerId`, never a `Customer`. Hold the object and three things follow: loading an order loads a customer which loads their orders; one aggregate's rules start reaching into another's state; and the boundary stops being visible in the code.

**3. No aggregate's invariants may depend on another's.** Each one has to be correct on its own, checkable from what it holds. If keeping A valid requires reading or changing B in the same breath, you have not found two aggregates — you have found one, drawn in the wrong place.

---

## 19.7 Domain Events

A **domain event** is a fact that has already happened, named in the past tense: `OrderPlaced`, `PaymentDeclined`. Not a command, not a request — a record of something that is now true.

```dovetail
public record OrderPlaced =
    orderId: OrderId
    customerId: CustomerId
    total: Money
    placedAt: Instant
```

### Return the event from the function that caused it

The temptation is an ambient event bus the aggregate can publish to. Resist it: publishing is IO, and this layer has none. The function that caused the fact returns it:

```dovetail
    public function place(
        self,
        now: Instant,
        creditApproved: Bool
    ): Result<(Order, OrderPlaced), OrderError> =
        match self.status with
            case OrderStatus.Draft =>
                if self.lines.isEmpty then
                    Error(OrderError.NoLines)
                else if !creditApproved then
                    Error(OrderError.CreditRefused)
                else
                    let total: Money = self.total()
                    let placement: Placement = Placement { placedAt = now; total = total }
                    let placed: Order = self with { status = OrderStatus.Placed(placement) }
                    let event: OrderPlaced = OrderPlaced {
                        orderId = self.id
                        customerId = self.customerId
                        total = total
                        placedAt = now
                    }
                    Ok((placed, event))
            case other => Error(OrderError.NotDraft(other))
```

The signature tells the whole story: *placing either fails for one of these reasons, or it succeeds, and here is both the order that resulted and the fact that is now true.* The caller decides what to do with the fact (20.5); the domain does not care whether it is published, logged, or dropped.

Two things in that body are worth calling out. The `Placement` is built once and stored *in the state*, so a placed order can never be missing its total — the invariant from 19.2 is maintained by construction rather than by a later check. And `now` is a parameter, not a clock reading, for the reason in 19.8.

This pair — **the new state and the event** — is the shape of every state-changing operation in the functional style. If an operation genuinely produces several events, return `Result<(Order, List<OrderEvent>), OrderError>`; the tuple already generalizes.

**The alternative** — carrying pending events on the aggregate and draining them in the application layer — handles multi-event operations more gracefully, at the cost of a field that is not really part of the aggregate's state and that someone must remember to drain. Prefer returning events.

### Domain events stop at the boundary

A domain event is phrased in domain types and **never leaves the context**. The application layer translates the ones that matter into *integration* events, phrased in contract types (18.9, 20.5, 21.5).

Conflating the two makes a model impossible to refactor: rename a field in your aggregate and you have broken a stranger's deployment. Keeping them separate costs one mapping function and buys the freedom to change your model whenever you understand it better — which is the entire point of having one.

Events carry value objects and IDs, never entities.

---

## 19.8 Domain Services

Some logic is genuinely about the domain but belongs to no single entity — because it spans two aggregates, because it is a policy about value types, or because the calculation is a concept in its own right. That is a **domain service**: a module of stateless functions in the domain package.

**A service is not a consolation prize.** The common advice is to suspect any service that reads only one aggregate, on the grounds that it is really a method with its receiver passed as an argument. Sometimes it is. Often it is not, and pricing is the archetype: a fee schedule has its own rules, changes on its own schedule, and may be swapped per tier, per contract, or per date. `Pricing.feeFor(order)` is not `Order.fee()` waiting to be discovered. An order is not the home of every rule that happens to read an order, and treating it as one is how an aggregate ends up owning half the model.

**The test is ownership, not argument shape.** Ask whether the rule is part of what the type *is*, or a policy applied to it. *"An order cannot be placed with no lines"* is the order's own business — it defines a valid order. *"This customer tier pays 1.5%"* is not; it is a policy that takes an order as input and would still make sense if the order type were replaced.

**The genuine failure is at the other extreme.** A domain layer that is all services and no methods is the anemic model (19.10) wearing a hat. If `Order` has no functions that can refuse, the rules that define an order have gone to live somewhere else.

### Pass the fact, not the port

Some rules need a fact the aggregate does not own. *"No two customers may share an email."* *"A customer may not have more than three open orders."* The aggregate cannot check either, and the obvious move is to hand it a repository so it can go and look.

Do not. **A domain function takes the answer to the question, never the means of asking it:**

```dovetail
    // Good: the fact is a parameter.
    public function place(self, now: Instant, creditApproved: Bool): Result<(Order, OrderPlaced), OrderError>

    // Bad: the domain now performs IO, and cannot be pure.
    public function place(self, credit: CreditService): Async<Result<OrderPlaced, OrderError>, ...>
```

The application service does the asking; the domain does the deciding. Three things follow:

- The domain stays **pure and total** — no `Async` in the layer, which is what makes the four-project layout of 18.5 possible.
- The test stays a one-liner. `order.place(noon(), false)` needs no fake credit service.
- The **cost becomes visible**. A lookup hidden inside a rule is a network call nobody budgeted for; a parameter forces the use case to fetch it deliberately, where a reader can see what it costs.

**The clock is the case you will hit first.** `Instant.now()` is IO, so `place` takes `now: Instant`. That reads like pedantry until the first time you test an expiry rule at an arbitrary date, or replay a day of events and get the right answers because time was an input rather than an ambient fact.

**On uniqueness specifically:** check-then-act races no matter which layer checks. Two requests can both find the email free. The guarantee is a unique constraint in the database; the domain-side check exists to produce a good error message in the common case. The domain layer is not where you solve concurrency, and pretending otherwise produces code that looks safe and is not.

---

## 19.9 Factories and Reconstitution

A **factory** is for construction complex enough to be its own concept, or where an invariant spans the whole assembly. In Dovetail that is a static function — `Order.draft`, `Money.of` — returning `Result` when it can refuse. There is no separate factory type unless the construction genuinely needs one.

### Reconstitution is a different operation

Creating an order and *rebuilding* one from a database row are not the same operation, though they end at the same type:

| | Creation | Reconstitution |
|---|---|---|
| Runs the creation rules | yes | **no** — they ran once, when it was created |
| Emits a creation event | yes | **no** — nothing happened |
| Assigns identity | yes | no — the identity is in the row |
| Valid starting states | `Draft` only | any state the order has reached |

`Order.draft` cannot rebuild a placed order — it hard-codes `OrderStatus.Draft`, and it should.

Modelling the states as an enum makes this notably easier: the adapter reads a status column plus its associated columns and builds the one `OrderStatus` variant they describe. If those columns disagree — a `placed` row with a null timestamp — the adapter has found corrupt data and must fail, which is exactly right and exactly where you want that check. In the `Option`-soup model of 19.2 that row would have loaded happily and failed somewhere else, much later.

Reconstitution needs a door of its own, since `Order.draft` is not it. Three options:

1. **A reconstitution constructor**, `public` and unmistakably named — `Order.fromStorage(...)`. Simple; the cost is a public door application code could also walk through, so naming carries the weight.
2. **A snapshot record** the aggregate exports and imports. The persistence-facing shape is explicit and the adapter's mapping is simpler; the cost is a second type to keep in step.
3. **Put the adapter's mapping inside `module Order`.** This grants construction access, but drags persistence vocabulary into the model. Moving the mapping to another function in the domain package does not grant private construction access.

Prefer (1), and (2) once the internal and stored shapes diverge. Whichever you choose, the function that actually reconstructs an `Order` lives in `module Order`. It must check the stored shape and invariants without repeating command preconditions or emitting a new creation event. A snapshot is data, not construction permission: decode the snapshot, then pass it to that module function. A generated decoder cannot bypass private construction either.

---

## 19.10 The Anemic Domain Model

The failure mode worth naming, because it is where most codebases end up without a fight:

- Entities are records of public fields with no behavior.
- All behavior lives in services that take those records as arguments.
- The domain layer is a namespace for data structures, and the rules are in the application layer — or in three places at once.

It happens because it is what ORMs teach, what DTO-mapping habits encourage, and what CRUD screens make look reasonable. It is not stupid; it is the path of least resistance.

**How to spot it in review:**

- Application services full of `if` statements about the business.
- Entities with setters and no function that can refuse.
- The same validation in the HTTP handler, the service, and the database.
- **A record whose fields are mostly `Option`** — the tell from 19.2, and usually the earliest one. Optional fields are states that were never modelled, and the rules keeping them consistent had to go somewhere else.
- **A model written in `String` and `Int32`** — the tell from 19.3. Rules attach to types; a layer with no types of its own has nowhere to put them.
- A rule you cannot point at.

**Why it costs:** the rules are still in your program, but duplicated, with no copy authoritative. Every change means finding all of them, and the compiler cannot help, because a missing rule is not a type error.

**The fix is directional, not a rewrite.** Each time you touch a rule, move it onto the type that owns the data it reads, and delete the other copies. Where the rule is really about which combinations are legal, fix the type instead and the rule disappears. A codebase converges surprisingly fast under that habit, and it is safe at every step — which a big-bang re-model is not.

---

## 19.11 Testing the Domain

Domain tests are the ones to have thousands of: they test the thing that matters and cost nothing to run — no database, no runtime, no `Async`, no fakes.

**Write them in the module, not in `test/`.** Domain tests belong at the bottom of `Order.dove`, under the functions they pin. 15.2 draws the line by what a test is for: `test/` is for exercising a package's public API from the outside, `src/` is for tests that are part of the code. A rule and the test that holds it in place are one thought, and they should be one edit — when a rule changes, its test is on the screen; when a rule is deleted, so is its test, rather than being found six months later in a file nobody opened.

The practical difference is the top of the file. In `test/` the example below would open with `package test` and eleven `import` lines, every one of them a thing to update when the model moves. In `Order.dove` there are none: the aggregate, its states, and its errors are already in scope, and `Money` and `Sku` are already imported for the production code above. Private helpers are reachable too, which `test/` cannot do.

```dovetail
// the tail of Order.dove, after module Order

private function anOrder(): Order =
    Order.draft(OrderId.parse("o-1").require, CustomerId.parse("c-1").require)

private function aLine(): OrderLine =
    OrderLine {
        sku = Sku.parse("ABCD1234").require
        quantity = Quantity.parse(2).require
        unitPrice = Money.of(1250i64, Currency.Usd)
    }

private function noon(): Instant = Instant.ofEpochSecond(1700000000i64)

test "an order with no lines cannot be placed" =
    assert anOrder().place(noon(), true).isError

test "a placed order carries when it was placed and what it came to" =
    let drafted: Order = anOrder().addLine(aLine()).require
    match drafted.place(noon(), true) with
        case Ok((placed, event)) =>
            match placed.status with
                case OrderStatus.Placed { total; placedAt } =>
                    assert total.minorUnits == 2500i64
                    assert placedAt == noon()
                case _ => panic "expected a placed order"
            assert event.total.minorUnits == 2500i64
        case Error(_) => panic "expected the order to place"

test "the draft is unchanged by placing it" =
    let drafted: Order = anOrder().addLine(aLine()).require
    drafted.place(noon(), true)
    assert drafted.placedAt().isNone

test "placing twice reports what the order actually was" =
    let drafted: Order = anOrder().addLine(aLine()).require
    let (placed, _e) = drafted.place(noon(), true).require
    match placed.place(noon(), true) with
        case Error(OrderError.NotDraft(OrderStatus.Placed(_))) => assert true
        case _ => panic "expected NotDraft carrying the real status"
```

Things worth copying:

- **A couple of small builders** (`anOrder`, `aLine`, `noon`) keep the tests about the rule rather than the setup. They are file-private functions, so they never leak into the package's surface. Tests after an inline module are still file-level declarations: being in `Order.dove` does not grant private construction access. `anOrder` correctly calls `Order.draft`.
- **Notice what you are not testing.** There is no test that a draft has no `placedAt`, or that a placed order has a total — those are not true because of a check, they are true because of a type, and a test would only be re-stating the type. **The better your types, the fewer tests you need**, and the ones left are about behavior rather than about data hygiene.
- **The third test cannot be written in a mutable model.** `place` is called, its result discarded, and the draft is still a draft.
- **Match the specific error, including its payload.** The fourth test asserts not just "this refused" but that the refusal carried the real status — which is the thing 19.2 added, so it is the thing worth pinning.
- **A controlled clock is free** because `now` is a parameter (19.8).

Test rules, not plumbing: `Order.draft` needs no test, and neither does an accessor.

**What does belong in `test/`** is the layer above: a use case exercised through its public surface, with the repository faked (20.8). That is a test of the package from outside, which is exactly what the directory is for.

---

## 19.12 A Change, End to End

The reason for all of this is what happens when the business changes its mind. Suppose orders above 1,000.00 now need approval.

The rule concerns data the aggregate already has, so it goes in `place` — and the error carries the total that tripped it, so the caller need not re-derive it:

```dovetail
                    let total: Money = self.total()
                    if total.isGreaterThan(Money.of(100000i64, total.currency)) then
                        Error(OrderError.ApprovalRequired(total))
                    else
                        ...
```

One new error variant in `types.dove`, four lines in `Order.dove`, one new test. The application service, HTTP handler, repository, and database are untouched — and every caller that matches exhaustively on `OrderError` now gets a compile error telling them there is a new outcome to handle.

Now a harder change, and a more instructive one: *"an approved order records who approved it."* That is not a new check, it is a **new state**, so it belongs in the type:

```dovetail
public record Approval =
    approvedBy: UserId
    approvedAt: Instant

public enum OrderStatus =
    Draft
    AwaitingApproval(Money)
    Approved(Approval)
    Placed(Placement)
    Cancelled(Cancellation)
```

Adding that variant breaks every exhaustive match in the domain layer, and each break is a real question: can you add a line to an order awaiting approval? Can you cancel one? The compiler has just produced the design review, and you cannot ship until you have answered all of it. Compare that with adding an `approvedBy: Option<UserId>` field, which compiles immediately and answers nothing.

And when a rule genuinely needs a fact the aggregate does not have — *"the approver must not be the buyer"* — it becomes a parameter, and the use case must supply it:

```dovetail
    public function approve(self, now: Instant, approver: UserId): Result<(Order, OrderApproved), OrderError>
```

so the application layer changes too. That is the design telling you the truth: a new input is a new cost, and it shows up at the layer that pays for it.

---

## Summary

- **Start with the types.** One `types.dove` you can read in one sitting is the model; behavior goes in a file per concept. Design the shapes before the functions.
- **Make illegal states unrepresentable.** Put data inside the state that gives it meaning, so a `Draft` has no `placedAt` and a `Placed` order cannot lack a total. Parallel `Option` fields next to a status enum is the anti-pattern to recognize.
- **`Option` means optional in every state**, not filled in later. Errors carry their own evidence. A `case _ =>` in the domain layer forfeits the compile errors that would have found you.
- **Value objects** by default — a bare `String` or `Int32` in the model is a smell, because the rule about it usually arrives later and needs somewhere to live. Private newtype construction through its module so holding one proves it is valid, living at the innermost place that needs it.
- **Entities** have identity; IDs and clocks are supplied from outside. Private record construction and `with` updates make the associated module own their transitions, while callers can still inspect them.
- **Pick one style for the whole layer.** The functional style is recommended — it agrees with Dovetail's `Result`-based, IO-free layer.
- **Aggregates** are consistency boundaries, drawn by invariants. They reference each other by ID, and none of them depends on another's invariants.
- **Domain events are facts**, returned from the function that caused them, stopping at the context boundary.
- **Pass the fact, not the port** — including the clock.
- Every `Result` in a domain signature is a rule a type could not carry. That is often fine, and it is always worth noticing.

Part 20 puts this model to work: use cases, transactions, the repository ports that load and save aggregates, and the three kinds of trigger that drive them.
