# Filesystem Library Design (`standard-io-fs`)

This document designs **`standard-io-fs`** — a high-level, capability-aware filesystem library for Dovetail. It sits between the low-level `wasi:filesystem@0.2.10` wrappers (in the `wasi` project) and end-user code, providing an ergonomic `Path` + static-helper surface plus typed `Directory` / stream handles for capability-passing work.

**Status:** v1 shipped. Core API in `standard-io-fs/`:

- `Path` (pure value type, `Div<String>`/`Div<Path>` for `/` join)
- `Directory` (preopens, root, cwd, create, remove, entries, exists, metadata)
- `File` (readString, readBytes, writeString, writeBytes, remove, exists)
- Types: `FsError`, `EntryType`, `DirEntry`, `Metadata`

Dependencies landed:
1. [`resource-management-design.md`](resource-management-design.md) — `use` / `Usable` trait.
2. `Div<R>` trait + `/` operator overload (Rust-style naming, not `Joinable`).
3. `standard-text/src/Charset.dove` (UTF-8 only at v1; isolated project to support cross-cutting uses like network/parser).
4. `resource.drop` compiler intrinsic + `close()` on all relevant wasi handle types (`FileDescriptor`, `DirectoryStream`, `Pollable`, `InputStream<E>`, `OutputStream<E>`).
5. `Awaitable<T>.whileLoop(cond, body, trace)` trait method — desugar target for `await` inside `while` body.

**Known follow-ups:**

- `File.writeAtomic` (atomic-rename pattern from design §3.2) — not implemented.
- `Directory.removeAll` (recursive rm), `Directory.createAll` (mkdir -p), `Directory.rename` — not implemented.
- Instance-form methods on `Directory` (capability-rooted API) — only static forms ship in v1.
- `File.openInput` / `File.createFile` (stream-opener form returning `Resource<AsyncInputStream<FsError>, FsError>`) — not implemented. v1 `File.readBytes`/`writeBytes` inline the subscribe/checkWrite loops against the wasi stream directly because `AsyncInputStream<FileSystemError>::read` triggers a wasmtime compile error during monomorphization (likely a `use`-desugar + `Resource.make` interaction when `E` is a concrete enum; works for `E = Never`). The wrapper types `AsyncInputStream<E>` / `AsyncOutputStream<E>` exist in `standard-io` and typecheck; investigating the monomorphize bug remains an open task.

**In scope (v1):**

- New `standard-text` project containing `enum Charset = Utf8` (single variant for now). Created here so the fs API can take `Charset` parameters without reserving the namespace inside fs.
- Pure `Path` value type (sealed `Absolute` / `Relative`), with operator and methods.
- Capability handles: `Directory` (real type), and `InputStream<FsError>` / `OutputStream<FsError>` (reused from `wasi`) as the file-handle abstraction.
- One-shot helpers on the `File` and `Directory` modules that take a `Path` and route through preopens internally.
- One `FsError` enum mirroring WASI's `error-code`.
- Compact `Metadata` record.
- Atomic file write (`File.writeAtomic`).
- Type-specific existence checks (`File.exists`, `Directory.exists`).
- Recursive `removeAll` / `createAll` plus single-step variants.

**Out of scope (v1):**

- Symlink operations (`readlink`, `symlink`, `lstat`-style "don't follow"). Operations follow links by WASI host default.
- Directory walker. Users write their own recursion over `Directory.entries(path)`.
- Charsets other than UTF-8.
- POSIX permission bits (WASI 0.2 filesystem does not surface them).
- File locking, seeking/random access beyond what `InputStream`/`OutputStream` already give, mmap.
- `AsyncStream<DirEntry>` form of `entries()`.

---

## 1. Overview & Principles

### 1.1 Capability model

WASI 0.2 filesystem is **capability-rooted**: there is no `open("/etc/passwd")`. Every operation descends from a `Descriptor` that was either (a) granted to the program as a preopen by the host, or (b) derived (via `open-at`) from one that was. This library makes that model explicit but keeps the common case ergonomic.

Programs obtain preopened directories through the `Directory` module:

```dovetail
Directory.preopens(): Async<Array<(String, Directory)>, FsError>  // raw list
Directory.root: Async<Directory, FsError>                          // sugar over preopens
```

The static helpers (`File.readString(path)`, `Directory.create(path)`, …) route automatically:

- A `Path.Absolute(...)` descends from the preopen whose mount path is `/` (or the first preopen with an absolute mount).
- A `Path.Relative(...)` descends from the current-working-directory preopen.

Power users that want a constrained capability — "this handler can only read inside `./uploads`" — pass a `Directory` value around and call instance methods on it.

### 1.2 Two API layers

Almost every user-facing operation has a static form and an instance form:

| Need                              | Static (Path-rooted)                          | Instance (Directory-rooted)                |
|-----------------------------------|------------------------------------------------|---------------------------------------------|
| read a whole file                 | `File.readString(path)`                        | `dir.readString(relPath)`                   |
| stream-read a file                | `InputStream.openFile(path)`                   | `dir.openInput(relPath)`                    |
| stream-write a file (truncating)  | `OutputStream.openOrCreate(path)`              | `dir.openOrCreate(relPath)`                 |
| list a directory                  | `Directory.entries(path)`                      | `dir.entries()`                             |
| stat                              | `File.metadata(path)` / `Directory.metadata(p)`| `dir.metadata(relPath)`                     |

The static forms are sugar; they internally walk preopens, open the right `Directory`, and forward to the instance form. Users of the static forms do not see `Directory`.

### 1.3 No `File` type — streams are the handle

There is no nominal `File` type. Reading a file *is* holding an `InputStream<FsError>`; writing *is* holding an `OutputStream<FsError>`. Both already exist in `wasi`. This collapses the API:

- `InputStream.openFile(path): Async<Usable<InputStream<FsError>>, FsError>`
- `OutputStream.createFile(path): Async<Usable<OutputStream<FsError>>, FsError>` (fail if exists)
- `OutputStream.openOrCreate(path): Async<Usable<OutputStream<FsError>>, FsError>` (truncate if exists, create if not)
- `OutputStream.appendFile(path): Async<Usable<OutputStream<FsError>>, FsError>`
- `OutputStream.truncate(path): Async<Usable<OutputStream<FsError>>, FsError>` (must exist)

`File` exists as a **module** (a namespace for the one-shot helpers — `File.readString`, `File.writeString`, `File.writeAtomic`, `File.remove`, `File.exists`, `File.copy`, `File.metadata`). It is not a type.

### 1.4 Everything I/O is async; `Path` math is not

Every method that touches a descriptor returns `Async<T, FsError>`. `Path` itself is a pure value type — `path.parent`, `path.name`, `path / "sub"`, `path.isAbsolute` all return synchronously, allocate normal records, and never block.

---

## 2. Types

### 2.1 `Path` — pure value

```dovetail
public enum Path =
    Absolute(segments: Array<String>)
    Relative(segments: Array<String>)
```

**Properties (all sync, all pure):**

| Property                | Type               | Notes                                       |
|-------------------------|--------------------|---------------------------------------------|
| `segments(self)`        | `Array<String>`    | Same array for both variants.               |
| `isAbsolute(self)`      | `Bool`             | Pattern-match shortcut.                     |
| `name(self)`            | `Option<String>`   | Last segment, or `None` if empty.           |
| `stem(self)`            | `Option<String>`   | `name` with extension stripped.             |
| `extension(self)`       | `Option<String>`   | After last `.` in `name`; `None` if absent. |
| `parent(self)`          | `Option<Path>`     | All-but-last; `None` on bare root/empty.    |

**Methods (sync, pure):**

| Method                          | Returns | Notes                                                |
|---------------------------------|---------|------------------------------------------------------|
| `join(self, segment: String)`   | `Path`  | Append one segment.                                  |
| `join(self, other: Path)`       | `Path`  | Append a relative path; `Absolute` `other` panics.   |
| `withName(self, name: String)`  | `Path`  | Replace `name`.                                      |
| `withExtension(self, ext: String)` | `Path` | Replace extension (or add if absent).            |
| `normalize(self)`               | `Path`  | Collapse `.` / `..` to the extent possible (purely syntactically; does not consult the disk). |

**Constructors:**

| Constructor                    | Returns | Notes                                                          |
|--------------------------------|---------|----------------------------------------------------------------|
| `Path.of(s: String)`           | `Path`  | Parse a string; leading `/` → `Absolute`. Errors on NUL bytes (panic — not a user-input boundary). |
| `Path.absolute(segs: Array<String>)` | `Path` | Construct directly.                                       |
| `Path.relative(segs: Array<String>)` | `Path` | Construct directly.                                       |

**Operator `/` (depends on `Div<T>` trait):**

```dovetail
public trait Div<R> =
    type Output
    function join(self, rhs: R): Self.Output

implement Div<String> for Path =
    type Output = Path
    function join(self, rhs: String): Path = self.join(rhs)

implement Div<Path> for Path =
    type Output = Path
    function join(self, rhs: Path): Path = self.join(rhs)
```

The compiler desugars `a / b` to `a.join(b)` when a `Div<typeof(b)>` impl exists for `typeof(a)`. (See §6.1 dependencies — this is small new compiler work, not specific to fs.)

```dovetail
Path.of("logs") / "today" / "input.log"   // Relative path
Path.absolute(["etc"]) / "config.toml"    // Absolute path
```

### 2.2 `Directory` — opened capability handle

Newtype over the WASI `Descriptor` resource handle. Implements `Usable` (see [resource-management-design.md](resource-management-design.md)) so it's used with `use`:

```dovetail
public newtype Directory = Int32   // WASI descriptor handle

implement Usable for Directory =
    function close(self): Async<Unit, FsError> = intrinsic
```

**Module `Directory` — static helpers:**

| Function                                          | Returns                                  | Notes                                       |
|---------------------------------------------------|------------------------------------------|---------------------------------------------|
| `Directory.preopens(): …`                         | `Async<Array<(String, Directory)>, FsError>` | Raw WASI preopen list.                  |
| `Directory.root`                                  | `Async<Directory, FsError>`              | Sugar — picks the `/` preopen or first.     |
| `Directory.cwd`                                   | `Async<Directory, FsError>`              | Working-directory preopen.                  |
| `Directory.open(path: Path)`                      | `Async<Usable<Directory>, FsError>`      | Smart-routed. Implicit preopen by variant.  |
| `Directory.create(path: Path)`                    | `Async<Unit, FsError>`                   | Single level; parent must exist.            |
| `Directory.createAll(path: Path)`                 | `Async<Unit, FsError>`                   | `mkdir -p`.                                 |
| `Directory.remove(path: Path)`                    | `Async<Unit, FsError>`                   | Must be empty.                              |
| `Directory.removeAll(path: Path)`                 | `Async<Unit, FsError>`                   | Recursive rm.                               |
| `Directory.rename(from: Path, to: Path)`          | `Async<Unit, FsError>`                   | Atomic when on same descriptor.             |
| `Directory.entries(path: Path)`                   | `Async<Array<DirEntry>, FsError>`        | One level.                                  |
| `Directory.exists(path: Path)`                    | `Async<Bool, FsError>`                   | True only if a directory exists at `path`.  |
| `Directory.metadata(path: Path)`                  | `Async<Option<Metadata>, FsError>`       | `None` if missing.                          |

**Instance methods (capability-rooted form):**

| Method                                                            | Returns                                  |
|-------------------------------------------------------------------|------------------------------------------|
| `dir.open(relPath: Path): …`                                      | `Async<Usable<Directory>, FsError>`      |
| `dir.create(relPath: Path)` / `createAll` / `remove` / `removeAll`/ `rename`/`entries`/`exists`/`metadata` | (same shapes as static, rooted at `dir`) |
| `dir.openInput(relPath: Path)`                                    | `Async<Usable<InputStream<FsError>>, FsError>` |
| `dir.openOrCreate(relPath: Path)`/`createFile`/`appendFile`/`truncate` | `Async<Usable<OutputStream<FsError>>, FsError>` |
| `dir.readString(relPath: Path)`/`writeString`/`readBytes`/`writeBytes`/`writeAtomic`/`copy` | (file-helper shapes, rooted at `dir`) |

The instance form takes only `Path.Relative(...)`; passing an absolute path panics (capability violation).

### 2.3 `InputStream<FsError>` / `OutputStream<FsError>` — file handles

Reused from the `wasi` package. The fs library adds **static module-level constructors** that produce `Usable<InputStream<FsError>>` / `Usable<OutputStream<FsError>>` rooted in a preopen:

```dovetail
// in standard.io.fs

module InputStream =   // adds methods to the existing wasi InputStream module
    public function openFile(path: Path): Async<Usable<InputStream<FsError>>, FsError> = …

module OutputStream =
    public function createFile(path: Path): Async<Usable<OutputStream<FsError>>, FsError> = …
    public function openOrCreate(path: Path): Async<Usable<OutputStream<FsError>>, FsError> = …
    public function appendFile(path: Path): Async<Usable<OutputStream<FsError>>, FsError> = …
    public function truncate(path: Path): Async<Usable<OutputStream<FsError>>, FsError> = …
```

(These additions are written as a named extension on the existing `InputStream<E>` / `OutputStream<E>` types — using the same multi-target named-extension feature added for `TimeExtension`. The extension's `for_type` is the concrete `InputStream<FsError>` / `OutputStream<FsError>` instantiation.)

### 2.4 `File` — module-only

`File` is **not a type**. It is a namespace for one-shot helpers:

| Function                                                  | Returns                       |
|-----------------------------------------------------------|-------------------------------|
| `File.readString(path: Path)`                             | `Async<String, FsError>` (UTF-8) |
| `File.readString(path: Path, encoding: Charset)`         | `Async<String, FsError>`     |
| `File.readBytes(path: Path)`                              | `Async<Array<Uint8>, FsError>`       |
| `File.writeString(path: Path, content: String)`           | `Async<Unit, FsError>`       |
| `File.writeBytes(path: Path, content: Array<Uint8>)`             | `Async<Unit, FsError>`       |
| `File.writeAtomic(path: Path, content: String)`           | `Async<Unit, FsError>` (see §3.2) |
| `File.writeAtomicBytes(path: Path, content: Array<Uint8>)`       | `Async<Unit, FsError>`       |
| `File.copy(from: Path, to: Path)`                         | `Async<Unit, FsError>`       |
| `File.remove(path: Path)`                                 | `Async<Unit, FsError>`       |
| `File.exists(path: Path)`                                 | `Async<Bool, FsError>` (true only if regular file) |
| `File.metadata(path: Path)`                               | `Async<Option<Metadata>, FsError>` |

The `String` / `Array<Uint8>` helpers open a stream, read/write, and close, all inside `use`. Atomic write writes to a sibling temp file, fsyncs, and renames.

### 2.5 `DirEntry`, `EntryType`, `Metadata`, `FsError`

```dovetail
public enum EntryType =
    File
    Directory
    Symlink
    Other

public record DirEntry =
    name: String
    type: EntryType

public record Metadata =
    size: Int64
    type: EntryType
    modified: Instant
    created: Option<Instant>   // None on hosts that don't track creation time

public enum FsError =
    NotFound
    AccessDenied
    AlreadyExists
    IsDirectory
    NotDirectory
    NotEmpty                // for Directory.remove on a non-empty dir
    InvalidPath             // bad UTF-8, NUL byte, etc.
    InvalidUtf8             // for readString when bytes aren't UTF-8
    Loop                    // symlink loop
    NameTooLong
    TooManyLinks
    OutOfSpace
    ReadOnly
    Interrupted
    PipeBroken              // for stream operations
    InvalidCharset         // for readString with an explicit encoding mismatch
    Unsupported             // host doesn't support the op
    Other(code: Int32)      // catch-all for anything not enumerated

implement IoError for FsError
implement Display for FsError    // human-readable per-variant strings
```

`FsError` implements `IoError`, so it slots into the existing `InputStream<E>` / `OutputStream<E>` shape (`InputStream<FsError>`).

---

## 3. Operation details

### 3.1 One-shot helpers route through preopens

Pseudocode for `File.readString(path)`:

```dovetail
public async function readString(path: Path): Async<String, FsError> =
    use stream = await InputStream.openFile(path)
    await stream.readAllString()    // assumes a helper on InputStream<FsError> that reads to EOF + decodes UTF-8

// InputStream.openFile, in turn:
public async function openFile(path: Path): Async<Usable<InputStream<FsError>>, FsError> =
    let dir: Directory = await rootForPath(path)
    use opened: Directory = ... walk relative segments via dir.open(...) ...
    await opened.openInputLocal(lastSegment)
```

The `rootForPath` helper picks `Directory.cwd` for `Relative` and the `/`-preopen for `Absolute`. Multi-segment paths walk via `open-at` for each intermediate directory.

### 3.2 `File.writeAtomic` semantics

```
1. Generate temp name in the same directory: e.g. ".<basename>.tmp.<random>"
2. Write content to temp via OutputStream.createFile(tempPath).
3. fsync the temp via OutputStream.sync() (exposed on the OutputStream).
4. Directory.rename(tempPath, path) — atomic on POSIX-like hosts.
5. fsync the directory entry (if available; best-effort).
```

If any step fails before the rename, the temp file is removed. Documented as: caller is guaranteed to see either the old content (unchanged) or the new content (committed) in the absence of host-level failures past step 4. The temp-file name is chosen to be hidden on most hosts; we don't expose it. Random suffix is 8 bytes from `Random.bytes`.

### 3.3 `Directory.entries` ordering and stability

Entries are returned in the order WASI yields them — i.e., **unspecified**. Users that need a stable order sort the result themselves. `.` and `..` are never included.

### 3.4 `Directory.removeAll` failure semantics

Documented as best-effort: if removal of an inner entry fails, the operation stops and returns the error. Partially-removed state is observable. Future enhancement: a variant that takes a callback to decide on continue/abort per error.

### 3.5 Cross-FS limits

`Directory.rename(from, to)` requires `from` and `to` to be reachable from the same root descriptor for atomicity. If they aren't (e.g., two different preopens), the operation falls back to copy + remove and is no longer atomic. Documented behavior.

---

## 4. Package layout

Two new projects ship together: `standard-text` (housing the `Charset` enum, see §4.1) and `standard-io-fs`.

### 4.1 `standard-text`

A new minimal project to host text-encoding types. v1 is intentionally tiny — one file, one enum, one variant — but creating the project now reserves the namespace, so future additions (charset detection, decoder helpers, more encodings) won't be a breaking move from `standard.io.fs.Charset` to `standard.text.Charset`.

Manifest entry:

```toml
[[project]]
name = "standard-text"
root_package = "standard.text"
depends = []
packages = ["."]
```

Sources:

```
standard-text/
└── src/
    └── Charset.dove           -- public enum Charset = Utf8
```

`Charset.dove`:

```dovetail
package standard.text

/// Text encoding for `String` ↔ bytes conversions. v1 only supports UTF-8;
/// further variants (UTF-16, Latin-1, …) will be added when there is a real user.
public enum Charset =
    Utf8
```

No methods, no module — just the enum. Adding charset-aware decode helpers (`Charset.decode(self, bytes: Array<Uint8>): Result<String, CharsetError>` etc.) is future work and lives in this project when it comes.

### 4.2 `standard-io-fs`

Manifest entry:

```toml
[[project]]
name = "standard-io-fs"
root_package = "standard.io.fs"
depends = [
    "standard-collection",
    "standard-time",
    "standard-text",
    "wasi",
    "standard-io",
]
packages = ["."]
```

Sources:

```
standard-io-fs/
├── src/
│   ├── Path.dove               -- module standard.io.fs.Path + Path enum + Div impls
│   ├── Directory.dove          -- newtype Directory + module Directory (static + instance methods)
│   ├── File.dove               -- module File (one-shot helpers)
│   ├── Streams.dove            -- named extensions adding `openFile` / `createFile` / … to InputStream/OutputStream
│   ├── types.dove              -- FsError, EntryType, DirEntry, Metadata
│   └── intrinsics.dove         -- WASI fs intrinsic functions (open-at, read-directory, stat, etc.)
└── test/
    └── *_test.dove
```

The `intrinsics.dove` file is the only file that touches WASI directly. Everything else builds on it. `Charset` is imported from `standard.text`.

---

## 5. Examples

### 5.1 Read a config

```dovetail
import standard.io.fs.File
import standard.io.fs.Path

async function loadConfig(): Async<String, FsError> =
    await File.readString(Path.of("./config.toml"))
```

### 5.2 Atomic update

```dovetail
import standard.io.fs.File
import standard.io.fs.Path

async function bumpCounter(): Async<Unit, FsError> =
    let path = Path.of("./state.txt")
    let current: Int32 = (await File.readString(path)).parseInt32().or(0)
    await File.writeAtomic(path, (current + 1).format())
```

### 5.3 Walk a directory (user-written recursion)

```dovetail
import standard.io.fs.Directory
import standard.io.fs.Path

async function fileSizes(root: Path): Async<Array<(Path, Int64)>, FsError> =
    let entries = await Directory.entries(root)
    let mutable result: Array<(Path, Int64)> = []
    for entry in entries do
        let child = root / entry.name
        match entry.type with
        case EntryType.File =>
            let md = await File.metadata(child)
            match md with
            case Some(m) => result.push((child, m.size))
            case None => ()
        case EntryType.Directory =>
            let sub = await fileSizes(child)
            result.appendAll(sub)
        case _ => ()
    result
```

### 5.4 Streaming a large file

```dovetail
import standard.io.fs.OutputStream
import standard.io.fs.Path

async function dumpLines(target: Path, lines: Array<String>): Async<Unit, FsError> =
    use out = await OutputStream.openOrCreate(target)
    for line in lines do
        await out.write(line.bytes())
        await out.write("\n".bytes())
    await out.sync()
```

### 5.5 Capability-constrained access

```dovetail
import standard.io.fs.Directory
import standard.io.fs.Path

async function listUploads(root: Directory): Async<Array<DirEntry>, FsError> =
    use uploads = await root.open(Path.relative(["uploads"]))
    await uploads.entries()
```

`root` was granted by a caller; `listUploads` cannot escape it (cannot construct an absolute path from inside, cannot call the static helpers without already having a preopen).

---

## 6. Dependencies and open work

### 6.1 `Div<T>` + `/` operator

The parser already accepts `a / b` as a binary expression (integer division). To overload, we add:

- A `Div<R>` trait with associated type `Output` and method `join(self, rhs: R): Output`.
- A typechecker rule: when the LHS of `/` is not a numeric type, look up `Div<typeof(RHS)> for typeof(LHS)` and rewrite `a / b` to `a.join(b)`. Failure → existing "no overload" diagnostic.

Small isolated piece of work — separate design doc.

### 6.2 `Usable` / `use` from resource-management

Every `Directory`, `InputStream<FsError>`, `OutputStream<FsError>` produced by an `open*` method is wrapped in `Usable<…>`. The fs library imports the trait from the prelude (or wherever resource-management lands it) and implements it on its newtypes. No fs code ships until that design lands.

### 6.3 `Charset` type

`File.readString(path, encoding: Charset)` references `standard.text.Charset`. v1 of `standard-text` (created as part of this work — see §4.1) defines:

```dovetail
public enum Charset =
    Utf8
```

The fs helpers accept only `Charset.Utf8` for now and return `FsError.InvalidCharset` for any other value (none exist yet, but the error variant is reserved). Adding more encoding variants — and the corresponding decode/encode implementations — is future work inside `standard-text`; no fs-side change is needed when new variants land.

### 6.4 Future work (not in v1)

| Feature                        | Notes                                                                |
|--------------------------------|----------------------------------------------------------------------|
| Symlink ops                    | `readlink`, `symlink`, `linkMetadata` (no-follow stat).              |
| Recursive walker               | `Directory.walk(path, filter)` once we have a clear use case.        |
| `AsyncStream<DirEntry>`        | Blocks on a streaming-design for `AsyncStream`.                      |
| Hard links                     | WASI exposes them; we just don't surface them in v1.                 |
| File modes / permission bits   | Blocked on WASI exposing them (it doesn't, in 0.2).                  |
| File locking                   | Out of scope; advisory locking is host-dependent.                    |
| Random-access reads/writes     | WASI streams aren't seekable; would need a different abstraction.    |
| `Directory.copy(from, to)`     | Recursive copy. Easy to write but easy to get wrong (errors mid-tree). |

---

## 7. Verification plan (post-implementation)

When the library is built:

- Unit tests in each `src/*.dove` for pure operations (`Path` math, error mapping).
- Integration tests in `standard-io-fs/test/`:
  - **Round-trip**: write a string, read it back, compare. Static and instance forms.
  - **Atomic write**: `writeAtomic` produces final content; temp file is gone on success.
  - **Atomic write under failure**: simulate a write error; original file is unchanged.
  - **Directory ops**: create/list/remove a tree.
  - **Capability isolation**: a `Directory` handle cannot be used to escape its subtree (`dir.open(Path.absolute([...]))` errors).
  - **Charset errors**: `readString` on non-UTF-8 bytes returns `InvalidUtf8`.
  - **Recursive remove**: tree of 3 levels deep; verify empty afterwards.
  - **Rename across preopens**: falls back to copy+remove; non-atomic but succeeds.
- Manual smoke against a real CLI invocation (the test workspace mounts a tempdir as a preopen, the test runs as a WASI component against it).
