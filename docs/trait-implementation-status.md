# Trait and Interface Implementation Status

**Status:** Audit complete. The implemented scope is recorded below. Earlier
proposals listed as obsolete are retired requirements, not implemented features
or pending work. The [trait completion design](trait-completion-design.md) reopens and implements the generic-default, projection, and inherited-overload limitations.

This audit covers the six trait/interface design documents, their book update,
the trait coherence recommendations in `type-theory-and-improvements.md`, and
trait-related claims in the async, early-return, resource, class, erasure, and
monomorphization documents.
Trait consumers such as async, resources, operators, and iterators retain their
own designs; unrelated future features such as tuple extension and type IDs are
not marked complete by this audit.

## Document disposition

| Document | Disposition |
|----------|-------------|
| [traits-design.md](traits-design.md) | Implemented core; original object representation and rollout plan superseded by the interface design; syntax and bound claims corrected. |
| [trait-design-appendix.md](trait-design-appendix.md) | Implemented inheritance, resolution, defaults, associated types/GATs, and coherence within the restrictions below. Unsupported original proposals are obsolete. |
| [interface-objects-design.md](interface-objects-design.md) | Phases 1–6 implemented, including the compiler rename and book sections. |
| [array-traitobject-demonomorphize-design.md](array-traitobject-demonomorphize-design.md) | Implemented shared array/interface representations and removal of the type-discovery walk; implementation notes override the historical plan. |
| [interfaces-design.md](interfaces-design.md) | Obsolete; superseded in full by `interface-objects-design.md`. |
| [trait-objects-removal-design.md](trait-objects-removal-design.md) | Obsolete; superseded in full by `interface-objects-design.md`. |
| [type-theory-and-improvements.md §4](type-theory-and-improvements.md#4-trait-coherence-and-elaboration) | Reference updated: coherence and elaboration recommendations are implemented or superseded by the current pipeline. |

Related documents were reconciled with the same scope:

- [Async/await](async-await-design.md): associated-type declarations and static
  member syntax corrected; definitions belong in implementation blocks.
- [Early return](railway-early-return-design.md): obsolete “not started” status
  removed; associated output constraints noted.
- [Resources](resource-management-design.md): abstract `use` preserves the associated continuation result.
- [Classes](classes-design.md): old generic-class trait deferrals marked historical;
  interface coercion and generic-class defaults implemented.
- [Full erasure](full-erasure-design.md) and [monomorphization](monomorphize-separation-design.md):
  old trait-object terminology and pipeline sketches superseded by current interface
  representations and coercion.

## Implemented requirements and evidence

| Requirement | Implementation / regression evidence |
|-------------|--------------------------------------|
| Generic/non-generic traits and implementations, properties, method generics, bounds, orphan rule, class implementations | [traits tests](../dovetail/tests/traits.rs), [class tests](../dovetail/tests/classes.rs), [trait collection](../dovetail/src/compiler/typechecker/collect/traits.rs) |
| Associated types and GATs in implementation blocks; associated output equality in bounds and inference | [associated-type tests](../dovetail/tests/associated_type_bounds.rs), [trait tests](../dovetail/tests/traits.rs), [bound regression tests](../dovetail/tests/trait_bound_review.rs) |
| Transitive and multiple inheritance, flattened inline requirements, direct-parent implementation precedence, provider ambiguity | [inheritance tests](../dovetail/tests/trait_extends.rs), [flattening](../dovetail/src/compiler/typechecker/collect/trait_flatten.rs), [bound regressions](../dovetail/tests/trait_bound_review.rs) |
| Default methods/properties, overrides, diamond handling, generic implementation blocks and classes, method-level generic defaults | [default tests](../dovetail/tests/trait_defaults.rs), [collection regressions](../dovetail/tests/trait_collect_review.rs), [default inference](../dovetail/src/compiler/typechecker/infer/trait_defaults.rs) |
| Module → imported extension → implementation resolution; explicit method/property qualification; ambiguity diagnostics | [disambiguation tests](../dovetail/tests/trait_disambiguation.rs), [trait inference](../dovetail/src/compiler/typechecker/infer/traits.rs) |
| No overlapping implementations, including shaped generic implementations | [coherence rules and unit tests](../dovetail/src/compiler/typechecker/rules/coherence.rs) |
| Interface declaration checks, type-position gate, coercion, intersections, upcasts, bare `Self` returns | [interface tests](../dovetail/tests/interfaces.rs), [coercion](../dovetail/src/compiler/coerce.rs), [lowering regressions](../dovetail/tests/trait_lowering_review.rs) |
| Shared interface representations, erased ABI bridges, distinct generic application dispatch | [codegen](../dovetail/src/compiler/codegen/mod.rs), [trait tests](../dovetail/tests/traits.rs), [lowering regressions](../dovetail/tests/trait_lowering_review.rs) |
| `TraitObject` → `InterfaceObject`, `$TraitObj$` → `$IfaceObj$` | [typed IR](../dovetail/src/compiler/typechecker/types.rs), [name construction](../dovetail/src/common/types.rs); old names survive only in historical documents and test names/comments |
| Generic defaults, inherited overload dispatch, ordinary/GAT projections, abstract resource continuations | [completion regressions](../dovetail/tests/trait_completion.rs), [advanced examples](../book/26-advanced-generics.md#267-generic-trait-defaults) |
| Book coverage | [Part 6 §6.11](../book/06-type-system.md#611-interfaces-and-interface-types), [Part 8 §8.5–8.7](../book/08-traits.md#85-trait-and-interface-inheritance); 11 new/corrected runnable examples validated during Phase 6 |

## Obsolete proposals and current boundaries

| Earlier proposal or claim | Current decision |
|---------------------------|------------------|
| Any object-safe trait may be a value type | Obsolete. Only `interface` opts into value types; plain traits remain contracts for bounds/implementations. |
| Three-field objects with `type_info` arrays and runtime type-info arguments | Obsolete, never implemented. Objects hold `(data, vtable)`; wrappers bridge erased and concrete signatures. |
| Flat intersection member vtables | Superseded by component vtables and nested super-vtable references; upcasts are static. |
| A separate parent implementation fills a child implementation's requirements | Obsolete. Child implementations supply the flattened requirements inline or use defaults. |
| Same-name overloads inherited through `extends` | Implemented: distinct inherited parameter lists form overloads, with separate dispatch identities. |
| Defaults always work for every implementor and every method | Generic classes and method-level generics now support defaults. Classes still supply static members explicitly. Conflicting defaults for the same signature require an explicit body. |
| Associated-type/GAT defaults (`type Output = ...` inside a trait) | Obsolete for the current design. Trait declarations name associated types; implementation blocks must define them. |
| Associated-type definitions inside class bodies | Obsolete. Use an `implement Trait for Class` block for these definitions. |
| Unrestricted abstract associated-type/GAT projection | `P.Output` and `W.Wrapped<T>` work through bounds, normalizing via equalities or concrete implementations. Unknown outputs retain their identity and gain no unproven capabilities. |
| `use` works from only an abstract `Usable` bound | Supported when the continuation preserves the symbolic `R.Wrapped<U, E2>` result. The helper cannot assume a concrete wrapper without evidence. |
| A trait declaration has a trailing `where` clause; inline `<T: Bound>` syntax everywhere | Obsolete syntax claims. Trait declarations take plain parameters and optional `extends`; implementation and method constraints use their supported `where` clauses. |
| An implementation method may strengthen its trait method's requirements | Obsolete. Implementations must honor the declared contract rather than demand additional capabilities from callers. |
| Bare-parameter blanket implementations (`implement <T> Trait for T`) | Unsupported and retired from the current scope; shaped generic targets such as `Box<T>` remain supported. |
| Specialization allows overlapping implementations; bounds can prove disjointness | Obsolete. Coherence rejects overlap, and `where` bounds do not disprove it. Named-extension priority does not authorize overlapping trait implementations. |
| Negative bounds, Scala-style self types/linearization, argument-position interface-to-generic sugar | Exploratory proposals retired from the current scope. They are not prerequisites for completion. |
| Every generic type gets a separate runtime representation | Superseded by [full erasure](full-erasure-design.md) and the array/interface representation design; function monomorphization remains. |

The unsupported boundaries above are explicit limitations, not claims of feature
parity with every historical sketch. Runtime type-ID recovery and tuple
extension remain separately tracked designs in the [TOC](toc.md).

## Validation

The nine integration suites `traits`, `interfaces`, `trait_extends`,
`trait_defaults`, `trait_disambiguation`, `associated_type_bounds`,
`trait_bound_review`, `trait_collect_review`, and `trait_lowering_review` pass.
All 10 coherence unit tests pass. This audit changes documentation only; it does
not implement the retired proposals.
