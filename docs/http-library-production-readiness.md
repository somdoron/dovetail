# HTTP Library — Path to Production

This document inventories what the Dovetail HTTP stack does today, what is missing,
and what it would take to make `standard.io.http` production ready. It is a planning
doc, not a design spec — each large item links out to (or should grow) its own design
doc before implementation.

**Status:** `standard.io.http` is functional end-to-end (the `standard-io-http-interop`
smoke test exercises a Dovetail client against a Rust server and a Dovetail server against a
Rust client) but is a **v1 / proof-of-concept**. It is not yet safe to expose to
untrusted clients or to depend on for real workloads.

---

## 1. Current Architecture

The stack is two layers:

### `standard.http` — sans-I/O protocol codec
A pure Dovetail port of Python's [h11](https://github.com/python-hyper/h11). Bytes in,
events out, no I/O. This layer is **comparatively mature**:

- Full HTTP/1.1 request/response framing.
- Body framing: Content-Length, **chunked transfer-encoding**, and HTTP/1.0
  EOF-delimited bodies (RFC 7230 §3.3.3).
- **Keep-alive** via `startNextCycle()` (reset both connections to `Idle`).
- Connection state machine with `Connection: close` / `keep-alive` tracking.
- **Protocol upgrade / CONNECT** tunneling primitives (`SwitchedProtocol`, `trailingData()`).
- Header validation, Host-header rules, Content-Length de-duplication, Transfer-Encoding
  checks, buffer-overflow protection (`maxIncompleteEventSize`, default 16 KiB).
- Trailer headers carried on `EndOfMessage`.

Key point: **most of the protocol capability already exists in the codec** — the gaps
below are mostly in the *I/O layer that drives it*, not in the codec.

### `standard.io.http` — async I/O wiring + app API
Drives the codec over `TcpStream` and provides the application-facing API:

- **Client:** `HttpClient.get/post/...` → `RequestBuilder` → `send`. One TCP connection
  per request, always `Connection: close`. No pooling, no redirects, no cookies, no TLS
  (`https://` is rejected).
- **Server:** `HttpServer.serve(address, route)`. Fork-per-connection with a
  `Queue<Bool>` semaphore capping concurrency (default 128). Always emits
  `Connection: close`, so connection : request is 1:1.
- **Routing:** `Route.get/post/.../choose` combinators, `PathPattern` with path params,
  `QueryParams`, `Context` passed to handlers.
- **Bodies:** flat `Array<Uint8>` on both `Request` and `Response`; `bodyAsString()` /
  `jsonBody<T>()` helpers; `Response.text/json/bytes` constructors.

This layer is the **immature one** and is where almost all the production work lives.

---

## 2. Correctness & Robustness Gaps (fix before *any* production use)

These are not features — they are bugs or omissions that make the v1 unsafe or incorrect.
They should be addressed first.

### 2.1 No timeouts anywhere
There is **no** connect timeout, read timeout, write timeout, idle timeout, or
whole-request deadline. A single slow or malicious client holds a handler fiber (and one
of the 128 concurrency permits) **forever**. With ~128 slow connections the server is
fully wedged — classic **Slowloris**. This is the single most important robustness gap.

Needs: per-phase deadlines (header read, body read, handler run, response write) and an
idle-connection timeout, integrated with the async runtime's cancellation/`race`.

### 2.2 405 vs 404 on method mismatch
`Route` returns `None` (→ 404 Not Found) when the path matches but the method does not.
Correct behavior is **405 Method Not Allowed** with an `Allow` header listing the methods
registered for that path. Requires the router to know all methods bound to a path.

### 2.3 HEAD responses still send a body
The server path emits the response body unconditionally. For `HEAD` it must send headers
+ Content-Length but **no body**. (The codec knows about HEAD framing on the *receive*
side; the server emit path does not special-case it.)

### 2.4 O(n²) body accumulation
Both `transport.dove` and `server_transport.dove` accumulate the body with
`bodyBytes = bodyBytes.concat(chunk)` inside the read loop — reallocating and copying the
whole buffer on every chunk. For large bodies this is quadratic. Use a growable buffer
(`ArrayList<Uint8>` or a rope/segment list) and materialize once.

### 2.5 Body size limit is server-only and post-hoc
`maxBodyBytes` exists on the server but the **client** has no equivalent (a malicious or
buggy server can stream an unbounded body into client memory). Also there is no limit on
header count / total header size at the I/O layer beyond the codec's per-event
`maxIncompleteEventSize`.

### 2.6 No `Expect: 100-continue` handling
A client sending `Expect: 100-continue` will stall — the server never emits the interim
100 response. Needs handling on both sides (codec supports `InformationalResponse`).

### 2.7 Missing standard response headers
No automatic `Date` header (required by RFC 7231 for origin servers), no default `Server`
header. Add (configurable) defaults.

### 2.8 Connection error handling on emit
`ServerError.EmitFailed` is mapped to a 500 *response* in `errorToResponse`, but if the
emit itself failed the socket is already half-broken — writing a 500 on top is futile.
Audit the error-to-response path so failures that occur mid-write just drop the socket.

---

## 3. Feature Gaps for v1 Production (the user's list + framing)

### 3.1 Keep-alive (client + server)
**Server:** stop hard-coding `Connection: close`. After a request/response cycle, call
`startNextCycle()` on the codec and loop on the same `TcpStream` instead of closing.
Needs: keep-alive idle timeout, max-requests-per-connection cap, honoring client
`Connection: close` and HTTP/1.0 semantics. Interacts directly with §2.1 (idle timeout)
and changes the connection:request model from 1:1 to 1:N.

**Client:** a real reusable client instance with a **connection pool keyed by
(scheme, host, port)**. Today `HttpClient` is a stateless module that opens a fresh
socket per call. Production needs:
- A `Client` *value/resource* that owns the pool (so it can be `use`d and closed).
- Per-host connection caching with max-idle-per-host, max-total, idle eviction.
- Checkout/return lifecycle integrated with `startNextCycle()`.
- Correct handling of servers that close a pooled connection between requests (retry on a
  fresh connection for idempotent methods).

This is the biggest single client change and should get its own design doc.

### 3.2 Middleware infrastructure + standard middleware
There is no middleware/filter concept today — `Context` was explicitly shaped to allow
"middleware-injected state in the future," but nothing consumes it.

Needs:
- A middleware type — likely `(Context, next) => Async<Response, ServerError>` where
  `next` is the downstream handler — composable around routes.
- A way to attach typed state to `Context` (currently fixed fields: request, pathParams,
  queryParams). Consider an extensible bag or a generic context.
- **Standard middleware to ship:** access logging, request ID, CORS, gzip/deflate
  compression, panic-recovery → 500, basic/bearer auth, rate limiting, timeout wrapper,
  static-file serving (see §3.3), `Date`/`Server` header injection.

### 3.3 Static file support
Serve files from a directory. Needs: path-traversal protection (reject `..`, normalize),
content-type sniffing by extension, `Last-Modified`/`ETag` + conditional requests
(`If-None-Match` / `If-Modified-Since` → 304), **Range requests** (206 Partial Content),
directory index handling, and streaming the file body (§3.4) rather than buffering it.
Depends on the `fs` library.

### 3.4 Streaming bodies (client + server)
Today every body is a fully-buffered `Array<Uint8>`. Production needs streaming in both
directions:
- **Server request body:** hand the handler a stream instead of buffering (large
  uploads).
- **Server response body:** let a handler write incrementally (large downloads, SSE,
  generated content) — wire to **chunked transfer-encoding**, which the codec already
  supports.
- **Client request body:** stream an upload from a source.
- **Client response body:** expose a stream rather than buffering the whole response.

This is a significant API change: `Request`/`Response` bodies become a body *abstraction*
(buffered | stream | empty) rather than a flat byte array. Sits on top of
`AsyncInputStream`/`AsyncOutputStream`. Server-Sent Events fall out of response streaming.

### 3.5 Multipart (client + server)
`multipart/form-data` encoding (client, for file uploads + form fields) and decoding
(server, parsing parts with headers + bodies). Also `application/x-www-form-urlencoded`
parsing/encoding (small, should land alongside). Streaming multipart (§3.4) for large
uploads is a stretch goal; a buffered version is the v1.

### 3.6 Graceful shutdown (server)
Stop accepting new connections (close the listener), then allow in-flight requests up to a
grace period (e.g. N seconds) to complete before cancelling the rest and closing.
Half of this now falls out of fiber scopes: handlers are forked structurally into the serve
fiber's scope, so a serve loop that *ends on its own* already drains — it waits for the
in-flight handlers before finishing — while a *cancelled* server still interrupts them
immediately. What is missing is the part scopes cannot supply: a shutdown signal/handle that
makes the loop stop accepting, a grace deadline after which the remaining handlers are
cancelled rather than awaited, and integration with OS signals (SIGINT/SIGTERM) where the
WASI host exposes them.

---

## 4. Additional Gaps (what else is missing)

Beyond the user's list, a production HTTP stack needs:

### Client
- **Redirect following** (configurable max hops, method/body rules per 301/302/303/307/308,
  loop detection).
- **Cookie jar** (per-host, expiry, domain/path matching, secure/httponly).
- **Retries with backoff** for idempotent requests / connection-reset on pooled
  connections (interacts with §3.1).
- **Decompression** — transparent gzip/deflate/br on responses (`Accept-Encoding` +
  `Content-Encoding`). Depends on a compression library.
- **Proxy support** — HTTP proxy + CONNECT tunneling (codec already supports CONNECT). The
  code comments already anticipate "proxy / forwarding code."
- **Per-request timeouts / deadlines** and cancellation (ties to §2.1).
- **DNS / address selection** — IPv6, multiple A/AAAA records, Happy Eyeballs. Depends on
  what `standard.io.net.Tcp.connect` exposes.
- **`application/x-www-form-urlencoded`** request bodies and a typed form builder.

### Server
- **CORS** handling (likely middleware, §3.2).
- **Authentication helpers** (basic, bearer) — middleware.
- **Rate limiting / connection limiting** beyond the flat concurrency cap (per-IP).
- **Compression** of responses (gzip/deflate) negotiated via `Accept-Encoding` — middleware.
- **Custom error pages** — pluggable 404 / 405 / 500 handlers (today they are hard-coded
  empty-body responses in `errorToResponse` / `orNotFound`).
- **Virtual hosting** by Host header / SNI.
- **`OPTIONS` / `Allow`** auto-responses; automatic `HEAD` from a `GET` route.
- **Trailers** on responses (codec supports them; no app API).
- **Slowloris / header-flood protection** (ties to §2.1, §2.5).

### Cross-cutting
- **Observability** — structured access logs, metrics (request count/latency/in-flight),
  and tracing hooks. None exist today.
- **WebSocket** support — the codec has upgrade/`SwitchedProtocol` primitives, but there is
  no WebSocket framing layer or handshake helper.
- **Correct percent-encoding** throughout URL/query/path handling (audit `Uri` +
  `QueryParams` + `PathParams` for encode/decode correctness and edge cases).
- **Content negotiation** (`Accept`, `Accept-Charset`, `Accept-Language`) helpers.
- **A test/mock client + server harness** as a first-class public API (the test dir has a
  `mock_server`, but it isn't a supported testing surface).
- **Configuration surface** — today config is two `Int32` fields (`maxBodyBytes`,
  `maxConcurrency`). Production needs a much richer, well-documented config record for both
  client and server (timeouts, limits, keep-alive, TLS, logging).

---

## 5. Major v2 Items

### 5.1 TLS (`https://`)
Currently `https://` URLs are rejected (`HttpClientError.UnsupportedScheme`) and the server
is plaintext-only. There is already a design doc — [tls-library-design.md](tls-library-design.md)
— for a **sans-I/O TLS 1.3** stack built on `standard.crypto`, mirroring the h11 pattern
(bytes in, events out). Integration work for HTTP:
- An async `TlsStream` that wraps a `TcpStream` and drives the TLS handshake, exposing the
  same `AsyncInputStream`/`AsyncOutputStream` shape so the HTTP transport is agnostic.
- Client: scheme dispatch (`http` → TCP, `https` → TLS), SNI, certificate validation,
  default port 443.
- Server: cert/key configuration, optional ALPN (needed to negotiate HTTP/2, §5.2).
- **Status:** TLS library itself is "not started" per its design doc — this is a large
  prerequisite.

### 5.2 HTTP/2
A separate protocol implementation (binary framing, HPACK header compression, stream
multiplexing, flow control, server push). This is **not** an extension of the h11 codec —
it's a parallel sans-I/O codec plus a multiplexing I/O driver. Needs:
- ALPN negotiation over TLS (so depends on §5.1) plus optional h2c (cleartext) upgrade.
- A new framing/state-machine layer (`standard.http2`?) — large, own design doc.
- The app-facing `Request`/`Response`/`Route`/`Context` API should be reusable across h1
  and h2; design the body-stream abstraction (§3.4) with h2 streams in mind so it isn't
  re-litigated later.

HTTP/3 (QUIC) is explicitly out of scope for now.

---

## 6. Suggested Sequencing

A rough dependency-ordered plan:

1. **Robustness first (§2):** timeouts/Slowloris (§2.1), O(n²) body fix (§2.4), 405/HEAD
   correctness (§2.2, §2.3), `Date` header (§2.7), client body limit (§2.5). These make the
   current v1 *safe*, not just *more featureful*.
2. **Body-stream abstraction (§3.4)** — foundational; redesigning `Request`/`Response`
   bodies later is expensive, and static files, multipart, compression, and HTTP/2 all
   build on it. Do this before the feature work that depends on it.
3. **Keep-alive (§3.1)** — server side (cheap, codec-ready) then client pool (bigger).
4. **Middleware (§3.2)** + first standard middleware (logging, request ID, recovery, CORS).
5. **Static files (§3.3)**, **multipart (§3.5)**, **graceful shutdown (§3.6)**, **compression**,
   **redirects/cookies (§4)** — parallelizable once 1–4 land.
6. **TLS (§5.1)** — gated on the TLS library being built.
7. **HTTP/2 (§5.2)** — gated on TLS + the body-stream abstraction.

---

## 7. Open Questions

- **Client shape:** is the connection-pooling `Client` a `class`, a `record` + `module`, or
  a `use`-able resource? It must own sockets and be closable.
- **Context extensibility:** how does middleware attach typed state to `Context` without a
  generic explosion? (Extensible record? Type-keyed map? Generic `Context<S>`?)
- **Body abstraction:** one enum (`Empty | Bytes(Array<Uint8>) | Stream(...)`) vs. a trait?
  How do `jsonBody<T>()` / `bodyAsString()` behave on a stream (consume once)?
- **Cancellation model:** how do request deadlines compose with the async runtime's
  `race`/interrupt so a timed-out request reliably tears down its socket and frees its
  permit?
- **Where do middleware/standard components live** — same package, or a `standard.io.http.middleware` sub-package to keep the core lean?
