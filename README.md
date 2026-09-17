# Dovetail

Dovetail is a language for business logic, with records, enums, validated newtypes,
traits, classes, typed errors, and structured asynchronous work. It compiles to
WebAssembly with garbage collection and WASI component imports.

The compiler and libraries are evolving. Use matching compiler/library versions;
HTTP and TLS still need production hardening. Postgres and a full web framework
are not provided yet. See the [library guide](book/22-stdlib.md) for current scope.

Start with [Getting Started](book/01-getting-started.md) for installation, VS Code
setup, and a complete first program. Continue with the [book](book/toc.md),
[CLI guide](book/02-tool-commands.md), and [standard-library guide](book/22-stdlib.md).

From this checkout, with a Rust toolchain installed:

```bash
cargo build
cargo run -- --help
```

For language development and validation:

```bash
cargo run -- check
cargo run -- test
cargo test --workspace
python3 tools/check-book.py
```

Book validation requires Python 3.11+ and checks local links and marked complete
programs. See [what CI validates](book/validation.md). Use `cargo run --` during
compiler development so validation uses the current source.

Contributor references: [repository guidance](CLAUDE.md), [grammar](grammar.md),
[compiler architecture](compiler.md), and [design documents](docs/toc.md).
