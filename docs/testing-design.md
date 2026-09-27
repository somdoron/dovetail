# Testing Design

This document designs **testing support** in the Dovetail compiler: test declarations, test attributes, test organization (unit and integration), and the `dovetail test` CLI command. It aligns with [grammar.md](../grammar.md), [compiler.md](../compiler.md), [multi-project-multi-package-design](multi-project-multi-package-design.md), and [website/content/book/15-testing.md](../website/content/book/15-testing.md).

**In scope:** Test declaration syntax and AST; test attributes (`@skip`, `@panics`, `@timeout`); unit tests in `src/` with private-member access; integration tests in `test/` directory; test naming (fully qualified test names); `dovetail test` CLI with project filter, file filter, test name filter, colored output; test compilation and execution via WASM; test runner output format.

**Out of scope:** Test coverage reporting; parallel test execution (future optimization); benchmark declarations; property-based testing; test fixtures or setup/teardown hooks; watch mode.

**Implementation status:** Complete (Phases 1–5 implemented; Phase 6 Result/Option return types descoped).

---

## 1. Overview

- **Test declarations** use the `test` keyword followed by a string name and a body. Tests are top-level declarations (same level as functions, records, etc.) — they cannot be nested inside functions or classes.
- **Two test locations:** unit tests live in `src/` alongside production code (access to private members in the same file); integration tests live in a `test/` directory sibling to `src/` (access to public and internal members via imports).
- **Test attributes** modify test behavior: `@skip` (disable), `@panics` (expect panic), `@timeout` (time limit).
- **Test names** are fully qualified: `<package_path> <test_string_name>` (e.g. `com.example.app.math addition works`). This FQN is used for filtering and output.
- **`dovetail test`** compiles tests into a WASM component with a test harness entry point (instead of `main`), executes it, and reports results with colored output.

---

## 2. Test Declarations

### 2.1 Syntax

```
test_decl = { attribute } "test" STRING_LIT "=" block_expr
attribute = "@" IDENT [ "(" attr_arg ")" ]
attr_arg  = STRING_LIT | INT_LIT
```

Examples:

```dovetail
test "addition works" =
    assert 1 + 2 == 3

@skip("Not ready")
test "future feature" =
    assert newThing() == expected

@panics("out of bounds")
test "array bounds check" =
    let arr = [1, 2, 3]
    let _ = arr.get(100)

@timeout(1000)
test "completes quickly" =
    let result = compute()
    assert result == 42
```

### 2.2 Return types

A test body is a block expression that returns `Unit`. A test passes if it completes without panicking, and fails if it panics.

| Return type | Pass condition | Fail condition |
|-------------|---------------|----------------|
| `Unit` | Completes without panic | Panics |

> **Note:** Result/Option return types were considered but descoped. Tests always return `Unit`.

### 2.3 AST representation

Add to the `Declaration` enum:

```rust
pub enum Declaration {
    // ... existing variants ...
    Test(TestDecl),
}

pub struct TestDecl {
    pub name: StringLiteral,
    pub attributes: Vec<TestAttribute>,
    pub body: BlockExpr,
    pub span: Span,
}

pub enum TestAttribute {
    Skip { reason: Option<StringLiteral>, span: Span },
    Panics { message: Option<StringLiteral>, span: Span },
    Timeout { millis: u64, span: Span },
}
```

### 2.4 Test naming

Every test has a **fully qualified test name (FQTN)** composed of:

```
<package_path> <test_string_name>
```

For example, a test `test "addition works"` in a file with `package com.example.math` has FQTN:

```
com.example.math addition works
```

This FQTN is used for:
- **Output display** (e.g. `PASS  com.example.math addition works`)
- **Filtering** (the `--filter` flag matches against this full string)
- **Uniqueness** — within a package, test string names must be unique. The compiler reports a duplicate-test-name error if two tests in the same package share the same string name.

---

## 3. Test Attributes

### 3.1 `@skip`

Marks a test to be skipped (not executed). The test appears in output as `SKIP`.

- `@skip` — skip with no reason.
- `@skip("reason string")` — skip with a reason displayed in output.

A skipped test is still **parsed and typechecked** (to catch compilation errors even in skipped tests) but is **not executed**.

### 3.2 `@panics`

Marks a test that is expected to panic. The pass/fail semantics are inverted:

- **Passes** if the test body panics.
- **Fails** if the test body completes without panicking.

Optional message substring matching:

- `@panics` — any panic passes.
- `@panics("expected message")` — the panic message must contain the given substring for the test to pass.

### 3.3 `@timeout`

Sets a maximum execution time in milliseconds. If the test runs longer, it fails with a timeout error.

- `@timeout(1000)` — test must complete within 1000ms.

The argument must be a positive integer literal.

### 3.4 Combining attributes

Multiple attributes can be combined on a single test. Semantic rules:

- `@skip` + anything else: the test is skipped; other attributes are irrelevant at runtime (but still validated at compile time).
- `@panics` + `@timeout`: the test must panic within the timeout.
- Duplicate attributes of the same kind are a compile error.

---

## 4. Test Organization

### 4.1 Unit tests in `src/`

Tests written in source files under `src/` are **unit tests**. They:

- Share the same `package` declaration as the file's production code.
- Have **access to private members declared in the same file** — functions, types, and fields that are not `public` are visible to tests in that file.
- Are compiled and executed when `dovetail test` runs.
- Are **excluded from production builds** — `dovetail build` ignores test declarations entirely (they are not included in the WASM component).

Private-member access rule: a test declaration in file `F` can reference any symbol declared in `F`, regardless of visibility. Symbols from other files in the same package follow normal visibility rules (same-package access). Symbols from other packages require imports and must be public.

### 4.2 Integration tests in `test/`

Integration tests live in a `test/` directory that is a **sibling** of `src/`:

```
myproject/
├── src/
│   ├── math.dove
│   └── utils/
│       └── helpers.dove
├── test/
│   ├── math_test.dove
│   └── utils_test.dove
└── Dovetail.toml
```

Integration test files:

- Declare `package test` (or `package test.subpath` for subdirectories under `test/`).
- Must **import** symbols from the project's packages — they can access public and internal members, but not private members.
- Are compiled **after** all `src/` packages, so they see the full public registry of the project.
- Are only compiled and executed during `dovetail test`, never during `dovetail build`.

The `test/` directory is discovered automatically — it does not need to be listed in `Dovetail.toml`. If a project has no `test/` directory, there are simply no integration tests for that project.

### 4.3 Test package compilation order

When `dovetail test` runs for a project:

1. Compile all `src/` packages in the order specified by `packages` in `Dovetail.toml` (same as `dovetail build`).
2. Compile the `test/` package(s) last, with access to the full merged registry from step 1.

The `test` package depends on all `src/` packages implicitly — no explicit `depends` is needed.

### 4.4 Test discovery

The compiler discovers tests in two phases:

1. **Source tests:** While parsing and typechecking `src/` files, collect all `TestDecl` nodes. Each test is tagged with its source file path and package.
2. **Integration tests:** After `src/` compilation, parse and typecheck files under `test/`. Collect all `TestDecl` nodes from those files.

All discovered tests are gathered into a test manifest that the test runner uses to execute.

---

## 5. `dovetail test` CLI Command

### 5.1 Synopsis

```
dovetail test [OPTIONS] [PROJECT]
```

### 5.2 Arguments and flags

| Argument / Flag | Description |
|----------------|-------------|
| `[PROJECT]` | Optional project name. If given, run tests only for this project (and compile its dependencies, but don't run their tests). If omitted, run tests for **all projects**. |
| `--file <PATH>` | Run only tests defined in the specified source file. Path is relative to the workspace root (e.g. `myproject/src/math.dove` or `myproject/test/math_test.dove`). |
| `--filter <PATTERN>` | Run only tests whose FQTN contains the given substring. Matches against the full `<package_path> <test_name>` string. Multiple `--filter` flags are OR'd (a test runs if it matches any filter). |
| `-v`, `--verbose` | Show individual test results (PASS/FAIL/SKIP) as they complete. Without this flag, only the summary and failures are shown. |
| `--no-colors` | Disable colored output. By default, output uses ANSI colors (green for PASS, red for FAIL, yellow for SKIP). Colors are also auto-disabled when stdout is not a TTY. |

### 5.3 Examples

```bash
# Run all tests in all projects
dovetail test

# Run tests for a specific project
dovetail test myapp

# Run tests in a specific file
dovetail test --file myapp/src/math.dove

# Run tests matching a pattern
dovetail test --filter "addition"

# Run a specific test by its full name
dovetail test --filter "com.example.math addition works"

# Verbose output without colors
dovetail test --verbose --no-colors

# Combine project and filter
dovetail test myapp --filter "math"
```

### 5.4 Exit codes

| Exit code | Meaning |
|-----------|---------|
| 0 | All tests passed (or were skipped) |
| 1 | One or more tests failed |
| 2 | Compilation error (tests could not be compiled) |

### 5.5 Output format

Output uses colors by default (controlled by `--no-colors` and TTY detection).

**Default output** (non-verbose) — shows only failures and summary:

```
Running 12 tests

  FAIL  com.example.math broken test
        assertion failed at myapp/src/math.dove:42

Failures:
  com.example.math broken test
    assertion failed at myapp/src/math.dove:42

test result: FAILED. 10 passed; 1 failed; 1 skipped; finished in 45.23ms
```

**Verbose output** (`-v`) — shows each test as it completes:

```
Running 5 tests

  PASS  com.example.math addition works (28.67us)
  PASS  com.example.math multiplication works (167.00ns)
  PASS  com.example.math division works (125.00ns)
  SKIP  com.example.math future feature (Not implemented yet)
  FAIL  com.example.math broken test
        assertion failed at myapp/src/math.dove:42

Failures:
  com.example.math broken test
    assertion failed at myapp/src/math.dove:42

test result: FAILED. 3 passed; 1 failed; 1 skipped; finished in 823.00us
```

**Color scheme:**

| Status | Color |
|--------|-------|
| `PASS` | Green |
| `FAIL` | Red |
| `SKIP` | Yellow |
| Summary `ok` | Green |
| Summary `FAILED` | Red |
| Test timing | Dim/gray |

**Panics-expected tests** (verbose):

```
  PASS  com.example.math division by zero panics (panicked as expected, 47.96us)
```

**No tests found:**

```
No tests found.
```

**All tests pass (non-verbose):**

```
Running 12 tests

test result: ok. 12 passed; 0 failed; 0 skipped; finished in 45.23ms
```

---

## 6. Pipeline Integration

### 6.1 Lexer

- Add `Test` to `TokenKind` (keyword `"test"`).
- The `@` token is already available for attributes. Parse `@ident` and `@ident(arg)` as attribute tokens.

### 6.2 Layout filter

No changes needed. The `=` after the test declaration is already a layout opener. The test body is a block expression following the same layout rules as function bodies.

### 6.3 Parser

- Parse `test_decl` as a new `Declaration` variant.
- Parse attributes: before a `test` keyword, consume `@ident` (optionally with `(arg)`) tokens. Attributes are only valid on test declarations (for now); an attribute before any other declaration is a parse error.
- The test name is a `STRING_LIT`.
- `= block_expr` for the body (same as function bodies).

### 6.4 Typechecker — Collect

- Collect test declarations into the registry or a separate test registry. Each test is identified by its FQTN.
- Validate uniqueness of test names within a package.
- Validate test attributes: `@timeout` argument must be a positive integer; unknown attributes are errors; duplicate attributes of the same kind are errors.
- Tests do **not** contribute to the public API — they are never exported in the registry passed to dependent packages.

### 6.5 Typechecker — Inference

- Type-check the test body as a block expression with expected return type `Unit`.
- **Same-file private access:** when inferring a test body, the test can access all declarations in the same file, regardless of visibility. This is implemented by extending the resolution scope for test bodies to include private declarations from the file the test is defined in.

### 6.6 Typechecker — Rules

Standard rules apply. No special subtyping or trait-bound rules for tests.

### 6.7 TypedModule

Add a `tests` field to `TypedModule`:

```rust
pub struct TypedModule {
    pub main_function_fqn: Option<Fqn>,
    pub functions: BTreeMap<MangledName, TypedFunction>,
    pub globals: BTreeMap<MangledName, TypedGlobal>,
    pub types: BTreeMap<MangledName, TypeDef>,
    pub tests: Vec<TypedTest>,
}

pub struct TypedTest {
    pub name: String,
    pub fqtn: String,
    pub package_path: PackagePath,
    pub attributes: Vec<TypedTestAttribute>,
    pub return_type: Type,
    pub body: TypedBlockExpr,
    pub source_file: FilePath,
    pub span: Span,
    pub mangled_name: MangledName,
}

pub enum TypedTestAttribute {
    Skip { reason: Option<String> },
    Panics { message: Option<String> },
    Timeout { millis: u64 },
}
```

The `TypedModule::merge` function must merge `tests` vectors (concatenation).

### 6.8 Codegen (test mode)

When compiling for `dovetail test`, codegen produces a **test WASM component** instead of a production component:

- **No `main` entry point.** Instead, the component exports a **test harness** — a function that runs all (non-skipped, non-filtered) tests and reports results.
- Each test body is compiled as a separate WASM function (with a mangled name derived from FQTN).
- The test harness function iterates over the test list, calls each test function, catches panics (via WASM trap handling), measures time, and writes results to stdout.
- `@panics` tests: the harness catches the trap; if the test traps, it passes (with optional message check). If it doesn't trap, it fails.
- `@timeout` tests: the harness sets a time limit before calling the test function.
- `@skip` tests: the harness skips execution and reports `SKIP` with the optional reason.

**Alternative approach (host-side runner):** Instead of compiling the harness into WASM, keep each test as a separate exported WASM function. The **host** (Rust code using wasmtime) iterates over tests, invokes each function, handles traps, measures time, and reports results. This is simpler to implement and gives the host full control over output formatting, colors, and timing.

**Recommended: host-side runner.** The compiler exports test functions from the WASM component with a naming convention (e.g. `$test$<mangled_fqtn>`). The host discovers these exports, applies filters, and runs them one by one. The host handles:
- Trap catching (for `@panics` and failure detection)
- Timing (for `@timeout`)
- Result aggregation and formatted output

### 6.9 Test metadata

The compiler embeds test metadata in the WASM component (e.g. as a custom section or a separate JSON sidecar) so the host runner knows:
- The list of tests (FQTN, source file, span)
- Attributes for each test (skip/panics/timeout)
- Return type of each test

The host reads this metadata before executing tests to apply filters, skip tests, and know expected behavior.

---

## 7. `dovetail build` vs `dovetail test`

| Aspect | `dovetail build` | `dovetail test` |
|--------|---------------|---------------|
| Test declarations | Ignored (stripped from AST) | Compiled |
| `test/` directory | Ignored | Compiled after `src/` |
| Entry point | `main` function | Test harness / exported test functions |
| Output | Production WASM component | Test WASM component (or test results) |
| Private access from tests | N/A | Tests in `src/` access same-file privates |

When `dovetail build` encounters test declarations, it skips them during codegen (they are parsed and optionally typechecked for error reporting, but not emitted to WASM). This means `dovetail build` never pays a code-size cost for tests. Whether `dovetail build` type checks tests or silently ignores them is an implementation choice; typechecking them during build catches errors early, but could slow down builds. **Recommendation:** `dovetail build` skips test declarations entirely (no typecheck, no codegen) for fastest builds. `dovetail check` also skips tests unless `--include-tests` is passed.

---

## 8. Integration Test Details

### 8.1 Directory structure

```
myproject/
├── src/
│   ├── main.dove         # package com.example.myproject
│   └── utils/
│       └── helpers.dove   # package com.example.myproject.utils
├── test/
│   ├── main_test.dove     # package test
│   └── utils/
│       └── helpers_test.dove  # package test.utils
└── Dovetail.toml
```

### 8.2 Package naming for integration tests

Integration test files use `package test` (or `package test.<subpath>` for subdirectories). The `test` package namespace is reserved and cannot be used in `src/`.

Package path derivation for test files follows the same rule as `src/`: the path relative to `test/` determines the subpath after `test`. Files directly in `test/` use `package test`.

### 8.3 Imports in integration tests

Integration tests must import from the project's packages:

```dovetail
package test

import com.example.myproject.utils.helper

test "helper works" =
    assert helper(1) == 2
```

They can access public and internal symbols via imports — only private members are inaccessible.

### 8.4 Cross-project test dependencies

Integration tests can import from dependency projects (listed in `depends`), just like production code. No special rules.

---

## 9. Manifest Changes

### 9.1 No manifest changes required

The `test/` directory is discovered by convention — if `<project>/test/` exists and contains `.dove` files, those are integration tests. No `Dovetail.toml` changes are needed.

### 9.2 Future: test configuration (optional)

A future extension could add test configuration to `Dovetail.toml`:

```toml
[[project]]
name = "myapp"
root_package = "com.example.myapp"
packages = ["utils", "."]

[project.test]
timeout = 5000        # default timeout for all tests (ms)
```

This is out of scope for the initial implementation.

---

## 10. Implementation Phases

Each phase cuts vertically through the full pipeline (lexer → parser → typechecker → codegen → runner) following the project's feature-by-feature development approach.

### Phase 1: End-to-end basic tests

Minimal working `dovetail test` — from source to execution.

- **Lexer:** Add `Test` keyword token.
- **Parser:** Parse `test "name" = body` as a `TestDecl` in the `Declaration` enum. No attributes, no return type annotation yet.
- **Typechecker (Collect):** Collect tests. Validate test name uniqueness per package. Store in `TypedModule.tests`.
- **Typechecker (Inference):** Type-check test body as a block expression with expected return type `Unit`.
- **Codegen:** Compile each test body as an exported WASM function. Skip test declarations entirely during `dovetail build`.
- **Host runner:** Discover exported test functions, execute each via wasmtime, catch traps (trap = FAIL, clean return = PASS). Measure timing per test and total.
- **CLI:** Add `dovetail test [PROJECT]` command. Colored output by default (PASS green, FAIL red, summary). `--no-colors` flag and TTY auto-detection. `--verbose` flag. Project filter (run tests for one project only).
- **Tests:** Compiler integration tests using `dovetail test` on sample projects.

**Deliverable:** `dovetail test` compiles and runs basic tests with colored output. Failures show trap info. Summary line with counts and timing.

### Phase 2: Test attributes

- **Parser:** Parse `@skip`, `@panics`, `@timeout` attributes before `test` keyword. Attribute syntax: `@ident` or `@ident("string")` or `@ident(integer)`.
- **Typechecker:** Validate attributes — unknown attributes are errors; duplicates of same kind are errors; `@timeout` argument must be positive integer.
- **Runner:** `@skip` — skip execution, report SKIP with optional reason. `@panics` — invert pass/fail (trap = PASS, clean return = FAIL); optional message substring check. `@timeout` — enforce time limit, fail on timeout.
- **Tests:** Tests for each attribute and combinations.

**Deliverable:** All three attributes work end-to-end.

### Phase 3: Same-file private access

- **Typechecker (Inference):** Extend name resolution scope for test bodies to include all declarations in the same file, regardless of visibility. A test in `math.dove` can call private functions declared in `math.dove`.
- **Tests:** Tests verifying private function/type access from same-file tests, and that cross-file private access is still denied.

**Deliverable:** Unit tests in `src/` can test private implementation details.

### Phase 4: CLI filtering

- **CLI:** `--filter <pattern>` — substring match against the FQTN (`<package> <test_name>`). Multiple `--filter` flags OR'd. `--file <path>` — run only tests from a specific source file. Handle "no tests matched" case.
- **Tests:** Filter by name, filter by package prefix, filter by file, combined filters.

**Deliverable:** Fine-grained control over which tests run.

### Phase 5: Integration tests (`test/` directory)

- **Orchestrator:** After compiling all `src/` packages, discover `test/` directory. Parse and typecheck files under `test/` with access to the full merged registry (public + internal visibility).
- **Package naming:** Test files use `package test` (or `package test.<subpath>`). Reserve the `test` package prefix — error if used in `src/`.
- **Runner:** Integration tests are included alongside unit tests in the test run. FQTN uses `test` package prefix (e.g. `test math integration`).
- **Tests:** End-to-end integration test flow; verify internal access works but private access is denied.

**Deliverable:** Full two-tier testing model (unit + integration).

### Phase 6: Documentation polish (Result/Option descoped)

Result/Option return types for tests were descoped — tests always return `Unit`. Phase 6 became a documentation-only pass:

- **Book:** Updated `website/content/book/15-testing.md` to accurately reflect implemented features (removed Result/Option section, updated CLI docs, renumbered sections).
- **TOC:** Updated `website/content/book/toc.md` to match new section numbering.
- **Design doc:** Marked implementation status as complete, updated syntax and tables to reflect descoped return types.

**Deliverable:** Documentation accurately reflects the implemented testing feature.

**Dependencies:** 1 → 2, 3, 4, 5, 6. Phases 2–5 depend on 1 but are independent of each other. Phase 6 depends on 1.

---

## 11. Edge Cases and Considerations

- **No tests found:** `dovetail test` prints `No tests found.` and exits with code 0.
- **All tests skipped:** Exits with code 0; summary shows `0 passed; 0 failed; N skipped`.
- **Compilation error:** `dovetail test` exits with code 2 and prints diagnostics. No tests are executed.
- **Duplicate test name:** Two tests with the same string name in the same package → compile error. Tests in different packages can share the same string name (their FQTNs differ).
- **Test in wrong location:** A `test` declaration inside a function body or class body → parse error (tests are top-level only).
- **`@panics`:** Inverts pass/fail semantics — the test passes if it panics, fails if it completes normally.
- **`@timeout(0)`:** Compile error; timeout must be a positive integer.
- **Test name is empty string:** Allowed but discouraged. The FQTN would be `com.example.math ` (trailing space).
- **Very long test names:** No enforced limit; output may wrap.
- **`test/` directory with no `.dove` files:** Treated as no integration tests (not an error).
- **`package test` in `src/`:** Compile error — the `test` package prefix is reserved for integration tests.
- **Integration test accessing private members:** Standard visibility rules apply — private members are not visible across packages, so integration tests cannot access them. This is by design.
- **Filter matches no tests:** `dovetail test --filter "nonexistent"` prints `No tests matched the filter.` and exits with code 0.

---

## 12. Summary

| Concept | Design choice |
|---------|---------------|
| **Test syntax** | `test "name" = body`. Tests always return `Unit`. Top-level declaration only. |
| **Attributes** | `@skip`, `@panics`, `@timeout`; placed before `test` keyword. |
| **Unit tests** | In `src/` files; access to private members in the same file. |
| **Integration tests** | In `test/` directory (sibling to `src/`); `package test`; public and internal access via imports. |
| **Test naming** | FQTN = `<package_path> <test_name>`; unique within a package. |
| **CLI command** | `dovetail test [PROJECT]` with `--filter`, `--file`, `--verbose`, `--no-colors`. |
| **Output** | Colored by default; PASS (green), FAIL (red), SKIP (yellow); summary line. |
| **Exit codes** | 0 = pass, 1 = failure, 2 = compile error. |
| **Build vs test** | `dovetail build` ignores tests; `dovetail test` compiles and runs them. |
| **Runner** | Host-side (Rust/wasmtime): discovers exported test functions, runs each, handles traps/timeouts. |
| **Manifest** | No changes; `test/` discovered by convention. |

---

## 13. References

- [grammar.md](../grammar.md) — Syntax and layout rules.
- [compiler.md](../compiler.md) — Pipeline stages and architecture.
- [multi-project-multi-package-design.md](multi-project-multi-package-design.md) — Project/package structure and CLI.
- [website/content/book/15-testing.md](../website/content/book/15-testing.md) — User-facing testing documentation.
