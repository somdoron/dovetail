# Crypto Library Design

This document designs the **crypto standard library** for Dovetail: a pure, synchronous, zero-dependency cryptographic primitives package sufficient to implement TLS 1.3 and general-purpose application cryptography. It covers the source material being ported, the Dovetail API surface (traits, types, algorithms), the addition of `U128`/`I128` built-in types leveraging the WASM wide-arithmetic proposal, and how the pure-sync library coexists with the async ecosystem.

**In scope:** Hash functions (SHA-256, SHA-384, SHA-512), MAC (HMAC), KDF (HKDF), AEAD ciphers (AES-128-GCM, AES-256-GCM, ChaCha20-Poly1305), key exchange (X25519, ECDHE-P256), digital signatures (Ed25519, ECDSA-P256, RSA-PSS), X.509 certificate parsing and chain validation, constant-time utilities, Uint128 language type addition.

**Out of scope:** Async runtime; streaming abstractions; password hashing (argon2, scrypt); post-quantum cryptography.

**Related:** [TLS 1.3 design](tls-library-design.md) — sans-I/O TLS 1.3 protocol implementation, part of the `standard.crypto` package.

**Implementation status:** Core library implemented; the full checklist is not
completely closed. `standard-crypto` contains SHA-2, HMAC, HKDF, AES-GCM,
ChaCha20-Poly1305, X25519, Ed25519, P-256, RSA-PSS signing/verification, ASN.1,
PEM, and X.509 validation. `standard-crypto-roots` embeds Mozilla-derived roots;
`standard-io-crypto` supplies randomness/resource extensions and TLS I/O.

**Status reconciliation:**

- Core source contains 80 tests, with additional root-chain and I/O tests. This
  inventory establishes implementation coverage, not an independent security
  review or a fresh test run.
- `StreamingCrypto` (§7, item 27) is not implemented; §5.3 already describes it as
  future streaming-library work, outside the pure primitives package.
- The specifically promised NIST RSA vector coverage (§7, item 16) is not
  established by the RSA tests inspected: they include PSS encoding roundtrips,
  generated-key signing/verification, and malformed-input rejection. Keep that
  validation item open until independent-vector coverage is demonstrated.
- The implemented roots API returns `List<X509Certificate>` from an embedded PEM
  resource, rather than the proposed DER constants and array API. Root validation
  tests use captured real certificate chains.
- Several original choices are superseded: primitives use interfaces where they
  are value types; X.509 adds a `standard-time` dependency; P-384 verification and
  RSA PKCS#1 v1.5 certificate signatures are present despite the old exclusion
  table. That table and the API sketches below are historical where they disagree
  with these implementation notes.

Source: [primitives](../standard-crypto/src/Hash.dove),
[RSA implementation/tests](../standard-crypto/src/RsaPss.dove),
[X.509](../standard-crypto/src/X509.dove),
[roots](../standard-crypto-roots/src/Roots.dove),
[chain tests](../standard-crypto-roots/src/ChainValidation.dove), and
[randomness extensions](../standard-io-crypto/src/CryptoRandom.dove).

---

## 1. Overview and Principles

### 1.1 Purpose

This library provides the **cryptographic building blocks** needed to implement secure protocols (TLS 1.3, noise protocol, etc.) and application-level cryptography (encrypt/decrypt, sign/verify, key derivation) in Dovetail. It is the lowest cryptographic layer — pure computation, no I/O, no platform dependencies.

### 1.2 Design Principles

- **Pure and sync.** Every function is synchronous. No dependency on the async runtime, WASI, or any other package. The crypto package is a leaf in the dependency graph.
- **Trait-based abstractions.** Hash, MAC, AEAD, key exchange, and signature algorithms are defined as traits. This enables generic code (HMAC parameterized by hash, HKDF parameterized by hash, TLS cipher suites parameterized by AEAD + hash).
- **Typed keys by role.** Separate record types for `SymmetricKey`, `SecretKey`, `PublicKey`, `Nonce` prevent misuse (swapping a key for a nonce). Keys are typed by cryptographic role, not per-algorithm, to keep the API manageable.
- **Streaming building blocks.** Stateful objects (`Hasher`, `Mac`) support incremental `update`/`finalize` for processing data in chunks. The crypto library does not provide streaming or async wrappers — it provides the building blocks that streaming/async libraries compose.
- **Singleton algorithms.** Algorithm instances (e.g. `Sha256`, `Aes128Gcm`, `X25519`) are singletons. They carry no state; the `Hasher`/`Mac` objects returned from factory methods carry the mutable state.
- **Correctness over cleverness.** Constant-time operations where required. Clear, auditable code. No unsafe optimizations that sacrifice readability.

### 1.3 Package Structure

All cryptographic primitives live in a single `standard.crypto` package. Submodules may be used for organization but the package is one compilation unit with no internal dependency layering concerns.

```
crypto/
  src/
    types.dove          # SymmetricKey, Nonce, SecretKey, PublicKey, KeyPair, CryptoError
    hash.dove           # HashAlgorithm, Hasher traits; Sha256, Sha384, Sha512
    mac.dove            # MacAlgorithm, Mac traits; Hmac
    kdf.dove            # Hkdf
    aead.dove           # AeadAlgorithm trait; Aes128Gcm, Aes256Gcm, ChaCha20Poly1305
    kx.dove             # KeyExchange trait; X25519, EcdhP256
    sign.dove           # SignatureAlgorithm trait; Ed25519, EcdsaP256, RsaPss
    x509.dove           # X509Certificate, X509Chain, certificate parsing and validation
    asn1.dove           # ASN.1 DER parser (internal, used by X.509 and RSA key parsing)
    util.dove           # constantTimeEqual
```

---

## 2. Source: Porting from BearSSL

### 2.1 Why BearSSL

[BearSSL](https://bearssl.org/) by Thomas Pornin is the primary source for porting. Rationale:

- **~30K lines of clean, portable C** with minimal dependencies. No dynamic allocation, no global state — a natural fit for WASM.
- **Designed for embedded/constrained environments.** Small code size, bounded memory usage. Aligns with Dovetail's WASM target.
- **Constant-time by design.** Timing side-channel resistance is a first-class concern, not an afterthought.
- **Complete TLS 1.3 primitive coverage.** Includes every algorithm needed: AES-GCM, ChaCha20-Poly1305, SHA-2 family, HMAC, HKDF, X25519, P-256 (ECDHE + ECDSA), RSA, Ed25519.
- **Well-isolated algorithm implementations.** Each primitive is self-contained. The TLS engine is separate from the crypto primitives. We port only the primitives.
- **Written by a world-class cryptographer.** The implementations are peer-reviewed, efficient, and correct.

### 2.2 What We Port

We port BearSSL's **crypto primitive layer** — not the TLS engine, not the X.509 engine, not the SSL record layer. Specifically:

| BearSSL source | Dovetail target | Notes |
|---|---|---|
| `src/hash/sha2*.c` | `Sha256`, `Sha384`, `Sha512` | SHA-2 family |
| `src/mac/hmac.c` | `Hmac` | Generic over `HashAlgorithm` |
| `src/kdf/hkdf.c` | `Hkdf` | HKDF-Extract and HKDF-Expand |
| `src/aead/gcm.c`, `src/symcipher/aes_ct*.c` | `Aes128Gcm`, `Aes256Gcm` | Constant-time AES + GCM mode |
| `src/aead/ccm.c` | — | Not needed for TLS 1.3 |
| `src/symcipher/chacha20_ct.c`, `src/mac/poly1305_ctmul*.c` | `ChaCha20Poly1305` | ChaCha20 stream cipher + Poly1305 MAC |
| `src/ec/ec_c25519_m*.c` | `X25519` | Curve25519 Diffie-Hellman |
| `src/ec/ec_p256_m*.c` | `EcdhP256`, `EcdsaP256` | NIST P-256 ECDHE + ECDSA |
| `src/ec/eddsa.c` (or ref impl) | `Ed25519` | EdDSA signatures |
| `src/rsa/rsa_*_pss.c` | `RsaPss` | RSA-PSS signatures only (PKCS#1 v1.5 excluded) |
| `src/int/i*.c` | internal bignum helpers | Modular arithmetic for RSA |
| `src/x509/x509_minimal.c`, `src/x509/x509_decoder.c` | `X509Certificate`, `X509Chain` | X.509 certificate parsing and chain validation |

### 2.3 Excluded Algorithms

BearSSL includes legacy algorithms for backward compatibility with older TLS versions. We do **not** port any of these:

| BearSSL source | Algorithm | Why excluded |
|---|---|---|
| `src/hash/md5.c` | MD5 | Broken — collision attacks since 2004 |
| `src/hash/sha1.c` | SHA-1 | Broken — SHAttered collision attack (2017) |
| `src/symcipher/des_*.c` | DES / 3DES | 64-bit block size, vulnerable to Sweet32; retired by NIST |
| `src/symcipher/aes_*_cbc*.c` | AES-CBC | Not AEAD; padding oracle attacks (POODLE, Lucky13). TLS 1.3 removed all CBC suites |
| `src/aead/ccm.c` | AES-CCM | Not required by TLS 1.3 |
| `src/symcipher/chacha20_ct.c` (standalone) | ChaCha20 without Poly1305 | Unauthenticated stream cipher; only the AEAD combo is ported |
| `src/rsa/rsa_*_pkcs1.c` (encryption) | RSA PKCS#1 v1.5 encryption | Bleichenbacher attack; TLS 1.3 removed RSA key exchange entirely |
| `src/rsa/rsa_*_pkcs1.c` (sign/verify) | RSA PKCS#1 v1.5 signatures | Replaced by RSA-PSS; only RSA-PSS is ported |
| `src/ec/ec_prime_i*.c` (P-384, P-521) | NIST P-384, P-521 | P-256 covers TLS 1.3 needs; P-384/P-521 add complexity with marginal benefit |
| `src/ssl/ssl_rec_cbc.c` | CBC record layer | Legacy TLS record format |
| N/A | RC4 | BearSSL already excludes this; included here for completeness |
| N/A | DHE (finite field DH) | Replaced by ECDHE; TLS 1.3 removed DHE cipher suites |

**Principle:** Only modern, unbroken algorithms that serve TLS 1.3 or general-purpose application crypto. No legacy, no "just in case." If an algorithm is not in the porting table in §2.2, it is explicitly excluded.

### 2.4 Porting Strategy

1. **Algorithm by algorithm**, in dependency order: hash → HMAC → HKDF → ChaCha20-Poly1305 → AES-GCM → X25519 → Ed25519 → P-256 → RSA → X.509.
2. **Test vectors from standards** (NIST CAVP, RFC test vectors) ported as Dovetail `test` declarations at each step.
3. **Transliterate first, then refine.** Initial port preserves BearSSL's algorithm structure closely. Refactor to idiomatic Dovetail (traits, pattern matching, named fields) after correctness is established.
4. **Constant-time discipline.** Preserve BearSSL's constant-time patterns. No data-dependent branches or memory access patterns on secret data.

---

## 3. Uint128 Language Type

### 3.1 Motivation

Cryptographic field arithmetic (Poly1305, Curve25519, P-256, RSA bignum) requires `u64 × u64 → u128` widening multiplication and 128-bit accumulation. Without a native unsigned 128-bit type, every such operation requires manual 4-multiply decomposition with carry propagation — error-prone and 2-7x slower than native.

Signed 128-bit integers (`Int128`) are not needed for cryptography — all crypto arithmetic is unsigned. `Int128` can be added later if a general-purpose use case arises.

### 3.2 WASM Wide-Arithmetic Proposal

The [WebAssembly wide-arithmetic proposal](https://github.com/WebAssembly/wide-arithmetic/blob/main/proposals/wide-arithmetic/Overview.md) adds four instructions:

| Instruction | Signature | Operation |
|---|---|---|
| `i64.add128` | `[i64 i64 i64 i64] → [i64 i64]` | 128-bit addition |
| `i64.sub128` | `[i64 i64 i64 i64] → [i64 i64]` | 128-bit subtraction |
| `i64.mul128` | `[i64 i64 i64 i64] → [i64 i64]` | 128-bit multiplication |
| `i64.mul_wide` | `[i64 i64] → [i64 i64]` | Widening multiply: i64 × i64 → 128-bit result |

All represent 128-bit integers as pairs of `i64` (lo, hi).

**Runtime support:** Wide-arithmetic is **Tier 2 in Wasmtime** — fully implemented, tested, fuzzed, and API-complete ([docs](https://docs.wasmtime.dev/stability-wasm-proposals.html)). Since Dovetail already depends on Tier 2 for WASMGC, wide-arithmetic adds no new tier risk.

### 3.3 Language Design

Add `Uint128` as a **built-in value type** alongside `Int` (i64) and `Uint8` (u8):

```dovetail
let a: Uint128 = 42u128
let b: Uint128 = Uint128.from(x) * Uint128.from(y)   // widening: Int × Int → Uint128
let lo: Int = b.low()
let hi: Int = b.high()
```

**Operations:**

| Operation | Dovetail syntax | WASM lowering |
|---|---|---|
| Addition | `a + b` | `i64.add128` |
| Subtraction | `a - b` | `i64.sub128` |
| Multiplication | `a * b` | `i64.mul128` |
| Widening multiply | `Uint128.from(x) * Uint128.from(y)` | `i64.mul_wide` (when both operands are widened i64) |
| Bit shift right | `a >> n` | Manual: shift both halves, combine |
| Bit shift left | `a << n` | Manual: shift both halves, combine |
| Bitwise AND/OR/XOR | `a & b`, `a \| b`, `a ^ b` | Two `i64.and`/`i64.or`/`i64.xor` |
| Extract low/high | `a.low()`, `a.high()` | Multi-value: first/second i64 |
| Construct | `Uint128(lo, hi)` | Two i64 on stack |
| Comparison | `a == b`, `a < b`, etc. | Compare hi, then lo |

**Representation:** Two `i64` values on the WASM stack. **Not** a heap-allocated GC struct — this is critical for crypto performance where tight loops produce millions of intermediate 128-bit values. The compiler treats `Uint128` as a multi-value stack type, similar to how multi-value returns work in WASM.

### 3.4 Impact on Crypto Code

With `Uint128`, Poly1305 accumulation becomes:

```dovetail
let product = Uint128.from(a) * Uint128.from(b)   // single i64.mul_wide
let lo = product.low()
let hi = product.high()
```

Without `Uint128`, the same operation would be ~15 lines of manual 32-bit decomposition. The entire Curve25519 field multiply (25 cross-products) becomes 25 `i64.mul_wide` instructions instead of 100+ instructions with carry propagation.

---

## 4. Dovetail API

### 4.1 Core Types

```dovetail
package standard.crypto

newtype SymmetricKey = Array<Uint8>
newtype Nonce = Array<Uint8>
newtype SecretKey = Array<Uint8>
newtype PublicKey = Array<Uint8>
record KeyPair = 
  secret: SecretKey
  public: PublicKey

enum CryptoError
  AuthenticationFailed
  InvalidKeySize(expected: Int, got: Int)
  InvalidNonceSize(expected: Int, got: Int)
  InvalidInput(message: String)
```

`SymmetricKey` is for symmetric algorithms (AEAD, MAC, KDF). `SecretKey`/`PublicKey` are for asymmetric algorithms (key exchange, signatures). This role-based typing prevents the most dangerous class of misuse (swapping a key for a nonce, using a symmetric key where an asymmetric key is expected) without creating a per-algorithm type explosion.

### 4.2 Hashing

```dovetail
trait Hasher
  function update(data: Array<Uint8>): Unit
  function finalize(): Array<Uint8>
  function reset(): Unit

trait HashAlgorithm
  property digestSize: Int
  property blockSize: Int
  function hasher(): Hasher
  function hash(data: Array<Uint8>): Array<Uint8>
```

`HashAlgorithm` is the algorithm singleton (e.g. `Sha256`). `Hasher` is the stateful streaming primitive — callers feed chunks via `update`, then `finalize` to get the digest. `reset` allows reuse without reallocation.

**Implementations:** `Sha256`, `Sha384`, `Sha512`.

The one-shot `hash` method is a convenience: `hash(data)` is equivalent to creating a hasher, updating with data, and finalizing.

### 4.3 MAC (Message Authentication Code)

```dovetail
trait Mac
  function update(data: Array<Uint8>): Unit
  function finalize(): Array<Uint8>
  function verify(tag: Array<Uint8>): Boolean

trait MacAlgorithm
  property macSize: Int
  function mac(key: SymmetricKey): Mac
```

`Mac` mirrors `Hasher` with the addition of `verify`, which performs constant-time comparison internally. Users should always use `verify` rather than comparing `finalize()` output manually.

**Implementations:** `Hmac` — takes any `HashAlgorithm`.

```dovetail
class Hmac(hash: HashAlgorithm) implements MacAlgorithm =
```

Usage:

```dovetail
let hmac = Hmac(Sha256)
let m = hmac.mac(key)
m.update(data)
let tag = m.finalize()
```

### 4.4 KDF (Key Derivation Function)

```dovetail
class Hkdf(hash: HashAlgorithm) =
  function extract(salt: Array<Uint8>, ikm: Array<Uint8>): SymmetricKey
  function expand(prk: SymmetricKey, info: Array<Uint8>, length: Int): Result<SymmetricKey, CryptoError>
```

HKDF has two phases: `extract` (salt + input keying material → pseudorandom key) and `expand` (PRK + info → output keying material of desired length). `expand` can fail if the requested length exceeds 255 × hash digest size.

HKDF is built on HMAC, which is built on a hash. `Hkdf(Sha256)` uses `Hmac(Sha256)` internally.

### 4.5 AEAD (Authenticated Encryption with Associated Data)

```dovetail
trait AeadCipher
  function addAuthenticatedData(data: Array<Uint8>): Unit
  function process(data: Array<Uint8>): Array<Uint8>
  function computeTag(): Array<Uint8>
  function verifyTag(tag: Array<Uint8>): Boolean

trait AeadAlgorithm
  property keySize: Int
  property nonceSize: Int
  property tagSize: Int
  function cipher(key: SymmetricKey, nonce: Nonce): AeadCipher
  function encrypt(key: SymmetricKey, nonce: Nonce, plaintext: Array<Uint8>, authenticatedData: Array<Uint8>): Array<Uint8>
  function decrypt(key: SymmetricKey, nonce: Nonce, ciphertext: Array<Uint8>, authenticatedData: Array<Uint8>): Result<Array<Uint8>, CryptoError>
```

`AeadCipher` is the low-level streaming building block, matching BearSSL's design. A single type handles both encryption and decryption (the underlying stream cipher operation is symmetric). The workflow follows BearSSL's `aad_inject → flip → run → check_tag` pattern:

1. Feed authenticated data (metadata that is verified but not encrypted, e.g. headers) via `addAuthenticatedData`.
2. Call `process` with plaintext (encrypting) or ciphertext (decrypting). Data is processed **in-place** and returned immediately — on decrypt, this means the caller receives **unauthenticated** plaintext.
3. Call `computeTag` (sender) to get the tag to send, or `verifyTag` (receiver) to verify the received tag. `verifyTag` uses constant-time comparison.

This is what BearSSL and OpenSSL do. Rust's RustCrypto and Go's stdlib chose a safer one-shot API instead, but one-shot doesn't provide the streaming building blocks that the async/streaming library needs. The one-shot `encrypt`/`decrypt` on `AeadAlgorithm` are the safe convenience methods — `decrypt` verifies the tag before returning plaintext, returning `AuthenticationFailed` if invalid.

**Implementations:** `Aes128Gcm`, `Aes256Gcm`, `ChaCha20Poly1305`.

### 4.6 Key Exchange

```dovetail
trait KeyExchange
  property publicKeySize: Int
  property secretKeySize: Int
  property sharedSecretSize: Int
  function derivePublicKey(secret: SecretKey): PublicKey
  function computeSharedSecret(mySecret: SecretKey, theirPublic: PublicKey): Result<SymmetricKey, CryptoError>
```

`derivePublicKey` computes the public key from a secret key (scalar multiplication by the generator point). `computeSharedSecret` performs the Diffie-Hellman operation.

Note: there is no `generateKeyPair` — key generation requires randomness, which lives in the async/WASI layer. The caller obtains random bytes from WASI, constructs a `SecretKey`, and uses `derivePublicKey`. See §5 for how the async library provides `generateKeyPair` via named extensions.

**Implementations:** `X25519`, `EcdhP256`.

### 4.7 Signatures

```dovetail
trait SignatureAlgorithm
  property signatureSize: Int
  property secretKeySize: Int
  property publicKeySize: Int
  function derivePublicKey(secret: SecretKey): PublicKey
  function sign(key: SecretKey, message: Array<Uint8>): Array<Uint8>
  function verify(key: PublicKey, message: Array<Uint8>, signature: Array<Uint8>): Boolean
```

`sign` produces a signature. `verify` returns a boolean (invalid signature is an expected outcome, not an error).

**Implementations:** `Ed25519`, `EcdsaP256`, `RsaPss`.

### 4.8 X.509 Certificates

```dovetail
record X509Certificate
  serialNumber: Array<Uint8>
  issuer: DistinguishedName
  subject: DistinguishedName
  notBefore: DateTime
  notAfter: DateTime
  publicKey: PublicKeyInfo
  extensions: Array<X509Extension>
  signatureAlgorithm: SignatureAlgorithmId
  signatureValue: Array<Uint8>

record DistinguishedName
  commonName: Option<String>
  organization: Option<String>
  country: Option<String>
  raw: Array<Uint8>

record PublicKeyInfo
  algorithm: PublicKeyAlgorithmId
  key: PublicKey

enum PublicKeyAlgorithmId
  Rsa
  EcdsaP256
  Ed25519

enum SignatureAlgorithmId
  RsaPssSha256
  RsaPssSha384
  RsaPkcs1Sha256
  RsaPkcs1Sha384
  EcdsaP256Sha256
  Ed25519

record X509Extension
  oid: Array<Uint8>
  critical: Boolean
  value: Array<Uint8>

enum X509Error
  MalformedCertificate(message: String)
  UnsupportedAlgorithm(oid: Array<Uint8>)
  SignatureVerificationFailed
  CertificateExpired
  CertificateNotYetValid
  ChainTooLong(maxDepth: Int)
  MissingTrustAnchor
  NameConstraintViolation
```

**Parsing:**

```dovetail
function parseCertificate(der: Array<Uint8>): Result<X509Certificate, X509Error>
function parseCertificateChain(der: Array<Uint8>): Result<Array<X509Certificate>, X509Error>
```

Certificates are parsed from DER-encoded ASN.1. PEM decoding (Base64 armor) can be a utility function or handled by a higher-level library.

**Chain validation:**

```dovetail
class X509Validator(trustAnchors: Array<X509Certificate>, maxDepth: Int) =
  function validate(chain: Array<X509Certificate>, currentTime: DateTime): Result<Unit, X509Error>
```

`X509Validator` verifies a certificate chain against a set of trust anchors (root CAs). Validation checks: signature verification at each link, expiration/not-before dates, basic constraints (CA flag, path length), and key usage. The `currentTime` is passed explicitly — no dependency on a clock, keeping the crypto package pure.

**ASN.1 DER parser (internal):**

The X.509 and RSA key formats are ASN.1 DER-encoded. An internal ASN.1 DER parser handles tag-length-value decoding. This is not exposed as public API — it is an implementation detail shared between X.509 parsing and RSA public key parsing.

### 4.9 Utilities

```dovetail
function constantTimeEqual(a: Array<Uint8>, b: Array<Uint8>): Boolean
```

Constant-time comparison of two byte arrays. Used internally by `Mac.verify` and signature verification. Exposed publicly for use by higher-level protocol implementations.

---

## 5. Async Integration via Named Extensions

### 5.1 Architecture

The crypto package has **zero awareness of async**. The async/WASI library extends crypto types with platform-aware functionality using Dovetail's named extension mechanism. This preserves the crypto package as a pure leaf dependency while providing a seamless API to end users.

```
┌─────────────────────────────────────────────────────────┐
│  streaming / async-crypto / tls                         │
│  depends on: crypto, crypto-roots, async                │
├──────────────┬──────────────────┬───────────────────────┤
│  crypto      │  crypto-roots   │  async (wasi)         │
│  (pure sync, │  (Mozilla CA    │  (runtime, yield,     │
│   primitives │   bundle)       │   random, thread pool)│
│   + x.509)   │  depends on:    │                       │
│  no deps     │  crypto         │                       │
└──────────────┴──────────────────┴───────────────────────┘
```

### 5.2 Randomness Extensions

The `standard.io.crypto` package provides named extensions on the crypto traits that generate key material from WASI entropy. **Implemented** (`standard-io-crypto/src/CryptoRandom.dove`).

Two design rules, both motivated by §5.5:

1. **Every random secret is a `Resource`, never a bare value.** A returned `SymmetricKey`/`KeyPair` would be unmanaged — never wiped. Instead the secret is acquired → used → released, with release zeroizing it on every exit path. Consumed with `use`.
2. **Acquisition is asynchronous.** Randomness is an I/O effect, so `Random.bytes` is wrapped in `Async.thunk` and run by the runtime, rather than called synchronously from pure-looking code. (WASI's binding is a sync intrinsic, but the *effect* belongs in the async world.)

So there are no eager `generateKey`/`generateNonce`/`generateKeyPair` value-returning functions — only resource factories:

```dovetail
package standard.io.crypto

import standard.io.crypto.CryptoRandom   // a named extension is brought into scope via import,
                                         // even within its own package
import standard.wasi.random.Random
import standard.io.Async
import standard.io.Resource
import standard.crypto.AeadAlgorithm
import standard.crypto.KeyExchange
import standard.crypto.SignatureAlgorithm
import standard.crypto.SymmetricKey
import standard.crypto.Nonce
import standard.crypto.KeyPair
import standard.crypto.HasSensitiveData

extension CryptoRandom for AeadAlgorithm =
    // Random AEAD key, wiped on release.
    public function key(self): Resource<SymmetricKey, Never> =
        Resource.make(
            () => Async.thunk(() => SymmetricKey(Random.bytes(self.keySize.toInt64()))),
            (k: SymmetricKey) => Async.thunk(() => k.clear()))

    // Random nonce; public, so release is a no-op (resource only to keep
    // generation in the async/effect world).
    public function nonce(self): Resource<Nonce, Never> =
        Resource.make(
            () => Async.thunk(() => Nonce(Random.bytes(self.nonceSize.toInt64()))),
            (_: Nonce) => Async.succeed(()))

extension CryptoRandom for KeyExchange =          // and identically for SignatureAlgorithm
    // Random key pair; the secret is wiped on release. Generation rejection-
    // samples via the pure `secretKeyFromBytes` hook so NIST scalars land in
    // [1, n-1] (Curve25519-family always accept).
    public function keyPair(self): Resource<KeyPair, Never> = ...
```

From the user's perspective, generation and use compose through `use`:

```dovetail
import standard.crypto.X25519
import standard.crypto.Aes256Gcm

async function example(peerPublic: PublicKey): Unit =
    let kp  = use X25519.keyPair()    // random key pair; secret wiped on scope exit
    let key = use Aes256Gcm.key()      // random AEAD key; wiped on scope exit
    let nonce = use Aes256Gcm.nonce()
    let shared = X25519.computeSharedSecret(kp.secret, peerPublic)  // from crypto
    let ct = Aes256Gcm.encrypt(key, nonce, data, aad)              // from crypto
```

The `secretKeyFromBytes` validation hook this relies on lives on the pure `KeyExchange`/`SignatureAlgorithm` traits — turning random bytes into a valid secret is a pure operation; only the entropy and the resource wiring are async.

Same-name extensions on different types (`CryptoRandom for AeadAlgorithm` / `for KeyExchange` / `for SignatureAlgorithm`) coexist in one package — the compiler already allows this (only same-name-*same-type* is rejected). Resolving against a trait-typed singleton (`Aes256Gcm : AeadAlgorithm`) works because extension lookup matches the receiver's static type.

### 5.3 Streaming Extensions

A future streaming library can extend crypto building blocks with async streaming capabilities:

```dovetail
package standard.streaming

import standard.crypto.HashAlgorithm

extension StreamingCrypto on HashAlgorithm
  async function hashStream(stream: ReadStream): Array<Uint8> =
    let h = this.hasher()
    for chunk in stream do
      h.update(chunk)
    h.finalize()
```

The crypto library provides the `Hasher` (with `update`/`finalize`) as a building block. The streaming library drives it chunk by chunk, yielding between chunks to avoid blocking the event loop. The crypto package never needs to know about `ReadStream`, `Awaitable`, or yielding.

### 5.4 Future Multi-Threading

When Dovetail gains multi-threading support, the async runtime can dispatch expensive sync crypto operations (RSA sign, P-256 ECDHE) to a blocking thread pool. The crypto API does not change — the sync functions run on worker threads, and the async wrapper handles the dispatch:

```dovetail
extension BlockingCrypto on SignatureAlgorithm
  async function signAsync(key: SecretKey, message: Array<Uint8>): Array<Uint8> =
    runBlocking(() -> this.sign(key, message))
```

This is transparent to callers. Today (single-threaded WASM), `signAsync` could yield periodically or simply run synchronously. Tomorrow (multi-threaded), it dispatches to a thread pool. The crypto library itself never changes.

### 5.5 Sensitive Data Zeroization

Key material should be wiped from memory after use, to shrink the window in which a memory disclosure, swap-to-disk, or cold-boot attack can recover it. The pure crypto package does **not** attempt this on its own — zeroization belongs in the **async layer**, for two reasons that reinforce each other:

1. **Secrets always originate from I/O.** A real key is never produced synchronously: it comes from the RNG (`CryptoRandom`, §5.2) or is read from a file. So the *acquisition* of a secret is inherently async, which means the natural unit of management is an async resource, acquired → used → released.

2. **Only the async resource gives a sound cleanup guarantee.** Wiping is only meaningful if it runs on *every* exit path — success, error, and cancellation. The async `use` expression desugars to `Async.bracket`, whose release is owned by the runtime and runs unconditionally. A *synchronous* `Usable`/`use` cannot promise this as cleanly (its release depends on closure early-return semantics), and — per point 1 — there is no synchronous secret source for it to manage anyway. So a sync "zeroizing secret" wrapper would be solving a problem that does not occur, while giving a false guarantee on the error paths that matter most.

The shape (**implemented** in `standard.io.crypto`, §5.2):

```dovetail
// A secret is an async resource: acquired from entropy, released by zeroizing.
extension CryptoRandom for AeadAlgorithm =
    public function key(self): Resource<SymmetricKey, Never> =
        Resource.make(
            () => Async.thunk(() => SymmetricKey(Random.bytes(self.keySize.toInt64()))),
            (k: SymmetricKey) => Async.thunk(() => k.clear()))

async function example(): Unit =
    let key = use Aes256Gcm.key()   // wiped when this scope exits, on any path
    let ct = Aes256Gcm.encrypt(key, nonce, data, aad)
    ...
```

To support this, the role-based key newtypes (`SymmetricKey`, `SecretKey`, `Nonce`) implement a small `HasSensitiveData` trait whose `clear()` overwrites the backing `Array<Uint8>` with zeros (`standard-crypto/src/Sensitive.dove`). This `clear()` is cheap and lives in the pure package independently of the async layer; only the `Resource`/`use` wiring is async. The acquire side wraps `Random.bytes` in `Async.thunk` (not `Async.succeed`) so the entropy read is an effect run by the runtime, not a value precomputed at resource-construction time.

**Honest limitation (WASMGC).** `clear()` overwrites the *buffers we hold* — the long-lived key material. It cannot reach the secret-derived **temporaries** that primitives scatter across the GC heap (HMAC's `ipad`/`opad`, the AES key schedule, curve field-element limbs, …); those are unreferenced GC objects we don't hold, WASMGC does not zero freed objects, and a moving collector may leave stale copies. This is the same best-effort posture as Java (`Destroyable`), Go, and .NET; only non-GC targets (C `explicit_bzero`, Rust `zeroize`) get closer to airtight. The guarantee is therefore: *long-lived key buffers are wiped on scope exit on every path* — not *no secret byte remains anywhere*. Scrubbing internal per-operation temporaries is a separate, lower-value, partially-GC-defeated effort that can be revisited per high-value primitive (HKDF, key schedules) if warranted.

---

## 6. Trust Anchors: `standard.crypto.roots` Package

### 6.1 Problem

WASI provides no access to the host's system certificate store (macOS Keychain, Linux `/etc/ssl/certs`, Windows Certificate Store). A WASM component is sandboxed — it cannot read system files or call OS-specific certificate APIs. Any TLS implementation running in-guest must bundle its own set of trusted root CA certificates.

### 6.2 Solution: Separate `standard.crypto.roots` Package

Trust anchors ship as a separate `standard.crypto.roots` package, not inside `standard.crypto` itself. This keeps the ~200KB of root CA data out of binaries that don't need TLS or certificate validation.

```
crypto-roots/
  src/
    roots.dove          # mozillaRoots() function, compiled-in CA certificates
```

```dovetail
package standard.crypto.roots

import standard.crypto.X509Certificate

function mozillaRoots(): Array<X509Certificate>
```

The package contains Mozilla's NSS root CA list (the same list used by Firefox, curl, the Rust `webpki-roots` crate, and most of the non-browser internet). Approximately 150 root certificates, DER-encoded and compiled into the binary as constant data.

### 6.3 Usage

```dovetail
import standard.crypto.X509Validator
import standard.crypto.roots.mozillaRoots

// Default: trust the standard Mozilla root CAs
let validator = X509Validator(mozillaRoots(), maxDepth: 10)

// Extend with a corporate/internal CA
let roots = mozillaRoots().append(corporateCa)
let validator = X509Validator(roots, maxDepth: 10)

// Certificate pinning: trust only specific roots
let validator = X509Validator([myPinnedRoot], maxDepth: 3)

// Custom trust store loaded from file (via async/filesystem)
let customRoots = parseCertificateChain(pemDecode(certFileBytes))
let validator = X509Validator(customRoots.unwrap(), maxDepth: 10)
```

### 6.4 Update Strategy

The Mozilla CA bundle changes a few times per year (CAs added, removed, or distrusted). Update approach:

- **Versioned with the standard library.** The `standard.crypto.roots` package is updated as part of the standard library releases. Users get updated roots by updating their standard library dependency.
- **Decoupled from the compiler.** Since crypto is a standard library package (not shipped with the compiler like prelude), root CA updates can ship independently of compiler releases — important for urgent CA revocations.
- **Runtime override.** Users can always supply their own trust anchors at runtime (loading from filesystem, embedding their own bundle), so they are never blocked by a stale `standard.crypto.roots` version.

---

## 7. Implementation Plan

### Phase 0: Language Prerequisites

1. Add `Uint128` to Dovetail as a built-in type with wide-arithmetic codegen.
2. Support same-name extensions on different types (e.g. `extension CryptoRandom on KeyExchange` and `extension CryptoRandom on AeadAlgorithm` in the same package). Currently, named extensions must have unique names per package.

### Phase 1: Foundations
2. Implement `Sha256`, `Sha384`, `Sha512` (ported from BearSSL).
3. Implement `Hmac` (parameterized by hash).
4. Implement `Hkdf` (built on HMAC).
5. Implement `constantTimeEqual`.
6. Test with NIST CAVP / RFC 4231 / RFC 5869 test vectors.

### Phase 2: Symmetric Ciphers

7. Implement `ChaCha20Poly1305` (ported from BearSSL).
8. Implement `Aes128Gcm`, `Aes256Gcm` (constant-time AES + GCM, ported from BearSSL).
9. Test with RFC 7539 / NIST SP 800-38D test vectors.

### Phase 3: Asymmetric — Curves

10. Implement `X25519` (Curve25519 scalar multiplication, ported from BearSSL).
11. Implement `Ed25519` (EdDSA signatures).
12. Implement `EcdhP256`, `EcdsaP256` (NIST P-256, ported from BearSSL).
13. Test with RFC 7748 / RFC 8032 / NIST ECDSA test vectors.

### Phase 4: RSA

14. Implement bignum arithmetic helpers (modular exponentiation, Montgomery multiplication).
15. Implement `RsaPss` (RSA-PSS signatures, verify + sign).
16. Test with NIST RSA test vectors.

### Phase 5: X.509

17. Implement ASN.1 DER parser (internal).
18. Implement `parseCertificate` and `parseCertificateChain`.
19. Implement `X509Validator` with chain validation (signature verification, expiration, basic constraints, key usage).
20. Test with real-world certificate chains (Let's Encrypt, DigiCert, etc.) and RFC 5280 edge cases.

### Phase 6: Trust Anchors

21. Create `standard.crypto.roots` package.
22. Import Mozilla NSS root CA list, convert to DER-encoded constant data.
23. Implement `mozillaRoots()` returning parsed `Array<X509Certificate>`.
24. Test chain validation against live certificate chains using Mozilla roots.

### Phase 7: TLS 1.3 — **Implemented** (`standard-tls`)

25. Implement TLS 1.3 sans-I/O protocol (see [tls-library-design.md](tls-library-design.md)). Done as a
    separate `standard.tls` package (depends on `standard.crypto` + `standard.crypto.roots`): client +
    server, full 1-RTT handshake across all three cipher suites, application data, alerts/`close_notify`,
    `KeyUpdate`, and PSK resumption — validated by in-Dovetail client↔server self-interop and an RFC 8448
    key-schedule KAT. To call RSA-PSS from TLS, `RsaPss.verify`/`sign` were made `public`.

### Phase 8: Async Integration

26. Add `CryptoRandom` named extensions in the async library.
27. Add `StreamingCrypto` named extensions in the streaming library.
28. End-to-end integration test: full TLS 1.3 handshake over TCP using async I/O.

---

## 8. TLS 1.3 Cipher Suite Coverage

The following table shows TLS 1.3 mandatory and recommended cipher suites and their coverage by this library:

| TLS 1.3 Cipher Suite | Hash | AEAD | Key Exchange | Signature | Covered |
|---|---|---|---|---|---|
| `TLS_AES_128_GCM_SHA256` | SHA-256 | AES-128-GCM | X25519 or P-256 | ECDSA-P256, RSA-PSS, Ed25519 | ✅ |
| `TLS_AES_256_GCM_SHA384` | SHA-384 | AES-256-GCM | X25519 or P-256 | ECDSA-P256, RSA-PSS, Ed25519 | ✅ |
| `TLS_CHACHA20_POLY1305_SHA256` | SHA-256 | ChaCha20-Poly1305 | X25519 or P-256 | ECDSA-P256, RSA-PSS, Ed25519 | ✅ |

All three mandatory TLS 1.3 cipher suites are fully covered by the algorithms in this library.

---

## 9. Future: High-Level Crypto Library (libsodium port)

`standard.crypto` is a **low-level, multi-algorithm** library for protocol implementers and library authors. A separate, **high-level** library will be ported from [libsodium](https://doc.libsodium.org/) in the future, providing opinionated, pre-selected algorithms for application developers.

The relationship:

```
┌──────────────────────────────────────────────┐
│  standard.sodium (future)                    │
│  High-level, opinionated API                 │
│  Pre-selected algorithms, hard to misuse     │
│  Argon2id, SecretStream, SecretBox, CryptoBox│
│  depends on: standard.crypto                 │
├──────────────────────────────────────────────┤
│  standard.crypto (this document)             │
│  Low-level, multi-algorithm primitives       │
│  Traits, building blocks, TLS 1.3 coverage   │
└──────────────────────────────────────────────┘
```

The libsodium port would add: Argon2id (password hashing), XChaCha20-Poly1305 (extended nonce AEAD), SecretStream (streaming encryption with per-chunk authentication and key ratcheting), SecretBox/CryptoBox (simple encrypt/decrypt APIs), and BLAKE2b (fast general-purpose hashing). Algorithm choices are fixed — the user doesn't pick between SHA-256 and SHA-384, they just call `hash()`.

`standard.sodium` will reuse `standard.crypto` primitives where possible, but some algorithms (e.g. BLAKE2b, XSalsa20, Argon2id) may not exist in `standard.crypto` or may not be usable in the exact way libsodium needs. In those cases, the algorithms will either be ported into `standard.crypto` first (adding them to the multi-algorithm toolkit) and then used by `standard.sodium`, or implemented directly in `standard.sodium` if they don't fit the `standard.crypto` trait model. The goal is to maximize reuse, but not at the cost of forcing awkward abstractions.

This is out of scope for the current design.
