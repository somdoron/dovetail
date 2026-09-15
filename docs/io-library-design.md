# Low-Level IO Library Design

This document designs the **low-level IO library** for Dovetail: a foundational, non-blocking library that wraps WASI 2.0 worlds (IO, Sockets, Filesystem, Clocks, Random, CLI) in idiomatic Dovetail types and modules. It is intended for **async framework authors**, not end users. All operations are non-blocking; the library exposes `Pollable` as the core primitive for readiness notification, on top of which async runtimes are built.

**WASI worlds imported:** `wasi:io@0.2.10` (poll, streams, error), `wasi:sockets@0.2.10` (network, tcp, udp, ip-name-lookup, instance-network, tcp-create-socket, udp-create-socket), `wasi:filesystem@0.2.10` (types, preopens), `wasi:clocks@0.2.10` (monotonic-clock, wall-clock), `wasi:random@0.2.10` (random), `wasi:cli@0.2.10` (stdin, stdout, stderr, environment, exit, terminal-input, terminal-output, terminal-stdin, terminal-stdout, terminal-stderr).

**In scope:** Full non-blocking API surface for polling, byte streams, TCP/UDP sockets, IP address types, DNS resolution, filesystem operations (open, read, write, list directories, create/remove, stat, symlinks, rename), monotonic and wall clocks (with timer pollables), cryptographic random, standard I/O streams (stdin/stdout/stderr), terminal detection, environment variables, command-line arguments, and process exit. Every WASI resource is mapped to a Dovetail intrinsic type with a module-for-type providing its methods. Only non-blocking WASI functions are imported; all `blocking-*` variants and `pollable.block` are excluded.

**Out of scope:** Async/await syntax and runtime (see [async-await-design](async-await-design.md)); higher-level abstractions (buffered readers/writers, file paths, TLS); HTTP.

**Implementation status:** Not started.

---

## 1. Overview and Principles

### 1.1 Purpose

This library is the **lowest layer** between Dovetail programs and the operating system. It wraps WASI 2.0 capability-based APIs into Dovetail types that are:

- **Non-blocking** — Every I/O operation returns immediately. Operations that would block in POSIX instead return a `Pollable` (or `WouldBlock` result) that the caller can poll for readiness.
- **Pollable-centric** — The `poll` function is the single wait primitive. An async framework calls `Poll.wait(pollables)` as its event-loop tick, then dispatches ready operations.
- **Idiomatic** — WASI resources become Dovetail intrinsic types with module methods. WASI enums become Dovetail enums. WASI records become Dovetail records. WASI flags become Dovetail records with `Bool` fields.
- **Capability-based** — Filesystem access requires a `FileDescriptor` obtained from preopens. Network access requires a `Network` handle. These are passed explicitly.

### 1.2 Audience

Async framework and runtime authors. End users should use a higher-level async IO library built on top of this one.

### 1.3 WASM-GC and linear memory boundary

Dovetail compiles to WASM-GC — all Dovetail objects live on the GC heap, **not** in linear memory. WASI 2.0 operates on linear memory (byte buffers, strings) and represents resources as integer handles. At every WASI call boundary, the compiler must:

1. **Copy data to linear memory** before calling a WASI import (e.g., copy a Dovetail `Array<Int32>` of bytes into a linear-memory buffer for `output-stream.write`).
2. **Copy data from linear memory** after a WASI import returns (e.g., copy the bytes returned by `input-stream.read` from linear memory into a Dovetail `Array<Int32>`).
3. **Wrap/unwrap resource handles** — WASI resources are `i32` handles. Dovetail wraps them in newtypes for type safety.

This copying is unavoidable when bridging WASM-GC and WASI's linear memory model. The bump allocator (used only at the WASI boundary) manages the temporary linear-memory buffers.

### 1.4 Resource handle pattern

WASI resources (sockets, streams, descriptors) are represented as **newtypes over `Int32`**, wrapping the WASI resource handle. Their methods live in a module-for-type, with `= intrinsic` marking functions that the compiler lowers to WASI imports:

```dovetail
newtype TcpSocket = Int32

module TcpSocket =
    function startBind(self, network: Network, address: SocketAddress): Result<Unit, NetworkError> = intrinsic
```

When we want a friendlier API, we wrap the raw intrinsic:

```dovetail
module TcpSocket =
    private function unsafeAccept(self): Result<(TcpSocket, NetworkInputStream, NetworkOutputStream), NetworkError> = intrinsic
    function accept(self): Result<TcpConnection, NetworkError> =
        let t = try unsafeAccept(self)
        Ok(TcpConnection(t._0, t._1, t._2))
```

### 1.4 Blocking functions excluded

The following WASI functions are **not** imported — they block the caller and are unnecessary when using `Pollable`:

| Interface | Excluded functions |
|-----------|--------------------|
| `wasi:io/poll.pollable` | `block` (use `Poll.wait` with a single-element array instead) |
| `wasi:io/streams.input-stream` | `blocking-read`, `blocking-skip` |
| `wasi:io/streams.output-stream` | `blocking-write-and-flush`, `blocking-flush`, `blocking-write-zeroes-and-flush`, `blocking-splice` |

### 1.5 Package structure

The library is organized as a set of packages under `wasi`:

```
wasi/src/
  .                   -- package wasi: Pollable, InputStream, OutputStream, IoError, StreamError
  net/                -- package wasi.net: Network, IpAddress, SocketAddress, NetworkError, Dns
  tcp/                -- package wasi.tcp: TcpSocket, TcpConnection, ShutdownType
  udp/                -- package wasi.udp: UdpSocket, Datagram, DatagramStream
  fs/                 -- package wasi.fs: FileDescriptor, DirectoryStream, FileStat, etc.
  clock/              -- package wasi.clock: Instant, Duration, DateTime, MonotonicClock, WallClock
  random/             -- package wasi.random: Random
  cli/                -- package wasi.cli: Stdin, Stdout, Stderr, Terminal, Environment, Process
```

**Dependency DAG:** `wasi` ← `wasi.clock` ← `wasi.net` ← `wasi.tcp`, `wasi.udp`; `wasi` + `wasi.clock` ← `wasi.fs`; `wasi` ← `wasi.cli`; `wasi.random` has no intra-library dependencies.

---

## 2. Core IO — `package wasi`

Maps `wasi:io/poll`, `wasi:io/streams`, and `wasi:io/error`.

### 2.1 Pollable

The fundamental readiness primitive. A `Pollable` represents a single I/O event that may or may not be ready. Every I/O resource (sockets, streams, timers) can produce a `Pollable` via its `subscribe` method.

**WASI source:** `wasi:io/poll.pollable` resource and `wasi:io/poll.poll` function.

```dovetail
newtype Pollable = Int32

module Pollable =
    // Non-blocking readiness check. Returns true if the event is ready.
    // WASI: pollable.ready
    function ready(self): Bool = intrinsic

    // Subscribe to this pollable (identity — returns self).
    // Provided for uniformity; every subscribable resource returns a Pollable.
    function subscribe(self): Pollable = self
```

### 2.2 Poll module (standalone)

The global poll function — the heart of any event loop.

```dovetail
module Poll =
    // Block until at least one pollable in the array is ready.
    // Returns the indices of all ready pollables.
    // Traps if the array is empty.
    // WASI: poll.poll
    function wait(pollables: Array<Pollable>): Array<Int32> = intrinsic
```

### 2.3 IoError (trait)

The base error trait for all I/O errors in the library. In WASI, `wasi:io/error.error` is the root error resource — stream errors carry it as a payload, and domain-specific error extractors (`filesystem-error-code`, `network-error-code`) accept it to recover structured error codes. Modeling `IoError` as a trait lets `StreamError`, `NetworkError`, and `FileSystemError` all implement it, giving a unified error hierarchy:

- Code that works with any I/O error can accept `IoError` as a trait object.
- Because `InputStream`, `OutputStream`, and `StreamError` are all parameterized by `E : IoError`, errors are always statically typed — no downcasting needed. The intrinsic layer uses the WASI error extractors (`network-error-code`, `filesystem-error-code`) internally to produce the correctly typed `StreamError<E>`.

**WASI source:** `wasi:io/error.error` resource.

```dovetail
trait IoError

module IoError =
    // Human-readable debug description of the error.
    // This is a module function (not a trait method) — it wraps the WASI
    // error.to-debug-string intrinsic. Works on any IoError trait object.
    // WASI: error.to-debug-string
    function debugString(self): String = intrinsic
```

The trait is intentionally empty — it serves purely as a **marker trait** unifying all I/O error types under a common supertype. The `debugString` function lives in a module-for-type on `IoError`, wrapping the WASI `error.to-debug-string` intrinsic.

### 2.4 StreamError

Describes why a stream operation failed. Generic over the error type `E` so that the concrete error is always statically known: a filesystem stream uses `StreamError<FileSystemError>`, a network stream uses `StreamError<NetworkError>`. Covariant in `E` (`out E`) so that `StreamError<NetworkError>` is assignable to `StreamError<IoError>`.

**WASI source:** `wasi:io/streams.stream-error` variant.

```dovetail
enum StreamError<out E> where E : IoError =
    LastOperationFailed(E)
    Closed

implement IoError for StreamError<E> where E : IoError
```

### 2.5 InputStream

A non-blocking readable byte stream, parameterized by its error type. For example, a stream from a TCP socket is `InputStream<NetworkError>`, and a stream from a file is `InputStream<FileSystemError>`. Covariant in `E` so that `InputStream<NetworkError>` is assignable to `InputStream<IoError>`. The type parameter is phantom — all input streams are the same WASI resource handle at runtime; `E` exists only for compile-time type safety. To wait for data, call `subscribe` and poll the returned `Pollable`.

**WASI source:** `wasi:io/streams.input-stream` resource.

```dovetail
newtype InputStream<out E> where E : IoError = Int32

module InputStream<E> where E : IoError =
    // Non-blocking read of up to `len` bytes. Returns fewer bytes than requested
    // (or an empty array) when no data is immediately available.
    // WASI: input-stream.read
    function read(self, len: Int64): Result<Array<Int32>, StreamError<E>> = intrinsic

    // Non-blocking skip of up to `len` bytes. Returns number of bytes skipped.
    // WASI: input-stream.skip
    function skip(self, len: Int64): Result<Int64, StreamError<E>> = intrinsic

    // Returns a Pollable that resolves when bytes are available or the stream closes.
    // WASI: input-stream.subscribe
    function subscribe(self): Pollable = intrinsic
```

### 2.6 OutputStream

A non-blocking writable byte stream, parameterized by its error type. Call `checkWrite` before `write` to learn how many bytes are permitted, then `flush` to push buffered data. Poll `subscribe` for write readiness.

**WASI source:** `wasi:io/streams.output-stream` resource.

```dovetail
newtype OutputStream<out E> where E : IoError = Int32

module OutputStream<E> where E : IoError =
    // Check how many bytes may be written next. Never blocks.
    // Returns 0 when the stream is not ready; poll subscribe() and retry.
    // WASI: output-stream.check-write
    function checkWrite(self): Result<Int64, StreamError<E>> = intrinsic

    // Non-blocking write. The caller must first call checkWrite and pass at most
    // that many bytes, or the call traps.
    // WASI: output-stream.write
    function write(self, contents: Array<Int32>): Result<Unit, StreamError<E>> = intrinsic

    // Request to flush buffered output. Non-blocking; after calling, the stream
    // will not accept writes (checkWrite returns 0) until flush completes.
    // Poll subscribe() for completion.
    // WASI: output-stream.flush
    function flush(self): Result<Unit, StreamError<E>> = intrinsic

    // Write `len` zero bytes. Same preconditions as write (checkWrite first).
    // WASI: output-stream.write-zeroes
    function writeZeroes(self, len: Int64): Result<Unit, StreamError<E>> = intrinsic

    // Splice from an input stream. Reads from `source` and writes to self,
    // up to `len` bytes. Returns the number of bytes transferred.
    // WASI: output-stream.splice
    function splice(self, source: InputStream<E>, len: Int64): Result<Int64, StreamError<E>> = intrinsic

    // Returns a Pollable that resolves when the stream is ready for more writing.
    // WASI: output-stream.subscribe
    function subscribe(self): Pollable = intrinsic
```

---

## 3. Networking Primitives — `package wasi.net`

Maps `wasi:sockets/network`, `wasi:sockets/instance-network`, and `wasi:sockets/ip-name-lookup`.

### 3.1 Network

An opaque capability handle representing access to the network. Obtained via `Network.default()`.

**WASI source:** `wasi:sockets/network.network` resource, `wasi:sockets/instance-network.instance-network` function.

```dovetail
newtype Network = Int32

module Network =
    // Get the default network handle for this component instance.
    // WASI: instance-network.instance-network
    function default(): Network = intrinsic
```

### 3.2 IpAddressFamily

**WASI source:** `wasi:sockets/network.ip-address-family`.

```dovetail
enum IpAddressFamily =
    Ipv4
    Ipv6
```

### 3.3 IP address types

**WASI source:** `wasi:sockets/network.ipv4-address`, `ipv6-address`, `ip-address`.

```dovetail
record Ipv4Address =
    a: Uint8
    b: Uint8
    c: Uint8
    d: Uint8

record Ipv6Address =
    a: Uint16
    b: Uint16
    c: Uint16
    d: Uint16
    e: Uint16
    f: Uint16
    g: Uint16
    h: Uint16

enum IpAddress =
    V4(Ipv4Address)
    V6(Ipv6Address)
```

### 3.4 Socket address types

**WASI source:** `wasi:sockets/network.ipv4-socket-address`, `ipv6-socket-address`, `ip-socket-address`.

```dovetail
record Ipv4SocketAddress =
    port: Uint16
    address: Ipv4Address

record Ipv6SocketAddress =
    port: Uint16
    flowInfo: Uint32
    address: Ipv6Address
    scopeId: Uint32

enum SocketAddress =
    V4(Ipv4SocketAddress)
    V6(Ipv6SocketAddress)
```

### 3.5 NetworkError

A unified error type for socket and network operations. Implements `IoError` so that network streams carry `StreamError<NetworkError>` with fully typed errors.

**WASI source:** `wasi:sockets/network.error-code`.

```dovetail
enum NetworkError =
    Unknown
    AccessDenied
    NotSupported
    InvalidArgument
    OutOfMemory
    Timeout
    ConcurrencyConflict
    NotInProgress
    WouldBlock
    InvalidState
    NewSocketLimit
    AddressNotBindable
    AddressInUse
    RemoteUnreachable
    ConnectionRefused
    ConnectionReset
    ConnectionAborted
    DatagramTooLarge
    NameUnresolvable
    TemporaryResolverFailure
    PermanentResolverFailure

implement IoError for NetworkError
```

### 3.6 Network stream aliases

Convenience type aliases for streams carrying network-typed errors.

```dovetail
type NetworkInputStream = InputStream<NetworkError>
type NetworkOutputStream = OutputStream<NetworkError>
type NetworkStreamError = StreamError<NetworkError>
```

### 3.7 DNS resolution

Non-blocking DNS name resolution. Call `Dns.resolve` to begin, then poll the returned `ResolveStream` for results.

**WASI source:** `wasi:sockets/ip-name-lookup.resolve-addresses`, `resolve-address-stream` resource.

```dovetail
newtype ResolveStream = Int32

module ResolveStream =
    // Returns the next resolved IP address, or None when all addresses are exhausted.
    // Returns WouldBlock when a result is not yet available — poll subscribe().
    // WASI: resolve-address-stream.resolve-next-address
    function nextAddress(self): Result<Option<IpAddress>, NetworkError> = intrinsic

    // Pollable that resolves when the next address is available.
    // WASI: resolve-address-stream.subscribe
    function subscribe(self): Pollable = intrinsic

module Dns =
    // Begin resolving a hostname to IP addresses. Non-blocking; returns immediately
    // with a stream that yields addresses as they are resolved.
    // Unicode domain names are converted via IDNA encoding.
    // If the input is already an IP address string, it is parsed directly.
    // WASI: ip-name-lookup.resolve-addresses
    function resolve(network: Network, name: String): Result<ResolveStream, NetworkError> = intrinsic
```

---

## 4. TCP — `package wasi.tcp`

Maps `wasi:sockets/tcp` and `wasi:sockets/tcp-create-socket`.

### 4.1 ShutdownType

**WASI source:** `wasi:sockets/tcp.shutdown-type`.

```dovetail
enum ShutdownType =
    Receive
    Send
    Both
```

### 4.2 TcpConnection

A convenience record wrapping the result of a successful `accept` or `finishConnect`.

```dovetail
record TcpConnection =
    socket: TcpSocket
    input: NetworkInputStream
    output: NetworkOutputStream
```

### 4.3 TcpSocket

A TCP socket resource. Follows the WASI state machine: `unbound` → `bound` → `listening` or `connected` → `closed`. All transitions are non-blocking (start/finish pattern); poll `subscribe` between start and finish.

**WASI source:** `wasi:sockets/tcp.tcp-socket` resource, `tcp-create-socket.create-tcp-socket`.

```dovetail
newtype TcpSocket = Int32

module TcpSocket =

    // --- Construction ---

    // Create a new TCP socket for the given address family.
    // WASI: tcp-create-socket.create-tcp-socket
    function create(family: IpAddressFamily): Result<TcpSocket, NetworkError> = intrinsic

    // --- Bind (async: start → poll subscribe → finish) ---

    // Initiate a bind to a local address.
    // WASI: tcp-socket.start-bind
    function startBind(self, network: Network, localAddress: SocketAddress): Result<Unit, NetworkError> = intrinsic

    // Complete a pending bind. Call after subscribe() signals readiness.
    // WASI: tcp-socket.finish-bind
    function finishBind(self): Result<Unit, NetworkError> = intrinsic

    // --- Connect (async: start → poll subscribe → finish) ---

    // Initiate a connection to a remote address.
    // WASI: tcp-socket.start-connect
    function startConnect(self, network: Network, remoteAddress: SocketAddress): Result<Unit, NetworkError> = intrinsic

    // Complete a pending connect. Returns the I/O streams for the connection.
    // WASI: tcp-socket.finish-connect
    private function unsafeFinishConnect(self): Result<(NetworkInputStream, NetworkOutputStream), NetworkError> = intrinsic
    function finishConnect(self): Result<(NetworkInputStream, NetworkOutputStream), NetworkError> = unsafeFinishConnect(self)

    // --- Listen (async: start → poll subscribe → finish) ---

    // Begin transitioning the socket into the listening state.
    // The socket must already be bound.
    // WASI: tcp-socket.start-listen
    function startListen(self): Result<Unit, NetworkError> = intrinsic

    // Complete the listen transition.
    // WASI: tcp-socket.finish-listen
    function finishListen(self): Result<Unit, NetworkError> = intrinsic

    // --- Accept ---

    // Accept a new client connection (non-blocking).
    // Returns WouldBlock when no pending connections — poll subscribe().
    // WASI: tcp-socket.accept
    private function unsafeAccept(self): Result<(TcpSocket, NetworkInputStream, NetworkOutputStream), NetworkError> = intrinsic
    function accept(self): Result<TcpConnection, NetworkError> =
        let t = try unsafeAccept(self)
        Ok(TcpConnection(t._0, t._1, t._2))

    // --- Address queries ---

    // Local address the socket is bound to.
    // WASI: tcp-socket.local-address
    function localAddress(self): Result<SocketAddress, NetworkError> = intrinsic

    // Remote address the socket is connected to.
    // WASI: tcp-socket.remote-address
    function remoteAddress(self): Result<SocketAddress, NetworkError> = intrinsic

    // --- State queries ---

    // Whether the socket is in the listening state.
    // WASI: tcp-socket.is-listening
    function isListening(self): Bool = intrinsic

    // The address family of this socket (IPv4 or IPv6).
    // WASI: tcp-socket.address-family
    function addressFamily(self): IpAddressFamily = intrinsic

    // --- Socket options ---

    // Set the listen backlog size hint.
    // WASI: tcp-socket.set-listen-backlog-size
    function setListenBacklogSize(self, value: Int64): Result<Unit, NetworkError> = intrinsic

    // Keepalive configuration.
    // WASI: tcp-socket.keep-alive-enabled, set-keep-alive-enabled
    function keepAliveEnabled(self): Result<Bool, NetworkError> = intrinsic
    function setKeepAliveEnabled(self, value: Bool): Result<Unit, NetworkError> = intrinsic

    // Time (nanoseconds) the connection must be idle before sending keepalive packets.
    // WASI: tcp-socket.keep-alive-idle-time, set-keep-alive-idle-time
    function keepAliveIdleTime(self): Result<Duration, NetworkError> = intrinsic
    function setKeepAliveIdleTime(self, value: Duration): Result<Unit, NetworkError> = intrinsic

    // Interval (nanoseconds) between keepalive packets.
    // WASI: tcp-socket.keep-alive-interval, set-keep-alive-interval
    function keepAliveInterval(self): Result<Duration, NetworkError> = intrinsic
    function setKeepAliveInterval(self, value: Duration): Result<Unit, NetworkError> = intrinsic

    // Maximum number of keepalive packets before aborting.
    // WASI: tcp-socket.keep-alive-count, set-keep-alive-count
    function keepAliveCount(self): Result<Int32, NetworkError> = intrinsic
    function setKeepAliveCount(self, value: Int32): Result<Unit, NetworkError> = intrinsic

    // IP hop limit (TTL).
    // WASI: tcp-socket.hop-limit, set-hop-limit
    function hopLimit(self): Result<Int32, NetworkError> = intrinsic
    function setHopLimit(self, value: Int32): Result<Unit, NetworkError> = intrinsic

    // Kernel buffer sizes (bytes).
    // WASI: tcp-socket.receive-buffer-size, set-receive-buffer-size, send-buffer-size, set-send-buffer-size
    function receiveBufferSize(self): Result<Int64, NetworkError> = intrinsic
    function setReceiveBufferSize(self, value: Int64): Result<Unit, NetworkError> = intrinsic
    function sendBufferSize(self): Result<Int64, NetworkError> = intrinsic
    function setSendBufferSize(self, value: Int64): Result<Unit, NetworkError> = intrinsic

    // --- Shutdown ---

    // Initiate a graceful shutdown of one or both directions.
    // WASI: tcp-socket.shutdown
    function shutdown(self, direction: ShutdownType): Result<Unit, NetworkError> = intrinsic

    // --- Pollable ---

    // Returns a Pollable that resolves when the socket is ready for the next
    // async operation (finish-bind, finish-listen, finish-connect, or accept).
    // WASI: tcp-socket.subscribe
    function subscribe(self): Pollable = intrinsic
```

### 4.4 Typical usage by an async framework

An async framework would wrap the start/finish pattern into a single async function:

```dovetail
// Example (async framework code, not part of this library):
async function connect(socket: TcpSocket, network: Network, address: SocketAddress): Result<(NetworkInputStream, NetworkOutputStream), NetworkError> =
    try socket.startConnect(network, address)
    socket.subscribe().await
    socket.finishConnect()
```

---

## 5. UDP — `package wasi.udp`

Maps `wasi:sockets/udp` and `wasi:sockets/udp-create-socket`.

### 5.1 Datagram types

**WASI source:** `wasi:sockets/udp.incoming-datagram`, `outgoing-datagram`.

```dovetail
record IncomingDatagram =
    data: Array<Int32>
    remoteAddress: SocketAddress

record OutgoingDatagram =
    data: Array<Int32>
    remoteAddress: Option<SocketAddress>
```

### 5.2 IncomingDatagramStream

**WASI source:** `wasi:sockets/udp.incoming-datagram-stream` resource.

```dovetail
newtype IncomingDatagramStream = Int32

module IncomingDatagramStream =
    // Receive up to maxResults datagrams (non-blocking). Returns an empty
    // array when no datagrams are available — poll subscribe().
    // WASI: incoming-datagram-stream.receive
    function receive(self, maxResults: Int64): Result<Array<IncomingDatagram>, NetworkError> = intrinsic

    // Pollable that resolves when datagrams are available.
    // WASI: incoming-datagram-stream.subscribe
    function subscribe(self): Pollable = intrinsic
```

### 5.3 OutgoingDatagramStream

**WASI source:** `wasi:sockets/udp.outgoing-datagram-stream` resource.

```dovetail
newtype OutgoingDatagramStream = Int32

module OutgoingDatagramStream =
    // Check how many datagrams may be sent in the next send call. Never blocks.
    // WASI: outgoing-datagram-stream.check-send
    function checkSend(self): Result<Int64, NetworkError> = intrinsic

    // Send datagrams (non-blocking). The caller must call checkSend first and
    // send at most that many, or the call traps. Returns the number actually sent.
    // WASI: outgoing-datagram-stream.send
    function send(self, datagrams: Array<OutgoingDatagram>): Result<Int64, NetworkError> = intrinsic

    // Pollable that resolves when the stream is ready to send again.
    // WASI: outgoing-datagram-stream.subscribe
    function subscribe(self): Pollable = intrinsic
```

### 5.4 UdpSocket

A UDP socket resource. Non-blocking bind with start/finish pattern; datagram I/O via streams.

**WASI source:** `wasi:sockets/udp.udp-socket` resource, `udp-create-socket.create-udp-socket`.

```dovetail
newtype UdpSocket = Int32

module UdpSocket =

    // --- Construction ---

    // Create a new UDP socket for the given address family.
    // WASI: udp-create-socket.create-udp-socket
    function create(family: IpAddressFamily): Result<UdpSocket, NetworkError> = intrinsic

    // --- Bind (async: start → poll subscribe → finish) ---

    // WASI: udp-socket.start-bind
    function startBind(self, network: Network, localAddress: SocketAddress): Result<Unit, NetworkError> = intrinsic

    // WASI: udp-socket.finish-bind
    function finishBind(self): Result<Unit, NetworkError> = intrinsic

    // --- Stream setup ---

    // Set up inbound/outbound datagram channels, optionally connected to a specific peer.
    // When remoteAddress is Some, the streams are limited to that peer.
    // The socket must already be bound.
    // WASI: udp-socket.stream
    private function unsafeStream(self, remoteAddress: Option<SocketAddress>): Result<(IncomingDatagramStream, OutgoingDatagramStream), NetworkError> = intrinsic
    function stream(self, remoteAddress: Option<SocketAddress>): Result<(IncomingDatagramStream, OutgoingDatagramStream), NetworkError> =
        unsafeStream(self, remoteAddress)

    // --- Address queries ---

    // WASI: udp-socket.local-address
    function localAddress(self): Result<SocketAddress, NetworkError> = intrinsic

    // WASI: udp-socket.remote-address
    function remoteAddress(self): Result<SocketAddress, NetworkError> = intrinsic

    // --- State queries ---

    // WASI: udp-socket.address-family
    function addressFamily(self): IpAddressFamily = intrinsic

    // --- Socket options ---

    // Unicast hop limit (TTL).
    // WASI: udp-socket.unicast-hop-limit, set-unicast-hop-limit
    function unicastHopLimit(self): Result<Int32, NetworkError> = intrinsic
    function setUnicastHopLimit(self, value: Int32): Result<Unit, NetworkError> = intrinsic

    // Kernel buffer sizes.
    // WASI: udp-socket.receive-buffer-size, set-receive-buffer-size, send-buffer-size, set-send-buffer-size
    function receiveBufferSize(self): Result<Int64, NetworkError> = intrinsic
    function setReceiveBufferSize(self, value: Int64): Result<Unit, NetworkError> = intrinsic
    function sendBufferSize(self): Result<Int64, NetworkError> = intrinsic
    function setSendBufferSize(self, value: Int64): Result<Unit, NetworkError> = intrinsic

    // --- Pollable ---

    // WASI: udp-socket.subscribe
    function subscribe(self): Pollable = intrinsic
```

---

## 6. Filesystem — `package wasi.fs`

Maps `wasi:filesystem/types` and `wasi:filesystem/preopens`.

### 6.1 FileSystemError

Implements `IoError` so that filesystem streams carry `StreamError<FileSystemError>` with fully typed errors.

**WASI source:** `wasi:filesystem/types.error-code`.

```dovetail
enum FileSystemError =
    Access
    WouldBlock
    Already
    BadDescriptor
    Busy
    Deadlock
    Quota
    Exist
    FileTooLarge
    IllegalByteSequence
    InProgress
    Interrupted
    Invalid
    Io
    IsDirectory
    Loop
    TooManyLinks
    MessageSize
    NameTooLong
    NoDevice
    NoEntry
    NoLock
    InsufficientMemory
    InsufficientSpace
    NotDirectory
    NotEmpty
    NotRecoverable
    Unsupported
    NoTty
    NoSuchDevice
    Overflow
    NotPermitted
    Pipe
    ReadOnly
    InvalidSeek
    TextFileBusy
    CrossDevice

implement IoError for FileSystemError
```

### 6.2 Filesystem stream aliases

Convenience type aliases for streams carrying filesystem-typed errors.

```dovetail
type FileInputStream = InputStream<FileSystemError>
type FileOutputStream = OutputStream<FileSystemError>
type FileStreamError = StreamError<FileSystemError>
```

### 6.3 Descriptor types and metadata

**WASI source:** `wasi:filesystem/types.descriptor-type`, `descriptor-stat`, `directory-entry`, flags, etc.

```dovetail
enum DescriptorType =
    Unknown
    BlockDevice
    CharacterDevice
    Directory
    Fifo
    SymbolicLink
    RegularFile
    Socket

record FileFlags =
    read: Bool
    write: Bool
    fileIntegritySync: Bool
    dataIntegritySync: Bool
    requestedWriteSync: Bool
    mutateDirectory: Bool

record FileStat =
    descriptorType: DescriptorType
    linkCount: Int64
    size: Int64
    dataAccessTimestamp: Option<DateTime>
    dataModificationTimestamp: Option<DateTime>
    statusChangeTimestamp: Option<DateTime>

record PathFlags =
    symlinkFollow: Bool

record OpenFlags =
    create: Bool
    directory: Bool
    exclusive: Bool
    truncate: Bool

record DirectoryEntry =
    entryType: DescriptorType
    name: String

enum NewTimestamp =
    NoChange
    Now
    At(DateTime)

record MetadataHash =
    lower: Int64
    upper: Int64

enum FileAdvice =
    Normal
    Sequential
    Random
    WillNeed
    DontNeed
    NoReuse
```

### 6.4 DirectoryStream

**WASI source:** `wasi:filesystem/types.directory-entry-stream` resource.

```dovetail
newtype DirectoryStream = Int32

module DirectoryStream =
    // Read the next directory entry, or None when the listing is exhausted.
    // WASI: directory-entry-stream.read-directory-entry
    function next(self): Result<Option<DirectoryEntry>, FileSystemError> = intrinsic
```

### 6.5 FileDescriptor

The central filesystem resource. A `FileDescriptor` is a reference to a file, directory, or other filesystem object. All path operations are relative to a descriptor (capability-based sandboxing). Initial descriptors are obtained from `FileSystem.preopens()`.

**WASI source:** `wasi:filesystem/types.descriptor` resource.

```dovetail
newtype FileDescriptor = Int32

module FileDescriptor =

    // --- Stream access ---

    // Open an input stream for reading from the given offset.
    // WASI: descriptor.read-via-stream
    function readStream(self, offset: Int64): Result<FileInputStream, FileSystemError> = intrinsic

    // Open an output stream for writing from the given offset.
    // WASI: descriptor.write-via-stream
    function writeStream(self, offset: Int64): Result<FileOutputStream, FileSystemError> = intrinsic

    // Open an output stream for appending.
    // WASI: descriptor.append-via-stream
    function appendStream(self): Result<FileOutputStream, FileSystemError> = intrinsic

    // --- Direct read/write (non-streaming) ---

    // Read up to `length` bytes at `offset`. Returns the data and whether EOF was reached.
    // WASI: descriptor.read
    function read(self, length: Int64, offset: Int64): Result<(Array<Int32>, Bool), FileSystemError> = intrinsic

    // Write `buffer` at `offset`. Returns the number of bytes written.
    // WASI: descriptor.write
    function write(self, buffer: Array<Int32>, offset: Int64): Result<Int64, FileSystemError> = intrinsic

    // --- File metadata ---

    // Get the stat of this descriptor.
    // WASI: descriptor.stat
    function stat(self): Result<FileStat, FileSystemError> = intrinsic

    // Get the stat of a path relative to this descriptor.
    // WASI: descriptor.stat-at
    function statAt(self, pathFlags: PathFlags, path: String): Result<FileStat, FileSystemError> = intrinsic

    // Get the flags of this descriptor.
    // WASI: descriptor.get-flags
    function flags(self): Result<FileFlags, FileSystemError> = intrinsic

    // Get the type of this descriptor.
    // WASI: descriptor.get-type
    function descriptorType(self): Result<DescriptorType, FileSystemError> = intrinsic

    // --- File operations ---

    // Resize the file. Extra bytes are filled with zeros.
    // WASI: descriptor.set-size
    function setSize(self, size: Int64): Result<Unit, FileSystemError> = intrinsic

    // Set access and modification timestamps on this descriptor.
    // WASI: descriptor.set-times
    function setTimes(self, accessTime: NewTimestamp, modificationTime: NewTimestamp): Result<Unit, FileSystemError> = intrinsic

    // Set timestamps on a path relative to this descriptor.
    // WASI: descriptor.set-times-at
    function setTimesAt(self, pathFlags: PathFlags, path: String, accessTime: NewTimestamp, modificationTime: NewTimestamp): Result<Unit, FileSystemError> = intrinsic

    // Provide file advisory information (similar to posix_fadvise).
    // WASI: descriptor.advise
    function advise(self, offset: Int64, length: Int64, advice: FileAdvice): Result<Unit, FileSystemError> = intrinsic

    // Sync file data to disk (fdatasync).
    // WASI: descriptor.sync-data
    function syncData(self): Result<Unit, FileSystemError> = intrinsic

    // Sync file data and metadata to disk (fsync).
    // WASI: descriptor.sync
    function sync(self): Result<Unit, FileSystemError> = intrinsic

    // --- Directory operations ---

    // Open a file or directory relative to this descriptor.
    // WASI: descriptor.open-at
    function openAt(self, pathFlags: PathFlags, path: String, openFlags: OpenFlags, fileFlags: FileFlags): Result<FileDescriptor, FileSystemError> = intrinsic

    // Read directory entries. Returns a fresh stream starting from the beginning.
    // WASI: descriptor.read-directory
    function readDirectory(self): Result<DirectoryStream, FileSystemError> = intrinsic

    // Create a directory at the given relative path.
    // WASI: descriptor.create-directory-at
    function createDirectoryAt(self, path: String): Result<Unit, FileSystemError> = intrinsic

    // Remove an empty directory at the given relative path.
    // WASI: descriptor.remove-directory-at
    function removeDirectoryAt(self, path: String): Result<Unit, FileSystemError> = intrinsic

    // --- Link and symlink operations ---

    // Create a hard link.
    // WASI: descriptor.link-at
    function linkAt(self, oldPathFlags: PathFlags, oldPath: String, newDescriptor: FileDescriptor, newPath: String): Result<Unit, FileSystemError> = intrinsic

    // Create a symbolic link.
    // WASI: descriptor.symlink-at
    function symlinkAt(self, oldPath: String, newPath: String): Result<Unit, FileSystemError> = intrinsic

    // Read the target of a symbolic link.
    // WASI: descriptor.readlink-at
    function readlinkAt(self, path: String): Result<String, FileSystemError> = intrinsic

    // --- Rename and unlink ---

    // Rename a file or directory.
    // WASI: descriptor.rename-at
    function renameAt(self, oldPath: String, newDescriptor: FileDescriptor, newPath: String): Result<Unit, FileSystemError> = intrinsic

    // Remove a file (not a directory).
    // WASI: descriptor.unlink-file-at
    function unlinkFileAt(self, path: String): Result<Unit, FileSystemError> = intrinsic

    // --- Identity and hashing ---

    // Test whether two descriptors refer to the same filesystem object.
    // WASI: descriptor.is-same-object
    function isSameObject(self, other: FileDescriptor): Bool = intrinsic

    // Hash of the file's metadata (for change detection).
    // WASI: descriptor.metadata-hash
    function metadataHash(self): Result<MetadataHash, FileSystemError> = intrinsic

    // Hash of a path's metadata relative to this descriptor.
    // WASI: descriptor.metadata-hash-at
    function metadataHashAt(self, pathFlags: PathFlags, path: String): Result<MetadataHash, FileSystemError> = intrinsic
```

### 6.6 FileSystem module (standalone)

Top-level filesystem functions.

```dovetail
module FileSystem =
    // Return the set of preopened directories and their paths.
    // This is the entry point for all filesystem access — the only way
    // to obtain initial FileDescriptor values.
    // WASI: preopens.get-directories
    function preopens(): Array<(FileDescriptor, String)> = intrinsic
```

---

## 7. Clocks — `package wasi.clock`

Maps `wasi:clocks/monotonic-clock` and `wasi:clocks/wall-clock`.

### 7.1 Time types

```dovetail
// Monotonic instant in nanoseconds (relative to an unspecified epoch).
newtype Instant = Int64

// Duration in nanoseconds.
newtype Duration = Int64

// Wall-clock date/time (seconds since Unix epoch + sub-second nanoseconds).
record DateTime =
    seconds: Int64
    nanoseconds: Int32
```

### 7.2 MonotonicClock (standalone module)

A monotonic (non-decreasing) clock for measuring elapsed time. Also produces `Pollable` timers — the mechanism for implementing timeouts and delays in async frameworks.

**WASI source:** `wasi:clocks/monotonic-clock`.

```dovetail
module MonotonicClock =
    // Read the current monotonic time.
    // WASI: monotonic-clock.now
    function now(): Instant = intrinsic

    // Query the clock resolution (smallest measurable duration).
    // WASI: monotonic-clock.resolution
    function resolution(): Duration = intrinsic

    // Create a Pollable that resolves at the specified absolute instant.
    // WASI: monotonic-clock.subscribe-instant
    function subscribeInstant(when: Instant): Pollable = intrinsic

    // Create a Pollable that resolves after the specified duration from now.
    // WASI: monotonic-clock.subscribe-duration
    function subscribeDuration(when: Duration): Pollable = intrinsic
```

### 7.3 WallClock (standalone module)

A wall clock for querying the current date and time. Not monotonic — may jump backwards due to NTP or manual adjustment. Not suitable for measuring elapsed time; use `MonotonicClock` for that.

**WASI source:** `wasi:clocks/wall-clock`.

```dovetail
module WallClock =
    // Read the current wall-clock time (seconds since Unix epoch).
    // WASI: wall-clock.now
    function now(): DateTime = intrinsic

    // Query the clock resolution.
    // WASI: wall-clock.resolution
    function resolution(): DateTime = intrinsic
```

---

## 8. Random — `package wasi.random`

Maps `wasi:random/random`.

### 8.1 Random (standalone module)

Cryptographically-secure random number generation. Never blocks.

**WASI source:** `wasi:random/random`.

```dovetail
module Random =
    // Return `len` cryptographically-secure random bytes.
    // WASI: random.get-random-bytes
    function bytes(len: Int64): Array<Int32> = intrinsic

    // Return a cryptographically-secure random 64-bit integer.
    // WASI: random.get-random-u64
    function int64(): Int64 = intrinsic
```

---

## 9. CLI — `package wasi.cli`

Maps `wasi:cli/stdin`, `wasi:cli/stdout`, `wasi:cli/stderr`, `wasi:cli/environment`, `wasi:cli/exit`, `wasi:cli/terminal-input`, `wasi:cli/terminal-output`, `wasi:cli/terminal-stdin`, `wasi:cli/terminal-stdout`, `wasi:cli/terminal-stderr`.

### 9.1 Standard I/O streams

Access to the process's standard input, output, and error streams. These return the same `InputStream` / `OutputStream` types from `package io`, so they integrate directly with the non-blocking stream and polling model.

**WASI source:** `wasi:cli/stdin`, `wasi:cli/stdout`, `wasi:cli/stderr`.

```dovetail
module Stdin =
    // Get the standard input stream for this process.
    // WASI: stdin.get-stdin
    function stream(): InputStream<IoError> = intrinsic

module Stdout =
    // Get the standard output stream for this process.
    // WASI: stdout.get-stdout
    function stream(): OutputStream<IoError> = intrinsic

module Stderr =
    // Get the standard error stream for this process.
    // WASI: stderr.get-stderr
    function stream(): OutputStream<IoError> = intrinsic
```

### 9.2 Terminal detection

Opaque resources that indicate whether a standard stream is connected to a terminal (TTY). Useful for deciding whether to emit ANSI colors, interactive prompts, etc.

**WASI source:** `wasi:cli/terminal-input`, `wasi:cli/terminal-output`, `wasi:cli/terminal-stdin`, `wasi:cli/terminal-stdout`, `wasi:cli/terminal-stderr`.

```dovetail
newtype TerminalInput = Int32
newtype TerminalOutput = Int32

module Terminal =
    // If stdin is connected to a terminal, returns a TerminalInput handle.
    // Returns None when stdin is piped or redirected.
    // WASI: terminal-stdin.get-terminal-stdin
    function stdin(): Option<TerminalInput> = intrinsic

    // If stdout is connected to a terminal, returns a TerminalOutput handle.
    // Returns None when stdout is piped or redirected.
    // WASI: terminal-stdout.get-terminal-stdout
    function stdout(): Option<TerminalOutput> = intrinsic

    // If stderr is connected to a terminal, returns a TerminalOutput handle.
    // Returns None when stderr is piped or redirected.
    // WASI: terminal-stderr.get-terminal-stderr
    function stderr(): Option<TerminalOutput> = intrinsic
```

### 9.3 Environment and arguments

Access to POSIX-style environment variables, command-line arguments, and the initial working directory.

**WASI source:** `wasi:cli/environment`.

```dovetail
module Environment =
    // Get all environment variables as key-value pairs.
    // WASI: environment.get-environment
    function variables(): Array<(String, String)> = intrinsic

    // Get the command-line arguments.
    // WASI: environment.get-arguments
    function arguments(): Array<String> = intrinsic

    // Get the initial current working directory path, if available.
    // Programs should interpret "." as this path.
    // WASI: environment.initial-cwd
    function initialCwd(): Option<String> = intrinsic
```

### 9.4 Process exit

Terminate the current process.

**WASI source:** `wasi:cli/exit`.

```dovetail
module Process =
    // Exit the process. Ok(()) indicates success; Err(()) indicates failure.
    // This function does not return.
    // WASI: exit.exit
    function exit(status: Result<Unit, Unit>): Unit = intrinsic

    // Exit the process with a specific numeric status code.
    // 0 typically means success. This function does not return.
    // WASI: exit.exit-with-code
    function exitWithCode(code: Int32): Unit = intrinsic
```

---

## 10. WASI-to-Dovetail Type Mapping Summary

| WASI interface | WASI type | Dovetail type | Dovetail package |
|----------------|-----------|-------------|----------------|
| `wasi:io/poll` | `pollable` | `Pollable` | `wasi` |
| `wasi:io/poll` | `poll` (func) | `Poll.wait` | `wasi` |
| `wasi:io/error` | `error` | `IoError` (trait) | `wasi` |
| `wasi:io/streams` | `stream-error` | `StreamError<E>` | `wasi` |
| `wasi:io/streams` | `input-stream` | `InputStream<E>` (newtype, phantom `E`) | `wasi` |
| `wasi:io/streams` | `output-stream` | `OutputStream<E>` (newtype, phantom `E`) | `wasi` |
| `wasi:sockets/network` | `network` | `Network` | `wasi.net` |
| `wasi:sockets/network` | `error-code` | `NetworkError` (implements `IoError`) | `wasi.net` |
| `wasi:sockets/network` | `ip-address-family` | `IpAddressFamily` | `wasi.net` |
| `wasi:sockets/network` | `ipv4-address` | `Ipv4Address` | `wasi.net` |
| `wasi:sockets/network` | `ipv6-address` | `Ipv6Address` | `wasi.net` |
| `wasi:sockets/network` | `ip-address` | `IpAddress` | `wasi.net` |
| `wasi:sockets/network` | `ipv4-socket-address` | `Ipv4SocketAddress` | `wasi.net` |
| `wasi:sockets/network` | `ipv6-socket-address` | `Ipv6SocketAddress` | `wasi.net` |
| `wasi:sockets/network` | `ip-socket-address` | `SocketAddress` | `wasi.net` |
| `wasi:sockets/ip-name-lookup` | `resolve-address-stream` | `ResolveStream` | `wasi.net` |
| `wasi:sockets/ip-name-lookup` | `resolve-addresses` (func) | `Dns.resolve` | `wasi.net` |
| `wasi:sockets/tcp` | `tcp-socket` | `TcpSocket` | `wasi.tcp` |
| `wasi:sockets/tcp` | `shutdown-type` | `ShutdownType` | `wasi.tcp` |
| `wasi:sockets/udp` | `udp-socket` | `UdpSocket` | `wasi.udp` |
| `wasi:sockets/udp` | `incoming-datagram` | `IncomingDatagram` | `wasi.udp` |
| `wasi:sockets/udp` | `outgoing-datagram` | `OutgoingDatagram` | `wasi.udp` |
| `wasi:sockets/udp` | `incoming-datagram-stream` | `IncomingDatagramStream` | `wasi.udp` |
| `wasi:sockets/udp` | `outgoing-datagram-stream` | `OutgoingDatagramStream` | `wasi.udp` |
| `wasi:filesystem/types` | `descriptor` | `FileDescriptor` | `wasi.fs` |
| `wasi:filesystem/types` | `directory-entry-stream` | `DirectoryStream` | `wasi.fs` |
| `wasi:filesystem/types` | `descriptor-type` | `DescriptorType` | `wasi.fs` |
| `wasi:filesystem/types` | `descriptor-flags` | `FileFlags` | `wasi.fs` |
| `wasi:filesystem/types` | `descriptor-stat` | `FileStat` | `wasi.fs` |
| `wasi:filesystem/types` | `directory-entry` | `DirectoryEntry` | `wasi.fs` |
| `wasi:filesystem/types` | `open-flags` | `OpenFlags` | `wasi.fs` |
| `wasi:filesystem/types` | `path-flags` | `PathFlags` | `wasi.fs` |
| `wasi:filesystem/types` | `new-timestamp` | `NewTimestamp` | `wasi.fs` |
| `wasi:filesystem/types` | `advice` | `FileAdvice` | `wasi.fs` |
| `wasi:filesystem/types` | `metadata-hash-value` | `MetadataHash` | `wasi.fs` |
| `wasi:filesystem/types` | `error-code` (fs) | `FileSystemError` (implements `IoError`) | `wasi.fs` |
| `wasi:clocks/monotonic-clock` | `instant` | `Instant` (newtype) | `wasi.clock` |
| `wasi:clocks/monotonic-clock` | `duration` | `Duration` (newtype) | `wasi.clock` |
| `wasi:clocks/wall-clock` | `datetime` | `DateTime` | `wasi.clock` |
| `wasi:random/random` | `get-random-bytes` | `Random.bytes` | `wasi.random` |
| `wasi:random/random` | `get-random-u64` | `Random.int64` | `wasi.random` |
| `wasi:cli/stdin` | `get-stdin` (func) | `Stdin.stream` | `wasi.cli` |
| `wasi:cli/stdout` | `get-stdout` (func) | `Stdout.stream` | `wasi.cli` |
| `wasi:cli/stderr` | `get-stderr` (func) | `Stderr.stream` | `wasi.cli` |
| `wasi:cli/terminal-input` | `terminal-input` | `TerminalInput` | `wasi.cli` |
| `wasi:cli/terminal-output` | `terminal-output` | `TerminalOutput` | `wasi.cli` |
| `wasi:cli/terminal-stdin` | `get-terminal-stdin` (func) | `Terminal.stdin` | `wasi.cli` |
| `wasi:cli/terminal-stdout` | `get-terminal-stdout` (func) | `Terminal.stdout` | `wasi.cli` |
| `wasi:cli/terminal-stderr` | `get-terminal-stderr` (func) | `Terminal.stderr` | `wasi.cli` |
| `wasi:cli/environment` | `get-environment` (func) | `Environment.variables` | `wasi.cli` |
| `wasi:cli/environment` | `get-arguments` (func) | `Environment.arguments` | `wasi.cli` |
| `wasi:cli/environment` | `initial-cwd` (func) | `Environment.initialCwd` | `wasi.cli` |
| `wasi:cli/exit` | `exit` (func) | `Process.exit` | `wasi.cli` |
| `wasi:cli/exit` | `exit-with-code` (func) | `Process.exitWithCode` | `wasi.cli` |

---

## 11. Design Decisions

### 11.1 Why non-blocking only

An async runtime built on this library calls `Poll.wait` as its event-loop tick. All I/O operations are non-blocking: they either succeed immediately or return `WouldBlock` / an empty result, at which point the caller subscribes to a `Pollable` and passes control back to the event loop. Including blocking functions would bypass the event loop, defeating the purpose of the library.

### 11.2 Why start/finish instead of a single function

WASI designs bind, connect, and listen as two-phase async operations. This gives the host runtime a chance to inject permission prompts (capability-based security). Our library preserves this pattern because:

1. It maps directly to the underlying WASI semantics — no hidden blocking.
2. Async frameworks can wrap start/finish trivially:
   ```dovetail
   async function bind(socket: TcpSocket, address: SocketAddress): Result<Unit, NetworkError> =
       try socket.startBind(Network.default(), address)
       socket.subscribe().await
       socket.finishBind()
   ```
3. It allows async frameworks to implement custom timeout logic by racing the subscribe pollable with a timer pollable.

### 11.3 Why expose Network explicitly

WASI's capability-based security model requires a `Network` handle for socket operations. Rather than hiding this behind a global, we expose it explicitly so that:

- Async frameworks can pass different network capabilities in sandboxed environments.
- The security model is not accidentally bypassed.

The `Network.default()` convenience function covers the common case.

### 11.4 Byte representation

WASI byte arrays (`list<u8>`) are initially represented as `Array<Int32>` in Dovetail. Each element holds a value in the range `0..255`. At the WASI boundary, the marshaling layer copies between the GC-heap `Array<Int32>` and a linear-memory byte buffer.

**Future optimization — `Bytes` type on linear memory:** A dedicated `Bytes` type can be introduced that stores its data directly in linear memory, eliminating the copy at the WASI boundary. This would use a **two-memory architecture**:

1. **Memory 0 (WASI boundary)** — Bump allocator for temporary marshaling buffers at WASI call sites. Short-lived; reset after each call.
2. **Memory 1 (Bytes storage)** — Malloc-style allocator for long-lived `Bytes` values. `Bytes` would be a newtype wrapping a linear-memory pointer and length, with the actual byte data living in this second memory.

With `Bytes`, I/O operations that read or write byte data would pass the linear-memory pointer directly to WASI — zero copy. The `Bytes` type would provide conversion to/from `Array<Int32>` for interop with the rest of Dovetail. WASM's multi-memory proposal (already widely supported) enables this cleanly. This optimization is out of scope for the initial implementation.

### 11.5 WASI flags as records

WASI `flags` types (bit fields) are represented as Dovetail records with `Bool` fields rather than integer bitmasks. This is more idiomatic and type-safe:

```dovetail
let flags = OpenFlags(create = true, directory = false, exclusive = false, truncate = false)
```

The intrinsic implementation converts between the record and the WASI bit representation.

### 11.6 WASI resources as newtypes over Int32

WASI resources (`pollable`, `tcp-socket`, `descriptor`, etc.) are represented as **`newtype X = Int32`** — wrapping the WASI resource handle (an `i32` index) in a zero-cost newtype for type safety. This is the natural representation because Dovetail uses WASM-GC (not linear memory) for all its objects, while WASI resources are opaque integer handles managed by the component model runtime.

Resource lifecycle is managed by the host runtime. When a resource handle is no longer reachable (the newtype value is garbage collected or explicitly dropped via a future `drop` intrinsic), the underlying WASI resource should be released.

### 11.7 Linear memory boundary

Dovetail compiles to WASM-GC — all Dovetail values (arrays, strings, records) live on the GC heap. WASI operates on linear memory. At every WASI import call, the compiler-generated glue code must:

1. **Marshal arguments to linear memory** — Copy Dovetail `Array<Int32>` byte data and `String` values into a linear-memory buffer (managed by the bump allocator on memory 0) before calling the WASI function.
2. **Unmarshal results from linear memory** — After the WASI function returns, copy result data (byte arrays, strings, records) from linear memory back into WASM-GC objects.
3. **Pass resource handles directly** — Newtype `Int32` handles are passed as-is (no copy needed).

This marshaling is invisible to the library user — the `= intrinsic` functions handle it. The performance cost is proportional to the size of data crossing the boundary, not the number of calls. When the `Bytes` type is introduced (see §11.4), byte data can be passed via memory 1 with zero copy.

### 11.8 Phantom type parameters on streams

`InputStream<E>` and `OutputStream<E>` are **newtypes** over `Int32` (same WASI resource handle), not traits. The type parameter `E : IoError` is a **phantom type** — it does not affect the runtime representation (always `Int32`) but provides compile-time type safety. A filesystem stream is `InputStream<FileSystemError>` and a network stream is `InputStream<NetworkError>`, preventing accidental mixing of error handling. Covariance (`out E`) allows assigning either to `InputStream<IoError>` when generic handling is needed.

---

## 12. Implementation Plan

| Phase | Scope | Notes |
|-------|-------|-------|
| **1** | Core IO: `Pollable`, `Poll`, `IoError`, `StreamError`, `InputStream`, `OutputStream` | Foundation for all other packages. Requires codegen support for resource handle newtypes, linear memory marshaling, and WASI imports for `wasi:io/poll`, `wasi:io/streams`, `wasi:io/error`. |
| **2** | Clocks: `Instant`, `Duration`, `DateTime`, `MonotonicClock`, `WallClock` | Timer pollables (`subscribeDuration`, `subscribeInstant`) are essential for async frameworks implementing timeouts. WASI import of `wasi:clocks/monotonic-clock`, `wasi:clocks/wall-clock`. |
| **3** | Random: `Random` module | Simple; no dependencies beyond the base compiler. WASI import of `wasi:random/random`. |
| **4** | Networking primitives: `Network`, `IpAddressFamily`, `IpAddress`, `SocketAddress`, `NetworkError`, `Dns`, `ResolveStream` | IP types, address types, DNS. WASI import of `wasi:sockets/network`, `wasi:sockets/instance-network`, `wasi:sockets/ip-name-lookup`. Depends on Phase 1 (Pollable). |
| **5** | TCP: `TcpSocket`, `TcpConnection`, `ShutdownType` | Full TCP lifecycle: create, bind, listen, accept, connect, shutdown. WASI import of `wasi:sockets/tcp`, `wasi:sockets/tcp-create-socket`. Depends on Phases 1 and 4. |
| **6** | UDP: `UdpSocket`, `IncomingDatagramStream`, `OutgoingDatagramStream`, datagram records | Full UDP lifecycle: create, bind, stream, send, receive. WASI import of `wasi:sockets/udp`, `wasi:sockets/udp-create-socket`. Depends on Phases 1 and 4. **Phase 6a (socket lifecycle, options, streams): implemented. Phase 6b (datagram send/receive marshaling): not yet implemented.** Note: datagram `data` field uses `Array<Uint8>` (not `Array<Int32>`) for consistency with streams. |
| **7** | Filesystem: `FileDescriptor`, `DirectoryStream`, all FS types, `FileSystem` module | Full filesystem access: open, read, write, list, create/remove dirs, stat, rename, link, symlink, unlink. WASI import of `wasi:filesystem/types`, `wasi:filesystem/preopens`. Depends on Phases 1 and 2 (DateTime for timestamps). |
| **8** | CLI: `Stdin`, `Stdout`, `Stderr`, `Terminal`, `TerminalInput`, `TerminalOutput`, `Environment`, `Process` | Standard I/O streams, terminal detection, environment variables, command-line arguments, process exit. WASI import of `wasi:cli/stdin`, `stdout`, `stderr`, `terminal-*`, `environment`, `exit`. Depends on Phase 1 (InputStream, OutputStream). |

**Dependencies:** 1 is base. 2 depends on 1. 3 is independent. 4 depends on 1. 5 and 6 depend on 1 and 4. 7 depends on 1 and 2. 8 depends on 1.

---

## 13. Compiler Requirements

Implementing this library requires the following compiler features:

| Requirement | Description |
|-------------|-------------|
| **WASI import codegen** | The compiler must generate WASM component imports for each WASI function used by an `= intrinsic` function. The mapping from Dovetail intrinsic function to WASI import is determined by the compiler (convention or annotation). |
| **Linear memory marshaling** | Compiler-generated glue code at each WASI call boundary must copy data between WASM-GC heap objects and linear memory. Uses the bump allocator on memory 0 (already present for WASI CLI). Includes: `Array<Int32>` ↔ `list<u8>`, `String` ↔ WASI strings, records ↔ linear-memory structs. Future: when `Bytes` is introduced on memory 1 (malloc allocator), byte I/O can skip the copy. |
| **Resource handle newtypes** | WASI resources are `newtype X = Int32`. The compiler passes the unwrapped `Int32` to WASI imports and wraps returned handles. Resource drop must be generated when handles go out of scope (or via explicit drop intrinsic). |
| **Phantom type parameters** | `InputStream<E>` and `OutputStream<E>` have a phantom type parameter that does not affect runtime representation. Codegen must erase `E` — all instantiations compile to the same `Int32` newtype. |
| **Flags-to-record conversion** | Marshaling layer converts between Dovetail `Bool`-field records and WASI integer bitflags at the WASI call boundary. |
| **Newtype codegen** | Newtypes (`Pollable`, `TcpSocket`, `Instant`, `Duration`, etc.) must compile to their underlying type with zero overhead. (Already supported per [newtypes-design](newtypes-design.md).) |
| **Modules and module-for-type** | The module system must support intrinsic functions in module-for-type, including generic modules (`InputStream<E>`). (Modules already supported per [modules-design](modules-design.md); generic modules per Phase 5.) |

---

## 14. References

- [WASI IO proposal (poll, streams, error)](https://github.com/WebAssembly/WASI/tree/main/proposals/io)
- [WASI Sockets proposal (network, tcp, udp, ip-name-lookup)](https://github.com/WebAssembly/WASI/tree/main/proposals/sockets)
- [WASI Filesystem proposal](https://github.com/WebAssembly/WASI/tree/main/proposals/filesystem)
- [WASI Clocks proposal](https://github.com/WebAssembly/WASI/tree/main/proposals/clocks)
- [WASI Random proposal](https://github.com/WebAssembly/WASI/tree/main/proposals/random)
- [WASI CLI proposal](https://github.com/WebAssembly/WASI/tree/main/proposals/cli)
- [async-await-design.md](async-await-design.md) — Async/await built on top of this library.
- [modules-design.md](modules-design.md) — Module system (standalone and module-for-type).
- [newtypes-design.md](newtypes-design.md) — Newtypes for Instant, Duration.
- [wasi-component-wasi-cli-design.md](wasi-component-wasi-cli-design.md) — WASM component model foundation.
- [compiler.md](compiler.md) — Compilation pipeline.
