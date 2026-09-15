# Debugging and Backtraces

This document designs **WASM debug information** and **runtime backtrace support** for Dovetail programs. It covers emitting function names in the WASM Name Section, generating DWARF debug info for source locations, printing panic messages at runtime, capturing call stacks on trap/panic, and configuring wasmtime to surface readable backtraces. It complements [compiler.md](../compiler.md) and [wasi-component-wasi-cli-design.md](wasi-component-wasi-cli-design.md).

**In scope:** WASM Name Section emission; DWARF line-level debug info (source file, line, column mapping); panic message printing before trap; wasmtime backtrace capture and formatting with function names and source locations; test infrastructure updates.

**Out of scope:** Full DWARF variable/type debug info for interactive debuggers (GDB/LLDB); source maps; step-through debugging; hot-reload.

**Implementation status:** Not started.

---

## 1. Current State

### 1.1 No debug info in WASM output

The codegen (`codegen/mod.rs`) emits a core WASM module with these sections: Type, Import, Function, Memory, Global, Export, Element, DataCount, Code, Data. No **Name Section** or **Custom Sections** (DWARF) are emitted. Function indices are numeric only; wasmtime cannot display function names or source locations in traps.

### 1.2 Panic and assert emit bare `unreachable`

`TypedExprKind::Panic` emits a single `Instruction::Unreachable` — no message is printed. `TypedExprKind::Assert` checks the condition and traps with `Unreachable` on failure. The panic expression already carries a `message: Box<TypedExpr>` (typed as `String`), but the message is unused at runtime.

### 1.3 wasmtime has no debug configuration

The runner (`runner.rs`) and test helper (`tests/common/mod.rs`) create the wasmtime `Config` with `wasm_component_model`, `wasm_gc`, and `wasm_function_references` only. No debug-related settings are enabled. Trap errors are stringified with no backtrace extraction.

### 1.4 Source spans are available

Every `TypedExpr` carries a `Span { file, line, column, end_line, end_column }` (1-indexed). Every function definition has span information in `TypedModule`. `MangledName` holds the human-readable mangled form of each symbol (e.g. `mypackage.myFunction$Int32$String`).

---

## 2. Overview and Goals

The implementation is split into four phases, each independently useful:

| Phase | Deliverable | User-visible improvement |
|-------|------------|------------------------|
| 1 | WASM Name Section | Function names appear in wasmtime trap messages |
| 2 | Panic message printing | Panic/assert messages printed to stderr before trap |
| 3 | DWARF debug info | Source file:line:column appears in wasmtime backtraces |
| 4 | Backtrace capture and formatting | Readable multi-frame backtraces on panic/trap |

Each phase is described in detail below.

---

## 3. Phase 1 — WASM Name Section

### 3.1 Background

The [WASM Name Section](https://webassembly.github.io/spec/core/appendix/custom.html#name-section) is a custom section named `"name"` that provides debug names for modules, functions, locals, etc. wasmtime reads it automatically and uses function names in trap/backtrace messages.

### 3.2 Implementation

`wasm-encoder` (v0.225) provides `NameSection`, `NameMap`, and `IndirectNameMap` types. After emitting the Data Section (last standard section), append a Name Section:

```rust
fn emit_name_section(&self) {
    let mut names = NameSection::new();

    // Module name
    names.module("dovetail");

    // Function names
    let mut func_names = NameMap::new();

    // Runtime functions (indices 0..USER_FUNC_BASE)
    func_names.append(FUNC_IMPORT_GET_STDOUT, "wasi:get-stdout");
    func_names.append(FUNC_IMPORT_WRITE, "wasi:blocking-write-and-flush");
    func_names.append(FUNC_RUN, "run");
    func_names.append(FUNC_RUN_POST, "run_post");
    func_names.append(FUNC_REALLOC, "realloc");
    func_names.append(FUNC_INITIALIZE, "initialize");
    func_names.append(FUNC_STRING_EQ, "string_eq");
    func_names.append(FUNC_STRING_CONCAT, "string_concat");
    func_names.append(FUNC_STRING_CMP, "string_cmp");
    func_names.append(FUNC_CHAR_TO_STRING, "char_to_string");
    func_names.append(FUNC_STRING_FROM_BYTES, "string_from_bytes");
    func_names.append(FUNC_STRING_GET_CHAR, "string_get_char");

    // User functions — use display_name from TypedFunction
    for (mangled, &idx) in &self.function_indices {
        if let Some(func) = self.typed_module.functions.get(mangled) {
            func_names.append(idx, &func.display_name);
        }
    }

    // Wrapper functions, closures, trampolines
    // (use descriptive synthetic names)

    names.functions(&func_names);
    self.module.section(&names);
}
```

### 3.3 Display names from inference (no demangling)

Rather than encoding a mangled string and reverse-engineering it back into a readable form, we pass a **pretty display name** forward from the inference phase. The typechecker already has all the information needed — FQN, function name, parameter types in their readable `Display` form — so it builds the display name at the point of knowledge and carries it through to codegen.

Add a `display_name: String` field to `TypedFunction`:

```rust
pub struct TypedFunction {
    pub visibility: Visibility,
    pub name: MangledName,
    pub display_name: String,   // e.g. "mypackage.add(Int32, Int32)"
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
    pub vtable_self_type: Option<Type>,
}
```

The inference phase populates `display_name` when constructing each `TypedFunction`:

```rust
let display_name = if params.is_empty() {
    format!("{}", fqn)
} else {
    let param_types: Vec<String> = params.iter()
        .map(|p| p.ty.to_string())
        .collect();
    format!("{}({})", fqn, param_types.join(", "))
};
```

This approach:
- **Avoids demangling entirely** — no fragile parsing of `$`-separated mangled strings.
- **Uses authoritative type names** — the inference phase has the canonical `Display` representation of each type, including generics (e.g. `Array[Int32]`, `Option[String]`).
- **Handles edge cases naturally** — extension methods, impl-block methods, generic instantiations, closures, etc. are all named by the phase that understands them.

### 3.4 Call site in `generate()`

Insert `self.emit_name_section()` after `self.emit_data_section()` and before `self.module.finish()`:

```rust
fn generate(mut self) -> Vec<u8> {
    self.collect_string_literals();
    self.emit_type_section();
    self.emit_import_section();
    self.emit_function_section();
    self.emit_memory_section();
    self.emit_global_section();
    self.emit_export_section();
    self.emit_element_section();
    self.emit_data_count_section();
    let codes = self.build_code_section();
    self.module.section(&codes);
    self.emit_data_section();
    self.emit_name_section();   // NEW
    self.module.finish()
}
```

### 3.5 Result

Before:
```
Error: WASM execution error: wasm trap: wasm `unreachable` instruction executed
```

After:
```
Error: WASM execution error: wasm trap: wasm `unreachable` instruction executed
  wasm backtrace:
    0: a.validateAge(Int32)
    1: a.main
    2: run
```

---

## 4. Phase 2 — Panic Message Printing

### 4.1 Goal

Before trapping, print the panic/assert message to stderr via the existing WASI `blocking-write-and-flush` import so the user sees **what** failed, not just that a trap occurred.

### 4.2 Panic runtime function

Add a runtime function `panic_with_message` (emitted in `build_code_section` alongside `string_eq`, etc.) that:

1. Receives a pointer and length (the message string in linear memory) — or, since strings are WASMGC structs (`(struct (field i32 i32))`), receives a `(ref $string_type)`.
2. Calls `get-stdout` (or a future `get-stderr` import) to get the output stream handle.
3. Calls `blocking-write-and-flush` with the string bytes.
4. Executes `unreachable` to trap.

### 4.3 Stderr import

Currently the compiler only imports `get-stdout`. To print panic messages to stderr:

**Option A — Use stdout.** Simplest; panic messages go to stdout. Acceptable for the first iteration.

**Option B — Add `get-stderr` import.** Add a WASI import for `wasi:cli/stderr@0.2.0/get-stderr` and use it for panic output. This is the correct approach and should be done in this phase or a follow-up. Requires updating the WIT definitions and component metadata.

**Recommendation:** Use Option B from the start — panic output belongs on stderr, and the WASI import is straightforward to add alongside the existing `get-stdout` pattern.

### 4.4 Codegen changes for panic/assert

Update `expressions.rs`:

```rust
TypedExprKind::Panic { message, .. } => {
    // Emit the message expression (produces a string ref)
    self.emit_expr(message, ExprContext::Value);
    // Call the panic runtime function (prints message, then traps)
    self.instruction(Instruction::Call(FUNC_PANIC_WITH_MESSAGE));
}

TypedExprKind::Assert { condition, message, .. } => {
    self.emit_expr(condition, ExprContext::Value);
    self.instruction(Instruction::I32Eqz);
    self.emit_if_block(BlockType::Empty);
    // Emit the message and call panic
    self.emit_expr(message, ExprContext::Value);
    self.instruction(Instruction::Call(FUNC_PANIC_WITH_MESSAGE));
    self.emit_end_block();
    if ctx == ExprContext::Value {
        self.instruction(Instruction::I32Const(0));
    }
}
```

### 4.5 Message formatting

For `assert`, auto-generate a message like `"Assertion failed at src/mypackage/file.dove:42:5"` using the expression's `Span`. The span information is available at codegen time, so the message string can be pre-computed and placed in the data section alongside other string literals.

For `panic`, emit the user-provided message expression.

### 4.6 Result

Before:
```
Error: wasm trap: wasm `unreachable` instruction executed
```

After:
```
Panic: age must be positive (src/mypackage/validators.dove:15:5)
Error: wasm trap: wasm `unreachable` instruction executed
  wasm backtrace:
    0: a.validateAge(Int32)
    ...
```

---

## 5. Phase 3 — DWARF Debug Info

### 5.1 Background

DWARF debug information is stored in WASM custom sections (`.debug_info`, `.debug_line`, `.debug_abbrev`, `.debug_str`, etc.). wasmtime reads DWARF sections and uses them to map WASM instruction offsets back to source file, line, and column. This provides source-level context in backtraces.

### 5.2 Scope — line info only

Full DWARF (variables, types, scopes) is complex and unnecessary for backtraces. We emit only:

- **`.debug_line`** — maps code offsets → source file:line:column.
- **`.debug_abbrev`** — abbreviation table (minimal: compilation unit + subprogram entries).
- **`.debug_info`** — compilation unit header + subprogram entries (function name, source location).
- **`.debug_str`** — string table for file paths and function names.

### 5.3 Instruction offset tracking

To map WASM instructions to source locations, the `FunctionEmitter` must track the byte offset of each emitted instruction within the code section. This requires:

1. **Tracking current offset** — `FunctionEmitter` maintains a running byte offset counter. Each `instruction()` call advances the counter by the encoded size of the instruction.

2. **Recording source mappings** — When emitting an expression, record `(wasm_offset, span)` pairs. Not every instruction needs a mapping; record at expression boundaries (the first instruction emitted for each `TypedExpr`).

3. **Collecting mappings per function** — After building each function body, collect the offset→span mappings. The code section builder aggregates these across all functions, adjusting offsets to be section-relative.

```rust
struct SourceMapping {
    code_offset: u32,  // byte offset within the code section
    file: FilePath,
    line: u32,
    column: u32,
}
```

### 5.4 DWARF section generation

After the code section is built and all `SourceMapping` entries are collected, generate DWARF sections using the `gimli::write` API (add `gimli` crate with the `write` feature):

1. **Build line program** — Create a `gimli::write::LineProgram` with the source directory and file table. Add rows for each `SourceMapping` (address, file, line, column).

2. **Build compilation unit** — Create a `gimli::write::Unit` with `DW_TAG_compile_unit`. Add `DW_TAG_subprogram` children for each function (name, low PC, high PC).

3. **Encode sections** — Call `dwarf.write(&mut sections)` to produce the `.debug_*` byte vectors.

4. **Emit as custom sections** — Use `wasm_encoder::CustomSection` to append each DWARF section to the module:

```rust
fn emit_debug_sections(&mut self, mappings: &[SourceMapping]) {
    // Build DWARF using gimli::write
    let mut dwarf = gimli::write::Dwarf::new();
    // ... populate line program and compilation unit ...
    let mut sections = gimli::write::Sections::new(WriterRelocate::new());
    dwarf.write(&mut sections).unwrap();

    // Emit each section as a WASM custom section
    for (id, data) in sections.iter() {
        let name = id.name(); // e.g. ".debug_line"
        self.module.section(&CustomSection {
            name: name.into(),
            data: &data,
        });
    }
}
```

### 5.5 Alternative — manual DWARF encoding

If adding `gimli` is undesirable, DWARF line info can be encoded manually. The `.debug_line` format is a state machine with opcodes (DW_LNS_advance_pc, DW_LNS_advance_line, etc.). This is more work but avoids a dependency. **Recommendation:** use `gimli::write` — it is the standard Rust DWARF library, well-maintained, and handles encoding details correctly.

### 5.6 wasmtime configuration

wasmtime must be configured to parse DWARF info. Add to `Config`:

```rust
config.debug_info(true);
```

This tells wasmtime to read `.debug_*` custom sections and use them for backtrace symbolication.

### 5.7 Result

With Name Section + DWARF:
```
Error: wasm trap: wasm `unreachable` instruction executed
  wasm backtrace:
    0: a.validateAge(Int32)
        at src/mypackage/validators.dove:15:5
    1: a.processUser(User)
        at src/mypackage/handlers.dove:42:12
    2: a.main
        at src/mypackage/main.dove:8:3
    3: run
```

---

## 6. Phase 4 — Backtrace Capture and Formatting

### 6.1 Goal

When a Dovetail program traps (panic, failed assert, or any `unreachable`), capture the WASM call stack, resolve function names and source locations from the debug info, and format a readable backtrace before reporting the error.

### 6.2 wasmtime trap handling

wasmtime automatically captures backtraces on trap when configured. The `wasmtime::Error` (or `anyhow::Error`) returned from `call_run` contains a `wasmtime::WasmBacktrace` accessible via downcast:

```rust
use wasmtime::WasmBacktrace;

let run_result = command.wasi_cli_run().call_run(&mut store);
if let Err(err) = run_result {
    if let Some(bt) = err.downcast_ref::<WasmBacktrace>() {
        for frame in bt.frames() {
            let func_name = frame.func_name()
                .unwrap_or("<unknown>");
            if let Some(loc) = frame.func_offset() {
                // With DWARF, source info is available via frame
            }
            eprintln!("  at {func_name}");
        }
    }
}
```

### 6.3 Backtrace formatting

Implement `format_backtrace(err: &anyhow::Error) -> String` in `runner.rs`:

1. Extract `WasmBacktrace` from the error.
2. Iterate frames, skip internal frames (runtime functions like `run`, `initialize`, WASI adapter frames).
3. For each user frame, format: `  at <function_name> (<file>:<line>:<column>)`.
4. If no debug info is available, fall back to: `  at <function_name> [+0x<offset>]`.

### 6.4 Filtering internal frames

Skip frames whose function name starts with known prefixes: `run`, `run_post`, `realloc`, `initialize`, `wasi:`, `cm32p2`, `string_eq`, `string_concat`, etc. Only show user-defined functions in the default backtrace. Optionally support a `--verbose` flag to show all frames.

### 6.5 Updated runner

```rust
pub fn run_component(wasm_bytes: &[u8]) -> Result<(), RunError> {
    let mut config = Config::new();
    config.wasm_component_model(true);
    config.wasm_gc(true);
    config.wasm_function_references(true);
    config.debug_info(true);         // NEW: enable DWARF parsing
    config.wasm_backtrace(true);     // NEW: enable backtrace capture

    // ... engine, component, linker, store setup ...

    let run_result = command.wasi_cli_run().call_run(&mut store);
    match run_result {
        Ok(Ok(())) => Ok(()),
        Ok(Err(())) => Err(RunError {
            message: "program exited with error".to_string(),
        }),
        Err(err) => {
            let backtrace = format_backtrace(&err);
            Err(RunError {
                message: format!("runtime error:\n{backtrace}"),
            })
        }
    }
}
```

### 6.6 Test infrastructure

Update `tests/common/mod.rs` similarly:
- Enable `debug_info(true)` and `wasm_backtrace(true)` in the test engine config.
- Add a helper `compile_and_expect_trap_with_backtrace(source) -> String` that returns the formatted backtrace for assertions in tests.

### 6.7 Result

Full output on panic:
```
Panic: age must be positive

Backtrace:
  at mypackage.validateAge(Int32) (src/mypackage/validators.dove:15:5)
  at mypackage.processUser(User) (src/mypackage/handlers.dove:42:12)
  at mypackage.main (src/mypackage/main.dove:8:3)
```

---

## 7. Dependencies

| Crate | Purpose | Phase |
|-------|---------|-------|
| `wasm-encoder` (existing) | `NameSection`, `NameMap`, `CustomSection` for debug sections | 1, 3 |
| `gimli` (new, `write` feature) | DWARF debug info generation | 3 |
| `wasmtime` (existing) | `Config::debug_info`, `WasmBacktrace`, frame iteration | 3, 4 |

---

## 8. Testing

### 8.1 Name Section tests

- Compile a program, parse the output with `wasmparser`, verify the Name Section exists and contains expected function names.
- Verify that user functions have demangled names (e.g. `a.add(Int32, Int32)`).
- Verify runtime functions have descriptive names.

### 8.2 Panic message tests

- `compile_and_expect_trap` verifies that panic message appears in captured stderr.
- Test both `panic "message"` and `assert false` produce output before trapping.

### 8.3 DWARF tests

- Compile a program, parse WASM with `wasmparser`, verify `.debug_line` custom section exists.
- Optionally use `gimli::read` to parse the debug info and verify source mappings point to correct file/line/column.

### 8.4 Backtrace tests

- Compile a program with a function call chain that panics. Run it and verify the backtrace contains the expected function names in order.
- Verify source locations (file:line:column) appear in backtrace frames.
- Verify internal/runtime frames are filtered out.

---

## 9. Implementation Order

1. **Phase 1 — Name Section** (smallest change, immediate value)
   - Add `display_name: String` field to `TypedFunction`; populate in inference.
   - Add `emit_name_section()` to `Codegen` using `display_name`.
   - Add unit test validating Name Section content.

2. **Phase 2 — Panic message printing**
   - Add `panic_with_message` runtime function.
   - Update panic/assert codegen to call it.
   - Add assert message auto-generation from span.
   - Test panic output.

3. **Phase 3 — DWARF debug info**
   - Add `gimli` dependency.
   - Track instruction offsets in `FunctionEmitter`.
   - Record `SourceMapping` entries during codegen.
   - Generate and emit DWARF custom sections.
   - Enable `config.debug_info(true)` in runner.

4. **Phase 4 — Backtrace formatting**
   - Enable `config.wasm_backtrace(true)`.
   - Implement `format_backtrace()`.
   - Filter internal frames.
   - Update test infrastructure.
   - Add backtrace integration tests.

---

## 10. Future Work

- **Full DWARF debug info** — variable types, scopes, and values for interactive debugging with GDB/LLDB via wasmtime's `--debug` flag.
- **Source maps** — alternative to DWARF for browser-based debugging (if Dovetail targets browser WASM runtimes).
- **Stderr import** — proper `wasi:cli/stderr` import for panic output instead of stdout.
- **Colored output** — terminal colors in backtrace formatting (red for panic message, dim for internal frames).
- **`DOVETAIL_BACKTRACE` env var** — control backtrace verbosity (0=none, 1=user frames, 2=all frames including runtime).
