# standard-http

A pure Dovetail implementation of HTTP/1.1 protocol handling, ported from Python's [h11](https://github.com/python-hyper/h11) library. It provides a sans-I/O state machine for parsing and serializing HTTP/1.1 messages — you bring the transport, it handles the protocol.

## Overview

`standard-http` does not perform any I/O. Instead, it exposes a `Connection` class that:

- Accepts raw bytes via `receiveData()`
- Produces parsed HTTP events via `nextEvent()`
- Serializes outgoing events to bytes via `send()`
- Tracks connection state (idle, sending headers/body, done, etc.)

This design makes it transport-agnostic — it works with TCP sockets, TLS, in-memory buffers, or any other byte stream.

## Quick Start

### Client

```dovetail
import standard.http.Connection
import standard.http.HttpRequest
import standard.http.HttpEvent
import standard.http.Headers
import standard.http.normalizeAndValidate

// Create a client connection
let client = Connection.client()

// Build and send a request
let headers = normalizeAndValidate([("Host", "example.com"), ("Content-Length", "0")]).require
let req = HttpRequest { method = "GET"; target = "/"; headers = headers; httpVersion = "1.1" }
let requestBytes = client.send(HttpEvent.Request(req)).require.require
let eomBytes = client.send(HttpEvent.EndOfMessage(Headers.empty())).require.require

// Send requestBytes and eomBytes over your transport...

// Feed response data from the transport
client.receiveData(responseBytes)

// Read events
let mutable done = false
while !done do
    match client.nextEvent() with
        case Event(ev) =>
            match ev with
                case Response(resp) =>
                    // resp.statusCode, resp.headers, resp.reason
                case Data(d) =>
                    // d is Array<Uint8> of body data
                case EndOfMessage(_) =>
                    done = true
                case _ => ()
        case NeedData =>
            // Feed more data from transport
            done = true
        case Error(e) =>
            // e.message, e.errorStatusHint
            done = true
        case Paused => done = true
```

### Server

```dovetail
import standard.http.Connection
import standard.http.HttpResponse
import standard.http.HttpEvent
import standard.http.Headers
import standard.http.normalizeAndValidate

// Create a server connection
let server = Connection.server()

// Feed incoming data from the transport
server.receiveData(incomingBytes)

// Parse the request
match server.nextEvent() with
    case Event(ev) =>
        match ev with
            case Request(req) =>
                // req.method, req.target, req.headers, req.httpVersion
            case _ => ()
    case _ => ()

// Send a response
let respHeaders = normalizeAndValidate([("Content-Length", "5")]).require
let resp = HttpResponse { statusCode = 200; headers = respHeaders; httpVersion = "1.1"; reason = "OK" }
let respBytes = server.send(HttpEvent.Response(resp)).require.require
let bodyBytes = server.send(HttpEvent.Data(stringToBytes("hello"))).require.require
let eomBytes = server.send(HttpEvent.EndOfMessage(Headers.empty())).require.require

// Write respBytes, bodyBytes, eomBytes to transport...
```

## Core Types

### Connection

The main entry point. Create with `Connection.client()` or `Connection.server()`.

| Method | Description |
|--------|-------------|
| `receiveData(data)` | Feed bytes from the transport. Pass empty array for EOF. |
| `nextEvent()` | Returns `NextEventResult`: `Event(e)`, `NeedData`, `Paused`, or `Error(e)`. |
| `send(event)` | Serialize an outgoing event. Returns `Result<Option<Array<Uint8>>, HttpError>`. |
| `startNextCycle()` | Reset for the next request/response on a keep-alive connection. |
| `ourState` / `theirState` | Current `ConnectionState` for each side. |
| `maxIncompleteEventSize` | Buffer overflow threshold (default 16 KiB, hint 431). |

### Events

All HTTP activity is represented as `HttpEvent` variants:

| Event | Description |
|-------|-------------|
| `Request(HttpRequest)` | A client request with method, target, headers, httpVersion. |
| `InformationalResponse(HttpResponse)` | A 1xx informational response (e.g., 100 Continue). |
| `Response(HttpResponse)` | A final response (2xx-5xx) with statusCode, headers, reason. |
| `Data(Array<Uint8>)` | A chunk of message body. |
| `EndOfMessage(Headers)` | End of message, with optional trailer headers. |
| `ConnectionClosed` | The connection has been closed. |

### Headers

Headers are stored as `Array<HeaderItem>` where each item has:
- `rawName` — original casing (e.g., `"Content-Type"`)
- `name` — lowercased for comparison (e.g., `"content-type"`)
- `value` — the header value

Always use `normalizeAndValidate()` when constructing headers from user input — it validates names/values, handles Content-Length deduplication, and checks Transfer-Encoding.

### NextEventResult

Returned by `nextEvent()`:

| Variant | Meaning |
|---------|---------|
| `Event(HttpEvent)` | A complete event was parsed. |
| `NeedData` | Not enough data yet — call `receiveData()` with more bytes. |
| `Paused` | Connection is in a state where no events are expected (e.g., DONE with trailing data, protocol switch pending). |
| `Error(HttpError)` | A protocol error occurred. |

## Header Utilities

```dovetail
import standard.http.normalizeAndValidate
import standard.http.getCommaHeader
import standard.http.setCommaHeader
import standard.http.connectionClose
import standard.http.hasChunkedTransferEncoding
import standard.http.contentLength
import standard.http.hasExpect100Continue

// Build validated headers
let headers = normalizeAndValidate([("Host", "example.com"), ("Connection", "keep-alive, upgrade")]).require

// Query comma-separated headers (lowercased, trimmed)
let values = getCommaHeader(headers, "connection")
// values == ["keep-alive", "upgrade"]

// Check specific header semantics
connectionClose(headers)                // false
hasChunkedTransferEncoding(headers)      // false
contentLength(headers)                   // None (no Content-Length)
```

## Connection Lifecycle

A typical HTTP/1.1 connection follows this state progression:

```
Client:  Idle -> SendBody -> Done -> (Idle or MustClose or Closed)
Server:  Idle -> SendResponse -> SendBody -> Done -> (Idle or MustClose or Closed)
```

### Keep-Alive

After both sides reach `Done`, call `startNextCycle()` on both `Connection` objects to reset to `Idle` for the next request. This fails if `Connection: close` was sent or received.

```dovetail
// After a complete request/response cycle:
client.startNextCycle().require
server.startNextCycle().require
// Both are now back in Idle, ready for another request.
```

### EOF Handling

Signal end-of-stream by passing an empty array to `receiveData()`:

```dovetail
server.receiveData(Array<Uint8>.empty()) // signals EOF
```

If EOF arrives mid-message, `nextEvent()` returns an appropriate error.

## Body Framing

The library automatically determines body framing per RFC 7230 section 3.3.3:

1. **Content-Length** — reads/writes exactly the declared number of bytes.
2. **Chunked Transfer-Encoding** — reads/writes chunked encoding with hex sizes.
3. **HTTP/1.0 (EOF-delimited)** — reads until connection close (responses only).

Special cases are handled: HEAD responses, 204/304 status codes, and CONNECT method.

## Error Handling

Errors are represented as `HttpError` with two variants:
- `LocalProtocolError(message, statusHint)` — our side violated the protocol.
- `RemoteProtocolError(message, statusHint)` — the peer violated the protocol.

The `errorStatusHint` provides a suggested HTTP status code for error responses (e.g., 400, 431).

## Protocol Upgrades

The library supports HTTP Upgrade (WebSocket, etc.) and CONNECT tunneling via the state machine:

- Client sends a request with `Upgrade` header or `CONNECT` method.
- State machine tracks the proposal.
- Server responds with 101 (Upgrade) or 2xx (CONNECT) to accept.
- Both sides transition to `SwitchedProtocol` state.
- Use `trailingData()` to get any buffered data after the switch.

## Validation

The library validates:
- HTTP method tokens (RFC 7230 tchar)
- Request targets (visible ASCII)
- Header field names and values
- Host header presence (required for HTTP/1.1) and uniqueness
- Content-Length consistency (no duplicates with different values)
- Transfer-Encoding (only "chunked" accepted)
- Status code ranges (100-999)
- Chunk footer bytes (\r\n)
- Buffer overflow protection (configurable via `maxIncompleteEventSize`)
