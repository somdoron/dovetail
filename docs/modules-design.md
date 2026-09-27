# Modules Design

This document designs **modules** in Dovetail: a named container that aggregates functions, properties, types, and other declarations under one name (similar to F# modules). There are **two kinds of modules**: **standalone modules** (namespace only) and **modules for a type** (when a type with the same name exists in the package). Unnamed extensions are **not** part of this design; modules are a separate concept. The design aligns with the [grammar](grammar.md), [compiler](compiler.md), and [book Part 11: Packages and Modules](../website/content/book/11-packages.md).

**In scope:** Two module kinds (standalone vs module-for-type), allowed members per kind, module declaration (inline and file-level), import rules (module only), qualified use and dispatch from module-for-type, FQN and visibility.

**Out of scope:** Unnamed extensions (removed); importing individual members from a module as bare names; nested modules; open/dotting to bring names into scope. **Named extensions** remain a separate language feature (`extension Name for Type = ...`).

---

## 1. Overview

- **Module** — A named container in a package. Its contents are accessed as `ModuleName.member`. There are two kinds, determined by whether a **type with the same name** exists in the package.
- **Standalone module** — No type in the package has the same name as the module. The module may contain only **functions without `self`**, **properties without `self`** (static), **top-level `let`** (static/global), and types. No instance members.
- **Module for a type** — A type with the same name as the module exists in the package (e.g. module `Array`, type `Array<T>`). The module may contain: **functions with `self`** (instance), **functions without `self`** (static), **`property name(self)`** (instance), **`property name()`** (static), and **top-level `let`** (static/global; no storage in the module aside from global). Instance members are dispatched on values of that type (e.g. `arr.length` when `length` is in the module for the type of `arr`).
- **Import rule:** You may only import a module as a whole (`import package_path.ModuleName`). You cannot import a single member. **Instance-level members** of a module-for-type are **always available on the type**: you do not need to import the module to use `value.member` when the value's type has a same-named module. If you have access to the type (e.g. through a record field, or the type is in scope), instance methods and instance properties from that type's module are in scope. Static/qualified use (`ModuleName.member`) requires the module to be in scope (same package or imported).
- **File-level form:** The first declaration may be **`module package_path.ModuleName`** or **`module package_path.TypeName<T>`** (for a generic type). The rest of the file is the module body. The **kind** of module (standalone vs for-type) is determined by whether the name is an existing type in that package.
- **Module uniqueness:** A package may have **at most one** module (standalone or for a type). For a generic type, use a single **`module Array<T> =`** (see §2.3). A second module declaration in the same package is an error.

This design is **distinct from named extensions**. Modules are not syntactic sugar for extensions; they are their own concept. Terminology and implementation use "module" and "module for a type" throughout, not "extension" or "unnamed extension."

**No scattering:** A module for a type **forbids scattering** code about that type across the package. There is at most one module per (package, name), so all instance and static members for a type (from that package) live in one coherent place. By contrast, extension methods (Dovetail), optional type extensions (F#), or `impl` blocks (Rust) allow many files or blocks to add members to the same type; behavior for the type can be spread across the codebase. With modules, you get a single, coherent namespace for the type's behavior.

**Implementation status:** In progress.

---

## 2. Two Kinds of Modules

### 2.1 Standalone module

**When:** No type in the package has the same name as the module (e.g. module `Math`, no type `Math`).

**Allowed members:**

- **Functions without `self`** — Static functions only. Example: `function floor(x: Float64): Int32 = ...`
- **Properties without `self`** — Static properties only. Syntax: `property name(): Type = ...` (no receiver). Example: `property pi(): Float64 = 3.14159`
- **Top-level `let` bindings** — Allowed; they are **static** (global storage, no per-module instance storage). Example: `let Pi: Float64 = 3.14159`
- **Types** — Records, enums, classes, traits, newtypes, type aliases (as in any module).

**Not allowed:** Functions with `self`, properties with `self`.

**Use:** After `import pkg.Math`, you write `Math.floor(...)`, `Math.pi`, `Math.Point`, etc. No instance dispatch.

### 2.2 Module for a type

**When:** A type with the same name as the module exists in the package (e.g. module `Array`, type `Array<T>`). For generic types, the module shares the type's name and its type parameters are in scope in the module body.

**Allowed members:**

- **Functions with `self`** — Instance methods. Example: `function get(self, index: Int32): T = ...`
- **Functions without `self`** — Static methods. In a **generic** module, statics have the type parameter in scope (see §2.3). Example: `function fill(size: Int32, value: T): Array<T> = ...`
- **Instance properties** — Syntax: `property name(self): Type = ...`. Example: `property length(self): Int32 = ...`
- **Static properties** — Syntax: `property name(): Type = ...`. In a generic module, statics have T in scope. Example: `property empty(): Array<T> = ...`
- **Top-level `let` bindings** — Allowed (static/global). In a **generic** module, top-level `let` is **instantiated per type argument** (see §2.3). Example: `let defaultCapacity: Int32 = 16`

**Use:** Qualified: for a generic module you must supply type arguments when accessing statics or globals (e.g. `Array<Int32>.fill(5, 42)`, `Array<Int32>.empty`, `Array<Int32>.defaultCapacity`). Instance: `arr.length`, `arr.get(0)` — when the type of `arr` is the type that has the same name as the module, resolution looks up the member in that module (instance function or instance property).

**Naming:** This is a **module for a type**, not an extension. The compiler and docs use "module for type" or "type-associated module." No "unnamed extension" or "extension" terminology.

### 2.3 Generic module: instantiated globals (C#-style)

For a **generic** module (e.g. **`module Array<T> =`**), type parameters are in scope for **all** members — instance and static — and for **top-level `let`**. Static members and globals are **instantiated per type argument** (like C#): each closed type (e.g. `Array<Int32>`, `Array<String>`) has its own copy of each static and each top-level `let`. So `Array<Int32>.defaultCapacity` and `Array<String>.defaultCapacity` are **different** storage locations.

**Rules:**

- **Instance members** (functions with `self`, instance properties) — Have the module's type parameter(s) in scope. Example: `function get(self, index: Int32): T`, `property length(self): Int32`.
- **Static members** (functions without `self`, static properties) — Have the module's type parameter(s) in scope. Example: `property empty(): Array<T> = ...`, `function fill(size: Int32, value: T): Array<T> = ...`. Each instantiation (e.g. `Array<Int32>`) has its own static members; one instantiation per type-argument list, as for generic functions.
- **Top-level `let`** — Allowed; they have `T` in scope (and may or may not use it). **One global per (module, type arguments)**. So `let defaultCapacity: Int32 = 16` in `module Array<T> =` yields one global for `Array<Int32>`, another for `Array<String>`, etc. Mutable state in one instantiation does not affect another.

**Access:** To use static members or globals of a generic module, you supply the module's type arguments (e.g. `Array<Int32>.empty`, `Array<Int32>.fill(5, 42)`, `Array<Int32>.defaultCapacity`). **Bi-directional inference:** when the compiler can infer the type from context (e.g. expected type of an assignment or function parameter), you may omit the type arguments: `let x: Array<Int32> = Array.empty` — the compiler infers the module instantiation from the expected type. When there is no such context, the module must be closed with type args explicitly.

**Codegen:** Emit one set of globals per (generic module, type-argument list). Mangled names include the type arguments. One instantiation per type-argument list (same as generic functions).

---

## 3. Defining Modules

### 3.1 Inline module declaration

```
module_decl   = [ doc_comment ] "module" IDENT [ type_params ] "=" module_body
module_body   = BEGIN { module_member SEP } module_member [ SEP ] END
```

When the module is for a **generic** type, declare **`module Array<T> =`** (type params required; see §2.3). For a non-generic type or a standalone module, use a single declaration without type_params: `module Math =`, `module Point =`.

**module_member** depends on the module kind: for a **standalone** module, static functions, static properties, top-level `let` (static), and type declarations; for a **module for a type** (non-generic or generic), instance and static functions, instance and static properties, and top-level `let`. In a **generic** module (`module Array<T> =`), **all** members and top-level `let` have the type parameter in scope; statics and globals are instantiated per type argument (§2.3). The type must already be declared in the package (or a dependency) when the module is collected. At most one module per package.

**Example — standalone module:**

```dovetail
package standard

module Math =
    let Pi: Float64 = 3.141592653589793
    property pi(): Float64 = 3.141592653589793
    public function floor(x: Float64): Int32 = ...
    public function ceil(x: Float64): Int32 = ...
    public record Point =
        x: Float64
        y: Float64
```

Top-level `let Pi` is static (global); no per-module storage.

**Example — module for a generic type (instantiated globals, all members have T in scope):**

```dovetail
package standard.prelude

// Type Array<T> is declared elsewhere in the package (e.g. intrinsic).

module Array<T> =
    let defaultCapacity: Int32 = 16   // one per instantiation (Array<Int32>, Array<String>, ...)
    public property empty(): Array<T> = intrinsic
    public function fill(size: Int32, value: T): Array<T> = intrinsic
    public function get(self, index: Int32): T = intrinsic
    public function set(self, index: Int32, value: T): Unit = intrinsic
    public property length(self): Int32 = intrinsic
```

Use: `Array<Int32>.empty`, `Array<Int32>.fill(5, 42)`, `Array<Int32>.defaultCapacity`; `arr.get(0)`, `arr.length`. Access to statics and globals normally requires the module type arguments (e.g. `Array<Int32>.empty`). **Bi-directional inference:** when the compiler can infer the type from context (e.g. expected type of an assignment or function argument), you may omit the module type arguments: e.g. `let x: Array<Int32> = Array.empty` — the compiler infers the module instantiation from the expected type `Array<Int32>`. For a **non-generic** type, a single `module TypeName =` (no type params) holds both static and instance members and may have top-level `let`.

### 3.2 File-level shorthand

If the **first** declaration in a file is:

```
module package_path.ModuleName
```

or, for a generic type, **`module package_path.TypeName<T>`** (no `=`, no body), then the **rest of the file** is the module body and the effective package is `package_path`. The **kind** of module is determined by whether `ModuleName` is an existing type in that package:

- **If `ModuleName` is an existing type** (e.g. `Array`) → the file defines the **module for that type**. For a generic type use **`module pkg.Array<T>`** (type params required); for a non-generic type use **`module pkg.TypeName`**. Body may contain instance and static members and top-level `let`; in a generic module, all members and `let` have the type parameter in scope and globals are instantiated per type argument (§2.3).
- **Otherwise** → the file defines a **standalone module** named `ModuleName`. The body may contain only static functions, static properties, top-level `let` (static), and types.

**Example — standalone (file-level):**

```dovetail
module standard.Math

let Pi: Float64 = 3.141592653589793
public function floor(x: Float64): Int32 = ...
```

**Example — module for a type (file-level, generic):**

```dovetail
module standard.prelude.Array<T>

let defaultCapacity: Int32 = 16
public property empty(): Array<T> = intrinsic
public function get(self, index: Int32): T = intrinsic
public property length(self): Int32 = intrinsic
```

For a generic type, the file-level form includes the type parameters: **`module package_path.Array<T>`**. For a non-generic type, use just the name: **`module package_path.Point`**.

**Grammar:**

```
file_header = "module" package_path "." IDENT [ type_params ]
```

Valid only as the first declaration in a file. When the type is generic, type_params are required (e.g. `Array<T>`). The typechecker resolves the name in that package to decide standalone vs module-for-type and enforces the allowed members for that kind.

**Restrictions:** (1) A file may use either this file-level form or a normal `package` declaration, not both. (2) **At most one module per package** — a second module in the same package is an error.

---

## 4. Imports and Qualified Use

### 4.1 Import rule: module only

- **Allowed:** `import package_path.ModuleName` (and `import package_path.ModuleName as Alias`). This brings the module into scope for **qualified** use (`ModuleName.member`).
- **Not allowed:** `import package_path.ModuleName.member`. The last segment must be a module (or top-level package symbol), never a module member.

**Instance members (module for a type):** Instance-level methods and properties from the module for that type are **always available on values of that type** whenever the type is in scope. You do **not** need to import the module to call `value.member` or use `value.property`. No separate module import is required for instance dispatch.

**Static/qualified use:** `ModuleName.member` (static functions, static properties, types, top-level `let`) requires the module to be in scope (same package or `import package_path.ModuleName`).

### 4.2 Qualified use and dispatch

- **Static / types:** For a **generic** module, static members and globals **require** the module to be instantiated with type arguments: `Array<Int32>.empty`, `Array<Int32>.fill(5, 42)`, `Array<Int32>.defaultCapacity`. For non-generic modules: `Math.floor(...)`, `Math.pi`, `Math.Point`. Resolution: resolve the leading segment to a module (with type args for generic modules when accessing statics or globals), then the trailing segment to a member. Requires the module to be in scope (same package or imported).
- **Instance (module for a type):** `arr.length`, `arr.get(0)`. Instance members are **always available on the type**: whenever the type of `arr` is in scope, the instance methods and properties from that type's module are available. **No module import is needed** for instance dispatch—only access to the type. Resolution: resolve the type of `arr`; look up the module for that type (in the type's package); resolve the member. This is **module dispatch**.

No bare names are brought into scope by importing a module.

---

## 5. Fully Qualified Names (FQN)

- **Module:** `package_path.ModuleName` (e.g. `standard.Math`, `standard.prelude.Array`).
- **Module member:** `package_path.ModuleName.member_name` (e.g. `standard.Math.floor`, `standard.prelude.Array.get`).

The registry stores each module and its members under these FQNs. For a **module for a type**, instance members are also used for dispatch on values of that type; the FQN still identifies the module member, not an "extension."

---

## 6. Visibility

- **public** — Visible to any package that can see the module.
- **internal** — Visible only within the same package.
- **private** — Visible only within the same file.

Visibility applies to module members. After stripping non-public members for export, if a module has no remaining public members, the module is removed from the registry.

---

## 7. Grammar and validation

- **Keywords:** `module` (and `property` for the new property syntax).
- **Declaration:** `module_decl` = `module IDENT [ type_params ] = module_body`. For a **generic** type, **`module Array<T> =`** is required (see §2.3). For a non-generic type or standalone module, use a single declaration without type_params. Optionally, file-level **`module package_path.IDENT [ type_params ]`** as the first declaration (e.g. `module standard.Math`, `module standard.prelude.Array<T>`). **At most one module per package** — the typechecker errors on a second module.
- **module_body:** Sequence of declarations; the **allowed** declarations are enforced by the typechecker based on module kind (standalone vs module for a type). Property syntax: **`property name(self): Type = expr`** (instance) or **`property name(): Type = expr`** (static). No "let property"; the new syntax is as stated.
- **Imports:** Semantic rule that `import_path` may not resolve to a module member.

---

## 8. Pipeline Impact

| Stage        | Change |
|-------------|--------|
| **Parser**  | Add `module_decl` and `module_body`; add file-level `module package_path "." IDENT [ type_params ]`. Parse property as `property name ( self ) : Type = expr` or `property name ( ) : Type = expr`. |
| **Collect** | Register modules; resolve whether each module is standalone or for-type (name exists as type in package). **Error** if more than one module per package. For generic type, require `module Array<T> =`. Register members under package.ModuleName[.type_args].member. Enforce allowed members per kind. In generic module, all members and top-level `let` have the type parameter in scope; statics and globals are instantiated per type argument (§2.3). For module-for-type, record the associated type for instance dispatch. Strip non-public and remove empty modules when building registry for dependents. |
| **Inference** | Resolve `ModuleName.member` or `ModuleName<args>.member` to module member. For `value.member`, instance dispatch uses the module for the value's type. Replace FQN with MangledName. |
| **Codegen**  | Module is namespace only. Emit members by FQN/MangledName. Instance calls lower to function call with receiver as first argument (same as today for method calls). |

---

## 9. Implementation Phases

| Phase | Scope | Notes |
|-------|--------|--------|
| **1** | Standalone module only (inline + file-level) | Parse module; Collect: register module and members. Only static functions and static properties and types. Infer: `ModuleName.member`. No instance dispatch. |
| **2** | Import module, cross-package | Allow import of module; reject import of module member. Resolve `ModuleName` from imports. |
| **3** | Module for a type | Collect: when module name equals existing type, treat as module-for-type; allow instance functions, instance properties, static properties, `let` bindings. Infer: `value.member` resolves to module-for-type when type matches. Codegen: instance call as function call with receiver. |
| **4** | Visibility and empty-module stripping | Strip non-public for export; remove module if no public members left. Enforce visibility on access. |
| **5** | Generic modules | Support **generic** module-for-type: `module Array<T> =` (and file-level `module package_path.Array<T>`). Parser: allow type_params on module_decl and on file_header. Collect: when module name matches a **generic** type, require type_params; register generic module and its members (one instantiation per type-argument list, as for generic functions). All members and top-level `let` have T in scope. Infer/codegen: resolve `Array<Int32>.member`, `arr.member` when arr: Array<Int32>. Access to statics and globals requires module type args. |
| **6** | Instantiated globals in generic modules | **Collect/Inference:** Give top-level `let` bindings in a generic module **access to the module's type parameters** (today they may not; this is a gap). So `let empty: Array<T> = ...` and `let defaultCapacity: Int32 = 16` are both allowed in `module Array<T> =`, with `T` in scope for the type and initializer of the `let`. **Inference:** Support **bi-directional inference** for generic module statics and globals: when the expression is in a context with an expected type (e.g. `let x: Array<Int32> = Array.empty`, or a function argument), infer the module's type arguments from that expected type so that `Array.empty` is valid and resolves to `Array<Int32>.empty`. **Codegen:** Emit **one global per (generic module, type-argument list)** for top-level `let` in generic modules (mangled name includes type args). One set of globals per type-argument list. Ensure statics also have T in scope. When type args are omitted, they must be inferable from context. |
| **7** | Migrate prelude from unnamed extension to module | Convert prelude sources (e.g. `array.dove`) from `extension <T> for Array<T> =` to `module Array<T> =` (single module; statics and globals with T in scope, instantiated per type argument per §2.3). Migrate `let property` to `property name(self)` / `property name()` as per new syntax. Update any prelude-specific resolution/codegen that assumed extension; ensure Array (and any other extended types) are exposed as module-for-type. All prelude tests and downstream tests must pass. |
| **8** | Remove unnamed extension completely | Parser: reject `extension for Type` and `extension <T> for Type<T>` (no name); only allow `extension Name for Type = ...`. Typechecker/Collect: remove registration and lookup for unnamed extensions; instance/static dispatch for types uses only module-for-type (and named extensions). Codegen: no unnamed-extension path. Remove dead code and update tests. Named extensions remain. |

**Dependencies:** 2 depends on 1. 3 depends on 1 (and type/registry for same-name lookup). 4 can follow 2–3. **5** (generic modules) depends on 3. **6** (instantiated globals) depends on 5. **7** (prelude migration) depends on 6 (need instantiated globals for Array<T>). **8** depends on 7 (prelude and any other unnamed-extension usages must be migrated to modules before removing the feature).

---

## 10. Summary

| Item | Description |
|------|-------------|
| **Two kinds** | **Standalone module:** no type with same name; static functions, static properties, top-level `let` (static), types. **Module for a type:** type with same name exists; instance/static functions, instance/static properties, top-level `let`. For a **generic** module (`module Array<T> =`), **all** members and `let` have T in scope; statics and globals are **instantiated per type argument** (C#-style; §2.3). Access to statics/globals requires `Array<Int32>.member`. |
| **Declaration** | Inline: `module IDENT [ type_params ] = module_body`. File-level: first declaration **`module package_path.ModuleName`** or **`module package_path.TypeName<T>`** (generic); rest of file = body. Kind determined by whether name is an existing type. **At most one module per package.** For generic type, **`module Array<T> =`** required. |
| **Property syntax** | Instance: `property name(self): Type = expr`. Static: `property name(): Type = expr`. |
| **Import** | Only `import package_path.ModuleName` (or with `as`). Not `import ... .ModuleName.member`. |
| **Use** | Qualified: `ModuleName.member` or `ModuleName<args>.member` (for generic module statics/globals, type args required). For module-for-type, instance: `value.member` when value's type has same-named module. |
| **FQN** | Module: `package.ModuleName`. Member: `package.ModuleName.member`. |
| **Distinct from extensions** | Modules are their own concept. No "unnamed extension" or "syntactic sugar for extension." Named extensions (`extension Name for Type = ...`) remain separate. |

This design gives two clear module kinds (standalone vs for-type) with distinct allowed members and no extension terminology.
