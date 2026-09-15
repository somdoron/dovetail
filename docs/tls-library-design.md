# TLS 1.3 Library Design

This document designs the **TLS 1.3 protocol implementation** for Dovetail: a sans-I/O, event-driven TLS 1.3 protocol stack built on top of `standard.crypto` primitives. It follows the same architectural pattern as the HTTP h11 library — bytes in, events out, no I/O. The implementation is built from **RFC 8446** directly, not ported from an existing library.

**In scope:** TLS 1.3 client and server, sans-I/O API, record layer, handshake protocol (full and PSK), key schedule, certificate validation, extensions (SNI, ALPN, supported_groups, signature_algorithms, key_share), alert protocol, session resumption via PSK, 0-RTT data (optional).

**Out of scope:** TLS 1.2 and earlier; DTLS; HTTP; async I/O (handled by the async library wrapping this one).

**Implementation status:** Core implemented (`standard-tls`); full design and
conformance checklist incomplete. The client and server support full 1-RTT
handshakes, all three listed cipher suites, application data, alerts and
`close_notify`, `KeyUpdate`, and PSK session resumption with binders.

**Remaining implementation boundaries:**

- **HelloRetryRequest:** recognized but explicitly rejected by the client; the
  retry handshake is not implemented.
- **RSA server CertificateVerify signing:** explicitly returns an unsupported
  error in [Signing.dove](../standard-tls/src/Signing.dove). Verifying RSA-PSS
  peer signatures is implemented; that does not imply RSA server-key support.
- **Client certificate authentication and optional 0-RTT:** not implemented.
  PSK resumption currently uses the ordinary handshake, not early application data.
- **Traffic-key erasure:** key rotation replaces stored keys/secrets without an
  explicit wipe, and connection close does not implement the promised traffic-key
  cleanup. Resource-based cleanup for loaded private keys is a separate mechanism.

**Validation coverage:** The repository contains 94 TLS tests, including
[self-interop](../standard-tls/test/Interop.dove) for full and resumed handshakes,
key updates, closure, and rejection paths. [Key schedule tests](../standard-tls/src/KeySchedule.dove)
check selected RFC 8448 values; this is not coverage of every proposed trace.
[HTTPS interop tests](../dovetail/tests/https_interop.rs) exercise both client/server
directions against OpenSSL over loopback sockets. They can skip when the local
OpenSSL TLS 1.3 capability probe fails. The old claim that external interop must
wait for an I/O bridge is obsolete.

No in-repository TLS-Anvil/tlsfuzzer runner or results were found, nor evidence
closing the complete rustls/Go/browser interop matrix in Phase 8. Those validation
items remain open. This inventory is source/test inspection, not a fresh test
run or an independent security audit.


**Package:** `standard.tls` (separate package depending on `standard.crypto` + `standard.crypto.roots`;
deviates from the original "same package as crypto primitives" plan to keep the crypto leaf lean,
mirroring how `standard.http` is its own package).

**Specification:** [RFC 8446 — The Transport Layer Security (TLS) Protocol Version 1.3](https://www.rfc-editor.org/rfc/rfc8446)

**References** (for validation, not porting):
- [rustls](https://github.com/rustls/rustls) — architectural reference for edge cases and security considerations
- [tlsfuzzer](https://github.com/tlsfuzzer/tlsfuzzer) — conformance test suite
- [RFC 8446](https://www.rfc-editor.org/rfc/rfc8446) — source of truth

---

## 1. Design Principles

### 1.1 Sans-I/O

The TLS library performs **zero I/O**. It does not open sockets, read from streams, or write to streams. Instead, it operates on byte buffers:

- **Input:** The caller feeds raw bytes received from the network via `receiveData`.
- **Output:** The caller retrieves bytes to send via `dataToSend`.
- **Events:** The caller polls for protocol events (handshake complete, application data received, errors) via `nextEvent`.

This is identical to how h11 works for HTTP. The async library wraps TLS connections with actual socket I/O, just like it wraps h11 connections.

### 1.2 TLS 1.3 Only

No TLS 1.2 fallback. No backward compatibility. This eliminates:
- CBC cipher suites and padding oracle attacks
- RSA key exchange
- Renegotiation
- Compression (CRIME attack)
- ~80% of the complexity in existing TLS libraries

A TLS 1.3-only client will work with virtually all modern servers (TLS 1.3 adoption is >95% on the public internet as of 2025).

### 1.3 Pure and Sync

Like the crypto primitives, the TLS protocol engine is synchronous. No `Awaitable`, no dependency on the async runtime. The async library wraps it:

```dovetail
// Async library provides the I/O integration
async function tlsConnect(tcp: TcpStream, config: TlsClientConfig): TlsStream =
  let tls = TlsClient(config)
  // drive the handshake: feed bytes from tcp → tls, send bytes from tls → tcp
  // return a TlsStream that wraps both
```

---

## 2. API

### 2.1 Client Connection

```dovetail
class TlsClient(config: TlsClientConfig) =
  function receiveData(data: Array<Uint8>): Unit
  function dataToSend(): Array<Uint8>
  function nextEvent(): Option<TlsEvent>
  function sendApplicationData(data: Array<Uint8>): Unit
  function sendKeyUpdate(requestUpdate: Boolean): Unit
  function initiateClose(): Unit
  property isHandshakeComplete: Boolean
  property negotiatedParams: Option<NegotiatedParams>
```

### 2.2 Server Connection

```dovetail
class TlsServer(config: TlsServerConfig) =
  function receiveData(data: Array<Uint8>): Unit
  function dataToSend(): Array<Uint8>
  function nextEvent(): Option<TlsEvent>
  function sendApplicationData(data: Array<Uint8>): Unit
  function sendKeyUpdate(requestUpdate: Boolean): Unit
  function initiateClose(): Unit
  property isHandshakeComplete: Boolean
  property negotiatedParams: Option<NegotiatedParams>
```

### 2.3 Events

```dovetail
enum TlsEvent
  HandshakeComplete(params: NegotiatedParams)
  ApplicationData(data: Array<Uint8>)
  ConnectionClosed
  Error(error: TlsError)
```

The event model is simple:
- `HandshakeComplete` — emitted once, when the handshake finishes successfully. After this, `sendApplicationData` is available.
- `ApplicationData` — emitted whenever decrypted application data is available. May be emitted multiple times.
- `ConnectionClosed` — peer sent a `close_notify` alert.
- `Error` — fatal error. The connection is no longer usable.

### 2.4 Configuration

```dovetail
record TlsClientConfig
  serverName: String
  cipherSuites: Array<CipherSuite>
  keyExchangeGroups: Array<KeyExchangeGroup>
  signatureAlgorithms: Array<SignatureScheme>
  trustAnchors: Array<X509Certificate>
  alpnProtocols: Array<String>
  enableTickets: Boolean

record TlsServerConfig
  certificate: X509Certificate
  certificateChain: Array<X509Certificate>
  privateKey: SecretKey
  cipherSuites: Array<CipherSuite>
  alpnProtocols: Array<String>
  enableTickets: Boolean

record NegotiatedParams
  cipherSuite: CipherSuite
  keyExchangeGroup: KeyExchangeGroup
  signatureScheme: SignatureScheme
  peerCertificates: Array<X509Certificate>
  alpnProtocol: Option<String>
  serverName: String
```

### 2.5 Cipher Suites and Enums

```dovetail
enum CipherSuite
  Aes128GcmSha256
  Aes256GcmSha384
  ChaCha20Poly1305Sha256

enum KeyExchangeGroup
  X25519
  SecP256R1

enum SignatureScheme
  EcdsaSecp256r1Sha256
  Ed25519
  RsaPssRsaeSha256
  RsaPssRsaeSha384
```

### 2.6 Errors

```dovetail
enum TlsError
  UnexpectedMessage(description: String)
  BadRecordMac
  RecordOverflow
  HandshakeFailure(description: String)
  BadCertificate(error: X509Error)
  UnsupportedCertificate
  CertificateRevoked
  CertificateExpired
  CertificateUnknown
  IllegalParameter(description: String)
  DecodeError(description: String)
  DecryptError
  ProtocolVersion
  InsufficientSecurity
  InternalError(description: String)
  MissingExtension(name: String)
  UnsupportedExtension
  NoApplicationProtocol
```

These map directly to TLS 1.3 alert descriptions (RFC 8446 Section 6.2), making debugging straightforward.

---

## 3. Architecture

### 3.1 Component Overview

```
┌─────────────────────────────────────────────────┐
│                  TlsClient / TlsServer          │
│                  (public API)                    │
├────────────┬────────────────────┬───────────────┤
│ Handshake  │    Key Schedule    │    Record     │
│ State      │    (HKDF pipeline) │    Layer      │
│ Machine    │                    │    (framing + │
│            │                    │     AEAD)     │
├────────────┴────────────────────┴───────────────┤
│              standard.crypto                     │
│  Sha256, Sha384, Hmac, Hkdf, AeadAlgorithm,    │
│  X25519, EcdhP256, Ed25519, EcdsaP256, RsaPss, │
│  X509Validator                                   │
└──────────────────────────────────────────────────┘
```

### 3.2 Record Layer

The record layer handles framing and encryption/decryption of TLS records.

**Record format (RFC 8446 Section 5.1):**
```
ContentType (1 byte) | ProtocolVersion (2 bytes) | Length (2 bytes) | Payload
```

After the handshake, all records are encrypted. The record layer uses the current traffic keys (derived from the key schedule) to encrypt/decrypt payloads via the negotiated AEAD algorithm.

```dovetail
enum ContentType
  ChangeCipherSpec
  Alert
  Handshake
  ApplicationData

record TlsRecord
  contentType: ContentType
  payload: Array<Uint8>
```

**Responsibilities:**
- Parse incoming byte stream into `TlsRecord` values (handling partial reads, buffering)
- Serialize outgoing `TlsRecord` values into bytes
- Encrypt outgoing records and decrypt incoming records using the current AEAD keys
- Enforce record size limits (max 2^14 bytes plaintext, 2^14 + 256 bytes ciphertext)
- Handle record padding (TLS 1.3 allows padding to hide content length)

### 3.3 Key Schedule

The TLS 1.3 key schedule (RFC 8446 Section 7.1) is a deterministic HKDF pipeline. Given a hash algorithm (determined by the cipher suite), it derives all keys and IVs:

```
             0
             |
             v
   PSK ->  HKDF-Extract = Early Secret
             |
             +-> Derive-Secret(., "c e traffic", ClientHello)
             |   = client_early_traffic_secret
             |
             +-> Derive-Secret(., "e exp master", ClientHello)
             |   = early_exporter_master_secret
             |
             v
       Derive-Secret(., "derived", "")
             |
             v
   ECDHE -> HKDF-Extract = Handshake Secret
             |
             +-> Derive-Secret(., "c hs traffic", ClientHello..ServerHello)
             |   = client_handshake_traffic_secret
             |
             +-> Derive-Secret(., "s hs traffic", ClientHello..ServerHello)
             |   = server_handshake_traffic_secret
             |
             v
       Derive-Secret(., "derived", "")
             |
             v
   0 ->    HKDF-Extract = Master Secret
             |
             +-> Derive-Secret(., "c ap traffic", ClientHello..server Finished)
             |   = client_application_traffic_secret_0
             |
             +-> Derive-Secret(., "s ap traffic", ClientHello..server Finished)
             |   = server_application_traffic_secret_0
             |
             +-> Derive-Secret(., "exp master", ClientHello..server Finished)
             |   = exporter_master_secret
             |
             +-> Derive-Secret(., "res master", ClientHello..client Finished)
                 = resumption_master_secret
```

Implementation is a class that progresses through these stages as the handshake advances:

```dovetail
class KeySchedule(cipherSuite: CipherSuite) =
  function initEarly(psk: Option<SymmetricKey>): Unit
  function initHandshake(sharedSecret: SymmetricKey, transcript: Array<Uint8>): Unit
  function initApplication(transcript: Array<Uint8>): Unit
  function deriveTrafficKeys(secret: SymmetricKey): TrafficKeys
  function updateTrafficSecret(currentSecret: SymmetricKey): SymmetricKey

record TrafficKeys
  key: SymmetricKey
  iv: Nonce
```

### 3.4 Handshake State Machine

The handshake is modeled as an explicit state machine. Each state knows which message it expects next, and transitions produce outgoing messages and key schedule updates.

**Client states:**

```
Start
  |──→ send ClientHello
  v
WaitServerHello
  |──→ receive ServerHello (extract ECDHE shared secret, init handshake keys)
  v
WaitEncryptedExtensions
  |──→ receive EncryptedExtensions
  v
WaitCertificateOrFinished
  |──→ receive Certificate (if server authenticates with cert)
  v
WaitCertificateVerify
  |──→ receive CertificateVerify (verify server signature)
  v
WaitFinished
  |──→ receive Finished (verify server finished MAC)
  |──→ send client Finished
  |──→ derive application keys
  v
Connected
```

**Server states:**

```
Start
  |──→ receive ClientHello
  |──→ send ServerHello
  |──→ send EncryptedExtensions
  |──→ send Certificate + CertificateVerify + Finished
  v
WaitClientFinished
  |──→ receive client Finished
  |──→ derive application keys
  v
Connected
```

```dovetail
enum ClientState
  Start
  WaitServerHello
  WaitEncryptedExtensions
  WaitCertificateOrFinished
  WaitCertificateVerify
  WaitFinished
  Connected
  Closed

enum ServerState
  Start
  WaitClientHello
  WaitClientFinished
  Connected
  Closed
```

### 3.5 Handshake Messages

Each handshake message type has a corresponding record type for parsing and serialization:

```dovetail
enum HandshakeMessage
  ClientHello(clientHello: ClientHelloMessage)
  ServerHello(serverHello: ServerHelloMessage)
  EncryptedExtensions(extensions: Array<Extension>)
  Certificate(certificate: CertificateMessage)
  CertificateVerify(verify: CertificateVerifyMessage)
  Finished(verifyData: Array<Uint8>)
  NewSessionTicket(ticket: NewSessionTicketMessage)
  KeyUpdate(requestUpdate: Boolean)

record ClientHelloMessage
  random: Array<Uint8>
  sessionId: Array<Uint8>
  cipherSuites: Array<CipherSuite>
  extensions: Array<Extension>

record ServerHelloMessage
  random: Array<Uint8>
  sessionId: Array<Uint8>
  cipherSuite: CipherSuite
  extensions: Array<Extension>

record CertificateMessage
  certificateRequestContext: Array<Uint8>
  certificates: Array<CertificateEntry>

record CertificateEntry
  certData: Array<Uint8>
  extensions: Array<Extension>

record CertificateVerifyMessage
  algorithm: SignatureScheme
  signature: Array<Uint8>

record NewSessionTicketMessage
  ticketLifetime: Int
  ticketAgeAdd: Int
  ticketNonce: Array<Uint8>
  ticket: Array<Uint8>
  extensions: Array<Extension>
```

### 3.6 Extensions

TLS 1.3 extensions are essential, not optional. The required extensions:

```dovetail
enum Extension
  ServerName(hostName: String)
  SupportedVersions(versions: Array<Int>)
  SupportedGroups(groups: Array<KeyExchangeGroup>)
  SignatureAlgorithms(algorithms: Array<SignatureScheme>)
  KeyShare(entries: Array<KeyShareEntry>)
  PreSharedKey(identities: Array<PskIdentity>, binders: Array<Array<Uint8>>)
  PskKeyExchangeModes(modes: Array<PskKeyExchangeMode>)
  Alpn(protocols: Array<String>)

record KeyShareEntry
  group: KeyExchangeGroup
  keyExchange: Array<Uint8>

record PskIdentity
  identity: Array<Uint8>
  obfuscatedTicketAge: Int

enum PskKeyExchangeMode
  PskOnly
  PskWithDhe
```

### 3.7 Transcript Hash

TLS 1.3 uses a running hash of all handshake messages for key derivation and the Finished message MAC. The transcript hash is updated as each handshake message is processed:

```dovetail
class TranscriptHash(hash: HashAlgorithm) =
  function update(message: Array<Uint8>): Unit
  function currentHash(): Array<Uint8>
  function clone(): TranscriptHash
```

`clone` is needed because some key schedule derivations need the hash at a specific point (e.g., up to ServerHello) while the transcript continues to accumulate later messages.

---

## 4. Usage Examples

### 4.1 Sans-I/O Client Handshake (manual)

```dovetail
let config = TlsClientConfig(
  serverName: "example.com",
  cipherSuites: [CipherSuite.Aes256GcmSha384, CipherSuite.ChaCha20Poly1305Sha256],
  keyExchangeGroups: [KeyExchangeGroup.X25519],
  signatureAlgorithms: [SignatureScheme.EcdsaSecp256r1Sha256, SignatureScheme.RsaPssRsaeSha256],
  trustAnchors: mozillaRoots(),
  alpnProtocols: ["h2", "http/1.1"],
  enableTickets: true
)

let tls = TlsClient(config)

// Drive the handshake manually (in practice, the async library does this)
// 1. TLS generates ClientHello
let clientHello = tls.dataToSend()
// ... send clientHello over TCP ...

// 2. Receive server response
tls.receiveData(serverBytes)

// 3. Process events
match tls.nextEvent()
  Some(TlsEvent.HandshakeComplete(params)) then
    // handshake done, send application data
    tls.sendApplicationData(httpRequest)
    let encrypted = tls.dataToSend()
    // ... send encrypted over TCP ...
  Some(TlsEvent.Error(err)) then
    // handle error
  _ then
    // need more data, continue reading from TCP
```

### 4.2 With Async Library Integration

```dovetail
import standard.crypto.TlsClient
import standard.crypto.TlsClientConfig
import standard.crypto.roots.mozillaRoots

async function fetchHttps(host: String, path: String): String =
  let tcp = await TcpStream.connect(host, 443)
  let tls = await TlsStream.connect(tcp, TlsClientConfig(
    serverName: host,
    trustAnchors: mozillaRoots(),
    alpnProtocols: ["http/1.1"]
  ))
  await tls.write("GET {path} HTTP/1.1\r\nHost: {host}\r\n\r\n".toBytes())
  let response = await tls.readAll()
  await tls.close()
  String.fromBytes(response)
```

`TlsStream` is provided by the async library. It wraps `TlsClient` and a `TcpStream`, driving the sans-I/O protocol engine by feeding bytes between the socket and the TLS state machine.

---

## 5. Security Considerations

### 5.1 Certificate Validation

Certificate validation is mandatory. The TLS library always validates the server's certificate chain against the configured trust anchors using `X509Validator` from `standard.crypto`. There is no option to skip validation — applications that need to connect to untrusted servers must provide their own trust anchors.

### 5.2 Constant-Time Operations

The Finished message verification uses `constantTimeEqual`. AEAD tag verification is handled by `standard.crypto`'s AEAD implementations, which use constant-time comparison internally.

### 5.3 Downgrade Prevention

The TLS 1.3 spec includes downgrade sentinels in ServerHello.random. The client checks for these and aborts if a downgrade to TLS 1.2 is detected. Since we don't support TLS 1.2, any downgrade sentinel is a fatal error.

### 5.4 Key Erasure

**Not implemented for TLS traffic state.** Traffic keys and secrets should be
wiped when replaced or when a connection closes. Resource management already
exists, and the I/O layer uses it for loaded private keys, but that does not wipe
the TLS state machine's internal traffic buffers. Current key rotation replaces
references without clearing the old buffers. This remains an implementation task;
WASMGC also limits any guarantee about stale copies and intermediate secrets.

---

## 6. Implementation Plan

### Phase 1: Record Layer

1. Implement `TlsRecord` parsing and serialization.
2. Implement record encryption/decryption using `AeadCipher` from `standard.crypto`.
3. Implement record buffering (handling partial reads).
4. Test with known record byte sequences from RFC 8446 Appendix B.

### Phase 2: Key Schedule

5. Implement `KeySchedule` class with HKDF-based key derivation.
6. Implement `TranscriptHash`.
7. Implement `deriveTrafficKeys` (key + IV from traffic secret).
8. Implement `HKDF-Expand-Label` (RFC 8446 Section 7.1).
9. Test with RFC 8448 test vectors (example handshake traces with all intermediate values).

### Phase 3: Handshake Messages

10. Implement serialization/deserialization for all handshake message types.
11. Implement extension parsing and serialization.
12. Test with known message byte sequences.

### Phase 4: Client Handshake

13. Implement `TlsClient` state machine (full handshake, no PSK).
14. Implement ClientHello generation with key share (X25519).
15. Implement ServerHello processing and key schedule advancement.
16. Implement EncryptedExtensions, Certificate, CertificateVerify, Finished processing.
17. Implement client Finished generation.
18. Test against a real TLS 1.3 server (e.g., localhost with openssl s_server).

### Phase 5: Server Handshake

19. Implement `TlsServer` state machine (full handshake, no PSK).
20. Test client ↔ server handshake within Dovetail.

### Phase 6: Application Data and Close

21. Implement `sendApplicationData` and application data decryption.
22. Implement `initiateClose` (close_notify alert).
23. Implement KeyUpdate (post-handshake key rotation).
24. Implement alert handling (parse incoming alerts, send alerts on error).

### Phase 7: Session Resumption

25. Implement NewSessionTicket processing (client).
26. Implement PSK-based handshake (0-RTT optional, PSK + ECDHE required).
27. Implement NewSessionTicket generation (server).

### Phase 8: Conformance Testing

Testing is layered from unit-level byte verification to full protocol conformance:

**Unit testing — [RFC 8448](https://www.rfc-editor.org/rfc/rfc8448) test vectors:**

28. Verify key schedule intermediate values byte-for-byte against RFC 8448 example traces. RFC 8448 provides complete handshake traces with every private key, shared secret, HKDF-Extract output, HKDF-Expand-Label output, traffic secret, key, and IV spelled out. Five scenarios: simple 1-RTT, resumed 0-RTT, HelloRetryRequest, client authentication, and compatibility mode.
29. Verify record layer encryption/decryption against RFC 8448 encrypted record bytes.

**Protocol conformance — [TLS-Anvil](https://tls-anvil.com/docs/Introduction) (~400 tests):**

30. Run TLS-Anvil's structured test suite against both `TlsClient` and `TlsServer`. TLS-Anvil is a Java-based conformance test framework with ~400 test cases derived from RFC 8446 and related RFCs. Tests run via Docker, takes ~15 minutes. Tests specification deviations, not just crashes.

**Edge cases and error handling — [tlsfuzzer](https://github.com/tlsfuzzer/tlsfuzzer) (~170+ TLS 1.3 scripts):**

31. Run tlsfuzzer against `TlsServer`. tlsfuzzer is a Python-based test suite that verifies correct error handling — it expects specific alert codes in response to malformed messages, not just "doesn't crash." Tests known vulnerabilities, protocol edge cases, and error paths.

**Interop testing:**

32. Test `TlsClient` against `openssl s_server`, rustls, Go's `crypto/tls`, and real-world servers (e.g., google.com, cloudflare.com).
33. Test `TlsServer` against `openssl s_client`, rustls, Go's `crypto/tls`, and web browsers.
34. Test `TlsClient` ↔ `TlsServer` handshake within Dovetail (self-interop).

---

## 7. Estimated Size

| Component | Estimated Lines |
|---|---|
| Record layer (framing, encryption) | 200-300 |
| Key schedule (HKDF pipeline) | 150-200 |
| Handshake messages (parse/serialize) | 300-400 |
| Extensions (parse/serialize) | 200-300 |
| Client state machine | 300-400 |
| Server state machine | 200-300 |
| Alert handling | 50-100 |
| Session resumption / PSK | 200-300 |
| **Total** | **~1600-2300** |

This is comparable in size to h11. The crypto heavy lifting is entirely in `standard.crypto` — the TLS library is protocol orchestration and byte manipulation.
