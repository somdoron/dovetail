# Generics Design (Specialized Only)

This document designs **generics** in Dovetail: generic functions and generic records with **monomorphization only** (specialized). Every instantiation is compiled as a separate WASM function or struct; there is no shared representation, no type-info at runtime, and no reified match. It aligns with the [type system book](book/06-type-system.md), [generics book](book/07-generics.md), [control flow book](book/04-control-flow.md), [grammar](grammar.md), and [compiler design](compiler.md). This design is the baseline for all generic types in Dovetail (records, and later enums, classes).

**Reified/shared generics** (single representation for reference-type arguments, type-info parameters, runtime type dispatch) have been **dropped** for simplicity. They may be revisited in a future version of the language.

---

## 1. Overview

- **Generic functions** are declared with type parameters (e.g. `function id<T>(x: T): T = x`). Grammar already has `function_decl = ... "function" IDENT [ type_params ] ...`.
- **Generic records** are declared with type parameters (e.g. `record Box<T> = value: T`). Grammar already has `record_decl = ... "record" IDENT [ type_params ] "=" record_body`.
- **Every instantiation** is **specialized**: one WASM function or struct per (generic definition, type-argument list). All type arguments are substituted at compile time; the typed AST and codegen see only concrete types. No type-info parameters, no `(ref any)`, no runtime polymorphism for type parameters.
- **Scope:** Generic function and record declaration, type-argument inference at call sites, construction, field access, `with`, match (scrutinee types are always concrete), and codegen. Future: constraints on type parameters (traits), variance, generic enums, generic classes.

**Implementation status:** Done for generic functions and generic records (specialized only); generic enums and classes to follow their respective designs.

---

## 2. Type System

### 2.1 TypeParamName

- **TypeParamName** is a newtype: `pub struct TypeParamName(pub String)`. It represents the name of a type parameter (e.g. `"T"`, `"U"`). Used in `GenericFunctionSignature`, `RecordTypeSignature`, and during inference; the **typed** AST does not expose `TypeParameter` in types because all instantiations are specialized (concrete types only).

### 2.2 Type and TypeParameter (during inference only)

- **Type** (in the typechecker) has a type-argument list where relevant: `Record(Fqn, MangledName, Vec<Type>)` — the third component is the (possibly empty) list of type arguments.
- **TypeParameter(TypeParamName)** exists only **during** collect and inference (e.g. in generic signatures and during unification). In **specialized** output, the typed AST never contains `TypeParameter`; all types are concrete after type-argument substitution.

### 2.3 Type parameter shadowing

Type parameter names must not be shadowed:

1. **Type parameter vs type parameter:** A type parameter must not shadow an enclosing type parameter of the same name (relevant when generic classes are added).
2. **Variable vs type parameter:** A variable binding must not shadow a type parameter name (e.g. `function foo<T>(x: T): Char = let T = 'a'; T` → error). Enforce in the rules phase.

### 2.4 Mangled names

**Functions:**

- **Specialized:** `fqn$ParamType1$ParamType2$...$TypeArg1$TypeArg2$...` — value parameter types first (concrete type mangles), then type-argument mangles. One mangled name per instantiation.

**Records:**

- **Specialized:** `fqn$param1$param2$...` where `param1`, `param2`, ... are the **mangled names of the type arguments**. One mangled name per instantiation. Align with the function overloading mangling scheme.

---

## 3. Registry

### 3.1 GenericFunctionSignature

- **GenericFunctionSignature** for generic functions: **type_params**, visibility, params (value params with `TypeParameter` in signatures), return_type. Overload resolution: same FQN can have multiple generic and/or non-generic signatures; type arguments select the concrete instantiation (always specialized).

### 3.2 Generic record signatures

- **RecordTypeSignature** has **type_params** for generic records (empty for non-generic). The registry stores the generic definition; concrete instantiations are produced during inference (one per type-argument list).

---

## 4. Typed AST

- **TypedFunction:** For generic functions we only emit **specialized** typed functions; `type_params` on the *definition* is used during inference, but each call site gets a concrete `FunctionCall` with the specialized mangled name. No shared function shape in the typed AST.
- **RecordTypeDef:** One per **specialized** instantiation, keyed by the specialized mangled name. Fields are concrete field types.
- **Expressions:** All generic calls and record operations use the same expressions as non-generic code: `FunctionCall`, `RecordCreate`, `FieldAccess`, `RecordWith`. No Shared* expressions.

---

## 5. Inference

### 5.1 Type argument inference

Inference determines type arguments by **unification** — matching each value argument's type (and, when needed, expected return type) against the generic signature. When unification is ambiguous or a type parameter does not appear in a value-parameter position, the caller must provide explicit type arguments.

### 5.2 Specialized instantiation only

- **Functions:** Infer concrete type arguments, substitute, produce a concrete function type, use the specialized mangled name, typecheck the call as a normal `FunctionCall`.
- **Records:** Produce a concrete `Type` (e.g. `Record(Fqn, MangledName, vec![Type::Int32])`); register one `RecordTypeDef` per instantiation. Construction, field access, and `with` use existing expressions.

---

## 6. Codegen

- **Functions:** One WASM function per specialized mangled name. Signature and body as for non-generic (all types concrete). No type-info parameters, no casts.
- **Records:** One WASM-GC struct per specialized mangled name. Layout = list of field types. Same as non-generic records.

---

## 7. Match

Match scrutinee types are always **concrete** (all type parameters resolved to concrete types at the call site). The compiler can eliminate non-matching arms and resolve the matching arm at compile time. No runtime type-info or reified match path.

Record patterns with type arguments (e.g. `Box<Int32> { value = v }`) are supported when the scrutinee type is a concrete instantiation; the type part is used for static resolution only.

---

## 8. Implementation Phases (summary)

- **Phase 1: Specialized generic functions** — Type params, unification, specialized mangled name, `FunctionCall`; codegen identical to non-generic.
- **Phase 2: Specialized generic records** — Type params on records, `Record(Fqn, MangledName, Vec<Type>)`, one `RecordTypeDef` per instantiation, construction/field access/`with`.
- **Phase 3.1: Type parameter shadowing** — Rules phase: variable must not shadow type parameter name.

(Phases for shared/reified generics are removed; not implemented.)

---

## 9. Summary Table

| Topic | Design |
|-------|--------|
| Instantiation | **Specialized only.** One WASM function/struct per (generic, type-argument list). |
| TypeParamName | Newtype for type parameter names. Used in signatures and during inference. |
| Type / TypeParameter | `TypeParameter` only during inference; typed AST has concrete types only. |
| Function mangled names | `fqn$P1$...$TA1$...` (param types then type args). |
| Record mangled names | `fqn$T1$T2$...` (type argument mangles). |
| GenericFunctionSignature | type_params + params + return_type. |
| RecordTypeDef | One per specialized instantiation; keyed by specialized mangled name. |
| Expressions | FunctionCall, RecordCreate, FieldAccess, RecordWith (no Shared*). |
| Match | Scrutinee always concrete; static resolution only. |

---

## 10. Future Work

- **Constraints on type parameters:** traits (e.g. `function f<T: Show>(x: T)`), variance.
- **Generic enums and classes:** Same specialized-only approach; one WASM type per instantiation.
- **Reified/shared generics:** Deferred; may be revisited in a future language version (single representation for reference-type arguments, type-info at runtime, reified match).
