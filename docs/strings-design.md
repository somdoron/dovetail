# Strings Design

This document designs **strings** and **characters** in Dovetail: representation (WASM-GC), character type, string literals (single-line and multi-line), string interpolation, concatenation, storage of literals in the component (data section by default, with an optional flag for globals), and intrinsics for byte access. It aligns with the [language basics book](../website/content/book/03-language-basics.md), [grammar](grammar.md), and [compiler design](compiler.md).

---

## 1. Overview

- **String**: Immutable UTF-8 text. Represented at runtime as a **WASM-GC packed array of `u8`** (bytes). No separate “string object” wrapper; the string *is* the array of bytes. Validity (well-formed UTF-8) is the responsibility of the compiler for literals and of Dovetail code / standard library for constructed strings.
- **Char**: A single Unicode scalar value. Represented as **`i32`** (Unicode code point). No separate WASM type; it is an integer.
- **String interpolation**: Supported in all string literals via `$ident` and `${expr}`. Resolved at compile time; the result is a string value.
- **Multi-line strings**: Supported via triple-quote syntax `"""..."""`.
- **Concatenation**: Operator **`++`** through `Concat<String>` with associated `Output = String`. String addition with `+` is rejected.
- **Literal storage in WASM**: **Data section** by default (copy from data section on each use). A **future flag** (e.g. in manifest or CLI) may enable **globals**: literals pre-allocated at instantiation and stored in globals, so no per-use allocation. The user can enable that if they prefer.
- **Bytes access**: Exposed as an **extension method** on `String`: **`s.unsafe_bytes()`** returns **`Array<Uint8>`** (the UTF-8 byte sequence). Implemented by a compiler intrinsic; all other UTF-8 decoding/encoding, indexing by code point, and higher-level string operations are implemented in Dovetail code (standard library).

**Implementation status:** Done (String, Char, interpolation, data section).

---

## 2. Representation

### 2.1 String = WASM-GC packed array of u8

- At runtime, a **String** is represented as a **WASM-GC packed array of `u8`**.
- Length is the number of **bytes** (not code points). This matches “string is UTF-8”: the underlying storage is byte-oriented.
- The type is opaque to users: they see `String`; they do not index or mutate the underlying array directly except via the intrinsic that exposes bytes (see §7).
- No length-prefix or other wrapper in this design: the value passed around is the array reference itself.

### 2.2 Char = Int32

- **Char** is an **`i32`** holding a Unicode code point (U+0000 .. U+10FFFF). Invalid code points (e.g. surrogates) are not normalized by the compiler; Dovetail code / standard library may enforce validity.
- Character literals: `'A'`, `'\n'`, `'\u{2764}'` etc. already in grammar. They produce an `i32` value; the typechecker treats them as `Char` (or we may introduce a distinct `Char` type that is codegen’d as `i32`).
- No separate WASM type: in codegen, `Char` is `i32`.

---

## 3. String Literals and Interpolation

### 3.1 Single-line and multi-line

- **Single-line**: `"..."` with `string_char`, `escape_seq`, and `interpolation` (see [grammar](grammar.md)).
- **Multi-line**: `"""..."""`; content may span lines and include unescaped newlines and quotes (except `"""`). Interpolation is allowed inside multi-line strings as well.
- Literals are **UTF-8** encoded. The compiler ensures that string literal bytes are valid UTF-8 (and that interpolation inserts UTF-8 or is converted to UTF-8 where the type is String).

### 3.2 Interpolation

- `$ident` — insert the value of the variable `ident` (must be of a type that can be converted to string, e.g. `String`, `Int32`, etc.; exact rules TBD in typechecker).
- `${expr}` — insert the result of the expression (same conversion requirement).
- Interpolation is **compile-time**: the compiler builds the final string (or constituent parts) at compile time where possible; for dynamic values, it generates code that produces the string at runtime (e.g. via concatenation or a helper).
- Escape sequences: `\n`, `\r`, `\t`, `\\`, `\"`, `\$`, `\0`, `\u{XXXX}` as in the book.

---

## 4. String Concatenation

- **Operator**: **`++`**, implemented by `Concat<String>`.
- **Type**: `String ++ String → String`.
- **Precedence**: Same as other additive operators (see grammar: `additive_expr` with `+`).
- **Semantics**: Produce a new string (new packed array of u8) containing the bytes of the left operand followed by the bytes of the right operand. No implicit conversion of non-String to String in this design; conversions (e.g. `Int32` to `String`) are done by explicit calls (e.g. standard library) or by interpolation in literals.
- The [language basics book](../website/content/book/03-language-basics.md) is updated to describe concatenation with `++`.

---

## 5. Storage of String Literals in the WASM Component

String literals must be represented in the component so that code can refer to them.

**Recommendation**: Use a **data section** by default. A **future flag** (e.g. in `Dovetail.toml` or `dovetail build`) may allow the user to enable **globals** for literals; when enabled, the compiler uses the globals strategy instead. The user can opt in if they want to avoid per-use allocation at the cost of higher runtime heap (all literals pre-allocated at startup).

### 5.1 Data sections (default)

- Literal bytes are placed in a **data section**. At runtime, when the literal is needed, the code **allocates** a packed array (or equivalent) and **copies** the bytes from the data section into it. So every use of the literal does an allocation and copy.
- **Pro**: Simple; no need to manage a start function or globals.  
- **Con**: Allocation and copy on every use.

### 5.2 Global variables (optional, future flag)

- When the user enables the (future) flag, each string literal (or a deduplicated set) is stored in a **global** that holds a reference to a WASM-GC array. The literal bytes still live in a **data section**; a **start function** runs at instantiation, allocates a GC array per literal, copies bytes from the data section into it, and stores the ref in the global. Uses of the literal just **load from the global**; no per-use allocation.
- **Binary size**: Effectively the same as data-section: the bytes are in a data section in both cases. Globals add one global slot (ref) per literal and the start-function code. So binary size is roughly equal.
- **Pro**: No allocation every time we need the literal; simple call sites (single load).  
- **Con**: (1) Need a start function and a clear story for ordering (one global per literal vs table of literals). (2) **Runtime heap**: every literal is one GC array allocated at startup and held for the program’s lifetime; peak heap is higher than with data-section copy-on-use.

---

## 6. Multi-line Strings (Syntax and Semantics)

- Grammar already has: `'"""' { any_char } '"""'`.
- **Leading/trailing newlines**: The book shows multi-line strings with leading and trailing newlines and indentation. Design choice: either (1) preserve exact bytes (including leading/trailing newline and indentation), or (2) define a “strip” convention (e.g. strip one leading newline and optionally normalize indentation). This design leaves the exact convention to the grammar/spec; the important point is that multi-line strings are UTF-8 and support interpolation.
- No additional storage design: they are string literals like single-line ones; the same storage applies (data section by default, or globals when the flag is enabled).

---

## 7. String bytes: extension method `s.unsafe_bytes()`

- **API**: Accessed like an **extension method** on `String`: **`s.unsafe_bytes()`** returns **`Array<Uint8>`**. The receiver is the string; the result is the UTF-8 byte sequence. Implemented by a compiler intrinsic behind the scenes.
- **Semantics**: The result is the **original** byte array — the same backing storage as the string (no copy). Length of the array = length of the string in bytes. Modifying the returned array would mutate the string. **Private to the prelude library**: `s.unsafe_bytes()` is not part of the public API; the prelude uses it to implement length and other internal string methods without allocating a copy.
- **Purpose**: UTF-8 decoding, encoding, and all higher-level string operations (code-point iteration, indexing by code point, etc.) are implemented in **Dovetail code** (e.g. prelude / standard library), using `s.unsafe_bytes()` plus array operations. The compiler does not implement UTF-8 logic beyond ensuring literal bytes are valid UTF-8 and providing this single primitive.
- **Implementation**: In codegen, `s.unsafe_bytes()` returns the same reference as the string (the string is the packed array of u8; the intrinsic exposes it as `Array<Uint8>` for prelude use). No copy or allocation.

---

## 8. Summary

| Topic | Decision |
|-------|----------|
| String type | WASM-GC packed array of `u8` (UTF-8 bytes) |
| Char type | `i32` (Unicode code point) |
| Interpolation | `$ident` and `${expr}` in all string literals (compile-time where possible) |
| Multi-line | `"""..."""`; UTF-8; interpolation supported |
| Concatenation | `Concat<String>` via `++`; `String ++ String → String`; book updated |
| Literal storage | Data section by default; optional (future) flag for globals; user can enable if desired |
| Bytes access | Extension method `s.unsafe_bytes()` returns `Array<Uint8>`; UTF-8 and rest in Dovetail code |

---

## 9. Grammar and Book Updates

- **Grammar**: `++` has the same precedence and associativity as `+`. String interpolation expands using `++`.
- **Book**: Use `++` in string concatenation examples.

---

## 10. Implementation Phases

The phases below are ordered by dependency — each builds on the previous and is independently testable.

### Phase 1 — String Literal Codegen (Foundation)

The compiler already lexes, parses, and typechecks string literals. This phase makes them work at runtime.

- **Codegen — packed array type**: Define a WASM-GC packed array type (`ArrayType` with `StorageType::I8`) at a fixed type index. This is the runtime representation of `String`.
- **Codegen — data section**: Add a `DataSection` to the module pipeline. Deduplicate literal bytes (identical literals share one data segment). Emit `array.new_data` for each literal use, referencing the data segment and packed array type.
- **`type_to_valtype()`**: Change String mapping from `i32` to `Ref(non-nullable, Concrete(string_type_idx))`.
- **Globals**: String globals use the nullable-ref pattern (like records) — global declared as nullable, initialized in the start function, uses cast to non-nullable.
- **Tests**: Compile-and-run with string variable binding (e.g. `let s: String = "hello"`).

### Phase 2 — String Equality (`==`, `!=`)

- **Codegen**: Implement byte-by-byte comparison in `emit_eq_instruction` for `Type::String` — compare lengths (`array.len`), then loop over bytes (`array.get_u`) comparing each pair.
- `!=` derives from negating `==`.
- **Tests**: `assert "hello" == "hello"`, `assert "a" != "b"`, `assert "" == ""`.

### Phase 3 — String Concatenation (`++`)

- **Typechecker**: Resolve `Concat<String>` for String and lower its intrinsic implementation to native concatenation.
- **Codegen**: Allocate a new packed array of combined length, `array.copy` both halves into it.
- **Tests**: `assert "hello" ++ " " ++ "world" == "hello world"`, `assert "" ++ "a" == "a"`.

### Phase 4 — Additional Escape Sequences

- **Lexer**: Add `\$`, `\0`, `\u{XXXX}` escapes to `scan_string_literal()`.
- **Tests**: `assert "\0" != ""`, `assert "\u{0041}" == "A"`.

### Phase 5 — Multi-line Strings (`"""..."""`)

- **Lexer**: Detect triple-quote opening; scan until closing `"""`; allow embedded newlines and unescaped quotes.
- **Tests**: Multi-line literal equals equivalent single-line with `\n`.

### Phase 6 — Char Type and Literals

- **Lexer**: Add `CharLiteral` token; scan `'c'` with escape support.
- **Parser**: Add `Expr::CharLiteral` AST node.
- **Typechecker**: Add `Type::Char`; infer char literals.
- **Codegen**: `Char` → `i32`; emit code point as `i32.const`.
- **Tests**: `let c: Char = 'A'`; `assert c == 'A'`.

### Phase 7 — String Interpolation

- **Lexer**: Detect `$ident` and `${expr}` inside strings; emit a sequence of string-part tokens (or desugar in parser).
- **Parser**: Desugar interpolated string to a concatenation chain (`"a${x}b"` → `"a" ++ x.format() ++ "b"`).
- **Depends on**: Phase 3 (concatenation).
- **Tests**: `let name = "World"` then `assert "Hello, $name!" == "Hello, World!"`.

### Phase 8 — `s.unsafe_bytes()` Intrinsic

- **Typechecker**: Recognize `unsafe_bytes()` on `String` as a compiler intrinsic returning `Array<Uint8>`.
- **Codegen**: No-op — the string IS the byte array; just leave the ref on the stack.
- **Depends on**: `Array` type being available.
- **Tests**: `assert "hello".unsafe_bytes().length() == 5`.

### Phase 9 — Global Literals Optimization (Future)

- Optional flag in `Dovetail.toml`; start function allocates all literal arrays into globals; literal uses load from the global instead of allocating per use.
- Not required for correctness — purely a performance optimization.
