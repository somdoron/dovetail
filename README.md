# Dovetail

Dovetail is a language for business logic, with records, enums, validated newtypes,
traits, classes, typed errors, and structured asynchronous work. It compiles to
WebAssembly with garbage collection and WASI component imports.

The compiler and libraries are evolving. Use matching compiler/library versions;
HTTP and TLS still need production hardening. Postgres and a full web framework
are not provided yet. See the [library guide](website/content/book/22-stdlib.md) for current scope.

Start with [Getting Started](website/content/book/01-getting-started.md) for installation, VS Code
setup, and a complete first program. Continue with the [book](website/content/book/toc.md),
[CLI guide](website/content/book/02-tool-commands.md), [container image guide](docs/container-images.md), and [standard-library guide](website/content/book/22-stdlib.md).

The [Astro website](website/README.md) publishes these same chapters for readers
and coding assistants, including Markdown exports and `llms.txt`. Preview locally
with `npm --prefix website ci` and `npm --prefix website run dev`.

Install the `dovetail` executable from crates.io:

```bash
cargo install dovetail-lang
```

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
programs. See [what CI validates](website/content/book/validation.md). Use `cargo run --` during
compiler development so validation uses the current source.

Contributor references: [repository guidance](CLAUDE.md), [grammar](grammar.md),
[compiler architecture](compiler.md), and [design documents](docs/toc.md).
For publishing compiler versions, see the [release guide](docs/releases.md).

Licensed under either the [MIT license](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option.
