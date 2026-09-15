# Trait-Objects Removal Design

> **Superseded** by [interface-objects-design.md](interface-objects-design.md), which consolidates this document with [interfaces-design.md](interfaces-design.md) and carries the implementation plan.

This **short** design records how Dovetail stops letting *any* object-safe trait be used as a runtime value and instead **gates fat pointers behind the `interface` keyword** ([interfaces-design](interfaces-design.md)). It is a surface-language change, not a removal of machinery.

**Status:** Obsolete / superseded in full by [interface-objects-design.md](interface-objects-design.md), now implemented. The text below is historical, including its migration, representation, and optional future proposals; none are outstanding requirements. See the [audit](trait-implementation-status.md).

---

## 1. Why

Trait objects make "can this be a runtime value?" an **implicit, use-site** property of a full-power trait — you discover at the use site whether the trait is object-safe. The `interface` keyword turns that into an **explicit, declaration-time** opt-in: the object-safety check runs because you wrote `interface`, and the error lands on the declaration. See [interfaces-design §1, §3](interfaces-design.md). Same capability, same codegen; only the gate moves.

Because an interface **is** a trait (same construct, any type may implement it — [interfaces-design §1.1](interfaces-design.md)), this loses **no capability**: every value-level thing a trait object can do, an interface object can do, including dynamic dispatch over non-class types.

---

## 2. What changes for users

| Before | After |
|---|---|
| Any object-safe `trait` in type position → trait object | Only an `interface` in type position → interface object; a plain trait there is an error |
| Object-safety discovered as a **use-site** error ("can't make `X` into an object") | Object-safety checked at the **interface declaration**, with a local error ([interfaces-design §3](interfaces-design.md)) |
| `Display and Equatable` (trait intersection) in type position | `I1 and I2` (interface intersection) in type position |
| To box a type, nothing extra — any object-safe trait works | To box a type, declare its contract `interface` (explicit opt-in) |

**No capability is dropped.** The only traits that can no longer appear in type position are those that were **never object-safe** (associated types, `Self`-in-params, generic methods — `Equatable`, `From<T>`, `Awaitable<T>`, …); using those as a trait object is already an error today. They remain usable as **bounds**.

---

## 3. Why generic methods can't be dynamic

A monomorphizing AOT compiler has neither a JIT (C#, which instantiates generic virtual methods at runtime) nor type erasure (Java, one boxed impl). A generic method reached through a vtable would need every `(impl, T)` pair known at compile time, but the call site holds an erased receiver, so the `T` set is not enumerable — the same wall NativeAOT/IL2CPP hit. Hence `interface` methods are non-generic, enforced at the declaration ([interfaces-design §3](interfaces-design.md)). (`Self`-return is *not* in this category — it is allowed and reinterpreted as the interface type.)

---

## 4. Codegen impact

Essentially none — this is a gate, not a re-implementation:

- The trait-object fat pointer `(data, type_info, vtable)`, wrappers, single calling convention, and de-monomorphized vtables are **reused verbatim** by interface objects ([traits-design §8.3–8.5](traits-design.md)). The runtime value is the **interface object**; the vtable struct is `$IfaceVtable`.
- The only codegen-relevant change: a vtable/interface object is emitted **only** for a type declared `interface`. Traits are bound-only → monomorphized, static dispatch, no vtable.
- Inference/codegen complexity is unchanged (see [interfaces-design §1.2](interfaces-design.md)).

---

## 5. Migration order

1. **Add `interface`** — keyword sharing `trait` parsing/collect/infer, plus the declaration-time trait-safety check; interface objects reuse trait-object codegen ([interfaces-design §9](interfaces-design.md) phase 1). Object-safe traits remain boxable during this step.
2. **Re-declare dynamically-used traits as `interface`** (all pass the check today):
   - `Iterator<T>` — [dovetail/prelude/src/Iterator.dove](../dovetail/prelude/src/Iterator.dove)
   - `HashAlgorithm`, `AeadAlgorithm`, `KeyExchange` — `standard-crypto`
   - `LogSink` — `standard-io-log`
   - `standard-tls` returns (`hashFor`, `aeadFor`, …)
   - Leave bound-only traits (`Iterable<T>`, `Equatable`, `From<T>`, …) as `trait`.
3. **Close the gate** — a plain trait name in type position becomes an error directing the user to `interface`. Trait-object surface content in [traits-design §8](traits-design.md) is reframed as "interface objects"; the fat-pointer mechanics stay (now reached only via `interface`).

Each step keeps the workspace compiling (`cargo run -- check`).

---

## References

- [interfaces-design.md](interfaces-design.md) — `interface` = trait + safety-check + boxable.
- [traits-design.md](traits-design.md) — Traits and the fat-pointer mechanics (§8) reused by interface objects.
- [trait-design-appendix.md](trait-design-appendix.md) — Object safety (§6.1).
