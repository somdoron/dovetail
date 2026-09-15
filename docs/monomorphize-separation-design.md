# Monomorphize Phase Separation Design

**Status:** Implemented (dedicated monomorphize phase).

**Trait terminology:** Historical `TraitObject` and `dyn Display` sketches below
are superseded by explicit interface values and `InterfaceObject` IR. The
[trait/interface audit](trait-implementation-status.md) records the implemented
coercion and dispatch pipeline; the old eager inference queue is not an
outstanding trait feature.
**Related:** [PR #188](https://github.com/somdoron/domain2/pull/188/) (first attempt — not merged, superseded by this design)

---

## 1. Motivation

### 1.1 The problem

Today, the inference layer performs three responsibilities simultaneously:

1. **Type-checking** generic bodies — validates types, resolves overloads, checks trait bounds
2. **Instantiating** concrete copies — creates a new `TypedFunction` per concrete type-argument combination, substitutes all `TypeParameter` types, registers concrete `TypeDef` entries
3. **Expanding implement blocks** — resolves trait impl methods at call sites by constructing `GenericFunctionDef` from `GenericTraitImplMethodDef`, re-infers the body with concrete types, and inserts the result as a standalone `TypedFunction`. For non-generic impl blocks, methods are inferred directly into `typed_functions`. The implement block structure is lost — methods become indistinguishable from regular functions.

This coupling creates several problems:

**LSP cannot serve generic code.** Generic function definitions exist only as raw AST (`GenericFunctionDef.body: Expr`) in the registry. When instantiated, the resulting `TypedFunction` gets a placeholder span `Span::point("<generic>", 1, 1)`. There is no `TypedFunction` for the generic definition itself, so hover, go-to-definition, and find-references cannot work on generic code.

**No type parameter information in the typed AST.** `TypedFunction` has no `type_params` field. `FunctionCall` has no `type_args` field. After inference, all generic information is baked into mangled names and concrete types — there is no way to recover what type parameters were involved.

**Code duplication and complexity.** The `typechecking_only` flag, `in_instantiation` flag, and `in_generic_context()` checks spread conditional logic throughout inference. Generic functions are type-checked twice: once for validation (with `typechecking_only = true`, discarding results), then again per instantiation (with concrete types). The `instantiate_function_body` path re-runs the full inference machinery with import scope switching.

**Blocks selective type erasure.** The planned variance-based type erasure (see `variance-type-erasure-design.md`) needs a monomorphize phase that is aware of variance annotations. Extracting monomorphization now enables that work.

### 1.2 Goals

1. **Inference produces a 1:1 representation of source code**, enriched with types — one `TypedFunction` per function declaration (generic or not), one `TypeDef` per type declaration (generic or not)
2. **Generic bodies are fully type-checked** with `TypeParameter` types preserved in the typed AST
3. **A separate monomorphize pass** walks the concrete code, discovers generic call sites, and produces instantiated copies — pure transformation, no inference machinery needed
4. **LSP gets everything it needs** from the pre-monomorphize `TypedModule` — spans, type parameters, type arguments at call sites
5. **Codegen receives a fully concrete `TypedModule`** — same as today, no `TypeParameter` types, all mangled names resolved

---

## 2. Current Architecture

### 2.1 Pipeline

```
Source → Collect (Registry) → Infer (TypedModule) → Rules → Desugar Passes → Codegen
```

### 2.2 Generic function flow (current)

1. **Collect:** `collect_generic_function` stores `GenericFunctionDef` in registry (raw AST body + type params)
2. **Infer generic body (typecheck only):** `infer_function_typecheck_only` sets `typechecking_only = true`, maps type params to `TypeParameter` types, infers the body for error checking, then **discards the result**
3. **Call site triggers instantiation:** `infer_bare_function_call` → `resolve_generic_function` → `instantiate_generic_function`:
   - Builds `TypeParamSubstitution` from concrete type args
   - Computes concrete param/return types via `substitute_and_instantiate`
   - Computes `MangledName` with `.with_type_args(type_args)`
   - Calls `instantiate_function_body` which **re-infers** the body with concrete types, switching import scope to the definition file
   - Inserts `TypedFunction` into `typed_functions` with the instantiated mangled name and `Span::point("<generic>", 1, 1)`
4. **Generic type instantiation** follows the same pattern: `instantiate_generic_record`/`instantiate_generic_enum`/`instantiate_generic_class` create concrete `TypeDef` entries

### 2.3 Key flags

| Flag | Purpose | Set when |
|------|---------|----------|
| `typechecking_only` | Suppress TypeDef/TypedFunction registration | Inferring generic definition body |
| `in_instantiation` | Skip unreachable match arms | Inferring instantiated generic body |
| `instantiated: BTreeSet` | Deduplication | Generic function/type already instantiated |
| `current_type_params` | Maps type param names → concrete types | Inside instantiated generic body |

### 2.4 Registry: fragmented implement block storage

The registry splits implement blocks into three separate storages:

| Storage | Key | Value | Purpose |
|---------|-----|-------|---------|
| `trait_impl_methods` | `type_fqn → method_name` | `Vec<FunctionSignature>` | Non-generic impl methods (pre-computed `MangledName`, concrete types) |
| `generic_trait_impl_methods` | `type_fqn → method_name` | `Vec<GenericTraitImplMethodDef>` | Generic impl methods (raw AST body, type params, `for_type` pattern) |
| `trait_impls` | `(trait_fqn, type_fqn)` | `Vec<TraitImplInfo>` | Tracks which (type, trait) pairs exist + associated types |

This fragmentation means:
- Collect registers the same implement block into 2-3 different maps
- The relationship between methods and their parent implement block is lost
- Non-generic and generic methods have different representations (`FunctionSignature` vs `GenericTraitImplMethodDef`)
- `MangledName` is pre-computed during collect for non-generic methods — but in the new design, monomorphize should compute it

### 2.5 Data structures lacking generic info

| Structure | Missing |
|-----------|---------|
| `TypedFunction` | No `type_params` field |
| `FunctionCall` | No `type_args` field |
| `RecordCreate` | No `type_args` field |
| `ClassNew` | No `type_args` field |
| `RecordTypeDef` | No `type_params` field |
| `ClassTypeDef` | No `type_params` field |
| `FunctionRef` | No `type_args` field |
| `MethodRef` | No `type_args` field |

---

## 3. New Architecture

### 3.1 Pipeline

```
Per package:
  Source → Collect (Registry) → Infer (TypedModule) → Rules → merge

After all packages merged:
  Desugar Passes → Monomorphize → Codegen
       ↑                ↑
  (skipped for     (skipped for
   LSP/check)       LSP/check)
```

`typecheck()` is purely: Collect → Infer → Rules. No transformations.

The `TypedModule` produced by inference contains **both** generic and concrete definitions. The LSP uses this pre-desugar, pre-monomorphize module — closest to the user's source code. Desugar passes and monomorphize run only for `build`/`test` mode, producing the concrete module that codegen expects.

### 3.2 Inference output (new)

Inference produces a `TypedModule` where:

- **Generic functions** have `TypedFunction` entries with:
  - `type_params: Vec<TypeParamName>` — non-empty indicates the function is generic
  - Body is fully type-checked; types may contain `Type::TypeParameter`
  - **Original source span** preserved (not `<generic>`)
  - `MangledName` is the base name (e.g., `pkg.identity`), not an instantiated name

- **Generic type definitions** have `TypeDef` entries with:
  - `type_params` on `RecordTypeDef`, `EnumTypeDef`, `ClassTypeDef`
  - Fields may contain `Type::TypeParameter`
  - `MangledName` is the base name

- **Call sites** carry `type_args`:
  - `FunctionCall { name, args, type_args: Vec<Type> }` — `type_args` is empty for non-generic calls
  - `RecordCreate { fqn, fields, type_args: Vec<Type> }` — same pattern
  - `ClassNew { mangled_name, args, type_args: Vec<Type> }` — same pattern
  - Other relevant variants follow the same pattern

- **No instantiated copies** — inference produces exactly one entry per function/type declaration, plus concrete helpers that don't need instantiation (vtable glue, closure wrappers, etc.)

### 3.3 Monomorphize output

Monomorphize produces a new `TypedModule` where:

- All `TypedFunction` entries have `type_params: vec![]`
- All types are concrete (no `Type::TypeParameter`)
- All `type_args` on call sites are empty (the call target is the instantiated mangled name)
- Generic definitions are **removed** — only their concrete instantiations remain

This is identical to what codegen receives today.

---

## 4. Data Structure Changes

**Invariant — `type_args` ordering:** For `ImplFunctionCall` and `ExtFunctionCall`, `type_args` is the concatenation of **block-level type args** followed by **method-level type args**. Monomorphize splits them using `block.type_params.len()` as the boundary. Inference and monomorphize must agree on this ordering — a mismatch causes silent type substitution errors.

### 4.1 `TypedFunction`

```rust
pub struct TypedFunction {
    pub visibility: Visibility,
    pub name: MangledName,
    pub type_params: Vec<TypeParamName>,    // NEW — empty for non-generic
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
    pub vtable_self_type: Option<Type>,
    pub is_async: bool,
    pub display_name: String,
}

impl TypedFunction {
    pub fn is_generic(&self) -> bool {
        !self.type_params.is_empty()
    }
}
```

### 4.2 `TypedExprKind` — variants with `type_args`

```rust
FunctionCall {
    name: MangledName,
    args: Vec<TypedExpr>,
    type_args: Vec<Type>,               // NEW — empty for non-generic
},
FunctionRef {
    name: MangledName,
    type_args: Vec<Type>,               // NEW
},
MethodRef {
    object: Box<TypedExpr>,
    method_name: MangledName,
    type_args: Vec<Type>,               // NEW
},
RecordCreate {
    fqn: Fqn,
    fields: Vec<(String, TypedExpr)>,
    type_args: Vec<Type>,               // NEW
},
EnumCreate {
    fqn: Fqn,
    variant_name: String,
    args: Vec<TypedExpr>,
    type_args: Vec<Type>,               // NEW
},
EnumVariantRecordCreate {
    fqn: Fqn,
    variant_name: String,
    args: Vec<TypedExpr>,
    type_args: Vec<Type>,               // NEW
},
ClassNew {
    mangled_name: MangledName,
    args: Vec<TypedExpr>,
    type_args: Vec<Type>,               // NEW
},
RecordWith {
    object: Box<TypedExpr>,
    fqn: Fqn,
    overrides: Vec<(String, u32, TypedExpr)>,
    type_args: Vec<Type>,               // NEW
},
```

The `type_args` field is always `Vec<Type>`. For non-generic calls it is an empty vec (no heap allocation). The monomorphize pass checks `if !type_args.is_empty()` to know whether instantiation is needed.

### 4.3 `ImplFunctionCall` — new variant for trait impl calls

All calls to trait implementation functions (both instance methods and static methods) use a dedicated variant instead of `FunctionCall`:

```rust
/// A call to a trait implementation function. Monomorphize resolves this to a concrete FunctionCall.
ImplFunctionCall {
    trait_fqn: Fqn,
    for_type: Type,              // the implementing type; may be TypeParameter in generic bodies
    method_name: SymbolName,
    args: Vec<TypedExpr>,        // includes self as first arg for instance methods
    type_args: Vec<Type>,        // block-level + method-level (may contain TypeParameter)
},

/// A trait implementation function used as a first-class value.
ImplFunctionRef {
    trait_fqn: Fqn,
    for_type: Type,
    method_name: SymbolName,
    type_args: Vec<Type>,
},
```

**Why a separate variant?** `FunctionCall` tells monomorphize "find the body in `functions`". `ImplFunctionCall` tells monomorphize "find the body in `implement_blocks`". The variant disambiguates where to look without encoding it into a string.

**What `ImplFunctionCall` covers:**
- Instance method calls resolved through trait impls (`value.display()`)
- Static method calls resolved through trait impls (`MyType.fromString("hello")`)
- Property access resolved through trait impls (`value.length`)
- Operator lowering to trait methods (`a + b` → `Addable.add`) when the receiver type is concrete

**NOT** trait bound calls in generic bodies — those get their own variant (see below).

Trait bound method calls in generic bodies get their own variant:

```rust
/// A call through a trait bound on a type parameter. The concrete implementing type is unknown
/// until monomorphize substitutes the type parameter.
/// Monomorphize: substitute type_param → concrete type, then resolve the implement block.
TraitBoundFunctionCall {
    trait_fqn: Fqn,
    method_name: SymbolName,
    args: Vec<TypedExpr>,        // includes self as first arg
    type_args: Vec<Type>,        // method-level type args (may contain TypeParameter)
},
```

**What `TraitBoundFunctionCall` covers:**
- Method calls on type parameters with trait bounds (`x.display()` where `x: T, T: Display`)
- Operator lowering when the operand type is a type parameter (`a + b` where `a: T, T: Addable`)
- Property access on type parameters via trait bounds
- Generic trait methods called through bounds (`x.map(f)` where `x: T, T: Functor` and `map<U>` has a method-level type param)

The key difference from `ImplFunctionCall`: inference knows the trait and method, but does **not** know which implement block to use. The receiver's type is `TypeParameter(T)`, so there is no `for_type` to look up. Monomorphize substitutes `T → concrete type`, then resolves which implement block matches `(trait_fqn, concrete_type)`.

`type_args` carries method-level type parameters (e.g., the `U` in `Functor.map<U>`). For non-generic trait methods, `type_args` is empty. `for_type` is not needed — monomorphize derives the concrete implementing type from the receiver expression's type after substitution.

Similarly, extension method calls get their own variant:

```rust
/// A call to an extension method. Monomorphize resolves this to a concrete FunctionCall.
ExtFunctionCall {
    ext_fqn: Fqn,
    for_type: Type,              // may be TypeParameter in generic bodies
    method_name: SymbolName,
    args: Vec<TypedExpr>,        // includes self as first arg for instance methods
    type_args: Vec<Type>,        // block-level + method-level
},

/// An extension method used as a first-class value.
ExtFunctionRef {
    ext_fqn: Fqn,
    for_type: Type,
    method_name: SymbolName,
    type_args: Vec<Type>,
},
```

**What stays as `FunctionCall`:**
- Regular function calls (standalone, module)
- Intrinsic calls (`IntrinsicCall` — already separate)
- Class virtual calls (`ClassVirtualCall` — already separate)
- Class constructor calls (`ClassNew` — already separate)
- Trait object calls (`TraitObjectMethodCall` — already separate)

**Open question: generic class methods.** Methods on generic classes (or methods with their own type params on any class) are stored in `ClassTypeSignature.generic_instance_methods` / `generic_static_methods` — same fragmented pattern as extensions and impl blocks. These could follow the same block-based treatment if needed, but class methods are tightly coupled with the class body (initializer, field access, `self` typing). For now, generic class method calls can stay as `FunctionCall` with `type_args`, since the class body inference already produces `TypedFunction` entries for non-generic methods and the generic ones can be added with `type_params` set. Revisit if the same LSP problems arise.

**Monomorphize handling per variant:**

`ImplFunctionCall`:
1. Use `for_type` to find the matching `TypedImplementBlock`
2. Find the method by `method_name`
3. If block or method has type params: substitute `type_args` and create concrete copy
4. Compute `MangledName::for_impl_method(trait_fqn, concrete_type_fqn, method_sym, concrete_trait_type_args)`
5. Rewrite node to `FunctionCall { name: mangled, args, type_args: vec![] }`

`TraitBoundFunctionCall`:
1. Get receiver type from `args[0].ty` — after substitution, this is a concrete type
2. Use `(trait_fqn, concrete_type.to_fqn())` to find the matching `TypedImplementBlock`
3. Find the method by `method_name`
4. Expand the method (may require substituting block-level type args inferred from the concrete receiver type, plus method-level `type_args`)
5. Compute `MangledName` and produce a concrete `FunctionCall`

`ExtFunctionCall`:
1. Use `ext_fqn` to find the matching `TypedExtensionBlock`
2. Find the method by `method_name`
3. If block or method has type params: substitute `type_args` and create concrete copy
4. Compute `MangledName::for_named_extension_method(...)` 
5. Rewrite to `FunctionCall`

### 4.4 Variants with method references (desugaring helpers)

These variants reference trait impl methods that the desugar passes will expand into calls. Since impl method `MangledName`s are not computed until monomorphize, these variants must carry structured resolution info instead of `MangledName`.

The methods referenced here are always trait impl methods (`Iterable.iterator`, `Async.andThen`, `Unwrap.unwrap`, etc.), so they use the same fields as `ImplFunctionCall`:

```rust
/// Resolved reference to a trait impl method — used by desugar helper variants.
/// Desugar passes convert these into ImplFunctionCall nodes.
pub struct ResolvedImplMethod {
    pub trait_fqn: Fqn,
    pub for_type: Type,
    pub method_name: SymbolName,
    pub type_args: Vec<Type>,
}
```

```rust
Await {
    operand: Box<TypedExpr>,
    return_type: Type,
    and_then_method: ResolvedImplMethod,   // CHANGED from MangledName
    map_method: ResolvedImplMethod,        // CHANGED from MangledName
},
ForLoop {
    pattern: TypedPattern,
    iterable: Box<TypedExpr>,
    iterator_method: ResolvedImplMethod,   // CHANGED from MangledName
    iterator_type: Type,
    element_type: Type,
    body: Box<TypedExpr>,
},
AsyncBlock {
    body: Box<TypedExpr>,
    succeed_method: ResolvedImplMethod,    // CHANGED from MangledName
},
Try {
    operand: Box<TypedExpr>,
    unwrap_method: ResolvedImplMethod,     // CHANGED from MangledName
    unwrap_return_type: Type,
    return_type: Type,
    from_method: Option<ResolvedImplMethod>,  // CHANGED from Option<MangledName>
},
```

The desugar passes convert these into `ImplFunctionCall` nodes (or `TraitBoundFunctionCall` when `for_type` contains `TypeParameter`). For example, `desugar_for` converts `ForLoop { iterator_method, ... }` into an `ImplFunctionCall { trait_fqn: Iterable, for_type, method_name: iterator, ... }`.

### 4.5 `TypeDef` variants

Today, none of the `TypeDef` variants carry a `span`. Since `TypedModule` must now serve both LSP and codegen, all user-declared type defs need spans for go-to-definition, find-references, and hover on type names.

```rust
pub struct RecordTypeDef {
    pub fqn: Fqn,
    pub mangled_name: MangledName,
    pub type_params: Vec<TypeParamName>,    // NEW — empty for non-generic
    pub fields: Vec<(String, Type)>,        // may contain TypeParameter when generic
    pub span: Span,                         // NEW — declaration span
}

pub struct EnumTypeDef {
    pub fqn: Fqn,
    pub mangled_name: MangledName,
    pub type_params: Vec<TypeParamName>,    // already exists, but populated correctly now
    pub variants: Vec<EnumVariantDef>,      // may contain TypeParameter when generic
    pub span: Span,                         // NEW — declaration span
}

pub struct ClassTypeDef {
    pub fqn: Fqn,
    pub mangled_name: MangledName,
    pub type_params: Vec<TypeParamName>,    // NEW — empty for non-generic
    pub fields: Vec<ClassFieldDef>,
    pub is_final: bool,
    pub is_abstract: bool,
    pub is_sealed: bool,
    pub parent_mangled_name: Option<MangledName>,
    pub vtable_methods: Vec<(String, MangledName)>,
    pub hierarchy_root_mangled: MangledName,
    pub constructor_params: Vec<TypedParam>,
    pub initializer: Vec<TypedExpr>,
    pub initializer_fields: Vec<(String, Type)>,
    pub extends_args: Option<Vec<TypedExpr>>,
    pub span: Span,                         // NEW — declaration span
}
```

`ArrayTypeDef`, `FunctionSigTypeDef`, and `TraitObjectTypeDef` are compiler-synthesized (not user-declared), so they don't need spans.

### 4.6 `TraitObjectCoerce` change

```rust
TraitObjectCoerce {
    inner: Box<TypedExpr>,
    trait_mangled_name: MangledName,
    concrete_type: Type,
    vtable_methods: Vec<(TraitObjectMemberName, MangledName, Vec<Type>)>,  // CHANGED — added type_args
},
```

Each vtable method entry now carries `type_args` so monomorphize can instantiate generic impl methods used in vtable slots.

### 4.7 Mangled names for generic definitions

Generic definitions need a stable `MangledName` that doesn't include type arguments:

| Definition | MangledName |
|------------|-------------|
| `function identity<T>(x: T): T` | `pkg.identity` |
| `record Pair<A, B>` | `pkg.Pair` |
| `class Box<T>` | `pkg.Box` |
| `extension method on Array<T>` | `pkg.Array.method` |
| `implement<T> Display for Box<T>` method `display` | `pkg.Display$Box.display$generic` |
| `implement Into<Array<T>> for MyType` method `into<T>` | `pkg.Into$MyType.into` (concrete impl, generic method) |

These base names are distinct from instantiated names (which include `$TypeArg` and `#TypeArg` suffixes). The monomorphize pass generates the instantiated names.

When a generic function or type is registered during inference, the `MangledName` stored in the `TypedFunction`/`TypeDef` is this base name. The call site's `name` field also uses the base name, and the `type_args` field carries the concrete types.

**Exception:** Non-generic overloads still use `MangledName::for_function(fqn, &param_types)` as today, since they are disambiguated by parameter types. Generic functions are disambiguated by `type_args` instead.

---

## 5. Collect and Registry Changes

### 5.1 Unified `CollectedImplementBlock` in the registry

Today, the collect phase splits each implement block into 2-3 separate registrations:
- `register_trait_impl_method(type_fqn, method_name, FunctionSignature)` — per non-generic method
- `register_generic_trait_impl_method(type_fqn, method_name, GenericTraitImplMethodDef)` — per generic method
- `register_trait_impl(trait_fqn, type_fqn, TraitImplInfo)` — once per block

In the new design, collect registers the entire implement block as a single unit:

```rust
/// An implement block as collected from source. Lives in the Registry.
/// Contains raw AST bodies — inference produces TypedImplementBlock with typed bodies.
pub struct CollectedImplementBlock {
    pub trait_fqn: Fqn,
    pub type_fqn: Fqn,
    pub for_type: Type,                         // concrete or with TypeParameter
    pub type_params: Vec<TypeParamName>,         // block-level (empty for non-generic)
    pub trait_type_args: Vec<Type>,              // may contain TypeParameter
    pub trait_bounds: TraitBounds,
    pub methods: Vec<CollectedImplMethod>,
    pub properties: Vec<CollectedImplMethod>,
    pub associated_type_defs: BTreeMap<String, (Vec<TypeParamName>, Type)>,
    pub span: Span,
    pub source_file: FilePath,
    pub package: PackagePath,
}

/// A method or property within a collected implement block.
pub struct CollectedImplMethod {
    pub name: SymbolName,
    pub visibility: Visibility,
    pub method_type_params: Vec<TypeParamName>,  // method-level (empty for non-generic methods)
    pub params: Vec<(String, Type)>,             // Self already resolved to for_type
    pub return_type: Type,
    pub body: Expr,                              // raw AST body
    pub span: Span,
    pub is_async: bool,
    pub is_intrinsic: bool,
    pub is_property: bool,
    pub trait_bounds: TraitBounds,                // merged block-level + method-level bounds
}
```

### 5.2 Unified `CollectedExtensionBlock` in the registry

Today, extensions have the same fragmentation:

| Storage | Value |
|---------|-------|
| `named_extensions` | `BTreeMap<Fqn, NamedExtensionInfo>` — non-generic extension methods as `FunctionSignature` |
| `named_generic_extensions` | `BTreeMap<Fqn, NamedGenericExtensionInfo>` — generic extension methods as `GenericExtensionMethodDef` |

Unified into:

```rust
pub struct CollectedExtensionBlock {
    pub ext_fqn: Fqn,
    pub for_type: Type,                          // concrete or with TypeParameter
    pub type_params: Vec<TypeParamName>,          // block-level (empty for non-generic)
    pub trait_bounds: TraitBounds,
    pub methods: Vec<CollectedExtMethod>,
    pub properties: Vec<CollectedExtMethod>,
    pub span: Span,
    pub source_file: FilePath,
    pub package: PackagePath,
}

pub struct CollectedExtMethod {
    pub name: SymbolName,
    pub visibility: Visibility,
    pub method_type_params: Vec<TypeParamName>,
    pub params: Vec<(String, Type)>,
    pub return_type: Type,
    pub body: Expr,
    pub span: Span,
    pub is_async: bool,
    pub is_intrinsic: bool,
    pub is_property: bool,
    pub trait_bounds: TraitBounds,
}
```

### 5.3 Registry storage

Replace the fragmented maps with unified storages:

```rust
// OLD:
trait_impl_methods: BTreeMap<Fqn, TraitImplMethods>,
generic_trait_impl_methods: BTreeMap<Fqn, GenericTraitImplMethods>,
trait_impls: BTreeMap<(Fqn, Fqn), Vec<TraitImplInfo>>,
named_extensions: BTreeMap<Fqn, NamedExtensionInfo>,
named_generic_extensions: BTreeMap<Fqn, NamedGenericExtensionInfo>,

// NEW:
implement_blocks: Vec<CollectedImplementBlock>,
extension_blocks: Vec<CollectedExtensionBlock>,
```

### 5.4 Registry lookup methods

The existing lookup methods change to query the unified storages:

**Implement blocks:**

```rust
/// Check if a (trait, type) implementation pair exists.
pub fn has_trait_impl(&self, trait_fqn: &Fqn, type_fqn: &Fqn) -> bool {
    self.implement_blocks.iter().any(|b| b.trait_fqn == *trait_fqn && b.type_fqn == *type_fqn)
}

/// Find implement blocks for a given type and method name.
pub fn find_impl_method(
    &self,
    type_fqn: &Fqn,
    method_name: &SymbolName,
) -> Vec<(&CollectedImplementBlock, &CollectedImplMethod)> {
    self.implement_blocks.iter()
        .filter(|block| block.type_fqn == *type_fqn)
        .flat_map(|block| {
            block.methods.iter().chain(block.properties.iter())
                .filter(|m| m.name == *method_name)
                .map(move |m| (block, m))
        })
        .collect()
}

/// Find the implement block for a (trait, type) pair.
pub fn find_impl_block(&self, trait_fqn: &Fqn, type_fqn: &Fqn) -> Option<&CollectedImplementBlock> {
    self.implement_blocks.iter().find(|b| b.trait_fqn == *trait_fqn && b.type_fqn == *type_fqn)
}
```

**Extension blocks:**

```rust
/// Find extension methods for a given type and method name.
pub fn find_ext_method(
    &self,
    type_fqn: &Fqn,
    method_name: &SymbolName,
) -> Vec<(&CollectedExtensionBlock, &CollectedExtMethod)> {
    self.extension_blocks.iter()
        .filter(|block| block.for_type.try_to_fqn().as_ref() == Some(type_fqn))
        .flat_map(|block| {
            block.methods.iter().chain(block.properties.iter())
                .filter(|m| m.name == *method_name)
                .map(move |m| (block, m))
        })
        .collect()
}

/// Find the extension block by its FQN.
pub fn find_ext_block(&self, ext_fqn: &Fqn) -> Option<&CollectedExtensionBlock> {
    self.extension_blocks.iter().find(|b| b.ext_fqn == *ext_fqn)
}

/// Get all extension blocks for a given type (for LSP).
pub fn ext_blocks_for_type(&self, type_fqn: &Fqn) -> Vec<&CollectedExtensionBlock> {
    self.extension_blocks.iter()
        .filter(|b| b.for_type.try_to_fqn().as_ref() == Some(type_fqn))
        .collect()
}
```

**Performance note:** With `Vec` storages, lookups are O(n) scans. If this becomes a bottleneck, add index maps (`BTreeMap<Fqn, Vec<usize>>` keyed by `type_fqn`) that point into the vecs. For now, O(n) is fine — the number of blocks in a typical package is small.

### 5.5 Collect phase changes

**Implement blocks:** The `collect_implement` and `collect_generic_implement` functions merge into a single flow:

```
collect_implement(impl_decl):
    resolve type_params, for_type, trait, trait_type_args (same validation as today)
    for each method in impl_decl.methods:
        validate against trait signature (param count, types, return type — same as today)
        build CollectedImplMethod { name, params, return_type, body, ... }
    for each property in impl_decl.properties:
        validate against trait signature (same as today)
        build CollectedImplMethod { name, params, return_type, body, is_property: true, ... }
    register_implement_block(CollectedImplementBlock { trait_fqn, type_fqn, methods, properties, ... })
```

**Extension blocks:** Same pattern — `collect_extension` and `collect_generic_extension` merge:

```
collect_extension(ext_decl):
    resolve type_params, for_type, trait_bounds (same validation as today)
    for each method in ext_decl.methods:
        build CollectedExtMethod { name, params, return_type, body, ... }
    for each property in ext_decl.properties:
        build CollectedExtMethod { name, params, return_type, body, is_property: true, ... }
    register_extension_block(CollectedExtensionBlock { ext_fqn, for_type, methods, properties, ... })
```

The distinction between generic and non-generic blocks disappears at the collect level in both cases. A non-generic block just has `type_params: vec![]`. All validation stays in collect.

**What's removed from collect:**
- `MangledName` computation for impl/extension methods — deferred to monomorphize
- `FunctionSignature` creation for non-generic methods — replaced by `CollectedImplMethod`/`CollectedExtMethod`
- Separate `collect_generic_implement`/`collect_generic_extension` functions — merged
- `register_trait_impl_method`, `register_generic_trait_impl_method` — replaced by `register_implement_block`
- `register_named_extension`, `register_named_generic_extension` — replaced by `register_extension_block`

**What stays in collect:**
- All validation (trait conformance, parameter matching, return types, associated types)
- `Self` substitution via `substitute_self`

### 5.6 How inference consumes the new registry

During method resolution, inference calls registry lookups that return block + method pairs instead of `FunctionSignature` or `GenericTraitImplMethodDef`/`GenericExtensionMethodDef`:

**Implement block method resolution:**
- Today: `lookup_trait_impl_methods(type_fqn, method_name)` → `Vec<FunctionSignature>` → use `sig.return_type` and `sig.mangled_name`
- New: `find_impl_method(type_fqn, method_name)` → `Vec<(block, method)>` → use `method.return_type`. No mangled name needed — emit `ImplFunctionCall { trait_fqn: block.trait_fqn, for_type: block.for_type, method_name, args, type_args: vec![] }`

**Generic impl method resolution:**
- Today: `lookup_generic_trait_impl_methods(type_fqn, method_name)` → `Vec<GenericTraitImplMethodDef>` → unify, instantiate
- New: `find_impl_method(type_fqn, method_name)` → `Vec<(block, method)>` → unify receiver against `block.for_type` to infer block-level type args, unify arg types for method-level type args → emit `ImplFunctionCall { ..., type_args }`

**Extension method resolution:**
- Today: `lookup_named_extension` / `lookup_generic_extension` → `FunctionSignature` or `GenericExtensionMethodDef`
- New: `find_ext_method(type_fqn, method_name)` → `Vec<(block, method)>` → unify if generic, emit `ExtFunctionCall { ext_fqn: block.ext_fqn, for_type: block.for_type, method_name, args, type_args }`

The unification logic stays in inference but works on the unified block types. The key change: it emits `ImplFunctionCall` / `ExtFunctionCall` instead of calling `instantiate_generic_function`.

### 5.6 Registry merge

When merging registries (dependency → accumulated), `implement_blocks` are concatenated:

```rust
fn merge(&self, other: &Registry) -> Registry {
    let mut merged = self.clone();
    merged.implement_blocks.extend(other.implement_blocks.iter().cloned());
    // ... merge other fields ...
    merged
}
```

Deduplication is handled by the same (trait_fqn, type_fqn, trait_type_args) uniqueness check that `register_trait_impl` does today.

---

## 6. Inference Changes

### 6.1 Remove `typechecking_only`

Today, generic bodies are inferred with `typechecking_only = true` purely for error checking, then the typed body is discarded. In the new design:

- `typechecking_only` is removed entirely
- Generic bodies are inferred **once**, producing a `TypedFunction` with `type_params` set and body containing `TypeParameter` types
- The typed body is **kept** — inserted into `typed_functions` with the base mangled name

### 6.2 Remove `in_instantiation`

Today, `in_instantiation` is set when re-inferring a generic body with concrete types. It affects match exhaustiveness (skip unreachable arms instead of reporting errors). In the new design:

- `in_instantiation` is removed
- Match exhaustiveness on `TypeParameter` types is handled by treating them as open types (any variant could match)
- The monomorphize pass does not re-run exhaustiveness — it was validated on the generic body

### 6.3 Remove `instantiated: BTreeSet`

The deduplication set moves to the monomorphize pass. Inference doesn't need it because it produces exactly one entry per definition.

### 6.4 Change `current_type_params` role

Currently maps `TypeParamName → concrete Type`. In the new design:

- During generic body inference: maps `TypeParamName → Type::TypeParameter(name, bounds)` — the type parameter resolves to itself
- This is set once when entering a generic function/class body and cleared when leaving
- `resolve_type_expr` for a type parameter name looks up `current_type_params` and gets `Type::TypeParameter` back

### 6.5 Generic function inference (new flow)

```
infer_function(func):
    if func.type_params.is_empty():
        // Non-generic — same as today
        infer body, register TypedFunction
    else:
        // Generic — infer body with TypeParameter types
        set current_type_params = { param → TypeParameter(param) }
        infer body (types may contain TypeParameter)
        register TypedFunction with type_params set, base mangled name, original span
        clear current_type_params
```

### 6.6 Generic call sites (new flow)

When resolving a generic function call:

```
resolve_generic_function(fqn, arg_types, explicit_type_args):
    lookup GenericFunctionDef from registry
    infer type_args (from explicit args or unification with arg types)
    compute concrete return type via substitution
    return ResolvedFunction::Generic {
        base_mangled_name,      // base name without type args
        return_type,            // concrete return type (or with TypeParameter if in generic context)
        type_args,              // the resolved type arguments
    }
```

The call site is emitted as:

```rust
TypedExprKind::FunctionCall {
    name: base_mangled_name,
    args: typed_args,
    type_args: resolved_type_args,
}
```

### 6.7 Generic type references (new flow)

When encountering `Option<Int32>` in a type expression:

```
resolve_generic_type(fqn, type_args):
    lookup signature from registry
    check trait bounds
    compute base mangled name (without type args)
    return Type::GenericRecord/Enum/Class {
        fqn,
        mangled_name: base_mangled_name,
        type_args: with_variance,
    }
    // Do NOT create a TypeDef entry — the generic TypeDef already exists
```

The generic `TypeDef` (with `TypeParameter` fields) was registered when the generic type itself was inferred. Concrete `TypeDef` entries are created by the monomorphize pass.

### 6.8 `substitute_and_instantiate` → `substitute_type`

The function is renamed and simplified. It no longer triggers type/function instantiation — it just substitutes type parameters:

```rust
pub(super) fn substitute_type(ty: &Type, substitution: &TypeParamSubstitution) -> Type {
    let substituted = apply_substitution(substitution, ty);
    // Recursively substitute nested generic type args
    match &substituted {
        Type::GenericRecord { fqn, type_args, .. } => {
            let new_args = type_args.iter()
                .map(|(v, t)| (*v, substitute_type(t, substitution)))
                .collect();
            let mangled = MangledName::for_generic_type(fqn, &new_args.iter().map(|(_, t)| t).collect::<Vec<_>>());
            Type::GenericRecord { fqn: fqn.clone(), mangled_name: mangled, type_args: new_args }
        }
        // ... same for GenericEnum, GenericClass, Array, Function, Tuple, etc.
        _ => substituted,
    }
}
```

This is now a **pure function** — no `&mut self`, no side effects, no TypeDef registration.

### 6.9 Implement blocks as a first-class concept

#### The problem with the current approach

Implementing a trait is always a form of specialization — binding `Self` to a concrete type. Even non-generic implement blocks involve this substitution. Yet today, implement blocks have no representation in `TypedModule`:

- **Non-generic impl methods** are inferred and placed into `TypedModule.functions` as standalone functions keyed by `MangledName::for_impl_method(...)`. The relationship to the trait and implementing type is baked into the mangled name — there is no structured record.
- **Generic impl methods** exist only in the Registry as `GenericTraitImplMethodDef` with raw AST bodies. They appear in `TypedModule.functions` only when a call site triggers instantiation. **If an implement block is never used, its methods are never type-checked and never appear in the typed module.** The LSP cannot see them.

This means the LSP can't answer "what types implement Display?", can't navigate from a trait to its implementations, and can't offer completions/hover inside generic impl method bodies that haven't been instantiated yet.

#### Solution: `implement_blocks` field on `TypedModule`

Add implement blocks as a first-class concept in `TypedModule`:

```rust
pub struct TypedImplementBlock {
    pub trait_fqn: Fqn,
    pub type_fqn: Fqn,
    pub for_type: Type,                         // e.g., Type::Class for non-generic, may contain TypeParameter for generic
    pub type_params: Vec<TypeParamName>,         // block-level type params (empty for non-generic)
    pub trait_type_args: Vec<Type>,              // may contain TypeParameter for generic blocks
    pub trait_bounds: TraitBounds,
    pub methods: Vec<TypedImplMethod>,
    pub properties: Vec<TypedImplMethod>,
    pub span: Span,
}

pub struct TypedImplMethod {
    pub name: SymbolName,
    pub method_type_params: Vec<TypeParamName>,  // method-level type params (empty for non-generic methods)
    pub params: Vec<TypedParam>,                 // Self already resolved to for_type
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
    pub is_async: bool,
    pub visibility: Visibility,
}
```

```rust
pub struct TypedExtensionBlock {
    pub ext_fqn: Fqn,
    pub for_type: Type,
    pub type_params: Vec<TypeParamName>,
    pub trait_bounds: TraitBounds,
    pub methods: Vec<TypedExtMethod>,
    pub properties: Vec<TypedExtMethod>,
    pub span: Span,
}

pub struct TypedExtMethod {
    pub name: SymbolName,
    pub method_type_params: Vec<TypeParamName>,
    pub params: Vec<TypedParam>,
    pub return_type: Type,
    pub body: TypedExpr,
    pub span: Span,
    pub is_async: bool,
    pub visibility: Visibility,
}
```

```rust
pub struct TypedModule {
    pub main_function_fqn: Option<Fqn>,
    pub functions: BTreeMap<MangledName, TypedFunction>,
    pub globals: BTreeMap<MangledName, TypedGlobal>,
    pub types: BTreeMap<MangledName, TypeDef>,
    pub tests: Vec<TypedTest>,
    pub type_references: Vec<TypeReference>,
    pub implement_blocks: Vec<TypedImplementBlock>,   // NEW
    pub extension_blocks: Vec<TypedExtensionBlock>,   // NEW
}
```

#### How inference populates `implement_blocks`

**ALL implement blocks** — generic and non-generic — go into `implement_blocks`. Inference no longer puts impl methods directly into `functions`:

**Non-generic implement blocks:**
- Inference infers each method body with concrete types (Self resolved to `for_type` by the collect phase)
- Creates a `TypedImplementBlock` with `type_params: vec![]` and fully concrete method bodies
- Methods have `method_type_params: vec![]` (unless the method itself is generic — see below)

**Non-generic implement blocks with generic methods:**
- The method body is inferred with `TypeParameter` types for the method-level type params
- The `TypedImplMethod` has `method_type_params` set
- The enclosing `TypedImplementBlock` has `type_params: vec![]` (the block itself is concrete)

**Generic implement blocks:**
- Each method body is inferred with `TypeParameter` types for block-level (+ method-level) params
- The `TypedImplementBlock` has `type_params` set, `for_type` and `trait_type_args` containing `TypeParameter`
- **Bodies are always inferred** — whether or not any call site uses them. This ensures LSP coverage.

In all cases, inference does NOT insert impl methods into `typed_functions`. They live exclusively in `implement_blocks`.

#### How monomorphize expands `implement_blocks`

The monomorphize pass walks `implement_blocks` and produces concrete `TypedFunction` entries:

**Non-generic blocks, non-generic methods (trivial expansion):**
- Compute `MangledName::for_impl_method(trait_fqn, type_fqn, method_sym, trait_type_args)`
- Create `TypedFunction` from the `TypedImplMethod` with this name
- Insert into output `functions`
- This is not "monomorphization" in the traditional sense — it's the structural step of assigning mangled names and promoting methods to standalone functions

**Non-generic blocks, generic methods:**
- The method body has `TypeParameter` types
- Discovered at call sites via `type_args`
- Monomorphize substitutes method-level type params and creates concrete copies
- Compute `MangledName::for_impl_method(...).with_type_args(method_type_args)`

**Generic blocks:**
- Discovered at call sites via `type_args` (block-level + method-level combined)
- Monomorphize builds substitution from block's `type_params` + method's `method_type_params`
- Substitutes `TypeParameter` types in body, params, return type
- Computes concrete `type_fqn` and `trait_type_args` by substituting the block's patterns
- Compute `MangledName::for_impl_method(trait_fqn, concrete_type_fqn, method_sym, concrete_trait_type_args).with_type_args(type_args)`

#### Call site resolution (unchanged from current)

Inference still resolves which impl method to call at each call site:

- **Non-generic impl, non-generic method:** `resolve_trait_impl_method_for_type` finds a concrete match. The call site now references the implement block + method index (or a stable key) with empty `type_args`.
- **Generic impl / generic method:** `resolve_generic_trait_impl_instance` unifies receiver against `for_type`, infers type args. The call site carries `type_args`.

The key difference: instead of the call site referencing a `MangledName` (which for generic impls would require instantiation to compute), it references the **implement block identity** + `type_args`. Monomorphize computes the final `MangledName`.

**Implement block identity on call sites:** No `MangledName` is needed on the call site. `ImplFunctionCall` carries `trait_fqn`, `for_type`, `method_name`, and `type_args` — that's all monomorphize needs to find the implement block and expand the method. The `MangledName` is computed by monomorphize when it produces the concrete `TypedFunction`, not at the call site.

#### Vtable building

`TraitObjectCoerce.vtable_methods` references implement block methods. For each vtable slot:
- The entry stores the implement block key + method name + `type_args`
- Monomorphize expands the referenced methods and rewrites the entry with the final `MangledName`

#### Generic implement blocks with generic methods (nested generics)

```dovetail
implement<T> Functor for Box<T> =
    method map<U>(self, f: Function(T): U): Box<U> = Box(f(self.value))
```

Here `T` is block-level, `U` is method-level. The `TypedImplementBlock` has `type_params = [T]`. The `TypedImplMethod` has `method_type_params = [U]`.

At a call site like `box.map(fn)`, inference infers `T` from the receiver and `U` from the function argument. The call site carries `type_args = [Int32, String]` (block params first, then method params). Monomorphize splits these using `type_params.len()` and `method_type_params.len()`, builds a substitution for both, and expands.

#### LSP benefits

With `implement_blocks` as a first-class concept:

- **"Find implementations"** of a trait → scan `implement_blocks` for matching `trait_fqn`
- **"Go to implementation"** on a trait method → find the implement block for the receiver type, navigate to the method span
- **Hover** inside a generic impl body → types are available (with `TypeParameter` where generic)
- **Completions** inside generic impl bodies → trait bound resolution works on `TypeParameter` types
- **All impl methods are always type-checked** — even if never called at a concrete site

### 6.10 Extension blocks as a first-class concept

Extension blocks follow the same design as implement blocks. The same problems apply: generic extension methods aren't in `TypedModule` today, the grouping is lost, and the LSP can't answer "what extensions exist for this type?".

**`extension_blocks` field on `TypedModule`** — see `TypedExtensionBlock` and `TypedExtMethod` structs in §6.9.

**How inference populates `extension_blocks`:**

ALL extension blocks — generic and non-generic — go into `extension_blocks`. Inference no longer puts extension methods directly into `functions`:

- Non-generic extension blocks: methods are inferred with concrete types, stored in `TypedExtensionBlock` with `type_params: vec![]`
- Generic extension blocks: methods are inferred with `TypeParameter` types, stored with `type_params` set
- Generic methods within non-generic extensions: `TypedExtMethod` has `method_type_params` set

**How monomorphize expands `extension_blocks`:**

Same pattern as implement blocks:
- Non-generic block, non-generic methods: compute `MangledName::for_named_extension_method(package, ext_name, method_name, param_types)`, create `TypedFunction`, insert
- Generic cases: substitute type params, compute concrete mangled name, insert

**`ExtFunctionCall` variant** — see §4.3. Call sites to extension methods use `ExtFunctionCall` instead of `FunctionCall`, so monomorphize knows to look in `extension_blocks`.

### 6.11 Generic class methods

Generic class methods (methods with their own type parameters on a possibly non-generic class, or all methods on a generic class) follow the same pattern. They are stored in `ClassTypeSignature.generic_instance_methods` / `generic_static_methods`.

In the new design, inference produces `TypedFunction` entries for these with `type_params` set and bodies containing `TypeParameter` types.

### 6.12 What inference still validates on generic bodies

All type checking remains in inference:

- Type parameter bounds are checked (trait bounds, type constraints)
- Overload resolution works with `TypeParameter` types using trait bounds
- Binary/unary operators on `TypeParameter` types resolve via trait bounds
- Method calls on `TypeParameter` types resolve via trait bounds
- Pattern matching validates variant names and field counts (exhaustiveness handles `TypeParameter` as open)
- Return type compatibility
- Assignability between `TypeParameter` and concrete types (via bounds)

---

## 7. Monomorphize Pass

### 7.1 Entry point

```rust
pub fn monomorphize(module: TypedModule, registry: &Registry) -> TypedModule
```

Pure function — consumes the input module, produces a new concrete module. No mutation of shared state. Takes the pre-monomorphize `TypedModule` (with generic entries), returns a fully concrete `TypedModule` (no `TypeParameter` types, no generic entries, no `implement_blocks`/`extension_blocks` — everything expanded into `functions`).

### 7.2 Algorithm

```
1. Initialize:
   - output_functions: BTreeMap<MangledName, TypedFunction> = {}
   - output_types: BTreeMap<MangledName, TypeDef> = {}
   - instantiated: BTreeSet<MangledName> = {}
   - work_queue: Vec<MangledName> = [main, all test functions, all non-generic functions]

2. Seed:
   - Copy all non-generic functions and types from input module to output, add to work_queue
   - Expand all non-generic implement_blocks and extension_blocks into output_functions (trivial promotion)
   - Add all trivially promoted impl/ext methods to work_queue (they may contain generic call sites)

3. Fixpoint loop:
   while work_queue is not empty:
       take next function from work_queue
       walk its body looking for generic call sites:
         - FunctionCall with non-empty type_args → look up in functions
         - ImplFunctionCall → look up in implement_blocks
         - TraitBoundFunctionCall → substitute receiver, then look up in implement_blocks
         - ExtFunctionCall → look up in extension_blocks
       for each such call site:
           compute instantiated MangledName
           if not in instantiated set:
               instantiated.insert(mangled)
               look up generic body from the appropriate source
               clone body, substitute TypeParameter → concrete types
               register instantiated TypedFunction in output
               add to work_queue (instantiated generic may call other generics)
           produce concrete FunctionCall { name, args, type_args: vec![] } in the output

4. Same for types:
   walk all output functions, discover generic type references
   for each Type::GenericRecord/Enum/Class with concrete type args:
       compute instantiated MangledName
       look up generic TypeDef, clone, substitute, register

5. Return output TypedModule
```

### 7.3 Type substitution

The monomorphize pass needs a recursive substitution function:

```rust
fn substitute_expr(expr: &TypedExpr, subst: &TypeParamSubstitution) -> TypedExpr
fn substitute_type(ty: &Type, subst: &TypeParamSubstitution) -> Type
fn substitute_pattern(pat: &TypedPattern, subst: &TypeParamSubstitution) -> TypedPattern
```

These are straightforward recursive walks. `substitute_type` replaces `Type::TypeParameter` with the concrete type from the substitution. `substitute_expr` walks the expression tree, substituting types on every node and updating mangled names.

### 7.4 Mangled name computation

When instantiating a generic function `pkg.identity` with type args `[Int32]`:

1. Look up base `TypedFunction` by `MangledName("pkg.identity")`
2. Build substitution: `T → Int32`
3. Compute concrete param types after substitution
4. Compute instantiated name: `MangledName::for_function(fqn, &concrete_params).with_type_args(&type_args)`
5. This matches the current naming scheme — codegen sees the same names

### 7.5 Impl method expansion from `implement_blocks`

Monomorphize walks `TypedModule.implement_blocks` and produces concrete `TypedFunction` entries. This is one of its primary responsibilities.

**Algorithm:**

```
for each implement_block in module.implement_blocks:
    if block.type_params.is_empty():
        // Non-generic block: expand all methods immediately
        for each method in block.methods ++ block.properties:
            if method.method_type_params.is_empty():
                // Fully concrete: compute MangledName, create TypedFunction, insert
                let name = MangledName::for_impl_method(
                    block.trait_fqn, block.type_fqn, method.name, block.trait_type_args
                );
                output.functions.insert(name, TypedFunction { ... });
            else:
                // Method-level generics: register as generic TypedFunction
                // Will be instantiated when discovered at call sites
                generic_impl_methods.insert(base_key, (block, method));
    else:
        // Generic block: defer — will be instantiated when discovered at call sites
        generic_impl_blocks.insert(block_key, block);

// During fixpoint loop, when a call site references a generic impl method:
//   1. Look up the implement block + method by base key
//   2. Split type_args into block-level and method-level
//   3. Substitute block.for_type and block.trait_type_args to get concrete values
//   4. Substitute method body
//   5. Compute MangledName::for_impl_method(trait_fqn, concrete_type_fqn, method_sym, concrete_trait_type_args)
//   6. Insert concrete TypedFunction
```

**Vtable expansion:**
- Walk `TraitObjectCoerce` nodes
- For each vtable slot referencing an impl method: look up the implement block, expand the method, record the final `MangledName`

All metadata needed for mangled name computation (trait_fqn, type_fqn, trait_type_args) is on the `TypedImplementBlock` — no separate `ImplMethodInfo` needed.

### 7.6 Cross-package generics

Package A exports generic `identity<T>`. Package B calls `identity<Int32>`:

1. Package A's `TypedModule` contains `TypedFunction { name: "a.identity", type_params: [T], ... }`
2. Package A's `TypedModule` is merged into the accumulated module via `merge_from`
3. Package B's inference resolves the call, emits `FunctionCall { name: "a.identity", type_args: [Int32] }`
4. Monomorphize (run on the merged module) finds the generic `TypedFunction` from A, instantiates it

This works because `merge_from` propagates all entries — `functions`, `globals`, `types`, `tests`, `type_references`, **`implement_blocks`**, and **`extension_blocks`**. The monomorphize pass sees everything.

**`merge_from` updated signature:**

```rust
pub fn merge_from(&mut self, other: TypedModule) {
    self.functions.extend(other.functions);
    self.globals.extend(other.globals);
    self.types.extend(other.types);
    self.tests.extend(other.tests);
    self.type_references.extend(other.type_references);
    self.implement_blocks.extend(other.implement_blocks);   // NEW
    self.extension_blocks.extend(other.extension_blocks);   // NEW
}
```

### 7.7 Trait and extension method instantiation

Trait impl method expansion is handled through `implement_blocks` (§7.5). Extension method expansion follows the same pattern through `extension_blocks`.

For trait object vtable methods: the inference phase resolves vtable slots and records impl block references + `type_args` in `TraitObjectCoerce.vtable_methods`. The monomorphize pass walks these and expands the referenced implement block methods (§7.5).

### 7.8 Codegen contract

**Invariant:** Codegen never receives `ImplFunctionCall`, `ExtFunctionCall`, or `TraitBoundFunctionCall` nodes. Monomorphize resolves **all** of them to concrete `FunctionCall` nodes before the `TypedModule` reaches codegen. This is true from Phase 1 onward — even when generic functions are still instantiated by the old inference path, monomorphize handles the new call variants in all function bodies.

Codegen pattern matches on these variants should be `unreachable!()` as a safety check.

### 7.9 What monomorphize does NOT do

- No type checking — all errors were caught during inference
- No overload resolution — call targets are already resolved
- No trait bound checking — validated during inference
- No import scope handling — works on the flat `TypedModule`
- No exhaustiveness checking — validated during inference

---

## 8. Pipeline Wiring

### 8.1 New pipeline

```
Per package:
  Source → Collect (Registry) → Infer (TypedModule) → Rules → merge

After all packages merged:
  Desugar → Monomorphize → Codegen
              ↑
         (skipped for LSP/check mode)
```

`typecheck()` becomes purely: Collect → Infer → Rules. No transformations, no desugaring.

The desugar passes (`desugar_for`, `desugar_try`, `desugar_await`, `coerce_byname`, `capture`, `variance_cast`) move out of `typecheck()` and into the build pipeline. They are pure transformations on `TypedModule` — they don't do type checking.

### 8.2 `typecheck()` changes

```rust
pub fn typecheck(package_ast: &PackageAst, registry: &Registry) -> TypeCheckerResult {
    // Phase 1: Collect
    let package_registry = collect::collect(...);
    let merged_registry = registry.merge(&package_registry);

    // Phase 2: Infer
    let typed_module = infer::infer(...);

    // Phase 3: Rules
    rules::check_rules(&typed_module, ...);

    // No desugar, no monomorphize — those run after all packages are merged
    TypeCheckerResult { registry: package_registry, typed_module, diagnostics }
}
```

### 8.3 `build_project()` changes

```rust
fn build_project(...) -> ProjectResult {
    // 1. Compile each package: typecheck + merge
    for package in &project.packages {
        let tc_result = typechecker::typecheck(&package_ast, &accumulated_registry);
        accumulated_registry = accumulated_registry.merge(&tc_result.registry);
        accumulated_module.merge_from(tc_result.typed_module);
    }

    if mode == CheckOnly {
        // LSP/check mode: return pre-desugar, pre-monomorphize module
        return ProjectResult { typed_module: accumulated_module, ... };
    }

    // 2. Desugar passes (on the merged module, once)
    desugar_for::desugar_for_expressions(&mut accumulated_module);
    desugar_try::desugar_try_expressions(&mut accumulated_module);
    desugar_await::desugar_await_expressions(&mut accumulated_module);
    coerce_byname::coerce_byname_args(&mut accumulated_module);
    capture::analyze_captures(&mut accumulated_module);
    variance_cast::elaborate_variance_casts(&mut accumulated_module, &accumulated_registry);

    // 3. Monomorphize
    let concrete_module = monomorphize::monomorphize(accumulated_module, &accumulated_registry);

    // 4. Codegen
    let wasm = codegen::generate_component(&concrete_module, &test_exports);
    ProjectResult { ... }
}
```

### 8.4 `compile()` changes (single-file)

```rust
fn compile(source: &str) -> CompileResult {
    // ... parse, typecheck ...
    tc_result.typed_module.merge_from(prelude_module.clone());

    // Desugar
    desugar_for::desugar_for_expressions(&mut tc_result.typed_module);
    desugar_try::desugar_try_expressions(&mut tc_result.typed_module);
    desugar_await::desugar_await_expressions(&mut tc_result.typed_module);
    coerce_byname::coerce_byname_args(&mut tc_result.typed_module);
    capture::analyze_captures(&mut tc_result.typed_module);
    variance_cast::elaborate_variance_casts(&mut tc_result.typed_module, &merged_registry);

    // Monomorphize + Codegen
    let concrete_module = monomorphize::monomorphize(tc_result.typed_module, &merged_registry);
    let wasm = codegen::generate_component(&concrete_module, &[]);
}
```

### 8.5 LSP integration

The LSP server receives the **pre-desugar, pre-monomorphize** `TypedModule`. This gives it:

- The AST closest to the user's source code — no desugared `for` loops, no `try` expansion, no `await` chains
- `TypedFunction` entries for generic functions with original spans → go-to-definition works
- `type_params` on generic functions → hover shows generic signatures
- `type_args` / `ImplFunctionCall` / `ExtFunctionCall` / `TraitBoundFunctionCall` on call sites → hover can show concrete type bindings and trait resolution
- Full typed bodies for generic functions → completion, signature help work inside generic code
- `implement_blocks` and `extension_blocks` → "find implementations", "find extensions"
- `TypeDef` entries with `span` fields → go-to-definition on type names, find-references

**Symbol-at-position lookup:** Today, the LSP's `classify_position` / `find_node_at_position` only searches `TypedModule.functions`. With the new design, it must search **all** of:

1. `functions` — standalone functions, module functions, generic function bodies
2. `implement_blocks` → method bodies within implement blocks
3. `extension_blocks` → method bodies within extension blocks
4. `types` — type definitions (records, enums, classes) now have `span`, so go-to-definition on type names works
5. `globals` — global variable definitions
6. `tests` — test declaration bodies

**Resolving new call variants for hover/go-to-definition:** The new call variants (`ImplFunctionCall`, `TraitBoundFunctionCall`, `ExtFunctionCall`) don't carry a `MangledName`. The LSP resolves them to their target definition using the structured fields:

- `ImplFunctionCall` → find `TypedImplementBlock` matching `(trait_fqn, for_type)`, then find `TypedImplMethod` by `method_name` → navigate to `TypedImplMethod.span`
- `TraitBoundFunctionCall` → navigate to the trait method signature (in the trait definition), since the concrete impl is unknown
- `ExtFunctionCall` → find `TypedExtensionBlock` matching `ext_fqn`, then find `TypedExtMethod` by `method_name` → navigate to `TypedExtMethod.span`

---

## 9. Interaction with Other Systems

### 9.1 Desugar passes

The desugar passes (`desugar_for`, `desugar_try`, `desugar_await`, `coerce_byname`) run **after** all packages are merged and **before** monomorphize (see §8.3). They operate on the merged `TypedModule` which contains generic bodies:

- They must handle `TypeParameter` types correctly (pass them through without crashing)
- They convert `ForLoop`, `Await`, `Try`, `AsyncBlock` nodes into `ImplFunctionCall` / `TraitBoundFunctionCall` nodes using the `ResolvedImplMethod` info on those variants
- They must preserve the structured call variants and `type_args` on nodes they don't transform

**Critical:** All desugar passes must walk **all** bodies in `TypedModule`, not just `functions`. This includes:
- `functions` — standalone functions, module functions, generic function bodies
- `implement_blocks` → every `TypedImplMethod.body` in every block
- `extension_blocks` → every `TypedExtMethod.body` in every block
- `globals` — global initializers
- `tests` — test bodies

Today, passes iterate only `module.functions`, `module.globals`, and `module.tests`. They must be updated to also iterate `module.implement_blocks` and `module.extension_blocks`. Without this, method bodies inside implement/extension blocks are never desugared — a silent correctness bug.

### 9.2 Capture analysis

`capture::analyze_captures` runs after desugar and before monomorphize. It walks function bodies to find captured variables. In generic bodies, captured variable types may be `TypeParameter`. This is fine — capture analysis cares about variable identity (by name), not types.

**Must also walk** `implement_blocks` and `extension_blocks` method bodies — same requirement as desugar passes.

### 9.3 Variance cast pass

`variance_cast::elaborate_variance_casts` runs after desugar and before monomorphize. It takes `&accumulated_registry` which at this pipeline point (post-merge, pre-codegen) includes all symbols — public and internal — from all packages. This is correct because internal symbols haven't been stripped yet (stripping happens only when returning a registry to dependent packages, not on the accumulated registry used for build).

In generic bodies, variance cast points may involve `TypeParameter` types. The pass should skip these (no concrete types to cast between). Monomorphize will produce concrete types, and variance casts on instantiated bodies can be elaborated either:

- **(A)** By running variance_cast again after monomorphize, or
- **(B)** By having monomorphize apply variance casts during substitution

Option A is cleaner (separation of concerns). The variance cast pass becomes: pre-monomorphize (handle concrete code) + post-monomorphize (handle newly instantiated code).

**Must also walk** `implement_blocks` and `extension_blocks` method bodies — same requirement as desugar passes.

### 9.4 Rules phase

`rules::check_rules` runs inside `typecheck()`, on the per-package `TypedModule` — before merging and before desugar. It validates constraints on all code including generic bodies. This is correct — type rules should hold for all possible type arguments, not just instantiated ones.

**Must also walk** `implement_blocks` and `extension_blocks` method bodies to validate constraints inside impl/ext methods.

### 9.5 Selective type erasure

The selective erasure design (`variance-type-erasure-design.md`) is orthogonal:

- Erasure happens in **codegen**, mapping concrete mangled names to erased WASM types
- Monomorphize produces concrete types and mangled names — same as today
- The codegen erased-type-index mapping works on monomorphize output

If anything, the separation makes erasure easier: the monomorphize pass could be extended to compute erased mangled names alongside concrete ones, feeding codegen the mapping directly.

### 9.6 Package caching

Today, package caching stores `TypeCheckerResult` (registry + typed module + diagnostics). With the new design:

- Cached `TypedModule` includes generic `TypedFunction`/`TypeDef` entries
- When a downstream package triggers monomorphization, it needs the upstream generic bodies
- This works naturally: cached modules are merged via `merge_from`, making generic bodies available

---

## 10. Handling Edge Cases

### 10.1 Match exhaustiveness on TypeParameter

Today, match exhaustiveness is checked on concrete types (after instantiation). In the new design, generic bodies have `TypeParameter` subjects. The exhaustiveness checker needs to handle this:

- `TypeParameter` is treated as an **opaque type** — only wildcard/variable patterns are valid
- If the type parameter has a trait bound that reveals variants (e.g., an enum trait), those variants can be matched
- In practice, most generic match expressions use trait-bound methods, not pattern matching on the type parameter itself

### 10.2 Closures with deferred type inference

Closures are sometimes re-inferred when their parameter types become known from context. In generic bodies, closure parameter types may involve `TypeParameter`. The re-inference must handle `TypeParameter` types without triggering instantiation.

Today this works because `typechecking_only` suppresses instantiation. In the new design, inference never instantiates, so this is naturally handled.

### 10.3 Recursive generic functions

A generic function `f<T>` that calls itself with a different type argument (e.g., `f<Array<T>>`) creates an infinite chain of instantiations: `f<Int32>`, `f<Array<Int32>>`, `f<Array<Array<Int32>>>`, etc. Each produces a new concrete type combination, so the `instantiated` deduplication set cannot bound it.

**Mitigation:** The monomorphize fixpoint loop enforces a **recursion depth limit** (e.g., 64 levels of nested generic instantiation). When a single generic definition is instantiated more times than the limit, monomorphize emits a compiler error: "generic instantiation depth exceeded for `f` — possible infinite recursion in type arguments." This is the same approach used by Rust's monomorphization.

### 10.4 Trait object coercion in generic bodies

When a value of type `T` (type parameter with trait bound `Display`) is coerced to `dyn Display`, inference needs to emit `TraitObjectCoerce`. In generic bodies, the concrete type is unknown. Options:

- **(A)** Emit `TraitObjectCoerce` with `concrete_type: TypeParameter(T)` and `vtable_methods` containing base mangled names with `type_args` including `TypeParameter`. Monomorphize substitutes the concrete type and resolves vtable entries.
- **(B)** Defer `TraitObjectCoerce` emission to monomorphize.

Option A is simpler — the inference emits the coercion node, monomorphize fills in concrete details.

Note: today, `TraitObjectCoerce` vtable building calls `find_impl_method_mangled_name` which may trigger generic impl method instantiation. In the new design, when the concrete type is known (non-`TypeParameter`), the vtable method base names + type_args are recorded. When the concrete type is `TypeParameter`, vtable methods cannot be resolved — the `vtable_methods` list is left with placeholder entries that monomorphize fills in after substituting the concrete type. This is the one case where monomorphize must do resolution-like work (finding the correct impl method for a concrete type), but the registry provides all the information needed.

### 10.5 Binary operators on TypeParameter

Today, `typechecking_only` mode has special handling for binary operators on `TypeParameter` types: it asserts `typechecking_only` and returns a placeholder. In the new design:

- Inference resolves the operator to the trait method (e.g., `Addable.add`) via trait bounds
- Emits a `TraitBoundFunctionCall { trait_fqn: Addable, method_name: add, args: [a, b], type_args: [] }`
- Monomorphize substitutes the receiver's `TypeParameter` to a concrete type, then resolves the implement block and expands the method

### 10.6 Generic module/static globals

Today, generic module globals are instantiated during inference. In the new design:

- Generic module globals produce `TypedGlobal` with `type_params`
- Call sites (`GlobalRef`) carry `type_args`
- Monomorphize instantiates them

### 10.7 `pending_trait_instantiations` and eager vtable instantiation

Today, when `is_assignable` encounters a trait object coercion (e.g., assigning `Box<Int32>` to `dyn Display`), it records a `(concrete_type, trait_fqn, trait_type_args)` tuple in `pending_trait_instantiations`. After each file's declarations are processed, `drain_pending_trait_instantiations` iterates these tuples and eagerly instantiates all trait impl methods for that (type, trait) pair — including calling `find_impl_method_mangled_name` which may trigger generic impl body instantiation.

This mechanism serves two purposes:
1. **Ensure vtable methods exist** in `typed_functions` before codegen needs them
2. **Handle generic impl blocks** — where the method body must be instantiated with the concrete receiver type

**New design:**
- `pending_trait_instantiations` is **removed** from inference
- Trait object coercion is still detected during inference, but vtable method resolution is deferred
- The `TraitObjectCoerce` node carries `vtable_methods` with base mangled names + `type_args` (for concrete receivers) or with `TypeParameter` placeholders (for generic receivers)
- **Monomorphize** handles eager vtable method instantiation: when it encounters a `TraitObjectCoerce` node, it ensures all referenced vtable methods are instantiated
- For generic receivers (inside generic bodies), monomorphize fills in the concrete vtable methods after substituting the type parameter

This simplifies inference significantly — it no longer has a side-channel (`RefCell<Vec<...>>`) for deferred work.

---

## 11. Implementation Plan

### 11.1 Strategy

The refactoring ports **one feature area at a time**, end-to-end through the pipeline (collect → inference → monomorphize → codegen). Each area is fully ported before moving to the next. At every step boundary, all ~1,900 integration tests pass.

The ordering is driven by self-containedness and dependency:

1. **Implement blocks** — most self-contained; own registry storage, own collect code, own inference paths. Porting them first forces creation of the monomorphize infrastructure.
2. **Extension blocks** — same pattern as implement blocks, smaller scope.
3. **Generic types** — records, enums, classes. Relatively straightforward substitution.
4. **Generic functions** — most deeply woven through inference. By this point, monomorphize already handles impl blocks, extensions, and types.
5. **Pipeline restructuring** — move desugar passes out of `typecheck()`, final wiring.
6. **Cleanup** — dead code removal.

Each feature port follows the same pattern:
1. Add data structures for this area (new variants, fields, structs)
2. Change collect/registry for this area
3. Change inference to emit new representation for this area
4. Remove old instantiation code for this area
5. Add monomorphize handling for this area
6. All tests pass

### 11.2 Phase 1 — Implement blocks

**Goal:** Port trait implementation blocks end-to-end. After this phase, inference no longer instantiates impl methods — monomorphize does.

**Step 1a — Data structures:**

1. Add `TypedImplementBlock`, `TypedImplMethod` structs
2. Add `implement_blocks: Vec<TypedImplementBlock>` to `TypedModule` (initially empty)
3. Add `ImplFunctionCall`, `ImplFunctionRef`, `TraitBoundFunctionCall` variants to `TypedExprKind`
4. Add `ResolvedImplMethod` struct for desugar helper variants
5. Update all pattern matches on `TypedExprKind` across desugar passes, codegen, rules, capture, variance_cast to handle new variants (unreachable for now)
6. Update `TypedModule::merge_from` to extend `implement_blocks`
7. Add `type_args: Vec<Type>` to `TraitObjectCoerce.vtable_methods` entries (default `vec![]`)

**Step 1b — Unified registry for implement blocks:**

8. Add `CollectedImplementBlock`, `CollectedImplMethod` structs
9. Replace `trait_impl_methods`, `generic_trait_impl_methods`, `trait_impls` with `implement_blocks: Vec<CollectedImplementBlock>` in `Registry`
10. Add lookup methods: `find_impl_method`, `find_impl_block`, `has_trait_impl`
11. Merge `collect_implement` / `collect_generic_implement` into unified flow
12. Update inference consumers to use new lookup methods
13. Update registry merge to concatenate `implement_blocks`

**Step 1c — Inference changes:**

14. Infer ALL implement block method bodies (generic and non-generic), produce `TypedImplementBlock` entries in `typed_module.implement_blocks`
15. Inference emits `ImplFunctionCall` instead of `FunctionCall` at trait impl call sites — with `trait_fqn`, `for_type`, `method_name`, `type_args`
16. Inference emits `TraitBoundFunctionCall` for calls through trait bounds on type parameters (operators, method calls, property access on `TypeParameter` types)
17. Stop inserting impl methods into `typed_functions` — they live exclusively in `implement_blocks`
18. Remove impl-related paths from `instantiate_generic_function` (no more `MethodKind::ImplMethod`)
19. Remove `pending_trait_instantiations` — vtable method resolution deferred to monomorphize
20. Populate `type_args` on `TraitObjectCoerce.vtable_methods`
21. Update `ForLoop`, `Await`, `Try`, `AsyncBlock` to carry `ResolvedImplMethod` instead of `MangledName` for their method references
22. Update desugar passes (`desugar_for`, `desugar_try`, `desugar_await`) to emit `ImplFunctionCall` instead of `FunctionCall` when expanding these variants
23. Update desugar passes to walk `implement_blocks` method bodies
24. Update `rules::check_rules` to walk `implement_blocks` method bodies
25. Update `capture::analyze_captures` to walk `implement_blocks` method bodies
26. Update `variance_cast` to walk `implement_blocks` method bodies

**Step 1d — Monomorphize infrastructure + impl block handling:**

27. Create `dovetail/src/typechecker/monomorphize.rs`
28. Implement core infrastructure: `substitute_expr`, `substitute_type`, `substitute_pattern`, fixpoint loop skeleton, instantiation depth limit
29. Implement impl block expansion: trivially promote non-generic blocks, instantiate generic blocks from call sites
30. Handle `ImplFunctionCall` → look up `TypedImplementBlock`, substitute, produce concrete `FunctionCall`
31. Handle `TraitBoundFunctionCall` → substitute receiver type parameter, resolve implement block, produce concrete `FunctionCall`
32. Handle `TraitObjectCoerce` vtable expansion from implement blocks
33. Wire monomorphize into the pipeline (after desugar, before codegen)

**Step 1e — LSP updates:**

34. Update `find_node_at_position` / `classify_position` to search `implement_blocks` method bodies
35. Handle `ImplFunctionCall` in hover: show trait, implementing type, method signature
36. Handle `TraitBoundFunctionCall` in hover: show trait bound and method signature
37. Handle `ImplFunctionCall` in go-to-definition: navigate to `TypedImplMethod.span`
38. Handle `TraitBoundFunctionCall` in go-to-definition: navigate to trait method signature
39. Add LSP integration tests for impl block navigation

**Exit criteria:** All tests pass (including LSP integration tests). Impl methods flow through `implement_blocks` → monomorphize → concrete `functions`. Old impl instantiation code is gone. Monomorphize infrastructure exists and handles impl blocks.

### 11.3 Phase 2 — Extension blocks

**Goal:** Port named extension blocks end-to-end. Same pattern as Phase 1.

**Changes:**

1. Add `TypedExtensionBlock`, `TypedExtMethod` structs
2. Add `extension_blocks: Vec<TypedExtensionBlock>` to `TypedModule`
3. Add `ExtFunctionCall`, `ExtFunctionRef` variants to `TypedExprKind`
4. Update pattern matches for new variants
5. Update `TypedModule::merge_from` to extend `extension_blocks`
6. Add `CollectedExtensionBlock`, `CollectedExtMethod` to registry
7. Replace `named_extensions`, `named_generic_extensions` with `extension_blocks: Vec<CollectedExtensionBlock>`
8. Merge `collect_extension` / `collect_generic_extension`
9. Inference: populate `extension_blocks`, emit `ExtFunctionCall` instead of `FunctionCall`
10. Stop inserting extension methods into `typed_functions`
11. Remove extension-related paths from `instantiate_generic_function`
12. Update desugar passes, rules, capture, variance_cast to walk `extension_blocks`
13. Monomorphize: expand extension blocks, handle `ExtFunctionCall`

14. Update `find_node_at_position` to search `extension_blocks` method bodies
15. Handle `ExtFunctionCall` in hover and go-to-definition: navigate to `TypedExtMethod.span`
16. Add LSP integration tests for extension block navigation

**Exit criteria:** All tests pass (including LSP integration tests). Extension methods flow through `extension_blocks` → monomorphize → concrete `functions`.

### 11.4 Phase 3 — Generic types

**Goal:** Port generic type definitions (records, enums, classes). After this phase, inference produces one `TypeDef` per generic type definition (with `TypeParameter` fields), and monomorphize creates concrete `TypeDef` entries.

**Changes:**

1. Add `type_params: Vec<TypeParamName>` to `RecordTypeDef`, `ClassTypeDef` (default `vec![]`; `EnumTypeDef` already has it)
2. Add `span: Span` to `RecordTypeDef`, `EnumTypeDef`, `ClassTypeDef`
3. Add `type_args: Vec<Type>` to `RecordCreate`, `EnumCreate`, `EnumVariantRecordCreate`, `ClassNew`, `RecordWith` (default `vec![]`)
4. Inference: register one `TypeDef` per generic type with `type_params` set and fields containing `TypeParameter`. Populate `span` with declaration span.
5. Inference: populate `type_args` on `RecordCreate`, `EnumCreate`, etc. at generic call sites
6. Inference: stop creating concrete `TypeDef` entries during instantiation (`instantiate_generic_record`, `instantiate_generic_enum`, `instantiate_generic_class`)
7. Remove `instantiate_generic_record`, `instantiate_generic_enum`, `instantiate_generic_class`
8. Monomorphize: discover generic type references in output functions, create concrete `TypeDef` entries by cloning the generic def and substituting `TypeParameter` → concrete types

9. Update `find_node_at_position` to search `types` for `TypeDef` entries with `span`
10. Go-to-definition on type names navigates to `RecordTypeDef.span` / `EnumTypeDef.span` / `ClassTypeDef.span`
11. Add LSP integration tests for type definition navigation

**Step 3b — Generic module/static globals:**

12. Add `type_params: Vec<TypeParamName>` to `TypedGlobal` (default `vec![]`) ✅
13. Add `type_args: Vec<Type>` to `GlobalRef` (default `vec![]`) ✅
14. Populate `type_args` on `GlobalRef` at call sites (`resolve_generic_module_global`, `lookup_global`) ✅
15. Add `instantiate_generic_globals` pass and `collect_generic_global_refs` walker in monomorphize ✅
16. Add `GlobalRef` type_args handling in `substitute_types_in_expr` ✅
17. Strip generic global templates after monomorphize ✅

**Deferred to Phase 4:** Generic module globals still use eager per-instantiation inference (`instantiate_generic_global` infers the AST body with concrete type args each time). The template approach (infer once with `TypeParameter` types, substitute in monomorphize) does not work yet because global initializers may contain concrete values only valid for specific type args (e.g., `Box<T> { value = 0i32 }` is only valid when `T=Int32`). This is the same fundamental problem as generic function bodies — both require Phase 4's ability to infer expression bodies with `TypeParameter` types. Once Phase 4 solves this for functions, globals get the same treatment and `instantiate_generic_global` can be removed.

**Exit criteria:** All tests pass (including LSP integration tests). Generic `TypeDef` entries have `type_params` and `TypeParameter` fields. Generic `TypedGlobal` entries have `type_params`. Monomorphize produces concrete `TypeDef` and `TypedGlobal` entries for all reachable instantiations.

### 11.5 Phase 4 — Generic functions

**Goal:** Port generic function instantiation. After this phase, inference produces one `TypedFunction` per generic function (with `TypeParameter` body), and monomorphize creates concrete copies. This is the final removal of old instantiation machinery.

**Remaining eager instantiation from Phase 3:**

After Phase 3, two key functions still perform eager TypeDef/body instantiation during inference:

- **`substitute_and_instantiate`** (66 call sites): Applies type substitution (`TypeParameter(name) → concrete_type`) AND triggers TypeDef registration as a side effect. Records and enums are already deferred to monomorphize (they return as-is after substitution). However, several types still require eager handling:
  - **Classes** → calls `infer_generic_class()` which creates `ClassTypeDef` eagerly, because classes have method bodies, vtable construction, and initializer expressions that require full inference-time type-checking
  - **Newtypes** → calls `instantiate_generic_newtype()`
  - **Arrays/Tuples/Functions** → registers their TypeDefs via `ensure_type_instantiated()`

- **`ensure_type_instantiated`** (15 call sites): The no-substitution variant — same TypeDef registration logic for already-concrete types.

- **`class_type_defs: BTreeMap`** on `Inference` struct: Still populated during inference because class TypeDef creation is coupled with method body type-checking and vtable construction.

- **`instantiate_generic_global` / `instantiate_class_static_global`**: Generic module globals and class static globals still use eager per-instantiation body inference because their initializers may contain expressions that are only valid for specific type args.

Phase 4 eliminates ALL of the above by teaching inference to work with `TypeParameter` types in expression bodies (not just type signatures). Once generic function bodies can be inferred with `TypeParameter` types, the same applies to class method bodies, global initializers, and vtable construction — enabling full deferral to monomorphize.

**Step 4a — Generic body retention:**

1. Add `type_params: Vec<TypeParamName>` to `TypedFunction` (default `vec![]`)
2. Add `type_args: Vec<Type>` to `FunctionCall`, `FunctionRef`, `MethodRef` (default `vec![]`)
3. Change `current_type_params` to map `TypeParamName → Type::TypeParameter(name, bounds)` during generic body inference
4. Infer generic function bodies once with `TypeParameter` types, keep the `TypedFunction` with `type_params` set and base mangled name
5. Populate `type_args` on `FunctionCall` at generic call sites
6. Handle generic class methods (stored in `ClassTypeSignature.generic_instance_methods` / `generic_static_methods`): these follow the same pattern as generic standalone functions. Produce `TypedFunction` with `type_params` set, populate `type_args` on calls. Unlike impl/ext methods, class methods stay in `functions` (not a separate block) since they are tightly coupled with the class body. Remove the separate `GenericClassMethodDef` registry storage — unify with `TypedFunction` entries.

**Step 4b — Remove old instantiation:**

7. Remove `typechecking_only` flag entirely
8. Remove `in_instantiation` flag — match exhaustiveness treats `TypeParameter` as opaque
9. Remove `instantiate_generic_function`, `instantiate_function_body`
10. Remove `instantiated: BTreeSet` from `Inference`
11. Refactor `substitute_and_instantiate` → `substitute_type`: pure function, no `&mut self`, no side effects, no TypeDef registration. This is a significant API change within inference — all callers must be updated.
12. Inference no longer produces concrete instantiations for generic functions — only the generic definition
13. Remove `instantiate_generic_global` and `instantiate_class_static_global` — generic module globals and class static globals now use the same template + substitute pattern as functions (deferred from Phase 3 because it requires TypeParameter-aware body inference)
14. Remove `class_type_defs: BTreeMap` from `Inference` struct — class TypeDefs now created by monomorphize
15. Remove `ensure_type_instantiated` — no longer needed; all TypeDef registration happens in monomorphize
16. Move Array/Tuple/Function TypeDef registration from inference to monomorphize

**Step 4c — Monomorphize generic functions and classes:**

17. Monomorphize: handle `FunctionCall` with non-empty `type_args` — look up generic `TypedFunction` by base name, substitute, produce concrete copy
18. Monomorphize: handle `FunctionRef` and `MethodRef` with `type_args`
19. Monomorphize: instantiate generic class TypeDefs — substitute method bodies, build vtable with concrete mangled names, substitute initializer expressions and `extends_args`
20. Monomorphize: instantiate generic newtype TypeDefs
21. Monomorphize: register Array/Tuple/Function TypeDefs discovered during expression walks
22. Fixpoint loop: instantiated generic functions/classes may reference other generics (already handled by queue)

**Step 4d — LSP updates:**

15. Hover on generic function calls shows `type_params` and `type_args` bindings
16. Go-to-definition on generic function calls navigates to generic definition (original span, no more `<generic>`)
17. Add LSP integration tests for generic function navigation and hover

**Exit criteria:** All tests pass (including LSP integration tests). No `typechecking_only`, no `in_instantiation`, no `instantiated`, no `instantiate_generic_function`, no `instantiate_generic_global`, no `instantiate_class_static_global`, no `class_type_defs`, no `ensure_type_instantiated`. `substitute_and_instantiate` replaced by pure `substitute_type`. Inference is purely type-checking with no TypeDef registration side effects. Monomorphize is the sole source of all concrete instantiations (functions, classes, newtypes, globals, Array/Tuple/Function TypeDefs).

### 11.6 Phase 5 — Pipeline restructuring (DONE)

**Goal:** Move desugar passes out of `typecheck()`. Restructure compiler modules into a clean hierarchy.

**What was done:**

1. Created `compiler/` folder; moved all compiler modules under it: codegen, desugar, layout, lexer, monomorphize, parser, typechecker
2. Moved pipeline orchestration (compile, check, build_project, build_workspace, compile_for_test, compile_prelude) into `compiler/pipeline.rs`
3. Moved `desugar_for`, `desugar_try`, `desugar_await` into `compiler/desugar/` module with a `desugar_all()` entry point
4. Moved `capture`, `coerce_byname`, `variance_cast` to standalone files under `compiler/` (not inside typechecker or desugar)
5. `typecheck()` became purely: Collect → Infer → Rules (no transformation passes)
6. `coerce_byname` and `capture` run once after monomorphize via `coerce_and_capture()` — eliminated the redundant two-pass design (was: run before monomorphize + re-run during monomorphize)
7. `check` mode and LSP receive the pre-desugar, source-faithful `TypedModule`
8. Build mode pipeline: desugar_all → monomorphize → coerce_and_capture → variance_cast → codegen
9. `lib.rs` became a thin re-export layer preserving all existing `crate::` paths for backward compatibility

**Exit criteria:** All tests pass. Pipeline is cleanly separated. LSP receives the source-faithful `TypedModule`.

### 11.7 Phase 6 — Cleanup ✓

**Goal:** Remove remaining dead code, consolidate registry types, clean up stale comments, polish.

Phases 1–5 already removed the major dead items (`in_generic_context()`, `Span::point("<generic>")`, `should_monomorphize`, `without_generic_functions()`, `as_generic_body()`, `strip_generics()`, `typechecking_only`, `in_instantiation`, `GenericTraitImplMethodDef`, `GenericExtensionMethodDef`, `TraitImplMethods`, `GenericTraitImplMethods`, `NamedExtensionInfo`, `NamedGenericExtensionInfo`). Phase 6 focuses on what remains.

**Step 6a — Remove stale comments: ✓**

1. ✓ Fixed comment in `monomorphize/mod.rs:36` — removed `typechecking_only` reference
2. ✓ Fixed comment in `common/types.rs:209` — replaced `instantiate_generic_function` with accurate reference
3. ✓ Fixed comment in `monomorphize/implement_blocks.rs:242` — replaced with `instantiate_generic_classes`

**Step 6b — Remove dead code / rename: ✓**

4. ✓ Removed `#[allow(dead_code)]` on `AwaitData` struct in `desugar/desugar_await.rs` — struct is actively used
5. ✓ Renamed `instantiate_generic_newtype` → `resolve_generic_newtype` (function doesn't instantiate; it resolves type args and returns `Type::GenericNewtype`)
6. ✓ `cargo clippy` clean, `cargo fmt` applied

**Step 6c — Registry type consolidation (deferred):**

The registry currently has 5 separate `Generic*Def` structs that all follow the same pattern (raw AST body + type params + scope info + visibility):

- `GenericFunctionDef` — standalone generic functions, also reused as the universal template for generic call resolution in inference
- `GenericClassMethodDef` — generic class instance/static methods (stored in `ClassTypeSignature`)
- `GenericClassStaticGlobalDef` — generic class static globals (stored in `ClassTypeSignature`)
- `GenericModuleMemberDef` — generic module functions/properties (stored in `ModuleInfo`)
- `GenericModuleGlobalDef` — generic module globals (stored in `ModuleInfo`)

All of these are consumed by inference to produce `TypedFunction` entries with `type_params` set. Consider unifying them into a single `GenericFunctionDef` (or a minimal set) with an enum discriminant for the source context. This would:
- Reduce type proliferation in registry.rs
- Simplify the inference paths that convert `Generic*Def` → `GenericFunctionDef` for template resolution (currently `infer/classes.rs`, `infer/generic_modules.rs` manually construct `GenericFunctionDef` from the other types)

**However:** This is a refactoring convenience, not a correctness issue. The current types work and each carries context-specific fields (e.g., `GenericClassMethodDef` has `is_override`, `GenericModuleMemberDef` has `is_property`). Unification may lose that specificity or require an enum to carry it. Deferred — the benefit doesn't justify the churn.

Note: `class_type_defs: BTreeMap` on the `Inference` struct is intentionally kept — inference needs it for class hierarchy resolution (parent lookups, vtable construction) during generic class body type-checking. This is not dead code.

**Exit criteria:** All tests pass. Clean `clippy`. No stale comments referencing removed concepts. Dead code removed.

### 11.8 Phase summary

| Phase | Description | Risk | Key files |
|-------|-------------|------|-----------|
| 1 | Implement blocks | High | registry.rs, collect/implements.rs, infer/implements.rs, infer/expressions.rs, monomorphize/ (new), all desugar passes |
| 2 | Extension blocks | Medium | registry.rs, collect/extensions.rs, infer/extensions.rs, monomorphize/ |
| 3 | Generic types | Medium | infer/types.rs, infer/generics.rs, monomorphize/ |
| 4 | Generic functions | High | infer/functions.rs, infer/generic_functions.rs, monomorphize/ |
| 5 | Pipeline restructuring | Medium | compiler/ (new folder), pipeline.rs (new), desugar/ (new), typechecker/mod.rs, lib.rs |
| 6 | Cleanup | Low–Medium | registry.rs, monomorphize/mod.rs, desugar/desugar_await.rs, stale comments |

Phase 1 is the largest because it creates the monomorphize infrastructure (substitution functions, fixpoint loop, pipeline wiring) alongside the impl block port. Phases 2–4 reuse that infrastructure and are progressively smaller.

### 11.9 LSP updates — no separate phase

LSP changes are **not** a separate phase. They happen within each phase as data structures change, because deferring them would leave the LSP broken between phases:

- **Phase 1:** `find_node_at_position` must search `implement_blocks` method bodies. Hover/go-to-definition must handle `ImplFunctionCall`, `TraitBoundFunctionCall`. Navigate `ImplFunctionCall` → `TypedImplMethod.span`, `TraitBoundFunctionCall` → trait method signature.
- **Phase 2:** `find_node_at_position` must search `extension_blocks` method bodies. Handle `ExtFunctionCall` → navigate to `TypedExtMethod.span`.
- **Phase 3:** `find_node_at_position` must search `types` for `TypeDef` entries with `span`. Go-to-definition on type names works.
- **Phase 4:** Hover on generic function calls shows `type_params` and `type_args` bindings. Go-to-definition navigates to generic function definitions (original span instead of `<generic>`).

Each phase's exit criteria should include: "LSP integration tests pass for the new structures added in this phase."

### 11.10 Testing strategy

**Existing tests:** The ~1,900 integration tests are the primary safety net. Every phase must pass all of them.

**New tests per phase:**

- **Phase 1:** Nested generic impl blocks (`implement<T> Trait for Box<T>` with `method<U>`), trait object coercion with generic impl methods, `TraitBoundFunctionCall` with method-level type args, vtable expansion from implement blocks. **type_args ordering invariant:** test `implement<T> Functor for Box<T>` with `method map<U>` — verify block-level `T` and method-level `U` are correctly split by monomorphize (a mismatch causes silent type errors).
- **Phase 2:** Generic extension methods, extension blocks for generic types
- **Phase 3:** Inspect `TypedModule` to verify generic `TypeDef` entries have `type_params` and `TypeParameter` fields; monomorphize produces concrete `TypeDef` for all reachable instantiations. Generic module/static globals instantiation.
- **Phase 4:** Recursive generics hit depth limit, cross-package generic function instantiation, generic functions calling other generics. **Closures in generic bodies:** test closures whose parameter types involve `TypeParameter` (e.g., `function apply<T>(x: T, f: Function(T): T): T = f(x)`) — verify closure re-inference with `TypeParameter` types works correctly.

**Regression detection:** Before starting, snapshot the full set of `MangledName` keys in `TypedModule.functions` and `TypedModule.types` for the standard-io workspace. After each phase, verify monomorphize output produces the same set.

---

## 12. Risk Assessment

| Risk | Severity | Mitigation |
|------|----------|------------|
| Pattern match update volume (~30 files) | Medium | Mechanical change; compiler errors guide it |
| Match exhaustiveness on TypeParameter | Medium | Treat as opaque; validate approach with existing generic match tests |
| Trait object coercion in generic bodies | Medium | Emit TraitObjectCoerce with TypeParameter; monomorphize resolves |
| Closure re-inference with TypeParameter | Low | Already works when typechecking_only — same semantics |
| Cross-package generic instantiation | Low | merge_from propagates generic entries and implement_blocks; monomorphize sees everything |
| Performance of fixpoint loop | Low | Same work as today's on-demand instantiation, just batched |
| Variance cast pass on generic bodies | Medium | Skip TypeParameter casts; re-run post-monomorphize if needed |
| Desugar passes handling TypeParameter | Low | Passes are structural transforms; TypeParameter flows through |
| Implement block identity on call sites | Medium | Structured fields on `ImplFunctionCall` (trait_fqn, for_type, method_name); no mangled name needed |

---

## 13. Comparison with PR #188

| Aspect | PR #188 | This design |
|--------|---------|-------------|
| Generic body identity | `$generic$` prefix hack | Base mangled name (proper identity) |
| Call site metadata | `CallInfo` enum (complex, 6 variants) | `type_args: Vec<Type>` (simple, uniform) |
| Monomorphize implementation | Re-runs inference via `instantiation.rs` | Pure substitution walk (no inference machinery) |
| Monomorphize independence | Depends on `Inference` struct and import scopes | Fully independent — operates on flat `TypedModule` |
| Implement blocks | Not represented in TypedModule | First-class `implement_blocks` field |
| Vtable tracking | `Vec<(String, MangledName, CallInfo)>` | `Vec<(TraitObjectMemberName, MangledName, Vec<Type>)>`; expanded from implement_blocks |
| `typechecking_only` | Removed | Removed |
| `in_instantiation` | Kept implicitly | Removed |
| `pending_trait_instantiations` | Kept | Removed — vtable expansion in monomorphize |
| `should_monomorphize` flag | Passed through typecheck() | Pipeline choice (call monomorphize or don't) |
| Codegen changes | Strips generics at codegen entry | Receives already-concrete module |
