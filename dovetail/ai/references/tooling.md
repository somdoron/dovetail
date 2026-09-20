# Development tools

## Commands

Install the compiler with `cargo install dovetail-lang` (add `--locked` to use
the release's locked dependencies). The package is `dovetail-lang`; the executable
is `dovetail`, and it includes the runtime. Cargo's bin directory must be on `PATH`.

The matching GitHub release includes a VS Code `.vsix` package. Install it with
**Extensions: Install from VSIX...**; install the compiler separately and set
`dovetail.serverPath` if it is not on `PATH`.

Use `dovetail --help` and `<command> --help` for the installed version. During
compiler development use `cargo run --` so the installed binary cannot mask changes.
Manifest-based commands normally run at the workspace root; fmt also searches
ancestors, as do API queries. `dovetail ai install` searches ancestors and refreshes remembered targets.

```sh
dovetail fmt
dovetail fmt --check
dovetail check myapp
dovetail build myapp
dovetail test myapp
dovetail test myapp --filter "quantity" --file myapp/src/Quantity.dove
```

`build` writes Wasm components under `build/` (override with `--output-dir`/`-o`).
`check` stops after type checking. Warnings do not fail compilation. Test filters
match fully qualified names; repeated filters are OR'd. There is no test `--verbose`.
`run` builds and executes an application's `function main(): Unit`.

## Tests

```dovetail
function add(left: Int32, right: Int32): Int32 = left + right

test "addition preserves a negative operand" = assert add(-2, 3) == 1
```

Place tests beside rules in `src/` or in the project's `test/` tree. Use descriptive
names, assert observable behavior, and test boundary/error cases. Documentation
comments are not executable doc tests. Use the supported `@panics` attribute for
expected defects (`@panics("message")` can match a message), `@timeout(1000)`
for millisecond time limits, and `@skip` for intentionally disabled tests; do not hide expected
input rejection in panic tests. Execute async work through its runner from tests.

Use real Dovetail workspaces for language/library validation. Tests need not return
results through main: main returns Unit and assertions check values. Keep external
services optional; prefer deterministic in-memory or injected dependencies.

## Formatter and diagnostics

The formatter controls layout, four-space indentation, and a 100-column target.
It preserves literal/comment text and import/declaration order; there are no style
settings or disable directives. It reparses output to check structure. Syntax errors
leave inputs unchanged. `fmt --check` exits 1 for differences and 2 for errors.
Explicit file formatting works without a manifest and does not fetch dependencies.
VS Code Format Document uses the same formatter through `lsp-server`.

Warnings include discarded Result/Async/Resource values and unsafe casts. Acknowledging
an ignored effect with `let _ = value` does not run it. Named bindings also do not
prove eventual execution. Ordinary non-Unit intermediate values can stand alone.

## Linter status

No linter command or configuration is implemented. Named arguments are supported,
but default parameter values are not. Proposed lint rules for named Boolean/numeric
arguments, large signatures, unused bindings, redundant mutability/discards, and
naming are proposals, not current compiler requirements. Offer relevant readability
suggestions without claiming automated enforcement. The formatter does not insert
argument names or change synchronous code into async code.

## API queries

Use compiler-backed search, package browsing, and declaration views to inspect
local and dependency APIs. Read [API discovery](api-discovery.md) when needed.
