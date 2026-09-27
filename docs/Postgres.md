# PostgreSQL Support in Dovetail

This document designs Dovetail's PostgreSQL support: a sans-io wire-protocol core, a byte-level codec library, and an async driver — layered so that the protocol is pure and testable, the IO is thin, and the SQL surface is shared with any future database.

> **Status: design proposal.** Companion to [SQL.md](SQL.md) (the `sql"..."` literal and `Sql` fragment). Nothing here is implemented. The plan is **foundation-first**: the cryptographic and transport primitives a real server needs (§3) come before the driver, because they gate connecting to anything beyond a local trusted socket.

---

## 1. Scope and Philosophy

- **PostgreSQL-specific, not a generic database abstraction.** Like [skunk](https://github.com/typelevel/skunk), the driver targets one protocol and embraces its features (extended query protocol, binary format, `COPY`, `LISTEN`/`NOTIFY`, arrays, typed errors) instead of a JDBC-style lowest common denominator. Each database gets its own protocol library and driver; they share only the SQL surface and the codec toolkit. Dovetail monomorphizes and favors explicitness — a leaky universal abstraction fights both.
- **Sans-io.** The wire protocol is a **pure state machine**: bytes and intents in, bytes and events out, zero IO. The async driver pumps it. This is not a new pattern for this codebase — `standard-http` already is a sans-io HTTP state machine (`Connection.client()` + [ReceiveBuffer](../standard-http/src/ReceiveBuffer.dove)) driven by the thin TCP client in [standard-io-http/src/transport.dove](../standard-io-http/src/transport.dove). PostgreSQL mirrors that split exactly. Sans-io also fits Dovetail's async model, where `Async` is an inert description run separately ([website/content/book/12-async.md](../website/content/book/12-async.md)).
- **Reuse the SQL surface.** `sql"..."`, fragment composition, and the `ToSqlParameter`/`FromSql`/`FromRow` traits live in `standard-sql` and are database-agnostic. The driver consumes them.
- **Inspirations, reimagined.** skunk for the layering; [scodec](https://github.com/scodec/scodec) for codecs — but reworked for a language with no implicits and no HLists (§4).

---

## 2. Package Architecture

```
standard-codec       (pure)  byte-level codec combinators + @derive(Codec)
standard-crypto      (pure)  SHA-256, HMAC, PBKDF2 (+ AEAD/ECDHE/X.509 later for TLS)
standard-sql         (pure)  sql"..." literal, Sql fragment, ToSqlParameter/FromSql/FromRow, SqlValue
standard-postgres    (pure)  wire messages + framing + connection state machine  ← sans-io
standard-tls         (pure)  TLS record layer + handshake state machine          ← sans-io, optional path (§3.2)
standard-io-postgres  (io)   Tcp loop, Session, pool, transactions; the Connection
                             that standard-sql's execute/fetch talk to
```

Dependency DAG (no cycles, per the package rules):

| Package | Depends on | Layer |
|---|---|---|
| `standard-codec` | prelude | pure |
| `standard-crypto` | prelude, `standard-codec` | pure |
| `standard-sql` | prelude | pure |
| `standard-postgres` | `standard-codec`, `standard-crypto` | pure (sans-io) |
| `standard-tls` | `standard-codec`, `standard-crypto` | pure (sans-io) |
| `standard-io-postgres` | `standard-io`, `standard-io-net`, `standard-postgres`, `standard-sql`, (`standard-tls`) | io |

`standard-postgres` deliberately does **not** depend on `standard-sql`: the protocol core deals in OIDs, format codes, and raw `Array<Uint8>`, so it stays usable standalone. The driver bridges `SqlValue ↔ wire` (§7).

---

## 3. Foundation Prerequisites (build first)

Neither of these exists today, and both sit *below* `standard-io`. The pure protocol core (§5) can be built and unit-tested without them, but connecting to a typical real server cannot.

### 3.1 `standard-crypto` — authentication

Modern PostgreSQL defaults to **SCRAM-SHA-256**. The stdlib has Base64/Hex but no hashing, so SCRAM is blocked. Needed:

- **SHA-256** — the hash.
- **HMAC-SHA-256** — challenge/response.
- **PBKDF2-HMAC-SHA-256** — salted iteration of the password.
- **A CSPRNG** for the client nonce — from `wasi:random/random`. This is an *effect*, so it lives in the driver; the pure SCRAM computation takes the nonce as input and stays testable.
- (Legacy) **MD5** for `md5` auth — deprecated, lower priority.

Crucially, **crypto unlocks auth independently of transport**: SCRAM is a challenge-response handshake and works fine over a plaintext connection. So `standard-crypto` alone makes most real servers reachable over a trusted network (sidecar, VPC, `sslmode=disable`), *before* any TLS exists. It is the smaller, higher-leverage half of the foundation, and it is well-specified and verifiable against published test vectors.

### 3.2 TLS — transport encryption

There is **no TLS** in the stdlib (the HTTP client even rejects `https://` "until TLS lands"). Managed PostgreSQL (RDS, Neon, Supabase, …) effectively *requires* SSL, so this gates production use. Three routes, in order of preference:

1. **Host-provided TLS (`wasi-tls` or a host import).** Bind a TLS interface offered by the runtime (wasmtime). Far less work, and — importantly — delegates a security-critical implementation to a vetted one. **Open question:** the WASI TLS story is unsettled; WASI p2 standardizes `wasi-sockets` (TCP/UDP) but not TLS. This route depends on what the host actually exposes.
2. **Sans-io `standard-tls`.** A pure TLS 1.3 record-layer + handshake state machine in the [rustls](https://github.com/rustls/rustls) mold, driven by the same IO loop as everything else — architecturally consistent. But it needs a large `standard-crypto`: AEAD (AES-GCM / ChaCha20-Poly1305), X25519 ECDHE, HKDF, and X.509 parsing + chain validation. Months of work.
3. **Pure hand-rolled TLS — not recommended.** 🚩 Rolling your own TLS is a classic security footgun; prefer a vetted implementation (route 1, else carefully-reviewed route 2).

PostgreSQL's TLS negotiation itself is trivial (an `SSLRequest` byte exchange before STARTTLS); the cost is entirely the TLS stack underneath.

---

## 4. `standard-codec` — byte-level codecs

A scodec-*inspired* combinator library, deliberately simpler than a literal port:

- **Byte-level, not bit-level.** The PostgreSQL v3 protocol is fully byte-aligned (int8/16/32/64, length-prefixed and NUL-terminated strings, byte arrays — no sub-byte packing). Drop scodec's `BitVector` entirely; an `Array<Uint8>` plus a `Reader`/`Writer` cursor is enough.
- **Flat-tuple aggregation via `~`/`++`, not HLists.** Dovetail has no implicits or shapeless. The [tuple-extension design](tuple-extension-design.md) gives exactly the missing piece: `Codec` implements `Concat`, so codecs combine into **flat** tuples — `Codec<A> ++ Codec<B> : Codec<A ~ B>` — the same ergonomics scodec gets from `~`/`::`, with none of the HList machinery. Inductive tuple impls (also from that doc) let one `Codec` derivation cover all arities.

```dovetail
// Primitives (big-endian, byte-aligned)
let int16:  Codec<Int16>
let int32:  Codec<Int32>
let oid:    Codec<Oid>            // 4-byte unsigned
let cstring: Codec<String>       // NUL-terminated
let lengthPrefixed: Codec<Array<Uint8>>   // Int32 length, then bytes

// Combination is flat, left-associative
let field: Codec<(String, Oid, Int16)> = cstring ++ oid ++ int16

// Bijection into a record
record FieldDescription =
    name: String
    typeOid: Oid
    typeSize: Int16

let fieldCodec: Codec<FieldDescription> =
    field.map(
        t -> FieldDescription { name = t._0; typeOid = t._1; typeSize = t._2 },
        f -> (f.name, f.typeOid, f.typeSize))
```

A `@derive(Codec)` macro (a Rhai derive, like `JsonEncoder`) generates the field-by-field codec for records and a tag-discriminated codec for enums — the latter is how the message types in §5 get encoded/decoded with their 1-byte type tags. `standard-codec` is generally useful beyond PostgreSQL (any binary format), which is why it is its own package.

---

## 5. `standard-postgres` — sans-io protocol core

Pure. No `standard-io`. Three parts:

1. **Messages.** Frontend and backend message types as enums with `@derive(Codec)`:
   - *Frontend:* `Startup`, `Query`, `Parse`, `Bind`, `Describe`, `Execute`, `Sync`, `Close`, `Terminate`, `PasswordMessage`/`SASLInitialResponse`/`SASLResponse`.
   - *Backend:* `Authentication*`, `RowDescription`, `DataRow`, `CommandComplete`, `ReadyForQuery`, `ErrorResponse`, `NoticeResponse`, `ParameterStatus`, `BackendKeyData`, `NotificationResponse`, `ParseComplete`, `BindComplete`, `NoData`, …

2. **Framing.** A `ReceiveBuffer`-style accumulator (mirroring [standard-http's](../standard-http/src/ReceiveBuffer.dove)) that, given received bytes, yields zero or more *complete* backend messages and retains the partial remainder. Every backend message is `[1-byte tag][Int32 length][body]`, so framing is uniform.

3. **Connection state machine.** A mutable class (idiom of the IO `Runtime` and HTTP `Connection`) holding protocol state — `AwaitingAuth`, `Ready { transactionStatus }`, `InExtendedQuery`, etc. Shape:

```dovetail
public class PgProtocol =
    // Feed bytes from the transport; returns decoded events.
    public function received(self, data: Array<Uint8>): Array<BackendEvent>
    // Queue a high-level intent; bytes accumulate in the send buffer.
    public function send(self, msg: FrontendMessage): Unit
    // Drain bytes the driver should write to the transport.
    public function takeOutgoing(self): Array<Uint8>
    public property state(self): ProtocolState
```

This is fully unit-testable against recorded server byte streams — no socket, no async. It deals in OIDs/format-codes/raw bytes only; it knows nothing about `SqlValue` or `sql"..."`.

**Mapping to skunk:** skunk's `MessageSocket`/`BufferedMessageSocket` → our framing (pure) + the driver's socket loop; skunk's `Protocol` exchanges (startup, simple query, extended query) → state-machine sequences here; skunk's `Session` → §6. Note skunk is *not* strictly sans-io (fs2/cats-effect runs throughout); making the core pure is a deliberate improvement.

---

## 6. `standard-io-postgres` — the driver

Depends on `standard-io`/`standard-io-net`, drives the §5 core, and exposes the high-level API.

- **Transport loop.** `Tcp.connect(host, port): Resource<TcpStream, NetError>` ([standard-io-net/src/Tcp.dove](../standard-io-net/src/Tcp.dove)); read via `stream.input.read(n)` and feed `protocol.received(bytes)`; write `protocol.takeOutgoing()` via `stream.output.write(bytes)`. (Wrap the stream in `standard-tls` / host TLS when enabled.)
- **Extended query protocol** for parameter binding: Parse → Bind → Describe → Execute → Sync. This is what makes `$1` parameters real and enables a **prepared-statement cache**.
- **`SqlValue ↔ wire` bridge** (§7): render an `Sql` fragment to a parameterized statement + a parameter list, encode each `SqlValue` to its PostgreSQL binary/text representation, and decode `DataRow` fields back through `FromSql`/`FromRow`.
- **`Session` / `Connection` API** — the `Connection` that `standard-sql`'s `execute`/`fetchAll`/`fetchOptional`/`fetchOne` ([SQL.md](SQL.md)) accept.
- **Beyond MVP:** connection pool, transactions (`begin`/`commit`/`rollback` + savepoints), `LISTEN`/`NOTIFY` channels (the async `NotificationResponse` path, buffered like skunk's `BufferedMessageSocket`), `COPY`, streaming large result sets (waits on a `Stream` abstraction in `standard-io`).

---

## 7. Generic vs PostgreSQL-specific — the seam

`SqlValue` ([SQL.md](SQL.md)) is the **shared currency**. `ToSqlParameter`/`FromSql` produce/consume it once and work for every driver, so user code (`@derive(FromRow) record User`) is portable. What is PostgreSQL-specific is the *encoding* of each `SqlValue` to the wire (OID + format + bytes), which lives in the driver.

A shared `SqlValue` is a lowest-common-denominator, so PostgreSQL gets an **escape hatch** for types that don't fit (ranges, geometric, `tsvector`, custom enums-by-OID): a `SqlValue.Custom(oid: Oid, bytes: Array<Uint8>)` variant and/or a supplementary `ToPgParameter` trait for full-fidelity binding. A future `standard-mysql`/`standard-io-mysql` pair reuses `standard-codec` + `standard-sql` and re-implements only the wire layer.

---

## 8. Authentication Flows

| Method | Needs | Status |
|---|---|---|
| `trust` / Unix peer | nothing | works with the plaintext MVP |
| cleartext password | nothing (send password) | works, but rarely enabled |
| `md5` | MD5 (legacy) | needs minimal crypto; deprecated |
| **SCRAM-SHA-256** | SHA-256 + HMAC + PBKDF2 + CSPRNG nonce | default on modern servers; needs `standard-crypto` (§3.1) |
| SCRAM-SHA-256-PLUS | above + TLS channel binding | needs TLS (§3.2); optional |

---

## 9. Build Order

1. **`standard-crypto`** — SHA-256, HMAC-SHA-256, PBKDF2 (verify against test vectors). Unlocks SCRAM.
2. **`standard-codec`** — `Reader`/`Writer` + big-endian primitives + `cstring`/`lengthPrefixed`; then `Concat`/`++` combination and `@derive(Codec)` (depends on the tuple-extension phases).
3. **`standard-postgres`** — messages + framing + state machine: startup → SCRAM auth → `ReadyForQuery`, then simple query, then extended query. Pure; tested against recorded bytes.
4. **`standard-io-postgres` (plaintext)** — single connection over `Tcp`, wired to `standard-sql` `execute`/`fetch`. Usable for local/trusted-network servers with SCRAM.
5. **TLS** — host/`wasi-tls` if available, else sans-io `standard-tls` (expands `standard-crypto` with AEAD/ECDHE/X.509). Enables managed PostgreSQL.
6. **Production** — pool, transactions, prepared-statement cache, `LISTEN`/`NOTIFY`, `COPY`, streaming.

---

## 10. Open Questions and Risks

- **WASI TLS.** 🚩 The single biggest unknown. Does the target runtime expose a TLS interface, or must we ship a sans-io TLS + full crypto stack? This decides whether step 5 is "bind an import" or "a multi-month project," and gates managed-PostgreSQL support.
- **Streaming.** `fetchAll` returns `Array<T>`; true row streaming needs a `Stream`/`AsyncStream` in `standard-io`, which doesn't exist yet.
- **Binary vs text format.** Binary is efficient and exact but requires per-type codecs for every OID we support; text is simpler but lossy/slower. Likely: binary for the common types, text fallback otherwise.
- **`SqlValue` breadth.** How much of PostgreSQL's type zoo to model as first-class `SqlValue` variants vs. push through `Custom(oid, bytes)`.
- **Tuple-extension dependency.** `standard-codec`'s ergonomics (and inductive `Codec` derivation) depend on the [tuple-extension](tuple-extension-design.md) `~`/`Concat`/`++` work landing first.

---

## 11. References

- [skunk](https://github.com/typelevel/skunk) — functional PostgreSQL client (architecture model)
- [scodec](https://github.com/scodec/scodec) — binary codec combinators (codec model)
- [rustls](https://github.com/rustls/rustls) — sans-io TLS (model for `standard-tls`)
- [PostgreSQL Frontend/Backend Protocol](https://www.postgresql.org/docs/current/protocol.html)
- [SCRAM (RFC 5802)](https://datatracker.ietf.org/doc/html/rfc5802) / [SCRAM-SHA-256 (RFC 7677)](https://datatracker.ietf.org/doc/html/rfc7677)
- Internal: [SQL.md](SQL.md), [tuple-extension-design.md](tuple-extension-design.md), [website/content/book/12-async.md](../website/content/book/12-async.md)
