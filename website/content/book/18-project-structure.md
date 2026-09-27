# Part 18: Project Structure and Onion Architecture

A Dovetail workspace is a list of **projects**, and each project is a list of **packages**. That is a small amount of machinery, but it is enough to express — and, more importantly, to *enforce* — a full onion architecture for a bounded context or microservice.

This part describes a layout that has worked well in practice: how to split a bounded context into projects, how to layer the inside of the main project, which rules the compiler checks for you, and which ones remain your responsibility. Part 19 covers what goes *inside* those layers; this part is about where the walls are.

> **The one idea:** dependencies point **inward**. The domain layer knows nothing about the outside world; the outside world knows about the domain layer. Everything below is a mechanical consequence of that rule.

---

## 18.1 The Dependency Rule

Onion architecture (also called hexagonal, ports-and-adapters, or clean architecture) has a single rule:

> **Source-code dependencies may only point inward, toward the domain.**

Concentrically, from the inside out:

```
              ┌──────────────────────────────────┐
              │        infrastructure            │   SQL, HTTP, queues, clocks
              │   ┌──────────────────────────┐   │
              │   │      application         │   │   use cases, ports, jobs
              │   │   ┌──────────────────┐   │   │
              │   │   │     domain       │   │   │   entities, value objects,
              │   │   │                  │   │   │   invariants — no IO
              │   │   └──────────────────┘   │   │
              │   └──────────────────────────┘   │
              └──────────────────────────────────┘
                            ▲
                  dependencies point inward
```

The payoff is that the interesting part of your program — the rules of the business — never mentions a database, an HTTP framework, or a wire format. You can read it, test it, and change it without starting a container.

The rest of this part is about turning that picture into `Dovetail.toml` entries and directories.

---

## 18.2 Three Projects per Bounded Context

A bounded context (in a microservice deployment, one service) is **three projects**:

```
acme-workspace/
├── Dovetail.toml
├── shared-kernel/          ← shared across ALL contexts (17.8)
├── orders-primitives/      ← the context's value vocabulary
├── orders-contract/        ← the context's public wire surface
└── orders-app/             ← the context itself
    ├── src/
    │   ├── domain/
    │   ├── application/
    │   ├── infrastructure/
    │   └── main.dove     ← composition root
    └── test/
```

```toml
[[project]]
name = "orders-primitives"
root_package = "acme.orders.primitives"
depends = ["shared-kernel"]
packages = ["."]

[[project]]
name = "orders-contract"
root_package = "acme.orders.contract"
depends = ["shared-kernel", "orders-primitives", "standard-io"]  # Async, for service contracts
packages = ["."]

[[project]]
name = "orders-app"
root_package = "acme.orders"
depends = [
    "shared-kernel",
    "orders-primitives",
    "orders-contract",
    "standard-io",
    "standard-json",
]
packages = ["domain", "application", "infrastructure", "."]
```

### `primitives` — the value vocabulary

Value types with no identity and no lifecycle: `OrderId`, `Money`, `Quantity`, `Sku`, `Currency`. Newtypes and small records, with their invariants enforced at construction.

```dovetail
package acme.orders.primitives

public newtype OrderId private = String

module OrderId =
    public function parse(raw: String): Result<OrderId, String> =
        if raw.isEmpty then
            Error("order id must not be empty")
        else
            Ok(OrderId(raw))

    public function text(self: OrderId): String = self.value
```

Every layer may use `primitives`, and `primitives` may use nothing but the shared kernel. That is what makes it the *lowest* layer.

**Why does this project exist at all?** Because `contract` and `domain` must speak the same value vocabulary without either depending on the other. A REST response and an aggregate both need to say `OrderId`, but the contract must not import the domain (that would leak your model onto the wire) and the domain must not import the contract (that would point a dependency outward). Extracting the shared vocabulary downward is the standard fix for that kind of cycle, and `primitives` is that extraction, made permanent.

**What does not belong here:** anything with identity, anything with a lifecycle, anything that performs IO, and — the one people get wrong — **anything only one layer needs**. A value type the model uses and the contract never mentions belongs in the domain layer, not here. `primitives` is for the *shared* vocabulary; promote a type into it when a second layer needs to speak it, not before (19.2).

### `contract` — the public wire surface

Everything another context or an HTTP client is allowed to know: request and response shapes, integration events, public error codes, and enum-like status values as they appear on the wire.

```dovetail
package acme.orders.contract

import acme.orders.primitives.OrderId
import acme.orders.primitives.Money

public record PlaceOrderRequest =
    customerId: String
    lines: Array<OrderLineDto>

public record OrderPlaced =
    orderId: OrderId
    total: Money
    occurredAt: Instant
```

Two rules:

1. **The domain layer must not use it.** A contract type is a serialization concern; letting it into the model means every wire change becomes a model change.
2. **It is versioned by other people's code.** Other contexts compile against it. Treat every change as a breaking-change question, and keep its own dependencies minimal so consumers do not inherit a tree.

**Service contracts.** A contract is not always just data. When a context publishes an RPC-style surface — the set of calls it answers — that callable shape belongs in `contract` too, and it necessarily mentions `Async`:

```dovetail
package acme.orders.contract

public interface OrderService =
    function listOrders(self: Self, query: OrderQuery): Async<Array<OrderSummary>, OrderServiceError>
    function placeOrder(self: Self, request: PlaceOrderRequest): Async<OrderId, OrderServiceError>
```

So `orders-contract` does depend on `standard-io`, for the `Async` type. That is not a contradiction of the no-IO rule: **declaring** that an operation is asynchronous is not **performing** IO. The contract project still has no implementation, no sockets, no files, no `use`, and no `main`. It says what can be called; it never says how.

Three rules keep a service contract from becoming a back door into your model:

1. **Contract types on both sides.** `listOrders` returns `Array<OrderSummary>` — a DTO — never `Array<Order>`, the aggregate. The compiler already enforces this: `contract` cannot see `domain`, so an aggregate is not even nameable there. That constraint is a feature; it is the reason the wire surface stays a wire surface.
2. **Contract errors, not domain errors.** `OrderServiceError` is a wire-stable enum of outcomes a caller can act on. Domain errors are internal detail and will change as the model does.
3. **The interface is not the use case.** Implement it in `infrastructure` as a thin adapter that delegates to the application service, rather than having `PlaceOrderService` implement `OrderService` directly. The two signatures look identical on day one and diverge by month three — and if they are the same type, every wire-compatibility constraint your consumers impose is now a constraint on your use cases. (For a small context with a single consumer, collapsing them is a defensible shortcut; know that you are trading a mapping function for that coupling.)

The payoff appears in 18.9: because `OrderService` is an interface rather than a URL, the same contract can be implemented in-process when both contexts share a binary, or as an HTTP stub when they do not — and no caller can tell which.

`contract` remains a **library**: no `main`, no adapters, no IO of its own, and nothing a consumer cannot afford to compile against.

### `app` — the context itself

The deployable. It holds the three layers as three packages, plus a composition root, and it is the only project that produces a WASM component.

---

## 18.3 Three Packages Inside `app`

```toml
packages = ["domain", "application", "infrastructure", "."]
```

That line is not documentation. **Packages are typechecked in the order listed, each against the accumulated registry of the packages before it.** A package cannot import a package that appears later in the list — the name simply does not exist yet. Your layering is a compile error, not a code-review convention.

| Package | Directory | May see | Contains |
|---|---|---|---|
| `acme.orders.domain` | `src/domain/` | primitives, shared kernel | entities, aggregates, domain services, domain events, invariants |
| `acme.orders.application` | `src/application/` | + domain, contract | use cases, ports, event handlers, jobs |
| `acme.orders.infrastructure` | `src/infrastructure/` | + application | repository/ACL implementations, HTTP wiring, SQL, clients |
| `acme.orders` | `src/` | everything | the composition root and `main` |

### The domain package

Pure. Entities and aggregates, the invariants that hold them together, domain services for logic that belongs to no single entity, and domain events. It returns values and `Result`, never `Async`.

```dovetail
package acme.orders.domain

import acme.orders.primitives.OrderId
import acme.orders.primitives.Money
import acme.orders.primitives.Quantity

public enum OrderStatus =
    Draft
    Placed
    Cancelled

public class Order private (
    public id: OrderId,
    mutable status: OrderStatus,
    mutable lines: List<OrderLine>
) =
    public function place(self): Result<OrderPlacedEvent, OrderError> =
        match self.status with
            case OrderStatus.Draft =>
                if self.lines.length == 0 then
                    Error(OrderError.Empty)
                else
                    self.status = OrderStatus.Placed
                    Ok(OrderPlacedEvent(self.id, self.total()))
            case _ => Error(OrderError.AlreadyPlaced)
```

Note what is absent: no `Async`, no `await`, no repository, no logging, no `use`. `place` decides; it does not persist.

### The application package

Use cases. An application service loads aggregates through a port, calls into the domain, and persists the result. It is where a unit of work begins and ends, and it is the layer that publishes integration events.

It also **declares the ports** — the interfaces that the outside world must implement:

```dovetail
package acme.orders.application

import acme.orders.domain.Order
import acme.orders.primitives.OrderId

public interface OrderRepository =
    function findById(self: Self, id: OrderId): Async<Option<Order>, RepositoryError>
    function save(self: Self, order: Order): Async<Unit, RepositoryError>
```

Declaring the port here, next to its consumer rather than next to its implementation, is what inverts the dependency: `infrastructure` depends on `application` in order to implement it, so the arrow points inward even though the data flows outward at runtime.

```dovetail
public class PlaceOrderService(
    orders: OrderRepository,
    events: EventPublisher,
    clock: Clock
) =
    public async function handle(self, command: PlaceOrder): Async<OrderId, PlaceOrderError> =
        let found: Option<Order> = await self.orders.findById(command.orderId)
            .mapError((e: RepositoryError) => PlaceOrderError.Storage(e))
        let order: Order = found orReturn Async.fail(PlaceOrderError.NotFound)

        let event: OrderPlacedEvent = order.place() orReturn
            Async.fail(PlaceOrderError.Rejected(...))

        await self.orders.save(order).mapError(...)
        await self.events.publish(OrderPlaced { orderId = order.id, ... })
        order.id
```

The shape is always the same: **load, decide, persist, announce.** The decision — `order.place()` — is one line, and it is the only line that encodes a business rule. Everything around it is plumbing. If your application services start containing `if` statements about the business, that logic belongs in the domain.

### The infrastructure package

Everything the real world forces on you: SQL, HTTP, queues, files, clocks, retries. Two kinds of adapter live here.

**Driven adapters** implement the ports the application declared:

```dovetail
package acme.orders.infrastructure

import acme.orders.application.OrderRepository
import acme.orders.domain.Order

public class SqliteOrderRepository(connection: Connection) implements OrderRepository =
    public async function findById(self, id: OrderId): Async<Option<Order>, RepositoryError> =
        let rows = await self.connection.query("select ... where id = ?", [id.text()])
        rows.first().map((row: Row) => self.toDomain(row))

    // Mapping row -> aggregate lives here, never in the domain.
    function toDomain(self, row: Row): Order = ...
```

**Driving adapters** translate the outside world into use-case calls — HTTP routes, message consumers, CLI entry points:

```dovetail
public async function handlePlaceOrder(
    service: PlaceOrderService,
    request: HttpRequest
): Async<HttpResponse, Never> =
    let dto: PlaceOrderRequest = Json.decode<PlaceOrderRequest>(request.body) orReturn
        Async.succeed(HttpResponse.badRequest("malformed body"))
    ...
```

This is the layer where the contract types are read and written: the HTTP handler decodes a `PlaceOrderRequest`, turns it into a command, and encodes the result back. Contract types get no further inward than here and the application layer.

> **If the context grows,** split the driving adapters into their own package: `packages = ["domain", "application", "api", "infrastructure", "."]`. Because `api` is listed before `infrastructure`, the compiler will then guarantee your HTTP layer cannot reach for a concrete repository — it can only see the application's ports. Start with one `infrastructure` package; split when the SQL and the routing stop fitting in one head.

---

## 18.4 What the Compiler Enforces

It is worth being precise about which walls are real and which are agreements, because you will lean on the real ones.

**Enforced — package order.** As above: `domain` cannot name anything in `application` or `infrastructure`. Attempting it is an unresolved-name error.

**Enforced — project dependencies.** `depends` in `Dovetail.toml` must be a DAG, and it is checked. `orders-contract` cannot depend on `orders-app`, so your wire surface can never accidentally drag in your implementation.

**Enforced — visibility.** Dovetail has four levels, and the default is narrower than most people expect:

| Modifier | Visible from |
|---|---|
| `public` | any package that can see the declaring package |
| `internal` | the same package **only** (this is the default) |
| `private` | the declaring **file** only |
| `protected` | the declaring class and its subclasses |

Because `internal` is per-package and each layer is its own package, **a layer's helpers are invisible to the next layer unless you write `public`**. Each layer's surface is therefore an explicit choice. This works especially well in the domain layer: make the aggregate root `public` and everything it coordinates `internal`, and the application layer is structurally unable to reach past the root.

**Not enforced — "the domain layer has no IO," and "the domain layer does not use the contract."** `depends` is declared per *project*, so every package in `orders-app` can see every one of that project's dependencies. If `orders-app` depends on `standard-io` and `orders-contract` — and it must, for the outer layers — then `src/domain/` can import them too. Nothing stops it.

In practice this is a review rule and an easy one to check (`grep -r "^import standard.io" src/domain/`). If you would rather the compiler checked it, use the variant in the next section.

---

## 18.5 Variant: the Domain Layer as Its Own Project

Promote `domain` from a package to a project, and the two unenforced rules become enforced — because a project only sees what its own `depends` lists:

```toml
[[project]]
name = "orders-domain"
root_package = "acme.orders.domain"
depends = ["shared-kernel", "orders-primitives"]      # no io, no contract
packages = ["."]

[[project]]
name = "orders-app"
root_package = "acme.orders"
depends = [
    "shared-kernel",
    "orders-primitives",
    "orders-contract",
    "orders-domain",
    "standard-io",
    "standard-json",
]
packages = ["application", "infrastructure", "."]
```

```
orders-primitives/
orders-contract/
orders-domain/           ← cannot import standard.io: it is not a dependency
│   ├── src/
│   └── test/            ← its own IO-free test suite
orders-app/
    ├── src/application/
    ├── src/infrastructure/
    ├── src/main.dove
    └── test/
```

| | Three projects | Four projects |
|---|---|---|
| Layering inside `app` | enforced (package order) | enforced (package order) |
| Domain has no IO | convention | **enforced** |
| Domain cannot see the contract | convention | **enforced** |
| Domain tests | share the app's `test/` | own `test/`, no IO deps |
| Cost | — | one manifest entry, one directory |

The trade-off is small enough that the four-project layout is the better default for any context that will outlive its first release, and for any team large enough that "we agreed not to" is not a mechanism. Use three projects for a small context, a prototype, or a service you expect to stay under a few thousand lines — and note that promoting `domain` later is a directory move plus a manifest entry, since the package path `acme.orders.domain` is unchanged either way.

---

## 18.6 Ports and Adapters

A **port** is an [interface](06-type-system.md#611-interfaces-and-interface-types) declared by the layer that *needs* the capability. These examples store ports in fields and pass them as values, so they use `interface`; a plain `trait` is usable only as a generic bound. An **adapter** is an implementation supplied by a layer further out. Two kinds matter here.

### Repositories

One repository per aggregate root, phrased in domain terms. Repository methods take and return **aggregates**, never rows, never DTOs:

```dovetail
public interface OrderRepository =
    function findById(self: Self, id: OrderId): Async<Option<Order>, RepositoryError>
    function save(self: Self, order: Order): Async<Unit, RepositoryError>
```

If a method on your repository returns `OrderSummaryDto` for a screen, it is not a repository — it is a query service. Give it its own port (`OrderQueries`) and let it return contract types directly, bypassing the domain. Read models and write models diverge for good reasons; do not force the aggregate to serve both.

### The anti-corruption layer

An ACL is a port whose implementation talks to *another bounded context* and translates its vocabulary into yours. The port is phrased entirely in your terms:

```dovetail
package acme.orders.application

import acme.orders.domain.Buyer
import acme.orders.primitives.CustomerId

public interface BuyerLookup =
    function fetch(self: Self, id: CustomerId): Async<Option<Buyer>, LookupError>
```

The implementation is where the other context's language appears — and stops:

```dovetail
package acme.orders.infrastructure

import acme.customers.contract.CustomerResponse   // the ONLY place this is imported
import acme.orders.application.BuyerLookup
import acme.orders.domain.Buyer

public class HttpBuyerLookup(client: HttpClient) implements BuyerLookup =
    public async function fetch(self, id: CustomerId): Async<Option<Buyer>, LookupError> =
        let response: CustomerResponse = await self.client.getJson(...)
        Some(Buyer(id, response.displayName, response.billingCountry))
```

That translation step is the entire point. `Buyer` is *your* notion of a customer — the two or three facts the Orders context needs — not the sixty fields the Customers context tracks. Without the ACL, their model becomes your model, and their next release becomes your next release.

---

## 18.7 The Composition Root

Someone has to know the concrete types. That someone is `main`, in the project's root package (`src/`, listed last in `packages`), and **nowhere else**.

```dovetail
package acme.orders

import acme.orders.application.PlaceOrderService
import acme.orders.infrastructure.SqliteOrderRepository
import acme.orders.infrastructure.HttpBuyerLookup
import acme.orders.infrastructure.OutboxPublisher

async function run(): Async<Unit, StartupError> =
    let connection = use Sqlite.open("orders.db")
    let httpClient = use HttpClient.make()

    let repository: OrderRepository = SqliteOrderRepository(connection)
    let buyers: BuyerLookup = HttpBuyerLookup(httpClient)
    let publisher: EventPublisher = OutboxPublisher(connection)

    let placeOrder = PlaceOrderService(repository, publisher, buyers)

    await Http.serve(8080, routes(placeOrder))

function main(): Unit = Async.run(run())
```

Three properties make this the right shape:

- **It is the only place `use` appears for long-lived resources.** The connection pool, the HTTP client, and the log sink are acquired here and released in LIFO order when `run` returns — including on failure and on interruption (Part 13).
- **It is the only place a concrete adapter is named.** Swapping SQLite for Postgres, or a real buyer lookup for an in-memory fake, is an edit to this file only.
- **It has no logic.** If a condition appears here that is not configuration, it belongs in the application layer.

There is no dependency-injection container, and none is needed: the composition root is the container, it is ordinary code, and its mistakes are compile errors.

---

## 18.8 The Shared Kernel

`primitives` shares a vocabulary between the layers of one context. The **shared kernel** shares one between *contexts*:

```toml
[[project]]
name = "shared-kernel"
root_package = "acme.kernel"
packages = ["."]
```

Same idea, much larger blast radius — and that difference should govern what you put in it.

**What belongs:** value types and their invariants, where the meaning is genuinely identical everywhere. `Money`, `Currency`, `Percentage`, `DateRange`, `EmailAddress`, `CountryCode`. Cross-context identifier newtypes: `CustomerId`, `TenantId`. Small, total, dependency-free.

**What does not belong:**

- **Anything with IO or `Async`.** The kernel sits below every domain layer; it must be as pure as they are.
- **Anything only one context uses.** Push it down into that context's `primitives`.
- **Repositories, services, ports.** Those are per-context by definition.
- **Entities.** This one is worth arguing.

### Do not share entities

It is tempting to define `Customer` once and have Orders, Billing, and Shipping all use it. Resist that, for two reasons.

The mechanical one: an entity has identity **and a lifecycle**, and that lifecycle is owned by exactly one context. Sharing the type shares the lifecycle. A rule Billing adds about delinquent accounts is now compiled into Orders, and every context must be redeployed and re-reasoned together — which is the coupling that having bounded contexts was meant to remove.

The conceptual one, which is the deeper point: **"customer" does not mean the same thing in the two contexts.** To Orders, a customer is a shipping address and a credit status. To Billing, a customer is a tax jurisdiction, a payment instrument, and an invoice history. A shared `Customer` becomes the union of every context's needs — an entity with forty fields of which each caller uses four, whose invariants must satisfy everyone and therefore constrain no one.

The alternative is smaller and better:

- Put **`CustomerId`** in the shared kernel. The identity *is* the same thing everywhere; that is precisely what makes it shareable.
- Let each context define its own model of a customer — Orders has `Buyer`, Billing has `Payer` — containing only what that context needs.
- Populate it through the ACL (18.6) from the owning context's contract.

Two contexts modelling the same real-world thing differently is not duplication to be eliminated. It is the reason bounded contexts exist.

### Governing the kernel

Every context compiles against the shared kernel, so a change to it is a change to all of them. Three habits keep that from hurting:

1. **Keep it small.** If you are unsure whether something belongs, it does not. Duplicating a value type into two contexts costs far less than a shared type that fits neither.
2. **Changes need agreement from every consumer.** This is a real cost, and it is the reason the kernel should be slow-moving by design.
3. **When a type grows context-specific behavior, copy it down.** The moment `Money` needs a `roundForTaxJurisdiction` that only Billing uses, Billing gets its own `Money` extension in its own `primitives` — or its own type.

---

## 18.9 Talking to Other Bounded Contexts

The workspace-level rule that keeps a multi-context repository from turning into a ball of mud:

> **A context may depend on another context's `contract` project and on the shared kernel. Never on its `app`, and never on its `domain`.**

```toml
[[project]]
name = "orders-app"
depends = [
    "shared-kernel",
    "orders-primitives",
    "orders-contract",
    "customers-contract",     # ✅ another context's wire surface
  # "customers-app",          # ❌ never
]
```

`customers-contract` is the same artifact whether the two contexts run in one process or on opposite sides of a network — which is what makes extracting a context into a separate service a deployment change rather than a rewrite. The dependency is already narrowed to the wire surface; only the transport underneath it changes.

### Service contracts, client stubs, and the ACL

If the Customers context publishes a service contract (18.2), a consumer ends up with two interfaces, and it is worth being clear about why neither one is redundant:

```dovetail
// acme.customers.contract — what the OWNER publishes. Their vocabulary, their granularity.
public interface CustomerService =
    function getCustomer(self: Self, id: CustomerId): Async<Option<CustomerResponse>, CustomerServiceError>

// acme.orders.application — what the CONSUMER needs. Your vocabulary, your granularity.
public interface BuyerLookup =
    function fetch(self: Self, id: CustomerId): Async<Option<Buyer>, LookupError>
```

Between them sits the consumer's infrastructure, holding both halves:

```dovetail
package acme.orders.infrastructure

import acme.customers.contract.CustomerService
import acme.customers.contract.CustomerResponse
import acme.orders.application.BuyerLookup

// 1. The client stub: implements the owner's contract over a transport.
public class HttpCustomerClient(client: HttpClient, baseUrl: Url) implements CustomerService = ...

// 2. The ACL: implements YOUR port by calling theirs, and translates.
public class CustomerServiceBuyerLookup(customers: CustomerService) implements BuyerLookup =
    public async function fetch(self, id: CustomerId): Async<Option<Buyer>, LookupError> =
        let response: Option<CustomerResponse> = await self.customers.getCustomer(id)
            .mapError((e: CustomerServiceError) => LookupError.Upstream(e))
        response.map((r: CustomerResponse) => Buyer(id, r.displayName, r.billingCountry))
```

The stub is a **transport** concern and the ACL is a **translation** concern, and keeping them apart is what buys you the deployment flexibility:

- **Contexts co-deployed?** Wire the ACL to the Customers app's own implementation of `CustomerService` directly — no HTTP, no serialization, no stub. Every call becomes a function call.
- **Contexts split across the network?** Wire the ACL to `HttpCustomerClient` instead.

The change is one line in the composition root. Nothing in the application layer, and nothing in the domain, knows which world it is running in.

The consumer's application layer depends only on `BuyerLookup`. Do not let it import `CustomerService` — that would put the owner's vocabulary and the owner's release schedule into your use cases, which is the exact coupling the ACL exists to prevent.

### Integration events

Integration events are the other half of the contract: they are how a context announces facts without anyone asking. They are declared in `contract`, published by the **application** layer through a port, and delivered by **infrastructure**.

```dovetail
// contract: the fact, in wire terms
public record OrderPlaced =
    orderId: OrderId
    total: Money
    occurredAt: Instant

// application: the port
public interface EventPublisher =
    function publish(self: Self, event: OrderPlaced): Async<Unit, PublishError>

// infrastructure: transactional outbox, Kafka, whatever it turns out to be
public class OutboxPublisher(connection: Connection) implements EventPublisher = ...
```

Do not confuse these with **domain events**. A domain event (`OrderPlacedEvent`, returned by `Order.place()` in 18.3) lives in the domain layer, is phrased in domain types, and **never leaves the context**. The application layer translates the ones that matter into integration events. Keeping the two separate is what lets you refactor your model without breaking every subscriber; Part 19 goes into the distinction in depth, and Part 21 into how consumers should receive them.

---

## 18.10 Testing the Layers

Each project has its own `test/` directory at the project root, and test packages are granted internal access to every source package in that project — so tests can reach a layer's `internal` helpers without you widening them to `public` for testing's sake.

```
orders-domain/test/    placeOrderTest.dove      pure, fast, no fakes
orders-app/test/       applicationTest.dove     services against in-memory ports
                       infrastructureTest.dove  real SQLite, real HTTP
```

The layering pays off directly in the test suite:

- **Domain tests** construct an aggregate, call a method, and assert on the result. No mocks, no async, no setup. These should be the overwhelming majority of your tests and should run in milliseconds.
- **Application tests** instantiate a service with in-memory adapters. Because ports are interfaces, an `InMemoryOrderRepository` backed by an array is a few lines and needs no mocking framework. These verify orchestration: was the event published, was the aggregate saved.
- **Infrastructure tests** are the only ones that touch a real database or socket. They are slower and fewer, and they verify the adapter, not the business rule.

```dovetail
package test

import acme.orders.domain.Order
import acme.orders.primitives.OrderId

test "placing an empty order is rejected" =
    let order = Order.draft(OrderId("o-1"))
    assert order.place().isError()
```

Note what this test does not need: no database, no runtime, no `Async`. That is the whole return on the dependency rule — and if a dovetail test *does* need those things, you have found IO that leaked inward.

---

## 18.11 Rules at a Glance

| Rule | Enforced by |
|---|---|
| `domain` cannot see `application` or `infrastructure` | package order in `Dovetail.toml` |
| `contract` cannot see `app` | project `depends` (DAG) |
| A layer's internals are hidden from the next layer | `internal` is the default, and is per-package |
| No dependency cycles anywhere | the compiler |
| `domain` has no IO | project `depends` (four-project layout) or review |
| `domain` does not use `contract` | project `depends` (four-project layout) or review |
| Only `main` names concrete adapters | review — and it is obvious when violated |
| A service contract cannot expose an aggregate | `contract` cannot see `domain` |
| Cross-context deps go through `contract` only | project `depends` |

And the shapes worth memorizing:

- **Ports point inward.** The interface is declared where it is *used*, implemented where the technology lives.
- **Application services read: load, decide, persist, announce.** The decision is a domain call.
- **Repositories speak aggregates.** Rows and DTOs stop at the infrastructure boundary.
- **Declaring `Async` is not doing IO.** A service contract may return `Async`; it may not contain an implementation.
- **The composition root is the only place that knows everything.** Keep it dumb and keep it small.

---

## Summary

- A bounded context is **three projects** — `primitives`, `contract`, `app` — plus an optional fourth if you promote `domain` out of `app` to have the compiler enforce its purity.
- `primitives` exists so `contract` and `domain` can share a vocabulary without depending on each other; the **shared kernel** does the same thing across contexts.
- Inside `app`, the layers are packages, and **the order in `packages = [...]` enforces the layering** — inner layers physically cannot name outer ones.
- **Ports are interfaces declared by the inner layer** and implemented by the outer one; that inversion is what makes the dependency rule survive contact with a database.
- **The composition root** (`main`, in the root package) is the only code that names concrete adapters or acquires long-lived resources.
- Share **value types and IDs** across contexts, never entities — the same word legitimately means different things in different contexts, and the ACL is where the translation lives.
- Cross-context dependencies go through `contract` and nothing else, which is what makes splitting a context into its own service a deployment change rather than a rewrite.

Part 19 turns to what fills these layers: aggregates, entities, value objects, domain events, and how to model a business rule so it has exactly one home. Part 20 covers the application layer that drives them — use cases, event handlers, and jobs. Part 21 zooms back out to the relationships *between* contexts: when to consume another context's `contract` directly, when to insulate yourself behind an anticorruption layer, and how that choice shows up in your `Dovetail.toml`.
