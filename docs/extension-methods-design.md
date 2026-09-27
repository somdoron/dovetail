# Extension Methods Design

This document designs **extension methods** in Dovetail: adding methods to any type (primitives, records, enums, newtypes, classes, `String`, `Array`, etc.). It aligns with the [type system book](../website/content/book/06-type-system.md) and [grammar](grammar.md). This design focuses on **non-generic** extensions; generic extensions are out of scope and will be designed separately.

---

## 1. Overview

- **Extension methods** allow the user to add methods and properties to any type without modifying the type’s definition. The extended type may be a primitive, record, enum, newtype, class, or an intrinsic type such as `String` or `Array`.
- **Named vs unnamed:** If the extended type is in the same package, the extension can be **unnamed** and is then imported automatically with the type. If the extension is **named** (same package or not), it is **never** auto-imported with the type—it must always be imported explicitly. Named extensions always behave the same: explicit import required. Extensions for a type in a different package **must** be named.
- **Instance vs static:** There is no `static` modifier on extension functions. An extension function whose first parameter is `self` is an **instance** method (e.g. `person.greet()`). One with no `self` parameter is a **static** method (e.g. `Person.create()`). The typechecker and codegen treat them as overloads distinguished by signature; mangled names always differ.
- **Visibility:** Extension functions have visibility (`public`, `internal`, `private`). They are **internal by default**.
- **Overloading:** Extension functions support overloads like regular functions (same name, different parameter types/arity).
- **Access syntax:** Extension methods and properties are invoked with the same dot syntax as regular methods and properties: `receiver.methodName(args)` for instance, `TypeName.methodName(args)` for static, `receiver.propertyName` for instance properties.

**Implementation status:** Done.

---

## 2. Applicable Types

Extensions can be defined **for** any type:

| Kind | Example | Notes |
|------|---------|--------|
| Primitive | `Int32`, `Float64`, `Bool`, etc. | From prelude or current package |
| Record | `Point`, `User` | Data-only; extensions are the only way to add methods |
| Enum | `Color`, `Option<T>`, `Result<T,E>` | Same as records |
| Newtype | `UserId`, `Email` | Wraps a single underlying type |
| Class | `Person`, `Employee` | Can add methods without subclassing |
| Intrinsic | `String`, `Array<T>` | Prelude types |

The grammar uses a single `type` non-terminal for the “for” clause:

```
extension_decl = [ doc_comment ] "extension" [ IDENT ] [ type_params ]
                 "for" type [ where_clause ] [ "=" extension_body ]
```

For this design, `type_params` and `where_clause` are not used (non-generic extensions only).

---

## 3. Named vs Unnamed Extensions

### 3.1 Unnamed extensions (same package only)

When the extended type is in the **same package** as the extension, the extension **may be unnamed**: `extension for Point = ...`

- Unnamed extensions are **imported automatically with the type**. Importing `geometry.Point` makes all same-package **unnamed** extension methods on `Point` available at the call site.
- No separate import of the extension is required.

**Example (unnamed, same package):**

```dovetail
package geometry

record Point =
    x: Int32
    y: Int32

extension for Point =
    function magnitude(self): Float64 = ...
    function origin(): Point = Point { x = 0; y = 0 }
```

From another package:

```dovetail
package app
import geometry.Point   // Unnamed extension methods on Point come with Point

function main() =
    let p = Point.origin()
    println(p.magnitude())
```

### 3.2 Named extensions (same or different package)

**Named extensions always behave the same:** they are **never** auto-imported with the type. They must always be **imported explicitly** to be used, whether the extended type is in the same package or a different package.

- A named extension is **defined once**: you cannot duplicate the same extension name (e.g. `UserHelpers`) in the same package; there is a single definition per named extension.
- When the extended type is in a **different package**, the extension **must** be named (you cannot have an unnamed extension for a type from another package).
- When the extended type is in the **same package**, you may still **choose** to name the extension (e.g. `extension PointHelpers for Point = ...`). If you do, it is **not** imported with the type—callers must import the named extension explicitly.

This keeps behavior consistent: named extensions are always opt-in via import, avoiding surprise method additions and allowing multiple extensions (named or from different packages) with the same method names without conflict.

**Example (named, different package):**

```dovetail
package myapp
import users.User

extension UserHelpers for User =
    function displayName(self): String = "${self.firstName} ${self.lastName}"
```

Usage:

```dovetail
package app
import users.User
import myapp.UserHelpers   // Must import the extension

function main() =
    let user = User { firstName = "Alice"; lastName = "Smith" }
    println(user.displayName())
```

**Example (named, same package — not auto-imported):**

```dovetail
package geometry

record Point = x: Int32; y: Int32

extension PointHelpers for Point =
    function magnitude(self): Float64 = ...

// In another file in the same package (or another package):
// import geometry.Point
// import geometry.PointHelpers   // Required even in same package when extension is named
```

### 3.3 Summary

| Extension | Import |
|-----------|--------|
| **Unnamed** (same package only) | Import type → extension methods available automatically |
| **Named** (same or different package) | Always: import named extension explicitly. Never auto-imported with the type. |

---

## 4. Visibility

Extension functions have the same **visibility** as regular functions:

- **`public`** — visible to other packages (subject to package visibility rules).
- **`internal`** — visible only within the same package. **Default** when no modifier is specified.
- **`private`** — visible only within the same file.

**Grammar** (from [grammar.md](grammar.md)):

```
method_decl         = [ doc_comment ] [ visibility ] "function" IDENT ...
visibility          = "public" | "private" | "internal"
```

Extensions use only `method_decl` (no `static` modifier). Instance vs static is determined by presence or absence of a `self` parameter. If no visibility is specified, the extension function is **internal**.

---

## 5. Invocation Syntax

Extension methods are called with the **same dot syntax** as ordinary methods.

**Grammar** (postfix):

```
postfix_op  = "." IDENT
            | "." IDENT "(" [ arg_list ] ")"   /* method call */
```

- **Instance:** `receiver.methodName()` or `receiver.methodName(a, b)`. The receiver’s type must be the extended type (or a subtype); the first argument to the extension is the receiver (as `self`).
- **Static:** `TypeName.methodName()` or `TypeName.methodName(a, b)`. The type name is the extended type; there is no `self` argument.

Resolution: the typechecker resolves `.methodName` by considering (1) built-in members of the type (e.g. class methods, record fields), (2) extension methods in scope. For extensions, “in scope” means: **unnamed** same-package extensions (imported automatically with the type), and **named** extensions that have been explicitly imported (whether in the same package or not). Overload resolution then picks the best match by signature (including presence or absence of `self`). **Ambiguity:** if two or more overloads match the arguments equally, the call is ambiguous and the compiler reports an error.

---

### 5.5 Extension properties

Extensions can add **properties** as well as methods. An extension property is a getter: it has no parameters and is declared with `let property name: Type = expression` in the extension body. The expression is evaluated each time the property is accessed.

**Syntax:** In the extension body, same as in trait implementations: `let property name: Type = expression`. Only **instance** properties are supported (they have an implicit `self`); there are no static extension properties in this design.

**Access:** Same dot syntax as fields and methods: `receiver.propertyName`. The typechecker resolves `receiver.propertyName` by considering (1) built-in members (record fields, class fields, etc.), (2) extension methods, (3) extension properties in scope. Resolution and “in scope” rules match extension methods (unnamed same-package auto-imported with the type; named extensions require explicit import).

**Visibility:** Extension properties have the same visibility as extension methods (`public`, `internal`, `private`; **internal** by default).

**FQN and mangling:** Same scheme as extension methods: unnamed → `typeFqn.propertyName`; named → `package.ExtensionName.propertyName`. Mangling uses the property name and the extension-for type so that codegen can emit a getter function.

**Example:**

```dovetail
extension for Point =
    function magnitude(self): Float64 = ...
    let property size: Int32 = self.x * self.y
```

Usage: `p.size` calls the extension property getter. Properties are read-only; this design does not define extension property setters.

**Grammar:** The extension body must allow property declarations. If the grammar currently has `extension_body = BEGIN { method_decl SEP } ...`, it is extended to something like:

```
extension_body   = BEGIN { extension_member SEP } extension_member [ SEP ] END
extension_member = method_decl | property_decl
property_decl    = [ doc_comment ] [ visibility ] "let" "property" IDENT ":" type "=" expression
```

---

## 6. Self Type and Self Argument

- **`Self`** (if supported in the language) is an alias for the type that the extension extends (the “extension-for” type). Inside an extension `extension Foo for Bar`, `Self` means `Bar`.
- **`self` argument:** If the first parameter of an extension function is `self` (and its type is the extension-for type), the function is an **instance** method. If the function has no `self` parameter, it is a **static** method (no modifier keyword—the lack of `self` makes it static).
- **Instance:** `person.greet()` — receiver becomes `self`. The extension function has signature `(self: Person, ...) -> T`.
- **Static:** `Person.create()` — no receiver; the extension function has signature `(...) -> Person` (no `self`). Used for constructors or factory functions.

Static and instance methods can share the same **logical** name (e.g. same FQN for the “method” part); they are distinguished by **signature** (presence of `self`), so overload resolution and mangling never conflate them.

---

## 7. Fully Qualified Names (FQN)

FQNs are used for resolution, diagnostics, and (indirectly) for mangling.

### 7.1 Unnamed extension (same package)

The extended type and the extension live in the same package. The FQN of an extension function in an **unnamed** extension is:

- **FQN** = **type FQN** + **function name**

Example: type `geometry.Point`, function `magnitude` → FQN is `geometry.Point.magnitude`. Another function `origin` in the same extension → `geometry.Point.origin`. So the “namespace” of the method is the type’s FQN.

### 7.2 Named extension (any package)

For a **named** extension, the FQN of an extension function is:

- **FQN** = **package path** + **extension name** + **function name**

The **package path** is the package **where the named extension is defined**, not the package of the extended (“for”) type.

Example: extension defined in package `myapp`, extension name `UserHelpers`, function `displayName` → FQN is `myapp.UserHelpers.displayName` (even if the “for” type `User` lives in package `users`).

So:

- Unnamed: `package.TypeName.functionName`
- Named: `package.ExtensionName.functionName`

The typechecker and registry store extension methods under these FQNs and resolve `receiver.methodName` / `TypeName.methodName` by looking up the extended type (or the type of the receiver), then finding in-scope extensions for that type and resolving the function name to the appropriate FQN (and signature) for overload resolution.

---

## 8. Overloading and ambiguity

Extension functions support **overloads** like regular functions:

- Same name, different parameter types or arity (including static vs instance).
- Instance methods always have at least one parameter (`self`); static methods have no `self`. So static and instance methods with the same name have different signatures and can coexist; they are chosen by overload resolution based on call site (receiver vs type name, and argument list).
- **Resolution:** collect all in-scope extension methods for the receiver/type with the given name, then apply normal overload resolution (arity, types, etc.).
- **Ambiguity:** if two or more overloads match the parameters (arguments) equally—no single best match—the compiler must **error**. The call is ambiguous and the user must disambiguate (e.g. by casting, or by changing the call so that one overload is strictly better). This applies to extension method resolution and to function overload resolution in general.

---

## 9. Mangling

- **Mangled names** must uniquely identify the function for codegen. They are derived from FQN and **full signature** (including receiver vs no receiver).
- **Instance vs static:** An instance method always has `self` in its signature; a static method does not. So the mangled name for `Point.magnitude(self)` and `Point.origin()` will always differ because one has a receiver parameter and the other does not. Even an instance method with no other parameters has the `self` parameter; the mangled name therefore distinguishes it from a static method with the same name and no parameters.
- Convention: mangle using package path + (for unnamed: type name + function name; for named: extension name + function name) + signature (parameter types, including `self` for instance). Exact mangling scheme can follow the same rules as for free functions and class methods (e.g. encoding parameter types and arity).

---

## 10. Grammar and Parsing

From [grammar.md](grammar.md):

```
extension_decl      = [ doc_comment ] "extension" [ IDENT ] [ type_params ]
                      "for" type [ where_clause ] [ "=" extension_body ]

extension_body      = BEGIN { extension_member SEP } extension_member [ SEP ] END
extension_member    = method_decl | property_decl
property_decl       = [ doc_comment ] [ visibility ] "let" "property" IDENT ":" type "=" expression
```

Extensions use **`method_decl`** and **`property_decl`**. There is no `static` modifier on methods. A method with a first parameter `self` is an instance method; a method with no `self` parameter is a static method. Extension properties are instance-only (implicit `self`); they are getters declared with `let property name: Type = expression`.

---

## 11. Scoping and Import Rules (Summary)

- **Unnamed extension (same package only):** When a type `T` is imported (e.g. `import pkg.T`), all **unnamed** extension methods for `T` defined in `pkg` are in scope wherever `T` is in scope.
- **Named extension (same or different package):** Named extensions are **never** auto-imported with the type. The named extension must always be imported explicitly (e.g. `import pkg.ExtensionName`). Only then are its extension methods on the extended type visible. The extended type itself must also be in scope (via its own import). This applies whether the extension is in the same package as the type or not.
- **Visibility:** After import, visibility is enforced: only `public` (and possibly `internal` within the same package) extension methods from another package are available; `internal` extension methods are visible only within the defining package; `private` only within the defining file.

---

## 12. Implementation Phases

Implementation is split into phases so that each delivers a testable slice and builds on the previous one. The pipeline (lexer → layout → parser → typechecker → codegen) is extended for extension methods at each stage. **Generic extensions are out of scope** in these phases.

| Phase | Scope | Parser | Typechecker | Codegen |
|-------|--------|--------|-------------|--------|
| **1** | Extension decl (unnamed, same-package only), instance methods | Parse `extension for Type =` and extension body (method_decl only) | Collect: register extension and its instance methods; bind extension-for type. Infer: resolve `receiver.methodName()` to extension method. Rules: visibility | Emit extension instance methods as functions; method call lowers to function call with receiver as first arg |
| **2** | Static extension methods (no self), FQN and mangling | — (method_decl only; no self ⇒ static) | Same-package: FQN = type FQN + function name. Distinguish instance vs static by signature. Mangling includes signature (self vs no self) | Static calls `Type.method()` lower to function call without receiver; mangle names for instance and static |
| **3** | Named extensions (same or other package), explicit import | Parse `extension Name for Type =` | Register named extension under package + ExtensionName; FQN = package.ExtensionName.functionName. Named extensions always require explicit import for resolution (never auto-imported with type) | No change from Phase 2 (same calling convention) |
| **4** | Visibility, overloads, Self type | Visibility on extension methods; overload resolution | Enforce visibility (internal default; public/private). Overload resolution for extension methods (same name, different params/static vs instance). Optional: Self alias for extension-for type | No change |
| **5** | All extended types (primitives, String, Array, records, enums, newtypes, classes) | — | Ensure extension-for type can be any type (primitives, intrinsics, records, enums, newtypes, classes). Prelude/extensions for Int32, String, Array etc. as needed | Codegen for all type kinds (receiver representation for primitives vs ref types) |
| **6** | Extension properties | Parse `property_decl` in extension body; `extension_body` allows `extension_member` (method_decl \| property_decl) | Collect: register extension properties (FQN same scheme as methods). Infer: resolve `receiver.propertyName` to extension property getter. Visibility as for methods | Emit property getter as function; `receiver.propertyName` lowers to call getter with receiver as first arg |

### Phase 1 — Unnamed extensions, instance methods only (same package)

- **Parser:** Parse `extension for type =` (no optional `IDENT` yet) and `extension_body` with `method_decl` only. Ensure layout handles extension body. Extension-for type: allow at least record/enum/newtype/class and named primitives from prelude.
- **Typechecker — Collect:** For each file in the package, when processing an `extension for T`, resolve `T` to a type in the same package. Register an “unnamed extension” for `T` and collect its instance methods (name, params with first param as `self: T`, return type). Store in registry keyed by type and (for resolution) by type FQN + function name.
- **Typechecker — Infer:** For postfix `receiver.methodName(args)`, if receiver type is `T`, look up extension methods for `T` in the same package (unnamed extension). Resolve `methodName` to the extension function; typecheck `args` and treat call as call to that function with `receiver` as first argument.
- **Codegen:** Emit each extension instance method as a function (mangled name). A call `receiver.methodName(args)` is emitted as a call to that function with `(receiver, ...args)`.
- **Tests:** Same-package record + unnamed extension with one instance method; call `value.method()`; multiple methods; extension for enum/newtype in same package.

### Phase 2 — Static extension methods (no self), FQN and mangling

- **Parser:** No grammar change. Same `method_decl`; a method with no `self` parameter is a static extension method.
- **Typechecker:** Register extension methods that have no `self` parameter as static. FQN for unnamed: `typeFqn.functionName` for both instance and static. Mangling: include full signature so instance `(self, ...)` and static `(...)` get different mangled names.
- **Infer:** Resolve `TypeName.methodName(args)` when `TypeName` is the extension-for type to the extension method that has no `self` parameter; no receiver argument.
- **Codegen:** Emit static extension methods as plain functions; `TypeName.methodName(args)` → call with `args` only.
- **Tests:** Same-package extension with both instance methods (`function magnitude(self): ...`) and static methods (`function origin(): ...`); `Point.origin()`, `p.magnitude()`; confirm no signature clashes.

### Phase 3 — Named extensions, explicit import (same or other package)

- **Parser:** Parse optional `IDENT` in `extension [IDENT] for type`: when present, treat as named extension. No change to body.
- **Typechecker — Collect:** For named extension, register under package + extension name. Extension-for type may be from the same package or another package (resolve by import). Store methods (instance and static) under FQN `package.ExtensionName.functionName`.
- **Manifest/Import:** Support importing a named extension (e.g. `import pkg.ExtensionName`). Named extensions are **never** auto-imported with the type; only if that import is present are that extension’s methods added to resolution for the extension-for type at call sites (including when the extension and type are in the same package).
- **Infer:** When resolving `receiver.methodName` or `TypeName.methodName`, consider (1) same-package **unnamed** extensions for the type (auto-imported with type), (2) explicitly imported **named** extensions for the type. Then overload resolution.
- **Tests:** Two packages: type in A, named extension in B; import type + named extension in C and call method; without importing extension, call fails. Same package: named extension for type in same package; without `import pkg.NamedExtension`, call fails; with import, call succeeds.

### Phase 4 — Visibility and overloads

- **Visibility:** Apply visibility to extension methods (internal by default; allow public/private). When resolving from another package, only see public methods of imported named extensions. Enforce internal within package, private within file.
- **Overloads:** Allow multiple extension methods (same name, different parameter lists or static vs instance). Overload resolution picks the best match. **Ambiguity:** if two or more overloads match the arguments equally, report an error (ambiguous call). Ensure FQN + signature is unique; mangling reflects signature.
- **Self (optional):** If the language supports `Self` in extensions, add `Self` as an alias for the extension-for type in the typechecker for the scope of the extension body.
- **Tests:** Overloaded extension methods; static and instance with same name; visibility: internal not visible from other package; public visible when extension is imported; ambiguous call (two overloads match equally) produces an error.

### Phase 5 — All extended types

- **Typechecker:** Ensure extension-for type can be any of: primitive (Int32, Float64, Bool, etc.), String, Array&lt;T&gt;, record, enum, newtype, class. Resolve type from prelude or current package or imported package. No special restriction.
- **Codegen:** For primitives and value types, receiver is passed by value; for reference types (classes, records as refs if applicable, String, Array), receiver is passed as appropriate (ref or value per language/ABI). Align with existing representation of types in codegen.
- **Tests:** Extension for Int32; extension for String; extension for Array&lt;Int32&gt;; extension for class; extension for newtype. Prelude or test package can define small extensions for intrinsics to validate end-to-end.

### Phase 6 — Extension properties

- **Parser:** Extend `extension_body` to allow `extension_member` = `method_decl` | `property_decl`. Parse `let property name: Type = expression` (with optional doc_comment and visibility).
- **Typechecker — Collect:** Register extension properties for the extension-for type. FQN: unnamed → `typeFqn.propertyName`; named → `package.ExtensionName.propertyName`. Apply same visibility rules as extension methods.
- **Typechecker — Infer:** For postfix `receiver.propertyName` (no arguments), after resolving built-in members and extension methods, consider extension properties in scope. Resolve to the extension property getter; type of expression is the property’s declared type.
- **Codegen:** Emit each extension property as a getter function (receiver as only parameter). A use `receiver.propertyName` is emitted as a call to that getter with `receiver`.
- **Tests:** Unnamed and named extensions with `let property size: Int32 = ...`; call `p.size`; visibility and import rules same as for methods.

**Dependencies:** Phase 1 is the base. Phase 2 depends on Phase 1. Phase 3 depends on Phase 2 (named extension is additive). Phase 4 can be done after Phase 3. Phase 5 can be done in parallel with 3/4 once Phase 2 is in place. Phase 6 (extension properties) can be done after Phase 2 (needs extension body and FQN/mangling); it is independent of Phase 3–5 for parsing/collect, but resolution of `receiver.propertyName` assumes the same “in scope” and import rules as methods.

---

## 13. Summary

| Topic | Design |
|-------|--------|
| **Applicable types** | Any type: primitive, record, enum, newtype, class, String, Array. |
| **Unnamed (same package only)** | Auto-imported with the type. |
| **Named (same or other package)** | Never auto-imported; must always be imported explicitly. Defined once—no duplicate extension name in the same package. |
| **Visibility** | public / internal / private; **internal by default**. |
| **Invocation** | Dot syntax: `receiver.method()` (instance), `Type.method()` (static). |
| **Instance vs static** | No `static` modifier. First param `self` ⇒ instance; no `self` ⇒ static. Static and instance can share name; differ by signature; mangling always different. |
| **Self type** | `Self` is alias for the extension-for type (optional in implementation). |
| **FQN (unnamed)** | type FQN + function name (e.g. `geometry.Point.magnitude`). |
| **FQN (named)** | package + extension name + function name (e.g. `myapp.UserHelpers.displayName`). |
| **Overloading** | Same name, different params or static vs instance; normal overload resolution. If two overloads match equally → **error** (ambiguous call). |
| **Extension properties** | `let property name: Type = expression` in extension body; instance-only getters. Access: `receiver.propertyName`. Same visibility, FQN, and import rules as extension methods. Read-only; no setters. |
| **Mangling** | Full signature (including presence of `self`); instance and static never share mangled name. Properties mangle by name and extension-for type. |
| **Generics** | Out of scope; generic extensions designed separately. |

This design aligns extension methods with the existing book and grammar, and defines naming, visibility, FQN, and implementation phases for non-generic extension methods across all type kinds.
