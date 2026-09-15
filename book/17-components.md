# Part 17: Components

Dovetail programs can use libraries written in **other languages** — C, Rust, Go — as long as they are packaged as **WASM components** (the WASI component model). A component is a self-contained `.wasm` file that describes its own interface in **WIT** (WebAssembly Interface Types). Dovetail treats that interface as the boundary: the compiler reads it, generates a low-level Dovetail bindings package for it, and links the component into your program at build time.

There is no foreign function interface, no header files, and no glue code to write. The final artifact is still a single portable component whose only requirements are standard WASI — it runs anywhere `wasmtime run` does.

This part covers *consuming* components. **SQLite** is a full database compiled to WASM and shipped as an artifact of the `standard-sqlite` library. Its Git revision pins both the Dovetail wrapper and the component binary.

---

## 17.1 Declaring a Component Dependency

Components are declared per project in `Dovetail.toml` (see [Part 11: Packages](11-packages.md)):

```toml
compiler-version = "0.1.0"

[[project]]
name = "myapp"
root_package = "app"
packages = ["."]

[[project.component]]
path = "artifacts/sqlite.wasm" # relative to the project directory
package = "sqlite.raw"          # the Dovetail package the bindings appear as
```

Use **`path`** to reference a component `.wasm` file inside its owning project directory. Commit the actual binary with the library so Git dependencies carry it automatically. The compiler does not bundle SQLite or other library components.

The `package` key chooses the Dovetail package name under which the generated bindings become importable. If the component exports more than one interface, pick one with `interface = "..."`.

Component dependencies are inherited: if your project depends on a library project that declares a component, you get the component (and its bindings) automatically — this is how `standard-sqlite` brings in the sqlite component for you.

---

## 17.2 What Gets Generated

At build time the compiler reads the component's WIT interface and injects a **generated bindings package** that you can `import` from like any package. Its source is written under `.dovetail/generated/` for editor navigation. The projection is mechanical:

| WIT construct | Dovetail declaration |
|---|---|
| `record` | `record` (fields 1:1, camelCase) |
| `variant`, `enum` | `enum` |
| `option<T>`, `result<T, E>` | `Option<T>`, `Result<T, E>` |
| `string`, `list<T>`, `tuple<...>` | `String`, `Array<T>`, tuples |
| `flags` | `newtype` over `Uint32` + constant functions |
| `resource` | `newtype` handle + a module of methods |

A WIT **resource** — an opaque handle owned by the component, like a database connection — becomes a `newtype` over `Int32` with a module of functions, so method-call syntax works. Every resource gets an explicit `drop`:

```dovetail
// Generated (virtual) package sqlite.raw — what the compiler injects:
public newtype Database = Int32

module Database =
    public function open(path: String, openFlags: Uint32): Result<Database, Int32> = ...
    public function prepare(self, sql: String): Result<Statement, Int32> = ...
    public function errmsg(self): String = ...
    public function drop(self): Unit = ...
```

The generated layer is deliberately **low-level and policy-free**: raw result codes, explicit handles, nothing renamed or reinterpreted. Ergonomics belong to wrapper libraries written in ordinary Dovetail — that separation keeps the generated layer stable while APIs evolve freely.

Calling a binding crosses the component boundary through the canonical ABI: values are *copied* between Dovetail's garbage-collected heap and the component's linear memory. The copy is what keeps the boundary safe — there is no shared mutable memory between your program and the component.

---

## 17.3 Using SQLite

For a Git dependency, select `standard-sqlite` through the standard-library shorthand described in [Part 11](11-packages.md#115-github-dependencies):

```toml
compiler-version = "0.1.0"
standard-tag = "<tag>"

[[project]]
name = "myapp"
root_package = "app"
packages = ["."]
depends = ["standard-sqlite"]
```

Choose a tag whose manifest declares the matching compiler version. You can also select `standard-sqlite` in an explicit `[[dependencies]]` repository declaration. The library owns `artifacts/sqlite.wasm` and its component declaration; applications only need `depends = ["standard-sqlite"]`. Fetching the library brings the prebuilt binary, bindings, and macros transitively, without a SQLite build toolchain.

You rarely use `sqlite.raw` directly. The `standard-sqlite` library wraps it into an idiomatic API: connections and statements are **resources** (scoped with `use` — see [Part 13: Resources](13-resources.md)), and every operation that touches the database engine is **async** (see [Part 12: Async](12-async.md)):

```dovetail
package app

import standard.sqlite.Connection
import standard.sqlite.SqlValue
import standard.io.Async

function main(): Unit =
    let program: Async<Unit, SqliteError> = app()
    program.run()

async function app(): Async<Unit, SqliteError> =
    let conn = use Connection.open("data/app.db")

    await conn.execute("CREATE TABLE IF NOT EXISTS users (id INTEGER PRIMARY KEY, name TEXT, score REAL)")

    let params: Array<SqlValue> = [SqlValue.Text("alice"), SqlValue.Real(99.5)]
    await conn.executeWith("INSERT INTO users (name, score) VALUES (?1, ?2)", params)

    let rows = await conn.query("SELECT id, name, score FROM users ORDER BY id")
    for row in rows do
        match row.get(1) with
            case SqlValue.Text(name) => Console.println(name)
            case _ => ()
```

The connection is closed automatically when the `use` scope ends — no manual `close` call, even on early error exits. The main pieces:

- **`Connection`** — `open` / `openWith` return `Resource<Connection, SqliteError>`; `execute` / `executeWith` run statements without result rows; `query` / `queryWith` / `queryOne` read; plus `lastInsertRowid`, `changes`, and `statement(sql)` (a prepared statement as a resource).
- **`SqlValue`** — a dynamically-typed cell: `Null`, `Integer(Int64)`, `Real(Float64)`, `Text(String)`, `Blob(Array<Uint8>)`. Used both for binding parameters (1-based, matching `?1`) and reading rows.
- **`SqliteError`** — every failure flows through the `Async` error channel, carrying the raw SQLite result code plus the connection's error message.
- **`PreparedStatement`** — for finer control: `use conn.statement(sql)`, then `bindAll`, `step` (async — this is where SQL executes), `row`, `reset`. Reuse one statement across executions with `reset`; it is finalized when its scope ends.

Today each operation completes synchronously under the hood (like the WASI filesystem behind `standard.io.fs`), but the async shape is the contract — code written this way keeps working unchanged when truly non-blocking hosts arrive.

Transactions are a combinator — commit on success, rollback on failure:

```dovetail
let outcome = await conn.transaction<Int64>((tx: Connection) =>
    tx.execute("INSERT INTO audit (event) VALUES ('signup')")
)
```

The database is a normal SQLite file on the real filesystem — other tools (including the `sqlite3` CLI) can open it. One caveat from the WASI build: **only one process should use a database file at a time** (the build uses dot-file locking, and WAL mode is unavailable).

---

## 17.4 Building and Running

`dovetail build` produces one self-contained component: your code and every component dependency linked together, with only `wasi:*` imports remaining.

```
$ dovetail build
compiled myapp -> build/myapp.wasm

$ wasmtime run --dir=./data build/myapp.wasm
```

The `--dir` flag (or `dovetail run myapp --allow-path ./data`) grants the sandboxed program access to a host directory — that is where SQLite's database file physically lives. No WIT files, no linker flags, and no sidecar `.wasm` files are needed at run time; the component model metadata travels inside the artifact.

Because everything WASI-shaped stays an import, the same artifact runs under any WASI 0.2 runtime — `wasmtime`, or platforms built on it.

---

## 17.5 Limits (v1)

The projection covers most WIT, with a few constructs rejected at build time with a clear error:

- imported interfaces must be **self-contained** — they may not `use` types from other WIT packages;
- async WIT types (`future`, `stream`) are not supported yet;
- `flags` with more than 32 flags, functions with more than 16 flattened parameters, and multi-named results are not supported.

Exporting Dovetail code *as* a component (so other languages can call you) is planned but not yet available.

---

## Summary

- A **component** is a self-describing `.wasm` library; its **WIT interface** is the contract Dovetail compiles against — WIT is Dovetail's FFI.
- Declare components in `Dovetail.toml` with `[[project.component]]`; `path` loads an artifact from its owning library project.
- The compiler injects a **virtual low-level bindings package** (resources = handle newtypes + `drop`); wrapper libraries like `standard-sqlite` provide the ergonomic API.
- `dovetail build` composes everything into one portable artifact that runs on any WASI runtime with just `--dir` for filesystem access.

In the next part, we'll tour the standard library.
