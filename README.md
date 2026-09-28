# Dovetail

[Website](https://dovetaillang.org/) · [Documentation](https://dovetaillang.org/book/) · [Getting Started](https://dovetaillang.org/book/getting-started/)

Dovetail is a language for business logic, with records, enums, validated newtypes,
traits, classes, typed errors, and structured asynchronous work. It compiles to
WebAssembly with garbage collection and WASI component imports.

The compiler and libraries are evolving. Use matching compiler/library versions;
HTTP and TLS still need production hardening. Postgres and a full web framework
are not provided yet. See the [library guide](https://dovetaillang.org/book/stdlib/) for current scope.

Start with [Getting Started](https://dovetaillang.org/book/getting-started/) for installation, VS Code
setup, and a complete first program. Continue with the [book](https://dovetaillang.org/book/),
[CLI guide](https://dovetaillang.org/book/tool-commands/), [container image guide](https://dovetaillang.org/guides/container-images/), and [standard-library guide](https://dovetaillang.org/book/stdlib/).

Coding assistants can discover Markdown chapters through [llms.txt](https://dovetaillang.org/llms.txt)
or read the [complete book as Markdown](https://dovetaillang.org/llms-full.txt).
For local documentation development, see the [website guide](website/README.md).

Install the prebuilt `dovetail` executable on macOS or Linux:

```bash
curl -fsSL https://dovetaillang.org/install.sh | sh
```

On Windows (PowerShell):

```powershell
irm https://dovetaillang.org/install.ps1 | iex
```

The installers verify SHA-256 checksums before installing. Follow their PATH
instructions, then run `dovetail --version`. Rust is only needed when installing
from source, including `cargo install dovetail-lang --locked`.

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
programs. See [what CI validates](https://dovetaillang.org/guides/book-validation/). Use `cargo run --` during
compiler development so validation uses the current source.

Contributor references: [repository guidance](CLAUDE.md), [grammar](grammar.md),
[compiler architecture](compiler.md), and [design documents](docs/toc.md).
For publishing compiler versions, see the [release guide](docs/releases.md).

Licensed under either the [MIT license](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option.
