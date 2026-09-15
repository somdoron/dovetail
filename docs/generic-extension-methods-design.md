# Generic Extension Methods Design

This document designs **generic extension methods** in Dovetail: extensions that declare type parameters and extend generic types (e.g. `extension <T> for Array<T>` or `extension ArrayHelper<T> for Array<T>`). It reuses the **generic function** framework from [generics-design](generics-design.md) (specialized only: type params, unification, mangling, codegen). It builds on [extension-methods-design](extension-methods-design.md) for naming, visibility, invocation, and FQN of extensions.

**Implementation status:** Done.

---

## 1. Overview

- **Generic extension methods** allow extensions that declare type parameters and extend a type that may reference those parameters (e.g. `Array<T>`). The same **specialized-only** (monomorphized) codegen rules as generic functions apply: one WASM function per instantiation.
- **Syntax:** 
  - **Unnamed:** `extension <T> for Array<T> = ...` — type parameters after `extension`, extended type may reference them.
  - **Named:** `extension ArrayHelper<T> for Array<T> = ...` — optional name and type parameters; the “for” type may reference the same type parameters.
- **Scope of type parameters:** The extension’s type parameters are in scope for the **“for” type** and for **every method** in the extension body. So `extension <T> for Array<T> = function get(self, index: Int32): T = ...` — `T` is used in the extended type and in the return type of `get`.
- **Framework reuse:** Same type system (TypeParamName, type_args during inference), inference (unification, specialized instantiation), mangled names (specialized scheme), and codegen (one function per instantiation) as **generic functions**. Extension methods are effectively generic functions whose first parameter (for instance methods) is the receiver; the “extension” is the mechanism for resolution and scoping.

---

## 2. Syntax

Grammar already has:

```
extension_decl = [ doc_comment ] "extension" [ IDENT ] [ type_params ]
                 "for" type [ where_clause ] [ "=" extension_body ]
```

- **Unnamed generic:** `extension <T> for Array<T> = ...` — `IDENT` omitted, `type_params` = `<T>`, `type` = `Array<T>` (reference to `T`).
- **Named generic:** `extension ArrayHelper<T> for Array<T> = ...` — `IDENT` = `ArrayHelper`, `type_params` = `<T>`, `type` = `Array<T>`.

The **“for” type** must be well-formed and may reference only the extension’s own type parameters (and types in scope). No additional grammar change is required.

**Examples:**

```dovetail
package prelude

extension <T> for Array<T> =
    function get(self, index: Int32): T = ...
    function set(self, index: Int32, value: T): Unit = ...
    function length(self): Int32 = ...
```

```dovetail
package myapp
import prelude.Array

extension ArrayHelper<T> for Array<T> =
    function first(self): T = self.get(0)
```

---

## 3. Type System and Registry

### 3.1 Type parameters

- **TypeParamName** and **TypeParameter(TypeParamName)** are as in [generics-design](generics-design.md) §2. The extension declares a list of type parameters (e.g. `T`); they are in scope for the “for” type and for every method in the extension body.
- The **extended type** (“for” type) may be a generic type applied to the extension’s type parameters, e.g. `Array<T>`, so the extension is “for” the generic type `Array` with type argument `T` (the parameter). The typechecker resolves `Array` and checks that the type argument list matches the type parameter count and that the “for” type is well-formed.

### 3.2 Generic extension signature (registry)

The registry needs a representation for **generic** extensions, analogous to **GenericFunctionSignature** for generic functions:

- **GenericExtensionSignature** (or equivalent):
  - **type_params:** `Vec<TypeParamName>` (e.g. `["T"]`).
  - **extended_type:** The “for” type, which may contain `TypeParameter` (e.g. `Array<T>`). Stored as a type expression or typed type that can reference type params.
  - **methods:** For each method, the same as today (name, params, return type), but param and return types may reference `TypeParameter` (e.g. `self: Array<T>`, `index: Int32`, return `T`).
  - Visibility, FQN base (package + type name for unnamed, package + extension name for named), etc., as in [extension-methods-design](extension-methods-design.md).

Non-generic extensions continue to use the existing (non-generic) extension representation; generic extensions use this new shape so that inference and codegen can treat each method as a generic function (with receiver) and apply specialized instantiation.

### 3.3 Resolution and FQN

- **Unnamed generic extension:** Same package as the extended type. FQN of a method = **extended type FQN** + method name. The extended type is generic (e.g. `prelude.Array`), so the logical FQN is e.g. `prelude.Array.get`. Overloads (e.g. different param lists) are distinguished by signature; generic vs non-generic by presence of type_params.
- **Named generic extension:** FQN of a method = **package** + **extension name** + method name (e.g. `myapp.ArrayHelper.first`), as in [extension-methods-design](extension-methods-design.md) §7.2. The extension name does not include type arguments; type arguments are determined at the call site.

---

## 4. Inference at Call Sites

### 4.1 Instance calls: `receiver.methodName(args)`

- **Receiver type:** e.g. `arr: Array<Int32>`. The typechecker knows the concrete type of the receiver (or a type that contains type parameters if inside a generic).
- **Resolve extension:** Find an in-scope extension for the **base** type (e.g. `Array`) such that the receiver type is an instantiation of the extension’s “for” type. For `Array<Int32>`, the extension `extension <T> for Array<T>` matches with `T` = `Int32`.
- **Type argument inference:** Unify the receiver type with the extended type. `Array<Int32>` vs `Array<T>` yields `T` = `Int32`. If there are multiple type parameters, unification proceeds as for generic function calls. Explicit type arguments on the method (if we allow `arr.get<Int32>(0)`) can override or supplement inference.
- **Emit typed call:** Produce a normal `FunctionCall` (or equivalent) with the specialized mangled name and concrete types. All instantiations are specialized.

### 4.2 Static calls: `TypeName.methodName(args)`

- **TypeName:** e.g. `Array<Int32>`. Resolve extension for the base type `Array` with type args `Int32`; same inference as above. No receiver argument; only `args` are passed.

### 4.3 Explicit type arguments

If the grammar supports `receiver.methodName<U, V>(args)` or `Array<Int32>.methodName(args)`, type arguments can be specified explicitly and checked against the extension’s type parameters; otherwise inference must determine them from the receiver type and value arguments.

---

## 5. Mangled Names

Reuse the **generic function** mangling scheme from [generics-design](generics-design.md) §2.4, treating each extension method as a generic function whose first parameter (for instance methods) is the receiver:

- **Specialized:** `fqn$ParamType1$ParamType2$...$TypeArg1$TypeArg2$...` — parameter types include the receiver type (e.g. `Array<Int32>`) and value parameter types; then type argument mangles. Example: `prelude.Array.get$Array_Int32$Int32$Int32` for `get(self: Array<Int32>, index: Int32): Int32`. One mangled name per instantiation.

The exact encoding (e.g. how `Array<Int32>` is stringified in the mangle) should match the rest of the codebase (e.g. `MangledName::for_function` and record/type mangling).

---

## 6. Codegen

Codegen for generic extension methods is **identical** to generic functions:

- **Specialized:** One WASM function per specialized instantiation. Receiver and value parameters have concrete types; no type-info; no casts. Same as non-generic extension methods today.
- **Shared:** One WASM function per generic extension method (shared mangled name). First N parameters = N `i32` type-info (one per type parameter). Remaining parameters: receiver as `(ref any)` if its type is a type parameter, then value parameters. Body works with `(ref any)`; no entry cast. **Call site:** pass type-info i32s (from type args or forwarded from enclosing generic), then receiver, then args; if the method returns a type-parameter type, emit `ref.cast` to the statically-known return type at the call site. **Nested generics:** when the receiver or type args are still `TypeParameter`, forward the corresponding type-info from the enclosing function’s leading i32 parameters.

This assumes the **reified generic functions** codegen (shared function shape, type-info passing, call-site cast, forwarding) is implemented; generic extension methods are then a thin layer that resolves the call to a generic “function” (receiver + method) and reuses that codegen.

---

## 7. Relationship to Non-Generic Extensions

- **Non-generic extensions** ([extension-methods-design](extension-methods-design.md)): No type_params; extended type is concrete (e.g. `Point`, `Int32`). Resolution, FQN, mangling, and codegen are as today.
- **Generic extensions:** type_params present; extended type may reference them (e.g. `Array<T>`). Resolution adds type-argument inference; mangling and codegen follow the generics design (specialized only). Import and visibility rules are unchanged: unnamed same-package extensions are auto-imported with the type; named extensions require explicit import.

Both can coexist. The typechecker distinguishes generic vs non-generic extensions by the presence of type_params and dispatches to the appropriate inference path.

---

## 8. Dependencies and Implementation Order

- **Depends on:** (1) **Generic functions** (specialized only) — type params, unification, specialized mangled names, codegen. (2) **Non-generic extension methods** — resolution of `receiver.methodName` and `TypeName.methodName`, FQN, and import rules; generic extensions extend that with type parameters and reuse generic-function codegen.
- **Arrays:** [arrays-design](arrays-design.md) requires generic extension methods so that `extension <T> for Array<T>` with get/set/length can be defined in the prelude and lowered to intrinsics. So either generic extension methods are implemented first (and array intrinsics are recognized as a special case of generic extension lowering), or the array intrinsics are implemented with a minimal “generic extension” path that only supports the prelude’s Array<T> extension.

---

## 9. Summary Table

| Topic | Design |
|-------|--------|
| **Syntax** | `extension <T> for Array<T> = ...` (unnamed); `extension ArrayHelper<T> for Array<T> = ...` (named). Grammar already has `[ type_params ]` on extension_decl. |
| **Type params** | In scope for “for” type and all methods. Same TypeParamName / TypeParameter (during inference) as generics design. |
| **Registry** | Generic extension signature: type_params, extended type (may use TypeParameter), methods with param/return types that may use TypeParameter. |
| **Inference** | Unify receiver type (or TypeName) with extended type to get type args; emit FunctionCall with specialized mangled name. |
| **Mangling** | Same scheme as generic functions: fqn$ParamTypes$TypeArgs (one per instantiation). |
| **Codegen** | One WASM function per instantiation; concrete types only. |
| **Dependency** | Generic functions (specialized only); non-generic extension methods. |

---

## 10. Relationship to Other Docs

- **Generics** [generics-design](generics-design.md): Generic extension methods use the same type system, inference, mangling, and codegen as **generic functions** (specialized only). No new concepts; only the “extension” resolution and receiver-as-first-argument wiring.
- **Extension methods** [extension-methods-design](extension-methods-design.md): Generic extensions extend the existing extension design with type_params and “for” types that reference them; naming, visibility, FQN, and import rules stay the same.
- **Arrays** [arrays-design](arrays-design.md): Arrays depend on generic extension methods for the prelude API (`extension <T> for Array<T>` with get, set, length). This document defines that API’s generic-extension shape so that the compiler can recognize and lower those calls to array intrinsics.
