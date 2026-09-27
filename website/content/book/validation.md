# Book validation

Run from the repository with Python 3.11+ and the Rust toolchain:

```bash
python3 tools/check-book.py
```

CI runs the same check using the local compiler through `cargo run`. It checks local
Markdown links and heading anchors in the README and book, extracts programs marked
**Complete example (checked in CI)**, checks their types, formats them, checks formatting idempotence,
builds and runs them, compares stdout, and runs their declared tests. It also checks
that `init` produces a project that formats, checks, builds, and runs the first
program, including its test selected by name and source file.

Examples use a temporary workspace with this checkout's standard libraries. They do
not require a published standard-library tag, a database server, or network access
at runtime. Cargo dependencies must already be available or downloadable.

Unmarked snippets are explanatory fragments, sometimes deliberately showing errors;
they are not automatically compiled. Documentation comments are not executable doc
tests. External websites, published release downloads, and editor installation are
not checked by this script.

When adding a complete program, place this marker immediately before its Dovetail
code fence (use a unique lowercase name matching the program's package):

```text
<!-- book-example: {"name": "example", "depends": [], "stdout": ""} -->
```

Include `function main(): Unit` and at least one `test`. List required local library
project names in `depends`; `stdout` defaults to empty. Keep assertions deterministic.
The check fails on malformed/duplicate markers, compiler errors, runtime failures,
unexpected output, missing build artifacts, or a run without passing tests.

For a quicker documentation edit:

```bash
python3 tools/check-book.py --links-only
python3 tools/check-book.py --example hello
```
