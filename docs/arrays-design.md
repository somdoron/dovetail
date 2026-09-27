# Arrays Design

This document designs **arrays** in Dovetail: representation (WASM-GC packed vs non-packed, reference-element arrays), the built-in generic `Array<T>`, and compiler intrinsics for element access and length. It aligns with the [language basics book](../website/content/book/03-language-basics.md), [type system book](../website/content/book/06-type-system.md), [grammar](grammar.md), [compiler design](compiler.md), [generics design](generics-design.md), and [strings design](strings-design.md).

---

## 1. Overview

- **Array** is a built-in generic type: `Array<T>`. It is declared in the prelude as an intrinsic (e.g. `type Array<T> = intrinsics`) and implemented by the compiler via WASM-GC (and packed) array instructions. **Array is a non-null reference type:** every value of type `Array<T>` is a valid reference to an array; there is no nullable array type in the core design.
- **Primitive element types** are **specialized per primitive**: one WASM representation per `Array<Int8>`, `Array<Int16>`, …, `Array<Uint8>`, …, `Array<Bool>`, `Array<Char>`. No shared representation for primitives. **We do not instantiate primitive arrays in the code.** Codegen always generates the fixed set of primitive array types and assigns each a **fixed type ID**; there is no per-use instantiation or type-parameter substitution for primitives.
- **Packed WASM-GC:** `Array<Int8>`, `Array<Int16>`, `Array<Uint8>`, `Array<Uint16>` use **packed** WASM-GC arrays (storage-efficient).
- **Non-packed WASM-GC:** `Array<Int32>`, `Array<Uint32>`, `Array<Int64>`, `Array<Uint64>`, `Array<Bool>`, `Array<Char>` use **non-packed** WASM-GC arrays.
- **Reference types** (including `String`, records, enums, classes, and `Array<U>`): `Array<T>` is **specialized** like other generics: one WASM representation per instantiation (e.g. `Array<String>`, `Array<SomeRecord>`). No type-info at runtime; the typechecker and codegen treat each instantiation as a concrete type.
- **Compiler intrinsics:** The compiler provides **access** primitives (get, set, length) and **construction** primitives (array.new, array.new_fixed, Array.fill). All other array operations (iteration, fold, map, etc.) are implemented in Dovetail code (prelude/standard library) on top of these.
- **Construction:** **array.new_fixed** is used for array literals `[e1, e2, …]`. **array.new(length)** allocates an array of given length. **Array.fill(length, value)** accepts a length and a fill value and creates an array of that length with each element set to the value; it uses **array.new** internally (allocate then fill).
- **Surface syntax:** Access intrinsics are exposed via **array extension syntax**: `array.get`, `array.set`, and `array.length` — i.e. `arr.get(index)`, `arr.set(index, value)`, `arr.length`. Construction is exposed as literals (lowered to array.new_fixed), and as **Array.fill(length, value)** (static method / intrinsic).

**Implementation status:** Done.

---

## 2. Representation by Element Type

| Element type | Representation | Notes |
|--------------|----------------|-------|
| `Int8`, `Int16`, `Uint8`, `Uint16` | Packed WASM-GC array | One GC type per element type; packed storage |
| `Int32`, `Uint32`, `Int64`, `Uint64` | WASM-GC array | One GC type per element type; non-packed |
| `Bool` | WASM-GC array | Non-packed (or packed if desired; design choice) |
| `Char` | WASM-GC array | `Char` = `i32`; non-packed GC array of i32 |
| `Float32`, `Float64` | WASM-GC array | One GC type per element type |
| `String`, records, enums, classes, `Array<U>`, … | Specialized | One WASM type per element type (e.g. `Array<String>`, `Array<SomeRecord>`) |

**Array is a non-null ref type:** In WASM-GC and in the type system, `Array<T>` is a non-nullable reference type. Every value of type `Array<T>` is a valid reference; the compiler and codegen use non-null ref types where possible.

**Specialization:**

- **Primitive arrays:** Fully **specialized**. Each primitive element type gets its own WASM-GC (or packed) array type and dedicated get/set/length code. No type-info at runtime. **No instantiation in the front end or codegen:** the compiler does not “instantiate” `Array<Int32>` etc. as a generic; codegen has a **fixed roster** of primitive-array types (one per primitive element type) and a **fixed type ID** for each. Those types and IDs are always present in the output; no per-use instantiation.
- **Reference-type arrays:** **Specialized.** One WASM type per instantiation (e.g. `Array<String>`, `Array<SomeRecord>`). Type inference and codegen treat each `Array<T>` instantiation like other generic types (one struct/function per type-argument list). Get/set/length use the concrete element type; no type-info at runtime.

---

## 3. Reference-Element Arrays (Specialized)

For `Array<T>` when `T` is a reference type, the same **specialized-only** rule as other generics applies: one WASM type per instantiation (e.g. `Array<String>`, `Array<SomeRecord>`). Type inference and codegen instantiate `Array<T>` per type-argument list. Get/set/length use the concrete element type; no type-info at runtime.

### 2.1 Fixed type IDs for primitive arrays (codegen)

- Codegen **does not instantiate** primitive array types. It maintains a fixed mapping from primitive element type to WASM-GC (or packed) array type and type ID.
- For each primitive that can be an array element (Int8–Int64, Uint8–Uint64, Bool, Char, Float32, Float64), codegen **always** emits the corresponding array type and assigns it a **fixed type ID** (e.g. a stable integer used in type section and in instructions). No generic instantiation pass is needed for these; they are built-in.
- **Reference-element** `Array<T>` (e.g. `Array<String>`, `Array<SomeRecord>`) are specialized like other generics: one WASM type per instantiation; type inference and codegen treat each instantiation as concrete.

---

## 4. Access Intrinsics (get, set, length)

The compiler implements three **access** primitives. All other operations (iteration, indexing syntax sugar, higher-order methods) are built in Dovetail code on top of these and the construction intrinsics below.

| Intrinsic | Meaning | Surface syntax |
|-----------|---------|----------------|
| **Get** | Element at index | `arr.get(index)` |
| **Set** | Write element at index | `arr.set(index, value)` |
| **Length** | Number of elements | `arr.length` |

- **Get:** `(array, index) -> element`. For primitives: direct GC array load. For reference arrays: load then cast to the instantiation’s element type .
- **Set:** `(array, index, value) -> Unit`. For primitives: direct GC array store. For reference arrays: store the reference; type-checked at compile time.
- **Length:** `(array) -> Int32` (or natural number type). Same for all representations: length field of the GC array.

**Bounds:** Out-of-bounds get/set **trap** (e.g. wasmtime traps on invalid array access). The prelude/standard library can provide safe wrappers that check bounds and panic with a clear message before calling the intrinsics.

---

## 5. Construction Intrinsics (array.new, array.new_fixed, Array.fill)

The compiler implements three **construction** primitives:

| Intrinsic | Meaning | Surface syntax |
|-----------|---------|----------------|
| **array.new** | Allocate array of given length | Used internally by **Array.fill** only; **not** exposed to user code (for ref types there is no default—it would mean `ref null`) |
| **array.new_fixed** | Allocate array with fixed elements | **Array literals** `[e1, e2, …, eN]` lower to this |
| **Array.fill** | Allocate array of length, fill with value | `Array.fill(length, value)` — uses **array.new** then sets each element to `value` |

- **array.new(length):** Allocates an array of type `Array<T>` (element type known from context) with the given length. Used **only** by **Array.fill** (which then sets every element to the fill value). **Not exposed** to user code: for reference types there is no default element (it would be `ref null`), so we do not offer a bare “allocate uninitialized” API.
- **array.new_fixed(e1, e2, …, eN):** Allocates an array of length N and sets element i to ei. This is the **array literal** implementation: `[a, b, c]` is lowered to `array.new_fixed(a, b, c)`.
- **Array.fill(length, value):** Allocates an array of the given length and sets every element to `value`. Implemented as: **array.new(length)** then set each index to `value` (or a single “new with fill” instruction if the backend supports it). Exposed as a static method / intrinsic: `Array.fill(length, value)` returns `Array<T>` where `T` is the type of `value`.

**Empty array:** `[]` can be implemented as `array.new_fixed()` (length 0) or as `array.new(0)`; the design treats empty array as a valid array value with length 0.

---

## 6. Array Extension Syntax (array.get, array.set, array.length)

The intrinsics are exposed as **extension-method-style** API so that user code and the prelude see a consistent, type-checked interface:

- **`arr.get(index)`** — get element at `index`.
- **`arr.set(index, value)`** — set element at `index` to `value`.
- **`arr.length`** — number of elements (property or nullary method as per grammar).

The prelude declares an extension for `Array<T>` (or the compiler treats these names specially) with:

- `function get(self, index: Int32): T` (or appropriate index type)
- `function set(self, index: Int32, value: T): Unit`
- `function length(self): Int32` (or a property)

The compiler **recognizes** these extension methods on `Array<T>` and lowers them to the three intrinsics; they are not implemented as normal Dovetail functions. So:

- **Resolution:** Normal name resolution and type-checking (e.g. `arr.get(i)` type-checks that `arr` is an `Array<T>`, `i` is an integer, and the result is `T`).
- **Codegen:** Calls to `array.get` / `array.set` / `array.length` on a value of type `Array<T>` are emitted as the corresponding WASM-GC (or packed) array instructions, with the correct representation chosen from the element type (specialized primitive vs specialized reference).

If the grammar supports both property and method syntax, `arr.length` can be defined as a parameterless method or a property; the design uses “array extension syntax” to mean this single, compiler-backed API surface.

---

## 7. Literals and Construction

- **Array literals:** Grammar has `array_expr = "[" [ expression { "," expression } ] "]"`. Typechecker infers `Array<T>` from the element types (all must unify to one `T`). Construction is implemented by the **array.new_fixed** intrinsic: `[e1, e2, …, eN]` lowers to `array.new_fixed(e1, e2, …, eN)`.
- **Empty array:** `[]` requires a type annotation (e.g. `[]: Array<Int32>`) so that the compiler knows which representation to create. Implemented as `array.new_fixed()` (length 0) or `array.new(0)`.
- **Array.fill(length, value):** User-visible construction for “array of N copies of value”; uses **array.new** then fill. Exposed as `Array.fill(length, value)`.
- **String and Array<Uint8>:** Per [strings-design](strings-design.md), `s.bytes()` returns `Array<Uint8>`. That array shares the same backing storage as the string (packed array of u8). No separate allocation.

---

## 8. Index Type and Bounds

- **Index type:** For the intrinsics and the extension API, index is an integer type (e.g. `Int32` or `Natural`). The design uses **Int32** for consistency with WASM and Char; unsigned index type can be added later if desired.
- **Bounds checking:** Out-of-bounds get/set trap (see §4). Safe, user-facing APIs in the prelude should perform bounds checks and panic with a clear message when the index is out of range.

---

## 9. Equality and Comparable

### 9.1 Primitive arrays (now)

- For **all primitive array types**, the compiler **generates** (or provides intrinsics for) **equality** and **comparable** behavior.
- **Equality:** Two arrays are equal iff they have the same length and each element is equal (element-wise). Exposed as an equality method or operator (e.g. `arr == other` or `arr.equals(other)`); exact syntax follows language conventions. Generated for every primitive element type.
- **Comparable:** Lexicographic comparison: compare element-wise; first index where elements differ determines the order; if one array is a prefix of the other, the shorter is considered less. Exposed as comparison methods or operators (e.g. `<`, `<=`, `>`, `>=` or `arr.compareTo(other)`). Generated for all primitive element types **except Bool**.
- **Bool:** `Array<Bool>` has **equality only**, no comparable (Bool has no ordering; arrays of Bool are not comparable).

So: **Int8, Int16, Int32, Int64, Uint8, Uint16, Uint32, Uint64, Char, Float32, Float64** → equality + comparable. **Bool** → equality only.

### 9.2 Reference-element arrays (later, trait-based)

- For **reference-element** arrays, **do not** generate equality or comparable in the initial implementation. Defer until the language has **Equatable** and **Comparable** traits.
- **Once traits exist:** Allow comparison of arrays of reference element type only when the element type implements the appropriate trait:
  - **Equality:** `Array<T>` supports equality when `T` implements **Equatable** (element-wise equality).
  - **Comparable:** `Array<T>` supports comparable (lexicographic order) when `T` implements **Comparable**.
- So: equality and comparable for reference-element arrays are **trait-gated**; the compiler (or prelude) provides implementations only for `Array<T>` when `T: Equatable` or `T: Comparable` respectively. No equality/comparable for arbitrary `Array<T>` of ref types until then.

### 9.3 Summary

| Element kind | Equality | Comparable |
|--------------|----------|------------|
| Primitive (except Bool) | Generated now | Generated now |
| Bool | Generated now | No (Bool has no ordering) |
| Reference (specialized) | Deferred; require `T: Equatable` | Deferred; require `T: Comparable` |

---

## 10. Summary Table

| Topic | Decision |
|-------|----------|
| Array type | Built-in generic `Array<T>`, prelude intrinsic |
| Primitive elements | Specialized per type; fixed type IDs in codegen—no instantiation |
| Packed WASM-GC | Int8, Int16, Uint8, Uint16 |
| Non-packed WASM-GC | Int32, Uint32, Int64, Uint64, Bool, Char (and Float32/64) |
| Reference elements | Specialized; one WASM type per element type |
| Array ref type | Non-null reference type |
| Access intrinsics | get, set, length |
| Construction intrinsics | array.new, array.new_fixed (literals), Array.fill(length, value) |
| Surface syntax | arr.get(i), arr.set(i, x), arr.length; literals → new_fixed; Array.fill(length, value) |
| Other operations | Implemented in Dovetail (prelude/stdlib) on top of these intrinsics |
| Equality | Primitive arrays: generated now (element-wise). Bool: equality only. Ref arrays: deferred until Equatable trait. |
| Comparable | Primitive arrays (except Bool): generated now (lexicographic). Bool: no comparable. Ref arrays: deferred until Comparable trait. |

---

## 11. Grammar and Book Updates

- **Grammar:** No change required for the three operations if they are expressed as normal extension methods (e.g. `arr.get(index)`, `arr.set(index, value)`, `arr.length`). The compiler identifies these by receiver type and method name and lowers to intrinsics.
- **Book** [website/content/book/06-type-system.md](../website/content/book/06-type-system.md) (§6.7 Arrays): Can state that array element access and length are provided via `arr.get(i)`, `arr.set(i, x)`, and `arr.length`, and that other operations (e.g. iteration, fold) are provided by the prelude. Optional indexing sugar (e.g. `arr[i]` and `arr[i] = x`) can be defined as syntactic sugar for `arr.get(i)` and `arr.set(i, x)` in a later design or grammar update.

---

## 12. Implementation Phases

**Dependency:** This work depends on **generic extension methods**. The prelude exposes array access as extension methods on `Array<T>` (get, set, length); the compiler must support extensions on generic types so that `extension for Array<T>` with type parameter `T` can be declared and resolved, and so that calls like `arr.get(i)` are recognized and lowered to intrinsics for any instantiation of `Array<T>`. See [extension-methods-design](extension-methods-design.md); generic extensions are required for arrays.

Suggested incremental phases. Each phase should be testable (unit tests per stage, integration tests for full pipeline).

| Phase | Scope | Deliverables |
|-------|--------|--------------|
| **1. Prelude and type resolution** | No codegen | Declare `Array<T>` in prelude as intrinsic. Typechecker/collect: resolve `Array` and type arguments; treat `Array<T>` as non-null ref type. Parser and grammar already support array types and literals; ensure they typecheck. No array codegen yet. |
| **2. Primitive arrays: non-packed, access only** | One element type (e.g. Int32) | Codegen: fixed type ID and WASM-GC array type for `Array<Int32>`. Lower **get**, **set**, **length** (extension syntax or direct intrinsic calls) to WASM-GC array instructions. Prelude (or test) defines extension with get/set/length; compiler recognizes and lowers. Integration test: create array via intrinsic/new_fixed stub, then get/set/length. |
| **3. Primitive arrays: new_fixed and literals** | Same element type(s) as phase 2 | Codegen **array.new_fixed** for primitives: allocate GC array of length N, store each argument. Lower array literal `[e1, e2, …]` to new_fixed. Empty array `[]` with type annotation → new_fixed() or new(0). Tests: literals, empty array, get/set after construction. |
| **4. Primitive arrays: all non-packed** | Int32, Uint32, Int64, Uint64, Bool, Char, Float32, Float64 | Extend fixed type-ID roster and codegen to all non-packed primitive array types. Same access + new_fixed logic per type. Tests per primitive. |
| **5. Primitive arrays: packed** | Int8, Int16, Uint8, Uint16 | Add **packed** WASM-GC array types and fixed type IDs for these four. Codegen get/set/length/new_fixed using packed instructions. Tests for packed arrays. |
| **6. Primitive arrays: Array.fill** | All primitives | Codegen **array.new(length)** (internal use only). Lower **Array.fill(length, value)** to new + loop of set. Expose as static/intrinsic `Array.fill(length, value)`; typechecker resolves it. Tests: fill for at least one primitive type. |
| **7. Reference-element arrays** | Array&lt;T&gt; when T is ref | Typechecker: when element type is reference, instantiate (specialized). Codegen: one WASM type per instantiation. Implement **new_fixed**, **get**, **set**, **length**, **new** (for fill), **fill** for ref arrays; get/set use concrete element type. Tests: `Array<String>`, `Array<SomeRecord>`, literals, fill, get/set. |
| **8. Polish and prelude** | Docs, strings, book | Prelude: Array intrinsic declaration; extension for get/set/length; Array.fill. Align strings-design: `s.bytes()` returns `Array<Uint8>` (same ref). Update book and compiler.md. Optional: indexing sugar `arr[i]` / `arr[i]=x` as sugar for get/set. |
| **9. Primitive arrays: equality and comparable** | All primitive element types | Generate equality (element-wise) for all primitive arrays; generate comparable (lexicographic) for all except **Bool**. Bool: equality only, no comparable. Expose as methods or operators per language conventions. Ref-element arrays: no equality/comparable until Equatable and Comparable traits exist. |

**Dependencies:** (1) **Generic extension methods** — required for the array access API (`extension for Array<T>` with get/set/length). (2) **Generics (specialized only)** — Phase 7 instantiates reference-element `Array<T>` like other generics; one WASM type per element type.

---

## 13. Relationship to Other Docs

- **Extension methods (generic):** Arrays depend on **generic extension methods**. The prelude defines an extension for `Array<T>` (get, set, length); see [extension-methods-design](extension-methods-design.md). Generic extensions are required so that `arr.get(i)`, `arr.set(i, x)`, and `arr.length` work for any `Array<T>` and are lowered to intrinsics.
- **Generics (specialized only):** Reference-type `Array<T>` follows the same specialized-only rules as other generics; one WASM type per instantiation.
- **Strings:** `String` is a packed array of `u8`; `s.bytes()` returns `Array<Uint8>` (same backing storage). So `Array<Uint8>` is both a primitive (packed) array and used as the bytes view of a string.
- **Compiler:** [compiler.md](compiler.md) Arrays and Intrinsics sections should reference this document for the split between packed/non-packed/specialized and the three intrinsics with array extension syntax.
