# Tuple Scalar Erasure Design

**Status:** Superseded by [tuple-multivalue-codegen-design.md](tuple-multivalue-codegen-design.md) — this note's sketch has been promoted into a full design (hybrid unboxed/boxed representation, box/unbox boundaries, `Uint128` substrate, phased plan). Kept for historical context.
**Date:** 2026-02-23

## Idea

At codegen, tuples are expanded ("erased") into multiple scalar values instead of being heap-allocated WASMGC structs. A `(Int32, Bool)` becomes two `i32` values — no struct allocation, no GC pressure.

For function returns, this leverages the **WASM multi-value** extension (Phase 4, enabled by default in wasmtime and all major runtimes).

## Current Implementation

Tuples are synthetic records:
- `Type::Tuple(Vec<Type>, MangledName)` in the type system
- `ensure_tuple_type_def()` generates a `RecordTypeDef` with fields `_0`, `_1`, ...
- Codegen: `StructNew` to create, `StructGet` to access
- Each distinct tuple type gets a WASMGC struct type

## Proposed Change

No changes to the typechecker — `Type::Tuple` stays as-is. The erasure is purely a codegen concern:

| Context | Current | Erased |
|---------|---------|--------|
| Function params | Single ref param | N scalar params |
| Function returns | Single ref return | Multi-value return |
| Local variables | One ref local | N scalar locals |
| Tuple creation | Emit fields + `StructNew` | Emit fields (done) |
| Field access `t._0` | `StructGet(idx)` | `LocalGet(base + idx)` |
| Destructuring | Extract from struct | Values already flat |

### Multi-value encoding

`wasm-encoder` already supports this — `types.ty().function(params, results)` takes `Vec<ValType>` for both. Zero encoder changes needed.

### Naming convention for expanded locals

Internally tracked by index range: `VarName → (base_index, count)`. No string-level naming needed.

## Complexity Assessment

### Feasible (function params, returns, locals)
- `type_to_valtypes(ty) -> Vec<ValType>` helper that flattens tuples recursively
- Locals mapping changes from `VarName → u32` to `VarName → (u32, u32)` (base, count)
- Tuple creation: just remove `StructNew`
- Destructuring: values already on stack, `LocalSet` each

### Tricky (field access on expressions)
- `f()._1` puts N values on stack from the call — need temp locals to capture them, then `LocalGet` the desired one

### Problematic (tuples in data structures)
- **Array elements**: WASMGC arrays are single-type — cannot flatten a `(Int32, Bool)` into an array slot
- **Record/class fields**: could expand struct fields, but high complexity
- **Trait objects / Any**: need boxed representation
- **Closure captures**: environments are structs, same issue as record fields

These cases would need a fallback to the current struct representation.

## Conclusion

The idea is sound and well-supported by WASM multi-value, but the **hybrid approach** (erase in some contexts, keep structs in others) adds significant complexity. The typechecker would need to track which tuples can be erased vs which need struct backing, or codegen would need to handle both representations for the same type.

**Recommendation:** Revisit when the compiler is more mature. The current struct-based approach is correct and sufficient. When performance matters, the highest-value subset (function returns) could be tackled first.
