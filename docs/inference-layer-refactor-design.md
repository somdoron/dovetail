# Inference Layer Refactor Design

This document designs a refactor of the typechecker **inference phase** so that it no longer behaves as a god object. The goals are: a lean **InferenceContext** with a **single impl block** (add/get/modify only), **free functions** in each file that take the context (by reference, mut or immut as needed), and **English-readable code** with repetitive patterns extracted into named helpers.

**In scope:** Inference phase only (the `infer/` directory and entry point in `typechecker/mod.rs`). Collect and Rules phases are out of scope.

**Out of scope:** Changing the external API of the typechecker, changing the inference algorithm or type system, or refactoring the collect phase in the same way (can be done later using this as a template).

**Implementation status:** Not started.

**Last plan sync:** Inventory verified against codebase (2026-02-21). Corrections applied: `drain_pending_trait_instantiations` is private (not pub(super)); `functions.rs` has `infer_function_typecheck_only`; `extensions.rs` has `typecheck_generic_extension`/`typecheck_generic_extension_method`; `implements.rs` has `typecheck_generic_impl_block`/`typecheck_generic_impl_method`; `match_expression.rs` has 10 pattern-inference helpers; `record_expressions.rs` has 4 internal helpers; `generic_modules.rs` methods updated to match actual names (`resolve_generic_module_property`, `instantiate_generic_module_member`); `types.rs` defines `SymbolKind` enum; `generic_functions.rs` defines `MethodKind<'a>` enum. Phase 3 split into sub-phases (3a–3f) to reduce risk. Added `ensure_*` boundary clarification and `expressions.rs` splitting guidance.

---

## 1. Current State

### 1.1 Structure

The inference phase is driven from `infer::infer()` in `infer/mod.rs`. It:

1. Creates one `Inference<'a>` per file.
2. For each declaration, calls methods on `Inference`: `infer_function`, `infer_global`, `infer_extension`, `infer_implement`, `infer_module`. (No-op for `Record`, `Enum`, `Trait`, `Newtype`, `TypeAlias`.)
3. After each file, calls `drain_pending_trait_instantiations()`.
4. Merges `typed_functions`, `typed_globals`, `record_type_defs`, `enum_type_defs`, and `array_type_defs` from all files into a single `TypedModule`.

The type `Inference<'a>` is defined in `infer/mod.rs` and holds:

- **Environment (read-only for the duration of a file):** `package_path`, `current_file`, `registry`, `diagnostics`, `import_scope`
- **Accumulated output:** `typed_functions`, `typed_globals`, `record_type_defs`, `enum_type_defs`, `array_type_defs`
- **Scopes:** `scopes: Vec<Scope>`, `loop_depth`
- **Generic/instantiation state:** `instantiated: BTreeSet<MangledName>`, `current_type_params`, `pending_trait_instantiations: RefCell<Vec<...>>`
- **Context hints:** `expected_type`, `current_module`, `typechecking_only`, `in_instantiation`

There is **no single impl block**. Instead, **many files each add an `impl Inference<'_>` block** with methods that are called from the main loop or from other methods. As a result:

- The same type is extended in 16+ places; finding “where does X live?” requires searching.
- Methods mix “logic” (how to infer a function) with “state access” (push scope, insert typed function).
- Repetitive patterns (resolve params → push scope → infer body → check assignable → insert) are duplicated across functions, extensions, implements, and modules.

### 1.2 Inventory of `impl Inference<'_>` Blocks and Methods

| File | Public (pub(super)) methods | Private / helper methods |
|------|-----------------------------|---------------------------|
| **mod.rs** | `push_scope`, `pop_scope`, `check_newtype_inner_access`, `define_variable`, `lookup_variable`, `ensure_tuple_type_def` | `Scope::new`, `define`, `lookup`, `Inference::new` |
| **types.rs** | `resolve_fqn`, `resolve_trait_fqn`, `is_assignable`, `check_assignable`, `resolve_type_expr`, `resolve_type_args`, `resolve_type_name`, `resolve_record_type`, `resolve_enum_type` | `symbol_exists`, `check_variance`, `resolve_generic_type`; also defines `SymbolKind` enum |
| **traits.rs** | `type_satisfies_trait`, `check_trait_bounds`, `resolve_trait_bounds_from_where_clause` | `drain_pending_trait_instantiations` (private, called from mod.rs entry point), `find_impl_method_mangled_name` |
| **functions.rs** | `infer_function` | `infer_function_typecheck_only` |
| **globals.rs** | `infer_global`, `lookup_global` | — |
| **extensions.rs** | `infer_extension` | `typecheck_generic_extension`, `typecheck_generic_extension_method` |
| **implements.rs** | `infer_implement` | `typecheck_generic_impl_block`, `typecheck_generic_impl_method` |
| **modules.rs** | `infer_module` | `infer_module_property`, `resolve_module_name` (private) |
| **expressions.rs** | `infer_expr`, `infer_array_literal`, `infer_field_access` | `infer_tuple_literal`, `infer_let_destructure`, `infer_tuple_destructure_pattern`, `mangled_for_trait_bound_method`, `build_lowered_op_result`, `try_lower_op_to_trait`, `check_binary_op`, `check_unary_op`, `try_resolve_module_instance_property`, `try_resolve_generic_module_instance_property`, `try_resolve_extension_property`, `try_resolve_trait_object_property`, `try_resolve_bare_module_property`, `try_resolve_bare_variant_identifier`, `try_resolve_static_property`, `infer_enum_variant_record_create` |
| **function_expressions.rs** | `infer_bare_function_call`, `infer_method_call`, `try_flatten_to_path`, `lookup_trait_impl_methods`, `lookup_named_extension_methods`, `error_expr` | `resolve_trait_object_method_call`, `try_resolve_method_from_trait_bounds`, `filter_overloads_resolved`, `resolve_overload`, `resolve_qualified_call`, `error_call`, `try_resolve_bare_variant_call`, `try_resolve_newtype_call`, `undefined_function_error` |
| **record_expressions.rs** | `try_resolve_record_field`, `infer_record_with`, `infer_record_create` | `infer_generic_record_with_type_params`, `infer_generic_record`, `build_generic_record`, `check_record_fields` |
| **match_expression.rs** | `infer_match_expr`, `infer_sub_pattern` | `is_assignable_for_pattern`, `check_variance_for_pattern`, `infer_type_annotated_pattern`, `infer_enum_variant_pattern`, `try_promote_variable_to_variant`, `infer_newtype_pattern`, `infer_enum_variant_no_args_pattern`, `infer_record_pattern`, `infer_enum_variant_record_pattern`, `infer_field_sub_pattern` |
| **generics.rs** | `substitute_and_instantiate`, `ensure_type_instantiated`, `ensure_array_type_registered`, `substitute_and_instantiate_named_types` | `type_contains_type_param` |
| **generic_functions.rs** | `infer_type_args`, `return_type_for_generic_call`, `instantiate_generic_function`, `resolve_generic_function`, `resolve_generic_extension_instance`, `resolve_generic_trait_impl_instance`, `resolve_generic_extension_property`, `resolve_generic_static_extension` | `lookup_all_generic_extension_methods`, `instantiate_function_body`; also defines `MethodKind<'a>` enum |
| **generic_records.rs** | `instantiate_generic_record` | — |
| **generic_modules.rs** | `resolve_generic_module_instance_method`, `resolve_generic_module_static_method`, `resolve_generic_module_property` | `resolve_module_name`, `instantiate_generic_module_member` |
| **generic_enums.rs** | `instantiate_generic_enum` | — |

Additional free functions (not on `Inference`):

- **generics.rs**: `apply_substitution` (substitution only; no re-instantiation)
- **function_expressions.rs**: `resolve_intrinsic_kind`
- **types.rs**: `types_assignable` (used by `is_assignable`)

Standalone module-level functions (no `self`):

- **records.rs**: `collect_record_type_defs`
- **enums.rs**: `collect_enum_type_defs`
- **type_param_substitution.rs**: `TypeParamSubstitution` and its impl (`new`, `from_pairs`, `with_self_type`, `get`, `resolve_type_params`, `resolve_with_variance_defaults`, `self_type`, `unify`, and more). **Note:** This is already a clean standalone type with its own impl that does not touch `Inference`. It requires no refactoring and serves as a good example of the target pattern (self-contained type with focused impl).

### 1.3 Repetitive Patterns Identified

1. **Resolve parameter list from AST**  
   Same pattern in: `functions.rs`, `extensions.rs` (methods + properties), `implements.rs` (methods + properties), `modules.rs` (infer_module_property).  
   Code: map over `params` with `resolve_type_expr(&p.type_annotation)`, build `TypedParam { name, ty, span }`.

2. **Infer body with param scope**  
   Same pattern everywhere a function/method/property body is inferred:  
   `push_scope()` → define each param with `define_variable` → `push_scope()` → `infer_expr(body)` → `pop_scope()` → `pop_scope()`.  
   Used in: functions, extensions (methods and properties), implements (methods and properties), modules (module property), generic_functions (instantiated body).

3. **Check assignable then insert typed function**  
   After inferring body: `check_assignable(body.span, &return_type, &body.ty)` then `typed_functions.insert(name, TypedFunction { ... })`.  
   The only variation is how the mangled `name` and `TypedFunction` fields (visibility, name, params, return_type, body, span) are built.

4. **Qualified symbol name for current module**  
   In `functions.rs`, `globals.rs`, `modules.rs`:  
   `if let Some(ref module_name) = self.current_module { format!("{}.{}", module_name, bare_name) } else { bare_name.clone() }`.

5. **Skip intrinsic / generic bodies**  
   Extensions and implements: `if matches!(method.body, Expr::Intrinsic(_)) { continue }` and `if !ext.type_params.is_empty() { return }`.

These should become shared helpers so that “infer a function-like thing” reads in one place and reuses one implementation of “resolve params → infer body in param scope → check return type → add function.”

---

## 2. Target Design

### 2.1 InferenceContext (single struct, single impl)

- **Rename** `Inference` → `InferenceContext` (or keep the name `Inference`; the doc uses `InferenceContext` to stress “context only”).
- The struct keeps the same fields (or minor renames). It remains **lean**: it does not implement inference logic, only:
  - **Construction:** `new(package_path, current_file, registry, import_scope, diagnostics)`.
  - **Scope stack:** `push_scope`, `pop_scope`, `define_variable`, `lookup_variable`.
  - **Accumulated definitions:** `add_typed_function`, `add_typed_global`, `add_record_type_def`, `add_enum_type_def`, `add_array_type_def` (or equivalent); accessors for merging (e.g. `take_typed_functions`, `take_typed_globals`, `record_type_defs`, `enum_type_defs`, `array_type_defs`) or getters if the driver needs to extend maps in place.
  - **Read-only environment:** `package_path()`, `current_file()`, `registry()`, `diagnostics()`, `import_scope()`, `current_module()` and optionally `set_current_module` for modules.
  - **Generic/instantiation state:** `instantiated()`, `mark_instantiated()`, `record_type_defs()` (get/mut), `enum_type_defs()` (get/mut), `array_type_defs()` (get/mut), `current_type_params()` / `set_current_type_params` (or push/pop for type-param scope), `expected_type()` / `set_expected_type`, `typechecking_only()` / `in_instantiation()`, and access to `pending_trait_instantiations` (e.g. for `drain_pending_trait_instantiations`).
  - **Helpers that stay on context (state + simple checks):** `check_newtype_inner_access`, `ensure_tuple_type_def`.
  - **Decision needed for `ensure_type_instantiated` / `ensure_array_type_registered`:** These are currently on `Inference` in `generics.rs` and look like simple map mutators, but they internally call `resolve_type_expr`, `instantiate_generic_record`, `instantiate_generic_enum`, and other logic functions. They therefore **cannot stay as context methods** in the final design — they must become free functions in `generics.rs` that take `ctx: &mut InferenceContext`. If kept on the context, they would pull logic into the "state-only" impl and violate the single-impl boundary. Decide this in Phase 3.1 when building the context API surface.
- **One impl block** in one place (e.g. `infer/context.rs` or the end of `infer/mod.rs`). No other file has `impl InferenceContext`.

So: “add, get, modify” only; no `resolve_type_expr`, no `infer_expr`, no `infer_function` on the type.

### 2.2 Free functions per concern

Each current “impl” file becomes a set of **free functions** that take the context:

- **By value / by ref:** Not by value (context is large and used in a loop). So `&self` or `&mut self` becomes `ctx: &InferenceContext` or `ctx: &mut InferenceContext`.
- **Naming:** Prefer names that read like English: `infer_function(ctx, func)`, `infer_expr(ctx, expr)`, `resolve_type_expr(ctx, type_expr)`, `infer_body_with_param_scope(ctx, params, body)`.

Files and responsibilities (conceptual):

- **context.rs** (or **mod.rs**): Defines `InferenceContext`, its fields, and the **single** `impl InferenceContext` with only the methods above.
- **types.rs**: `resolve_type_expr`, `resolve_type_args`, `resolve_type_name`, `resolve_fqn`, `resolve_trait_fqn`, `resolve_record_type`, `resolve_enum_type`, `resolve_generic_type`, `is_assignable`, `check_assignable`, `types_assignable` (free). All take `ctx: &…` or `ctx: &mut …` as needed.
- **traits.rs**: `type_satisfies_trait`, `drain_pending_trait_instantiations`, `check_trait_bounds`, `resolve_trait_bounds_from_where_clause`, and helpers (e.g. `find_impl_method_mangled_name` as a free function).
- **functions.rs**: `infer_function(ctx, func)`; uses helpers for param resolution and body inference.
- **globals.rs**: `infer_global(ctx, global)`, `lookup_global(ctx, name)`.
- **extensions.rs**: `infer_extension(ctx, ext)`.
- **implements.rs**: `infer_implement(ctx, impl_decl)`.
- **modules.rs**: `infer_module(ctx, module)`, `infer_module_property(ctx, property, module_name)`, `resolve_module_name(ctx, name)`.
- **expressions.rs**: `infer_expr(ctx, expr)`, `infer_array_literal`, `infer_field_access`, and the various `try_*` / `check_*` / `infer_*` helpers. **Note:** This file currently has a very large impl block with 18+ private helpers. Converting all of them to free functions in one file will produce a file with 20+ `pub(super)` functions. Consider splitting during the conversion into sub-files by concern (e.g., `operator_expressions.rs` for `check_binary_op`/`check_unary_op`/`try_lower_op_to_trait`, `control_flow_expressions.rs` for let/assignment/tuple destructuring, keeping `expressions.rs` as the dispatcher with `infer_expr` and field/property resolution helpers).
- **function_expressions.rs**: `infer_bare_function_call`, `infer_method_call`, `resolve_overload`, `lookup_trait_impl_methods`, `lookup_named_extension_methods`, `try_flatten_to_path`, `error_expr`, `resolve_intrinsic_kind`, etc.
- **record_expressions.rs**: `try_resolve_record_field`, `infer_record_with`, `infer_record_create`, and internal helpers.
- **match_expression.rs**: `infer_match_expr`, `infer_sub_pattern`, and pattern helpers.
- **generics.rs**: `apply_substitution` (free), `substitute_and_instantiate(ctx, …)`, `ensure_type_instantiated`, `ensure_array_type_registered`, `substitute_and_instantiate_named_types(ctx, …)`.
- **generic_functions.rs**: `infer_type_args`, `return_type_for_generic_call`, `instantiate_generic_function`, `resolve_generic_function`, `resolve_generic_extension_instance`, `resolve_generic_trait_impl_instance`, `resolve_generic_extension_property`, `resolve_generic_static_extension`, and body-instantiation helpers.
- **generic_records.rs**: `instantiate_generic_record`.
- **generic_modules.rs**: `resolve_generic_module_instance_method`, `resolve_generic_module_instance_property`, `resolve_generic_module_static_method`, `resolve_generic_module_global`, `instantiate_and_resolve_generic_global`, and `instantiate_generic_global` (free or internal).
- **generic_enums.rs**: `instantiate_generic_enum`.

The top-level driver `infer::infer(...)` still creates the context per file, calls `drain_pending_trait_instantiations(ctx)` after each file, and calls these functions, e.g. `infer_function(ctx, func)` instead of `ctx.infer_function(func)`.

### 2.3 Shared helpers (English-readable, no duplication)

Extract these and use them from the above functions:

1. **`resolve_param_list(ctx, params)`**  
   Takes `ctx: &mut InferenceContext` and something like `&[Param]` (parser AST param list). Returns `Vec<TypedParam>`. Used by functions, extensions, implements, modules.

2. **`infer_body_with_param_scope(ctx, typed_params, body_expr)`**  
   Pushes param scope, defines params, pushes body scope, infers body, pops twice. Returns `TypedExpr`. Used everywhere we infer a function/method/property body.

3. **`check_body_assignable(ctx, body, return_type)`**  
   Wrapper around “check assignable and emit diagnostic if not”: `check_assignable(ctx, body.span, return_type, body.ty)`. Keeps call sites to one line and one name.

4. **`add_typed_function(ctx, tf)`**  
   Already part of the context’s single impl: `ctx.add_typed_function(tf)`. Call sites build `TypedFunction` and call this instead of `ctx.typed_functions.insert(...)`.

5. **`qualified_symbol_name(ctx, bare_name: &str) -> String`**  
   If `ctx.current_module()` is `Some(m)`, return `format!("{}.{}", m, bare_name)`; else `bare_name.to_string()`. Used when building FQNs/symbol names in functions, globals, modules.

6. **Optional: “infer and add one function-like declaration”**  
   A higher-level helper that does: resolve params → infer body with param scope → resolve return type → check assignable → build `TypedFunction` and add. Callers pass in how to compute the mangled name and the visibility/span. This can be introduced in a later phase to reduce duplication in infer_extension / infer_implement / infer_module_property / infer_function.

All helpers take `ctx` (or `ctx` + minimal data) so that the code reads as “resolve param list using context,” “infer body with param scope using context,” etc.

---

## 3. Implementation Plan (Phased)

### Phase 0: Inventory and tests (no code change)

- **0.1** Confirm the method inventory (this doc) matches the codebase; add any missing `impl Inference` methods or files.
- **0.2** Ensure the inference test suite (integration tests under `dovetail/tests/` and any unit tests for infer) is green and sufficient. Add any missing tests for inference behavior that we want to preserve.
- **0.3** (Optional) Add a small “inference smoke” test that runs a few programs and checks `typed_module` shape, so regressions are caught early.

**Exit criterion:** Document and tests are up to date; no behavior change.

---

### Phase 1: Introduce InferenceContext and keep impl layout

- **1.1** Rename `Inference` → `InferenceContext` everywhere (struct, all `impl Inference<'_>` blocks, all `Inference::new`, and call sites in `infer::infer` and tests). Keep all methods on the type; still one impl per file. Goal: single type name “InferenceContext,” same behavior.
- **1.2** Run `cargo test` and fix any breakage. Commit as “Rename Inference to InferenceContext.”

**Exit criterion:** All tests pass; type is consistently named InferenceContext; no logic change.

---

### Phase 2: Extract repetitive helpers (still multiple impl blocks)

- **2.1** Add helper **functions** (free functions in a small module, e.g. `infer/helpers.rs`, or in the module that needs them first):
  - `resolve_param_list(ctx, params)` using `resolve_type_expr` and building `TypedParam`. Use the parser’s `Param` type (or the appropriate param type from AST). Place in a module visible to functions, extensions, implements, modules (e.g. `helpers` or `types`).
  - `infer_body_with_param_scope(ctx, typed_params, body_expr)` that does push_scope, define_variable for each param, push_scope, infer_expr(body), pop_scope, pop_scope. It will need to call `infer_expr` on the context; so it stays as a method for now, or we pass a closure/callback. Simplest: implement it as a method on InferenceContext in this phase, e.g. `infer_body_with_param_scope(&mut self, typed_params, body_expr)` in the single impl we’ll have later; for Phase 2 we can add it to one of the existing impl blocks (e.g. mod.rs) and use it from functions, extensions, implements, modules.
  - `qualified_symbol_name(ctx, bare_name)` as a free function or a small method. Prefer free function taking `ctx: &InferenceContext` so we get used to “functions take ctx.”
- **2.2** Replace duplicated “resolve params” and “infer body with param scope” and “qualified symbol name” in:
  - `functions.rs`
  - `extensions.rs` (methods and properties)
  - `implements.rs` (methods and properties)
  - `modules.rs` (infer_module_property)
  - `globals.rs` (qualified symbol name only)
  Use the new helpers. Keep `check_assignable` and `typed_functions.insert` as-is for now.
- **2.3** Optionally add `check_body_assignable(ctx, body, return_type)` and use it at those call sites.
- **2.4** Run tests; fix any regressions. Commit as “Extract param list, body-with-param-scope, and qualified symbol name helpers.”

**Exit criterion:** No duplicated “resolve param list” or “infer body in param scope” or “qualified symbol name” blocks; tests pass.

---

### Phase 3: Move logic into free functions; context only add/get/modify

This is the core structural change. To reduce risk, it is split into sub-phases that can each be committed independently. The general technique for each file is:

1. Add a free function that takes `ctx: &mut InferenceContext` with the same signature as the method.
2. Move the method body into the free function, replacing `self` with `ctx`.
3. Temporarily delegate the method to the free function: `pub(super) fn foo(&mut self, ...) { foo(self, ...) }`.
4. Update all call sites to use the free function directly, then remove the delegating method and its impl block.

#### Phase 3a: Create context API surface

- **3a.1** Create `infer/context.rs` with the **single** `impl InferenceContext` containing only:
  - `new(...)`
  - Scope: `push_scope`, `pop_scope`, `define_variable`, `lookup_variable`
  - Accumulated defs: `add_typed_function`, `add_typed_global`, and accessors for type def maps (e.g. `record_type_defs_mut()`, `enum_type_defs_mut()`, `array_type_defs_mut()`)
  - Getters (and minimal setters) for: `package_path`, `current_file`, `registry`, `diagnostics`, `import_scope`, `current_module` (get/set), `expected_type` (get/set), `instantiated` (check + insert), `current_type_params` (get/set or push/pop), `typechecking_only`, `in_instantiation`, `loop_depth` (get/set), and `pending_trait_instantiations` access
  - Simple state helpers: `check_newtype_inner_access`, `ensure_tuple_type_def`
  Move the implementations of these from the various impl blocks into this single block. Logic methods remain in their per-file impl blocks temporarily (two impl blocks coexist during this sub-phase).
- **3a.2** Replace direct field access in all per-file impl blocks with context method calls (e.g. `self.typed_functions.insert(...)` → `self.add_typed_function(...)`, `self.package_path` → `self.package_path()`). Tests must pass after this step.
- **3a.3** Decide the `ensure_type_instantiated` / `ensure_array_type_registered` boundary (see Section 2.1 note). These call into type resolution logic, so they should become free functions in `generics.rs`, not context methods.
- **3a.4** Run tests; commit as “Create InferenceContext API surface in context.rs.”

**Exit criterion:** Single context impl in `context.rs` with state-only methods; per-file impl blocks still have logic methods but use the context API; tests pass.

---

#### Phase 3b: Convert type resolution and traits to free functions

- **3b.1** Convert `types.rs`: all methods (`resolve_fqn`, `resolve_trait_fqn`, `is_assignable`, `check_assignable`, `resolve_type_expr`, `resolve_type_args`, `resolve_type_name`, `resolve_record_type`, `resolve_enum_type`, `resolve_generic_type`, `symbol_exists`, `check_variance`) become free functions. Update all call sites across the codebase (these are the most widely called helpers). Remove the `impl InferenceContext` block from `types.rs`.
- **3b.2** Convert `traits.rs`: `type_satisfies_trait`, `drain_pending_trait_instantiations`, `check_trait_bounds`, `resolve_trait_bounds_from_where_clause`, `find_impl_method_mangled_name` become free functions. Remove the impl block.
- **3b.3** Run tests; commit as “Convert type resolution and traits to free functions.”

**Exit criterion:** `types.rs` and `traits.rs` have no `impl InferenceContext` blocks; all their functions take `ctx`; tests pass.

---

#### Phase 3c: Convert expression inference to free functions

- **3c.1** Convert `expressions.rs`: `infer_expr` and all helper methods become free functions. Consider splitting the file (see Section 2.2 note on expressions.rs size).
- **3c.2** Convert `function_expressions.rs`: all methods become free functions.
- **3c.3** Convert `record_expressions.rs`: all methods become free functions.
- **3c.4** Convert `match_expression.rs`: all methods become free functions.
- **3c.5** Run tests; commit as “Convert expression inference to free functions.”

**Exit criterion:** All four expression files have no `impl InferenceContext` blocks; tests pass.

---

#### Phase 3d: Convert declaration inference to free functions

- **3d.1** Convert `functions.rs`: `infer_function`, `infer_function_typecheck_only` become free functions.
- **3d.2** Convert `globals.rs`: `infer_global`, `lookup_global` become free functions.
- **3d.3** Convert `extensions.rs`: `infer_extension` and typecheck helpers become free functions.
- **3d.4** Convert `implements.rs`: `infer_implement` and typecheck helpers become free functions.
- **3d.5** Convert `modules.rs`: `infer_module`, `infer_module_property`, `resolve_module_name` become free functions.
- **3d.6** Run tests; commit as “Convert declaration inference to free functions.”

**Exit criterion:** All five declaration files have no `impl InferenceContext` blocks; tests pass.

---

#### Phase 3e: Convert generics to free functions

- **3e.1** Convert `generics.rs`: `substitute_and_instantiate`, `ensure_type_instantiated`, `ensure_array_type_registered`, `substitute_and_instantiate_named_types`, `type_contains_type_param` become free functions (note: `apply_substitution` is already free).
- **3e.2** Convert `generic_functions.rs`: all methods become free functions.
- **3e.3** Convert `generic_records.rs`: `instantiate_generic_record` becomes a free function.
- **3e.4** Convert `generic_modules.rs`: all methods become free functions.
- **3e.5** Convert `generic_enums.rs`: `instantiate_generic_enum` becomes a free function.
- **3e.6** Run tests; commit as “Convert generics to free functions.”

**Exit criterion:** All five generics files have no `impl InferenceContext` blocks; tests pass.

---

#### Phase 3f: Final cleanup

- **3f.1** Ensure `Scope` and `VarBinding` remain internal to the context (in `context.rs`). The single impl block is the only one that knows about `Scope`/`VarBinding`; free functions use `push_scope`, `pop_scope`, `define_variable`, `lookup_variable`.
- **3f.2** Verify there is exactly one `impl InferenceContext` block (in `context.rs`). Remove any remaining delegating methods or empty impl blocks.
- **3f.3** Update the entry point `infer::infer()` to call free functions directly (e.g. `infer_function(ctx, func)` instead of `ctx.infer_function(func)`).
- **3f.4** Run full test suite and clippy; commit as “Finalize: single InferenceContext impl, all logic as free functions.”

**Exit criterion:** There is exactly one `impl InferenceContext` block; all inference behavior is in free functions taking `ctx`; tests and clippy pass.

---

### Phase 4: Consolidate context impl and improve readability

- **4.1** If the single impl is still split across two places (e.g. `mod.rs` and `context.rs`), merge into one file. Ensure all “add, get, modify” methods are in that single block and that no logic (e.g. `resolve_type_expr` body) remains in the impl.
- **4.2** Rename free functions where it improves English readability (e.g. “infer function” vs “infer_function” is already clear; consider “infer_function_body” vs “infer_body_with_param_scope” as decided in Phase 2).
- **4.3** Add a short doc comment to `InferenceContext` describing that it holds state and environment for the inference phase and that all inference logic lives in free functions in sibling modules.
- **4.4** Run tests and clippy; commit as “Consolidate context impl and docs.”

**Exit criterion:** One file holds the single impl; names and docs are clear; tests and clippy pass.

---

### Phase 5: Optional higher-level helper and cleanup

- **5.1** (Optional) Introduce a higher-level helper, e.g. `infer_and_add_function(ctx, param_asts, return_type_expr, body_expr, name_builder, visibility, span)` (or split into two: “infer function-like” returns `TypedFunction`, then “add” is just `ctx.add_typed_function(tf)`). Use it in infer_function, infer_extension, infer_implement, infer_module_property to reduce the remaining duplication (resolve params → infer body → check assignable → build name → add).
- **5.2** Final pass: ensure no remaining duplicated “resolve params + scope + body + check + insert” blocks; run tests and clippy; update this doc if the final structure diverges from the plan.

**Exit criterion:** Duplication minimized; tests and lints pass; design doc updated if needed.

---

## 4. Summary

| Before | After |
|--------|--------|
| Many `impl Inference<'_>` blocks across 16+ files | One `impl InferenceContext` with add/get/modify and scope only |
| Logic and state access mixed in methods | Logic in free functions; state only in context methods |
| Duplicated “resolve params → scope → body → check → insert” | Shared helpers: `resolve_param_list`, `infer_body_with_param_scope`, `check_body_assignable`, `qualified_symbol_name`, and optionally one “infer and add function” helper |
| “Where does X live?” requires search | “Context” in one place; “how we infer Y” in the corresponding module as free functions |

The refactor is done in small phases so that each step keeps tests green and can be committed independently. Phase 1 is a pure rename; Phase 2 removes duplication; Phase 3 is the main structural change (logic → free functions, single impl), split into 6 sub-phases (3a: context API, 3b: types+traits, 3c: expressions, 3d: declarations, 3e: generics, 3f: cleanup); Phase 4–5 polish and optionally add one more helper.
