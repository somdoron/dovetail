# Part 22: Standard Library Overview

The compiler bundles `standard.prelude`. Other libraries are projects selected in
`Dovetail.toml`: add a matching `standard-tag` (or an explicit Git dependency), then
list project names in `depends`. Import declarations by their **package** names.
For example, project `standard-json` exposes package `standard.json`.

See [Dependency Management](02-tool-commands.md#23-dependency-management) for a full
manifest. The examples here state their dependencies. In an initialized project,
replace `src/main.dove` with the whole example and set `root_package` to its `package`
name. Each example includes a test and can be checked, built, run, and tested.

## 22.1 Core Types

The [prelude source](../../../dovetail/prelude/src/) includes primitives, `String`, `Option`,
`Result`, `List`, `Array`, slices, `BigInt`, `Decimal`, and core traits such as
`Equatable`, `Comparable`, `Hashable`, and `Display`. These need no dependency or import.

Use `Option<T>` for ordinary absence and `Result<T, E>` for operations that can fail.
Private newtypes can validate data at construction. `@derive(Equatable)` opts a record
or enum into structural equality; equality is not automatically granted to every type.

**Complete example (checked in CI)** — no extra dependencies:

<!-- book-example: {"name": "core", "depends": []} -->
```dovetail
package core

newtype Quantity private = Int32

module Quantity =
    public function parse(value: Int32): Result<Quantity, String> =
        if value > 0 then Ok(Quantity(value))
        else Error("quantity must be positive")

    public function count(self): Int32 = self.value

function main(): Unit =
    assert Quantity.parse(3).require.count() == 3
    assert Quantity.parse(0).isError

test "quantity rejects zero" = main()
```

Here `.require` is appropriate in a test with a known value. In an application,
handle the error or propagate it with `try`/`orReturn`. See [Error Handling](10-error-handling.md).

## 22.2 Collections

`[1, 2, 3]` creates an immutable `List<Int32>`. `[|1, 2, 3|]` creates a fixed-size,
mutable `Array<Int32>`. `Slice<T>` is a writable view of an array, and
`ReadonlySlice<T>` prevents writes through that view without freezing other aliases.

List `length` and indexed lookup traverse the list. Arrays provide indexed access
and a stored length. Choose arrays for bytes, mutation, and repeated random access;
lists are useful for immutable sequential processing.

The [standard-collection project](../../../standard-collection/src/) adds `ArrayList`,
`MutableMap`, `MutableSet`, `MutableQueue`, `MutableStack`, `Range`, and `StringBuilder`.
Several mutable collections currently require `Default` elements; maps require both
`Default` keys and values, plus `Hashable` and `Equatable` keys. Validated types may
not have a meaningful default. Check these bounds before choosing a collection.

**Complete example (checked in CI)** — `depends = ["standard-collection"]`:

<!-- book-example: {"name": "collections", "depends": ["standard-collection"]} -->
```dovetail
package collections

import standard.collection.MutableMap

function main(): Unit =
    let counts = MutableMap<String, Int32>.new()
    counts.put("apples", 3)
    assert counts.get("apples").require == 3
    assert counts.get("pears").isNone
    let values = [1, 2, 3].map((value: Int32) => value * 2)
    assert values.foldLeft(0, (sum: Int32, value: Int32) => sum + value) == 12
    let array = values.toArray()
    array[0] = 9
    assert values.head == 2

test "collections preserve the expected values" = main()
```

Some existing collection constructors are named `new`; use the actual library API
even though new application APIs conventionally use `make`.

## 22.3 Text Processing

`String` is UTF-8 text. Its prelude module provides interpolation, comparison,
searching, and conversion to bytes or characters. Concatenate strings with `++`.
`standard.text.Charset` names encodings; currently it contains `Utf8`.
`standard.encoding` provides `Base64` and `Hex` codecs with typed decoding errors.
`standard.parser` provides composable parsers; see [Parser Composition](25-tuple-extension.md#255-parser-composition).

**Complete example (checked in CI)** — `depends = ["standard-encoding"]`:

<!-- book-example: {"name": "encoding", "depends": ["standard-encoding"]} -->
```dovetail
package encoding

import standard.encoding.Base64

function main(): Unit =
    let bytes = "hello".bytes()
    let encoded = Base64.encode(bytes)
    assert encoded == "aGVsbG8="
    assert Base64.decode(encoded).require.readonly == bytes.readonly

test "base64 round trips bytes" = main()
```

Byte chunks are not necessarily complete UTF-8 characters or complete protocol
messages. Preserve incomplete input when implementing a streaming decoder.

## 22.4 Math

`BigInt` and `Decimal` are prelude types, with `big` and `dec` literal suffixes.
Decimal literals are constructed exactly from source digits. Both types support
addition, subtraction, multiplication, negation, equality, and ordering. Decimal
division and rounding operations are not provided yet. Normalization removes
insignificant trailing zeros; Decimal is not a fixed-display-scale money type.

The [standard-math project](../../../standard-math/src/) currently provides
`RoundingMode`; the presence of that enum does not imply a Decimal
rounding API.

**Complete example (checked in CI)** — no extra dependencies:

<!-- book-example: {"name": "decimal", "depends": []} -->
```dovetail
package decimal

function main(): Unit =
    assert 0.1dec + 0.2dec == 0.3dec
    assert 19.99dec * 3dec == 59.97dec
    assert 12345678901234567890big + 1big == 12345678901234567891big

test "decimal arithmetic is exact" = main()
```

## 22.5 I/O

`standard.io` supplies `Async<T, E>`, `Resource<T, E>`, fibers, synchronization, and
console/byte-stream I/O. An async value describes work; constructing or discarding
it does not run it. Use `await` inside async code and `.run()` at the application
boundary. Acquire resources with `use` so their cleanup is scoped.

**Complete example (checked in CI)** — `depends = ["standard-io"]`:

<!-- book-example: {"name": "console", "depends": ["standard-io"], "stdout": "Hello from async I/O!\n"} -->
```dovetail
package console

import standard.io.Async
import standard.io.Console
import standard.io.ConsoleError

async function greet(): Async<Unit, ConsoleError> =
    await Console.writeLine("Hello from async I/O!")

function main(): Unit = greet().run()

test "an async result can be consumed" =
    let program: Async<Unit, Never> = async do
        assert (await Async.succeed(42)) == 42
    program.run()
```

Console failures remain in the typed error channel. `Console.writeLine` is an async
operation; there is no synchronous `Console.println`. The prelude's `debug` is a
separate diagnostic convenience.

`standard.io.fs` adds `Path`, file/directory operations, and file streams.
`standard.io.log` adds structured logging. `dovetail run` needs filesystem grants
for file access; see [runtime permissions](02-tool-commands.md#runtime-permissions).
See [Resource Management](13-resources.md) for release and cancellation rules.

## 22.6 Networking

| Project | Package | Purpose |
|---|---|---|
| `standard-uri` | `standard.uri` | URI representation and parsing |
| `standard-http` | `standard.http` | HTTP/1.1 protocol codec without I/O |
| `standard-io-net` | `standard.io.net` | Async TCP networking |
| `standard-io-http` | `standard.io.http` | HTTP/HTTPS client, server, routing, buffered bodies |
| `standard-tls` | `standard.tls` | TLS 1.3 protocol implementation |
| `standard-io-crypto` | `standard.io.crypto` | Async TLS streams and managed crypto resources |

Use `dovetail run <project> --allow-network` for programs that open sockets or resolve
names. HTTP has basic routing; it is not a complete web framework. The HTTP/TLS
stack has outstanding hardening and conformance work. Read the
[HTTP readiness inventory](../../../docs/http-library-production-readiness.md) and the
[TLS implementation boundaries](../../../docs/tls-library-design.md) before relying on it
for production workloads. Some older HTTP inventory entries lag the source; the
inventory is not a guarantee that a listed feature is absent or complete.

## 22.7 Time

`standard.time` provides values such as `Duration`, `Instant`, calendar dates, times,
and time zones. Constructing or comparing those values is pure; obtaining the
current time is I/O through the clock APIs. Pass time into business rules so tests
can choose it explicitly.

**Complete example (checked in CI)** — `depends = ["standard-time"]`:

<!-- book-example: {"name": "time", "depends": ["standard-time"]} -->
```dovetail
package time

import standard.time.Duration
import standard.time.Instant

function main(): Unit =
    let start = Instant.ofEpochSecond(1700000000i64)
    let later = start.plus(Duration.ofSeconds(60i64))
    assert later == Instant.ofEpochSecond(1700000060i64)

test "time arithmetic uses an explicit instant" = main()
```

For deterministic tests, `standard.io.TestClock` supplies scoped virtual time to
the time extensions, `Async.sleep`, and `timeout`, with real-clock fallback outside
the installation. See [Scoped Test Clocks](13-resources.md#139-scoped-test-clocks).

## 22.8 JSON

`standard.json.Json` is a JSON value tree. `Json.parse` returns a `Result`;
`json.encode()` serializes the tree. `JsonEncoder` and `JsonDecoder` support typed
conversion, and derives can generate implementations for public data shapes.
The decoder macro currently emits unqualified `Json` and `JsonError` names; import
both alongside the traits, as in the example.

**Complete example (checked in CI)** — `depends = ["standard-json"]`:

<!-- book-example: {"name": "json", "depends": ["standard-json"]} -->
```dovetail
package json

import standard.json.Json
import standard.json.JsonEncoder
import standard.json.JsonDecoder
import standard.json.JsonError

@derive(Equatable)
@derive(JsonEncoder)
@derive(JsonDecoder)
record Message =
    text: String

function main(): Unit =
    let original = Message { text = "hello" }
    let wire = original.toJson().encode()
    let decoded = Message.fromJson(Json.parse(wire).require).require
    assert decoded == original

test "a record round trips through JSON" = main()
```

The JSON tree stores numbers as `Float64`; do not assume arbitrary-precision integers
or decimals round-trip through a JSON number losslessly. Private construction is
also respected: a generated decoder cannot bypass a private constructor. Decode a
public input shape, then validate it through the private type's module.

## 22.9 Randomness and Crypto

The `wasi` project exposes host randomness through `standard.wasi.random`.
`standard.crypto` contains cryptographic primitives; `standard.crypto.roots` holds
trust anchors. `standard.io.crypto` supplies effectful random-key generation,
certificate loading, and resource-managed secrets.

Randomness and secret generation belong at the I/O boundary. Inject generated
identifiers into domain logic. Consult the [crypto design and implementation status](../../../docs/crypto-library-design.md)
for supported primitives and cleanup limitations; do not infer full protocol
readiness from the existence of a primitive.

## 22.10 Streams and SQLite

`standard-io-stream` exposes `standard.io.stream.Stream`. It supports chunked sources,
transformations, sequential composition, concurrent merging, resource lifetimes,
and terminal consumers. See [Streams](14-streams.md) for backpressure and ownership
rules. `runCollect()` retains the whole result; use a fold or output consumer for
large or unbounded sources.

`standard-sqlite` exposes `standard.sqlite`, bringing a prebuilt SQLite component
and its generated bindings transitively. Connections/statements are resources;
queries return async values. See the complete [SQLite example](17-components.md#173-using-sqlite)
for parameters and transactions. The current SQLite build has filesystem-locking
limitations and no WAL mode. Postgres and Redis clients are not provided yet.
