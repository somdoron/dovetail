# Dovetail

Dovetail is a programming language for business logic. It compiles to WebAssembly
with garbage collection and WASI component imports.

Install the compiler and its bundled runtime with a current stable Rust toolchain:

```sh
cargo install dovetail-lang
```

The installed executable is named `dovetail`:

```sh
dovetail --version
dovetail --help
```

Use `cargo install dovetail-lang --locked` to build with the release's locked
dependency versions. Cargo's bin directory (usually `~/.cargo/bin`) must be on
your `PATH`. Wasmtime does not need to be installed separately.

See the [website](https://dovetaillang.org),
[getting started guide](https://github.com/somdoron/dovetail/blob/main/website/content/book/01-getting-started.md),
and [source repository](https://github.com/somdoron/dovetail).

Licensed under either the [MIT license](LICENSE-MIT) or the
[Apache License 2.0](LICENSE-APACHE), at your option.
