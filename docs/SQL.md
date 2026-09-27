# Native SQL Support in Dovetail

This document specifies Dovetail's SQL support: type-safe query construction through smart string interpolation.

> **Status: implemented.** Prefixed string literals are in the compiler, and `standard-sqlite` is their first client. See [website/content/book/24-prefixed-literals.md](../website/content/book/24-prefixed-literals.md) for the language feature and `standard-sqlite/test/` for the tests.
>
> There was exactly one new *language* feature: **prefixed string literals**, described in [The Literal Mechanism](#the-literal-mechanism). It is domain-agnostic — the compiler learns nothing about databases. Everything SQL-specific lives in the driver library.

## Design Goals

1. **Safe parameter binding** — Every interpolated value goes through a typed conversion and is bound as a parameter, so SQL injection is not expressible
2. **Real SQL** — Write actual SQL syntax, not a DSL or ORM abstraction
3. **Modern interpolation** — Values inline at their point of use, not positional placeholders counted by hand
4. **Flexible composition** — Query fragments are ordinary values that compose
5. **Async execution** — Queries return `Async<T, SqliteError>`, non-blocking on the component's own fiber
6. **No database knowledge in the compiler** — Placeholder syntax, dialect quirks, and validation belong to the driver
7. **No cross-database abstraction** — See [below](#why-there-is-no-standardsql)

## Non-Goals

- A database-agnostic `standard.sql` layer
- Schema validation at compile time
- ORM-style object mapping
- Migration support

---

## Why there is no `standard.sql`

An earlier draft of this document put a generic `standard.sql` package between the language and the driver: a shared `SqlValue` union, a shared `Connection`, shared `ToSqlParameter` / `FromSql` / `FromRow` traits, with each driver adapting to them. That layer is dropped. It cost more than it bought:

- **The shared value type is wrong for every database.** A union of everything (`Uuid`, `Interval`, `Json`, `Array(...)`, four widths of signed and unsigned integers, …) forces SQLite — whose storage classes are exactly `NULL`, `INTEGER`, `REAL`, `TEXT`, `BLOB` — to accept values it cannot represent and reject them at *runtime*. `standard.sqlite` already declares the correct five-variant [`SqlValue`](../standard-sqlite/src/Sqlite.dove#L38-L43). A generic union would convert a static mismatch into a runtime error, which is a strict downgrade.
- **The pieces that look shared aren't.** Placeholder syntax differs (`?` in SQLite, `$N` in PostgreSQL). Errors differ (SQLite: a result code plus `sqlite3_errmsg`; PostgreSQL: SQLSTATE plus severity, detail, hint). Rows differ (SQLite reads columns off the live statement — there is no detached row to hand back). Connection lifecycle differs.
- **It never delivered portability anyway.** Dialect differences — `AUTOINCREMENT` vs `SERIAL`, `ILIKE`, upsert syntax, `RETURNING` — were already an explicit non-goal, so the query text was never portable. A shared vocabulary that doesn't make queries portable is paying a lowest-common-denominator price for nothing.

So each driver owns its whole stack: value type, error type, traits, derives, builder, and placeholder rendering. The duplication is about 150 lines of Dovetail and three small Rhai scripts per driver — less than the adapter each driver would otherwise write against a generic `SqlValue`.

The one thing genuinely lost is **middleware written once across drivers**: pooling, tracing, retry, migration tooling. That is recoverable later and additively — see [Deferred: a driver trait](#deferred-a-driver-trait). It is not a prerequisite for any of what follows.

---

## The Literal Mechanism

`sql"..."` is **not** a hard-coded compiler feature. It is the first client of a general facility: **prefixed string literals**. The compiler owns the *lexing and the interpolation grammar*; a registered handler owns the *semantics*. Nothing about databases lives in the compiler.

This is the same philosophy as derive macros (see [website/content/book/16-macros.md](../website/content/book/16-macros.md)): the compiler provides a small structural hook, libraries provide the meaning.

### What the compiler provides

1. **A prefixed-literal token.** Any `ident"..."` or `ident"""..."""` is lexed as a prefixed string literal. The prefix selects a handler; if no handler is registered for that prefix, it is a compile error.

2. **A fixed interpolation grammar**, parsed natively — with real source spans, so errors point at the offending expression rather than at generated code:
   - `$ident` — interpolate a bare identifier
   - `${ expr }` — interpolate an arbitrary expression
   - `$.. expr` — *spread*: interpolate a sequence of values

   The first two are exactly the forms ordinary Dovetail strings already support ([website/content/book/03-language-basics.md §3.6](../website/content/book/03-language-basics.md)); `$..` is new. Note that `$user.name` is **not** a form — as in ordinary strings, `$` binds a bare identifier only, and anything with a field access or an operator uses `${ ... }`. The literal parses into a structured AST of *literal text* and *interpolation* parts.

3. **A lowering to builder calls.** The compiler lowers the structured literal to a chain of calls on the handler's **builder type**:
   - literal text → `builder.literal("...")`
   - `$expr` → `builder.value(expr)`
   - `$..expr` → `builder.spread(expr)`

   The trait bounds come entirely from the builder's own method signatures — e.g. `value<T>(self, x: T): SqlBuilder where T: ToSqlValue`. So the type system enforces `ToSqlValue` at each interpolation site *without the compiler knowing what `ToSqlValue` is*. The compiler knows only the three shapes: literal, value, spread.

### What the library provides

A prefix is declared in Dovetail source with `@stringLiteral`, which takes no argument — the prefix is the declared name:

```dovetail
@stringLiteral
public type sql = SqlBuilder
```

The marker sits on a lowercase type alias so `SqlBuilder` keeps its descriptive name while the prefix reads as SQL. `standard.sqlite` supplies `SqlBuilder` (with `empty` / `literal` / `value` / `spread` / `build`), the result type `Query`, the `ToSqlValue` / `FromSqlValue` / `FromRow` traits, and the execution API.

**The prefix is an ordinary imported name.** `sql"..."` is available exactly where `sql` is in scope:

```dovetail
import standard.sqlite.sql
```

So a prefix collision cannot arise: two drivers may both export a `sql`, and a file imports the one it wants. `import com.pg.sql as pg` renames the literal to `pg"..."`, because import aliasing already does that for every other name. And the import tells a reader which library the literal comes from.

Adding a `re"..."` regex literal or a `json"..."` literal later means marking another name — no compiler change, and no build configuration.

### What was dropped, and why

An earlier draft proposed an in-string optional fragment, `$?opt { AND name = $opt }`, that rebound `opt` to its *unwrapped* type inside the block. That is the one form a generic interpolation grammar cannot express: it requires per-block type rebinding, which would push SQL-shaped typing rules into the compiler. It is replaced by [fragment composition](#fragment-composition) — a `Query` is an ordinary composable value, and optional inclusion is a normal combinator over a closure whose parameter is bound by ordinary lambda rules. No language magic required.

---

## Basic Syntax

```dovetail
let userId = UserId(123i64)
let status = Status.Active

let query: Query = sql"SELECT * FROM users WHERE id = $userId AND status = $status"
```

This lowers to:

```dovetail
SqlBuilder.empty()
    .literal("SELECT * FROM users WHERE id = ")
    .value(userId)
    .literal(" AND status = ")
    .value(status)
    .build()
```

producing a `Query` whose text is `"SELECT * FROM users WHERE id = ? AND status = ?"` with parameters `[userId.toSqlValue(), status.toSqlValue()]`.

The placeholder syntax is the driver's choice, and SQLite's anonymous `?` is a good one: SQLite assigns each anonymous `?` the next unused index, which matches positional binding, and — unlike numbered `$1`/`?1` placeholders — it means [concatenating two fragments needs no renumbering](#fragment-composition).

### Multi-line Queries

Use triple-quoted strings for multi-line SQL:

```dovetail
let query = sql"""
    SELECT u.id, u.name, u.email
    FROM users u
    JOIN orders o ON u.id = o.userId
    WHERE o.total > $minTotal
    ORDER BY o.createdAt DESC
    LIMIT $limit
"""
```

---

## Interpolation Syntax

### `$expr` — Single Value

Interpolates one value through `ToSqlValue`. `$name` interpolates a bare identifier; `${ ... }` interpolates any expression, including field access:

```dovetail
let user = User { name = "Bob"; email = "bob@example.com" }

let query = sql"INSERT INTO users (name, email) VALUES (${user.name}, ${user.email})"
```

**Type requirement:** the expression must have a type `T` where `T: ToSqlValue`. This is enforced by `SqlBuilder.value`'s signature — there is no SQL-specific typing rule in the compiler.

### `$..expr` — Spread Sequence

Expands an array into one placeholder per element, for `IN` clauses:

```dovetail
let ids: Array<Int64> = [1i64, 2i64, 3i64]

let query = sql"SELECT * FROM users WHERE id IN ($..ids)"
// Text:       "SELECT * FROM users WHERE id IN (?, ?, ?)"
// Parameters: [1, 2, 3]
```

**Type requirement:** the expression must have type `Array<T>` where `T: ToSqlValue` (enforced by `SqlBuilder.spread`'s signature).

`Array<T>` itself deliberately does **not** implement `ToSqlValue` — the spread iterates and converts each element, so there is no ambiguity between "bind the array as one blob" and "expand it".

**Empty arrays.** `IN ()` is a syntax error in SQLite, so an empty spread renders the single literal token `NULL`, giving `id IN (NULL)`. That is valid SQL and matches no rows, which is the intended meaning. Beware the standard SQL gotcha on the negated form: `id NOT IN (NULL)` also matches no rows, because every comparison against `NULL` is unknown. If a query needs "not in an empty set matches everything", guard the fragment with [`whenSome`](#fragment-composition) instead of spreading an empty array.

---

## Fragment Composition

A `Query` is a first-class value: parameterized text plus its bound parameters. Fragments compose, so conditional and dynamic queries are built with ordinary Dovetail code rather than in-string control flow.

```dovetail
public record Query =
    text: String
    params: Array<SqlValue>

module Query =
    /// An empty fragment (renders to "").
    public function empty(): Query = ...

    /// Concatenate two fragments. Because placeholders are anonymous `?`,
    /// this is text concatenation plus parameter concatenation — no
    /// renumbering pass.
    public function append(self, other: Query): Query = ...

    /// Include a fragment only when the Option is Some; the unwrapped value
    /// is bound by an ordinary lambda parameter — no type rebinding.
    public function whenSome<T>(opt: Option<T>, f: (T) => Query): Query =
        match opt with
            case Some(value) => f(value)
            case None => Query.empty()
```

Optional filters — the former `$?` use case — are a `whereAndSome` over a list of optional fragments:

```dovetail
let nameFilter: Option<String> = Some("Alice")
let statusFilter: Option<Status> = None

let query =
    sql"SELECT * FROM users"
        .append(Query.whereAndSome([
            nameFilter.map<Query>((name: String) => sql"name = $name"),
            statusFilter.map<Query>((status: Status) => sql"status = $status")
        ]))
// Text:       "SELECT * FROM users WHERE (name = ?)"
// Parameters: ["Alice"]   (statusFilter is None, so its fragment is omitted)
```

Note there is no `WHERE 1=1`: the clause builders emit nothing when every part is absent, so the placeholder predicate that dynamic-query code usually needs isn't required.

Inside `(name: String) => sql"name = $name"`, `name` is a plain `String` because it is a normal lambda parameter. Compound shapes work the same way:

```dovetail
record DateRange =
    start: Instant
    end: Instant

let range: Option<DateRange> = Some(DateRange { start = startDate; end = endDate })

let q =
    base.append(Query.whenSome(range, r ->
        sql" AND createdAt BETWEEN ${r.start} AND ${r.end}"))
```

`append` also has an operator form, `++`, via the prelude's `Concat` trait:

```dovetail
let q = sql"SELECT * FROM users" ++ Query.whereSome(nameFilter, (n: String) => sql"name = $n")
```

### Clause builders

Modeled on [doobie's `Fragments`](https://github.com/typelevel/doobie/blob/main/modules/core/src/main/scala/doobie/util/fragments.scala), adapted to Dovetail: doobie gives each combinator a varargs overload and a collection overload, and Dovetail has no varargs, so each is one function over an `Array`.

| | Renders | Empty input |
|---|---|---|
| `a ++ b` | `ab` — the operator form of `append` | — |
| `Query.andAll(parts)` | `((a) AND (b))` | empty |
| `Query.orAll(parts)` | `((a) OR (b))` | empty |
| `Query.andSome(parts)` / `orSome` | same, over the present parts | empty |
| `Query.commaSeparated(parts)` | `a, b` | empty |
| `frag.parenthesized()` | `(a)` | — |
| `Query.whereAnd(parts)` | ` WHERE (a) AND (b)` | empty |
| `Query.whereOr(parts)` | ` WHERE (a) OR (b)` | empty |
| `Query.whereAndSome(parts)` / `whereOrSome` | same, over the present parts | empty |
| `Query.whereSome(opt, f)` | ` WHERE (a)` | empty |
| `Query.set(parts)` | ` SET a, b` | empty |
| `Query.setSome(parts)` | `Some(` ` SET a, b` `)` | **`None`** |
| `Query.orderBy(parts)` | ` ORDER BY a, b` | empty |
| `Query.orderBySome(parts)` | same, over the present parts | empty |

Three rules run through that table:

**Clause builders emit a leading space**, so they append onto a fragment that doesn't end in one — `sql"SELECT id FROM users".append(Query.whereAndSome(filters))`. The joiners (`andAll`, `commaSeparated`, `parenthesized`) don't, since they build expressions rather than clauses.

**Boolean joiners parenthesize each part; list joiners don't.** A filter containing `OR` must not rebind against a surrounding `AND`, so `whereAnd` wraps its parts. But `ORDER BY (x DESC)` is a syntax error and `SET (x = ?)` is noise, so `orderBy`, `set`, and `commaSeparated` leave their parts alone.

**`setSome` returns `Option<Query>`, unlike every other builder.** An absent `WHERE` or `ORDER BY` still leaves a valid statement; an `UPDATE` with no assignments does not. Rendering nothing would produce broken SQL, so "there is nothing to update" comes back as a value the caller has to handle.

Two doobie combinators are deliberately absent. `in` / `notIn` are covered by the `$..` spread (`id IN ($..ids)`), which reads better and is what the language already provides — though the empty-`NOT IN` gotcha [above](#expr--spread-sequence) still applies, and a guard with `whereAndSome` is the fix. `values` (bulk `INSERT ... VALUES (row), (row)`) needs variadic typed rows that Dovetail cannot express today.

---

## Core Traits

These follow the shape of the existing `standard.json` traits (`JsonEncoder` / `JsonDecoder`): a `trait` declaration plus `implement` blocks, with built-in implementations for the primitives and `@derive` support for user types. They convert to and from `standard.sqlite`'s own [`SqlValue`](../standard-sqlite/src/Sqlite.dove#L38-L43) — there is no intermediate universal value type.

### `ToSqlValue`

```dovetail
public trait ToSqlValue =
    function toSqlValue(self): SqlValue
```

**Built-in implementations:** `Bool`, `Int8`/`Int16`/`Int32`/`Int64`, `Uint8`/`Uint16`/`Uint32`, `Float32`/`Float64`, `String`, `Array<Uint8>` (blob), `Instant`, and `Option<T> where T: ToSqlValue` (`None` → `SqlValue.Null`).

SQLite's `INTEGER` is a signed 64-bit value, so `Uint64` is **not** implemented: values above `Int64.max` cannot round-trip. Store them as `Text` or `Blob` with an explicit newtype implementation.

```dovetail
implement ToSqlValue for Int64 =
    public function toSqlValue(self: Int64): SqlValue = SqlValue.Integer(self)

implement ToSqlValue for String =
    public function toSqlValue(self: String): SqlValue = SqlValue.Text(self)
```

**Deriving:**

```dovetail
// Newtypes delegate to the inner type's implementation
@derive(ToSqlValue)
newtype UserId = Int64

// Enums with payload-free variants map to their variant name as TEXT
@derive(ToSqlValue)
enum Status =
    Active
    Inactive
    Pending
// Produces SqlValue.Text("Active"), etc.
```

**Manual implementation,** when the storage form should differ:

```dovetail
implement ToSqlValue for Status =
    public function toSqlValue(self: Status): SqlValue =
        match self with
            case Active => SqlValue.Integer(1i64)
            case Inactive => SqlValue.Integer(0i64)
            case Pending => SqlValue.Integer(2i64)
```

### `FromSqlValue`

```dovetail
public trait FromSqlValue =
    function fromSqlValue(value: SqlValue): Result<Self, SqliteError>
```

For individual column values; built-in implementations mirror `ToSqlValue`. The failure channel uses `Result`'s `Ok` / `Error` constructors — Dovetail's `Result` is `Ok(T) | Error(E)`, there is no `Err`. A type mismatch (a `Text` cell decoded as `Int64`) is reported as a `SqliteError` carrying `SQLITE_MISMATCH` (20) and a message naming both storage classes — `expected Integer, got Text`.

```dovetail
@derive(FromSqlValue)
newtype UserId = Int64

@derive(FromSqlValue)
enum Status =
    Active
    Inactive
    Pending
```

### `FromRow`

```dovetail
public trait FromRow =
    function fromRow(row: Row): Result<Self, SqliteError>
```

A `Row` is one result row with its column names attached — the shape the current `Array<SqlValue>` rows from [`queryWith`](../standard-sqlite/src/Sqlite.dove#L120-L123) become once names are carried alongside them:

```dovetail
public record Row =
    names: Array<String>
    values: Array<SqlValue>

module Row =
    /// Column by name. Error if no such column.
    public function column(self, name: String): Result<SqlValue, SqliteError> = ...
    /// Column by zero-based position. Error if out of range.
    public function at(self, index: Int32): Result<SqlValue, SqliteError> = ...
```

`FromRow` is typically derived for records:

```dovetail
@derive(FromRow)
record User =
    id: UserId
    name: String
    email: String
    status: Status
```

The derive maps each record field to the identically-named column and decodes it with `FromSqlValue`, so every field type must implement `FromSqlValue`.

**Records map by name; tuples map by index.** A record's field list and a `SELECT` list are edited independently, so positional mapping would silently swap two same-typed columns whenever either is reordered — `SELECT email, name` into `record Person = name: String; email: String` decodes cleanly and puts the email in `name`. Matching on names makes that a hard failure.

Names are the wrong key for two kinds of query, and tuples cover both, since a tuple has no field names to match on:

```dovetail
// Computed columns have no names of their own.
let stats: (Int64, Int64) =
    await sql"SELECT COUNT(*), MAX(balance) FROM accounts".fetchOne<(Int64, Int64)>(conn)

// A join where both tables contribute `id`.
let joined: (Int64, String, Int64, String) =
    await sql"SELECT a.id, a.owner, i.id, i.label FROM accounts a JOIN items i ON i.accountId = a.id"
        .fetchOne<(Int64, String, Int64, String)>(conn)
```

`FromRow` is implemented for tuples of arity 2 through 6, matching the prelude's convention for `Equatable` and `Default`. Positional decoding checks the row width first, so a widened `SELECT` list is an error rather than silently dropped columns — but nothing can catch a *reordered* list of same-typed columns, which is exactly why records are by name.

> **Language dependency — per-field column overrides.** Mapping a field to a differently-named column (e.g. `name` → `user_name`) needs a *field-level attribute* such as `@column("user_name")`. Dovetail's attribute system today supports only `@derive(MacroName)` on a whole declaration, and the derive `input` a macro receives exposes each field's `name` and `ty` but **no per-field attributes**. Column-name overrides are therefore not expressible yet. Until field attributes exist, either align field names with column names or alias the column in the query with `AS`.

### How these derives are provided

`ToSqlValue`, `FromSqlValue`, and `FromRow` are **not** built-in compiler derives. The only built-in derives are `Equatable` (prelude) and `JsonEncoder` / `JsonDecoder` (`standard.json`). These three ship with `standard-sqlite` as **custom derive macros** — Rhai scripts registered in `Dovetail.toml` alongside the existing `standard.json` entries:

```toml
[[project.macro]]
name = "FromRow"
package = "standard.sqlite"
kind = "derive"
trait = "standard.sqlite.FromRow"
script = "macros/FromRow.rhai"
```

Each script reads the `input` description of the type (its `kind`, `fields` / `variants`, `type_params`) and returns a single `implement` block as Dovetail source. See [website/content/book/16-macros.md](../website/content/book/16-macros.md) for the authoring model.

---

## Execution

The execution methods sit on `Query` and layer over the connection primitives that already exist — [`executeWith`](../standard-sqlite/src/Sqlite.dove#L107-L111) and [`queryWith`](../standard-sqlite/src/Sqlite.dove#L120-L123) — so the statement lifecycle, binding, and stepping are unchanged. The row type `T` comes from context.

```dovetail
module Query =
    /// Execute without returning rows (INSERT, UPDATE, DELETE).
    /// Returns the affected row count.
    public function execute(self, conn: Connection): Async<Int64, SqliteError> = ...

    /// Fetch all matching rows.
    public function fetchAll<T>(self, conn: Connection): Async<List<T>, SqliteError>
        where T: FromRow = ...

    /// Fetch zero or one row.
    public function fetchOptional<T>(self, conn: Connection): Async<Option<T>, SqliteError>
        where T: FromRow = ...

    /// Fetch exactly one row; fails if there are zero or more than one.
    public function fetchOne<T>(self, conn: Connection): Async<T, SqliteError>
        where T: FromRow = ...
```

`fetchAll` returns `List<T>` to match `queryWith`, which builds rows into a `List` as it steps.

> **Streaming.** A streaming API for large result sets is desirable — `step` already yields one row at a time, so the driver side is ready — but Dovetail has no `Stream` abstraction yet. Deferred until one lands (see [docs/stream-design.md](stream-design.md)).

**The row type is an explicit type argument:**

```dovetail
async function getUser(conn: Connection, id: UserId): Async<Option<User>, SqliteError> =
    await sql"SELECT id, name, email, status FROM users WHERE id = $id".fetchOptional<User>(conn)

async function countUsers(conn: Connection): Async<Int64, SqliteError> =
    await sql"SELECT COUNT(*) AS count FROM users".fetchOne<Int64>(conn)
```

Inference does not currently propagate a `let` annotation or a function's return type back through `await` into the fetch method's type parameter, so `T` has to be written. Omitting it reports `no matching overload for 'Query.fetchOne' with argument types (Connection)`, which names the right method but not the real cause — worth improving in the typechecker rather than in this library.

The second form needs `implement FromRow for Int64` — a single-column row decoding to a scalar. `standard-sqlite` provides it for each primitive that implements `FromSqlValue`, reading column 0 and failing if the row has a different column count.

### Transactions

Queries compose with the existing [`Connection.transaction`](../standard-sqlite/src/Sqlite.dove#L144-L155) unchanged — it takes a body over the connection, commits on success, and rolls back on failure, panic, or interrupt:

```dovetail
async function transferCredit(conn: Connection, from: UserId, to: UserId, amount: Int64): Async<Unit, SqliteError> =
    await conn.transaction<Unit, SqliteError>(async (tx: Connection) =>
        await sql"UPDATE accounts SET balance = balance - $amount WHERE userId = $from".execute(tx)
        await sql"UPDATE accounts SET balance = balance + $amount WHERE userId = $to".execute(tx)
        ())
```

---

## Complete Example

```dovetail
package com.example.users

import standard.io.Async
import standard.sqlite.Connection
import standard.sqlite.Query
import standard.sqlite.SqliteError

// `FromRow` on the newtype is what makes `fetchOne<UserId>` below work: a
// `RETURNING id` query yields a single-column row, decoded straight to the
// newtype rather than through a wrapper record.
@derive(ToSqlValue)
@derive(FromSqlValue)
@derive(FromRow)
newtype UserId = Int64

@derive(ToSqlValue)
@derive(FromSqlValue)
enum Status =
    Active
    Inactive
    Pending

@derive(FromRow)
record User =
    id: UserId
    name: String
    email: String
    status: Status
    createdAt: Instant

async function findUserById(conn: Connection, id: UserId): Async<Option<User>, SqliteError> =
    await sql"SELECT id, name, email, status, createdAt FROM users WHERE id = $id"
        .fetchOptional<User>(conn)

async function searchUsers(
    conn: Connection,
    nameFilter: Option<String>,
    statusFilter: Option<Status>,
    ids: Option<Array<UserId>>,
    limit: Int32
): Async<List<User>, SqliteError> =
    let query =
        sql"SELECT id, name, email, status, createdAt FROM users"
            .append(Query.whereAndSome([
                nameFilter.map<Query>((name: String) => sql"name LIKE '%' || $name || '%'"),
                statusFilter.map<Query>((status: Status) => sql"status = $status"),
                ids.map<Query>((list: Array<UserId>) => sql"id IN ($..list)")
            ]))
            .append(Query.orderBy([sql"createdAt DESC"]))
            .append(sql" LIMIT $limit")
    await query.fetchAll<User>(conn)

async function createUser(conn: Connection, name: String, email: String): Async<UserId, SqliteError> =
    await sql"""
        INSERT INTO users (name, email, status, createdAt)
        VALUES ($name, $email, ${Status.Active}, unixepoch())
        RETURNING id
    """.fetchOne<UserId>(conn)

async function updateUserStatus(conn: Connection, id: UserId, status: Status): Async<Bool, SqliteError> =
    let affected = await sql"UPDATE users SET status = $status WHERE id = $id".execute(conn)
    affected > 0i64

async function deleteUsers(conn: Connection, ids: Array<UserId>): Async<Int64, SqliteError> =
    await sql"DELETE FROM users WHERE id IN ($..ids)".execute(conn)
```

The connection is a `Resource`, so it is scoped with `use` and closed on scope exit:

```dovetail
async function app(): Async<Unit, SqliteError> =
    let conn = use Connection.open("app.db")
    let id = await createUser(conn, "Alice", "alice@example.com")
    let found = await findUserById(conn, id)
    ()

function main(): Unit =
    Async.run(app())
```

---

## AST Representation

The compiler parses a prefixed string literal into a structural AST — the same shape for any prefix, with no SQL-specific nodes:

```dovetail
record PrefixedLiteral =
    prefix: Identifier                 // e.g. "sql"
    parts: Array<LiteralPart>

enum LiteralPart =
    Literal(String)                    // raw text between interpolations
    Value(Expression)                  // $ident / ${expr}
    Spread(Expression)                 // $..expr
```

There is no `Optional` node: conditional inclusion is fragment composition at the value level, not a literal form.

### Lowering and Type Checking

The compiler lowers the parts to builder calls on the prefix's registered builder type, then type-checks the result like any other expression:

1. **`Literal(text)`** → `builder.literal(text)`
2. **`Value(expr)`** → `builder.value(expr)` — the bound `T: ToSqlValue` comes from `value`'s signature
3. **`Spread(expr)`** → `builder.spread(expr)` — the bound `Array<T>, T: ToSqlValue` comes from `spread`'s signature

Because the bounds live in the builder's method signatures, the type system enforces them with ordinary inference and trait resolution. The compiler carries no knowledge of `ToSqlValue`, `SqlValue`, or SQL itself.

---

## SQL Syntax Validation (Future)

Because the literal text is known at compile time, a future enhancement could validate syntax without a database connection:

1. Parse the literal text with interpolations substituted by placeholders
2. Validate against SQLite's grammar
3. Report syntax errors with source location

```dovetail
// Compile error: syntax error at line 2, column 12
let query = sql"""
    SELECT * FORM users
             ^^^^ expected FROM, found FORM
"""
```

This is a further argument for per-driver ownership: the check is against *SQLite's* grammar, not an abstract one. It would run inside the `sql` handler as a compile-time pass over the literal parts, not in the core compiler, and validates structure only — not schema.

---

## Deferred: a driver trait

If a second driver ever lands and something genuinely cross-driver is needed — a connection pool, tracing middleware, a migration runner — it can be added without disturbing anything above. The shape is a narrow trait over *drivers*, using associated types so no universal value type is reintroduced:

```dovetail
trait SqlConnection =
    type Value
    type Error
    function executeWith(self, sql: String, params: Array<Self.Value>): Async<Int64, Self.Error>
```

`standard.sqlite` would satisfy this with `Value = SqlValue`, `Error = SqliteError` and no changes to its own types. Note this abstracts over *connections*, not over user types: application records keep deriving against one concrete driver.

Associated types are implemented today — the prelude already uses them for `Awaitable.Rebind<U>`, `Div.Output`, and `EarlyReturn.OnFailure` — so this needs no new language feature, only the decision that a second driver is worth having. Nothing in this document depends on it.

---

## Summary

| Syntax | Purpose | Type Requirement |
|--------|---------|------------------|
| `sql"..."` | SQLite query literal (a prefixed literal) | — |
| `$ident` | Interpolate single value | `ident: T` where `T: ToSqlValue` |
| `${ expr }` | Interpolate an expression | `expr: T` where `T: ToSqlValue` |
| `$..expr` | Spread array | `expr: Array<T>` where `T: ToSqlValue` |
| `frag.append(other)` / `a ++ b` | Concatenate fragments | both are `Query` |
| `Query.whenSome(opt, f)` | Conditional fragment | `opt: Option<T>`, `f: (T) => Query` |

| Trait | Purpose |
|-------|---------|
| `ToSqlValue` | Dovetail value → `SqlValue` |
| `FromSqlValue` | `SqlValue` → Dovetail value |
| `FromRow` | `Row` → Dovetail type |

| Layer | Owns |
|-------|------|
| Compiler | Prefixed-literal lexing, the `$` / `${}` / `$..` grammar, the `@stringLiteral` marker, lowering to `literal`/`value`/`spread` |
| `standard-sqlite` | the `sql` prefix, `SqlBuilder`, `Query`, `Row`, the three traits and their derives, `SqlValue`, `SqliteError`, `Connection`, placeholder rendering, execution |

| Still missing | Needed for |
|-------|---------|
| Field-level attributes (`@column("...")`) | Column-name overrides in `FromRow` |
| `Stream` / `AsyncStream` | Streaming large result sets |
| Type-parameter inference through `await` | Dropping the explicit `fetchAll<T>` / `fetchOne<T>` type argument |
