# Type-ID Extension for Generic Type Reification

**Status:** Implemented
**Extends:** [full-erasure-design.md](full-erasure-design.md) (§2.5, §2.7, §6.2)
**Depends on:** [nominal-type-identity.md](nominal-type-identity.md) — single-rec-group emission; the base-type checks (§2.5) and dense set sizing (§2.4) here assume it

---

## 1. Motivation

Under [full erasure](full-erasure-design.md), every instantiation of a generic record / enum / class shares **one** WASM struct type — `Option<Int32>` and `Option<String>` have the same WASM `$Option` representation. This makes cross-instantiation subtyping free and deletes ~1500 lines of variance-cast synthesis, but it costs runtime precision on `is` / `as` / type-annotated `match`:

- `x is Option<Int32>` and `x is Option<String>` are indistinguishable — both succeed for any `Option`.
- The base design documents an erasure warning (full-erasure-design.md §6.2), but its runtime check cannot enforce the distinction. General warnings about potentially failing casts are separate and remain applicable.

A second, related hole — same-shape *non-generic* types aliasing under `is` / `as`, a bug fixed in the compiler — is covered separately in [nominal-type-identity.md](nominal-type-identity.md). Its implemented fix (single module-wide rec group) is a prerequisite for this extension: it is what makes the base-type checks below sound. The type\_id here stays scoped to what only it can do — discriminating instantiations that intentionally share one WASM struct under erasure.

This extension adds a **type\_id** field and **compile-time pre-computed subtype sets** to restore full runtime type discrimination for erased parameters, including variance-aware checks. It refines the deliberately imprecise erased `is` / `as` / typed-match behavior: checks that previously accepted the wrong type arguments now fail. Assignment and cast-back-on-read behavior are preserved, apart from layout offsets.

**Scope:** IDs reify generic records, enums, and classes, including fully-invariant parameters. Every class in an inheritance-connected hierarchy containing a generic class carries an ID, including non-generic ancestors and descendants, so the hierarchy has one consistent inherited layout. Other non-generic types need no ID: nominal WASM type identity already distinguishes them. Interface objects, arrays, tuples, closures, and generic newtypes are outside this extension's runtime representation scope. They may still occur as concrete type arguments in a recorded instantiation; comparing those arguments uses the language's type relation, without inspecting payload values.

**Compilation model:** one final whole-program compilation assigns IDs and builds complete membership sets after concrete construction and check types have been discovered. Independently compiled modules exchanging GC objects and dynamically introduced instantiations are outside this design.

---

## 2. Design

### 2.1 Type-ID field

Every generic record / enum carries an immutable **type\_id** (`i32`) as its first field, regardless of parameter variance. For classes, if any member of an inheritance-connected hierarchy is generic, reserve this field at the hierarchy root and inherit it throughout the hierarchy. It identifies the concrete most-derived class, including a non-generic class in such a hierarchy. There is exactly one ID field per object; subclasses never insert another ID before inherited fields.

This deliberately adds the field to non-generic roots of generic subclasses as well as non-generic descendants of generic bases. It keeps the inherited field prefix consistent. Hierarchies with no generic classes are unchanged.

The struct layout from full-erasure-design.md §2.5 gains one field:

```
;; Full erasure base:
struct $Option {}                              ;; base (empty)
struct $Some <: $Option { f0: anyref }         ;; T erased
struct $Pair { f0: anyref, f1: anyref }        ;; A, B erased (both invariant)

;; With type-ID extension:
struct $Option { f0: i32 }                     ;; type_id (covariant T)
struct $Some   <: $Option { f0: i32, f1: anyref }  ;; type_id + erased T
struct $Pair   { f0: i32, f1: anyref, f2: anyref } ;; type_id + erased A, B
                                                   ;;   (invariant — still gets a type_id)
```

Non-generic records, enums, and entirely non-generic class hierarchies are unchanged. Nominal emission makes their WASM struct checks precise.

All existing fields of ID-bearing types shift by +1, including concrete fields, inherited fields, and vtable fields. Cast-back-on-read (full-erasure-design.md §2.7), box-on-write (§2.6), and class vtable access (§4.3) use the adjusted layout.

### 2.2 Type-ID assignment

After final whole-program discovery, every concrete runtime construction type in scope receives a unique module-local `i32` constant. This includes non-generic classes in ID-bearing hierarchies; abstract classes need no construction ID. Enum variants share their instantiation's id; variant discrimination stays `ref.test` (sound — variants are distinct members of the module's rec group, see [nominal-type-identity.md](nominal-type-identity.md) §2). The assignment uses a single global namespace:

```
TYPE_ID_OPTION_INT32    = 0
TYPE_ID_OPTION_STRING   = 1
TYPE_ID_OPTION_ANIMAL   = 2
TYPE_ID_OPTION_DOG      = 3
TYPE_ID_RESULT_OK_ERR   = 4
...
```

The type\_id is set at construction time and **never modified**. Subtype assignments (including the cross-instantiation subtyping that full erasure now enables for free — full-erasure-design.md §8) remain pure no-ops at the WASM level; the subtype sets (§2.3) encode the variance/inheritance relationship at compile time, so the type\_id does not need updating when a value flows from a subtype slot into a supertype slot.

### 2.3 Subtype sets (compile-time pre-computation)

For each expected type that appears at an `is` / `as` / type-annotated match site, the compiler pre-computes the set of type\_ids that satisfy the variance-aware subtype check. Membership is defined as `actual <: expected` on complete concrete Dovetail types. The compiler should share this relation with static type checking, but cannot reuse general assignment coercions: the previous `is_assignable` cross-class branches accepted ancestry without validating substituted ancestor arguments (see `dovetail/src/compiler/typechecker/infer/types.rs`). The implementation fixes static class checking and uses the same representation-preserving relation for runtime membership. Parent substitutions are validated before variance checks; value-producing coercions remain outside this relation. Error-recovery shortcuts are not part of runtime subtype semantics.

**Example:** `x is Option<Animal>` where `T` is covariant.

All `Option` instantiations in the program: `Option<Int32>` (id=0), `Option<String>` (id=1), `Option<Animal>` (id=2), `Option<Dog>` (id=3). Covariant `T`, expected = `Animal`. Valid: all ids where actual `T <: Animal`:

- `Int32 <: Animal` ✗ → id 0 excluded
- `String <: Animal` ✗ → id 1 excluded
- `Animal <: Animal` ✓ → id 2 included
- `Dog <: Animal` ✓ → id 3 included
- **Valid set = {2, 3}**

**Variance rules per parameter:**

| Variance | Check | Example |
|----------|-------|---------|
| Invariant | `actual == expected` | `Pair<A, B>`: A must match exactly |
| Covariant (`out`) | `actual <: expected` | `Option<out T>`: Dog <: Animal ✓ |
| Contravariant (`in`) | `actual :> expected` | `Consumer<in T>`: Animal :> Dog ✓ |

For multiple type parameters of the same generic definition, all must pass (conjunction). Nested arguments are compared recursively using the language's type relation; invariant arguments require semantic type identity after normalization.

For different class definitions, project the actual class through its declared parent chain, substituting concrete arguments at every step, until the expected class definition is reached. Then compare its arguments using that definition's variance. Do not zip a child's parameters with its parent's: their counts, order, and structure can differ.

For example, `Succeed<Int32>` projects to `Async<Int32, Never>`. It satisfies `Async<Int32, String>` because `Never <: String`, but fails `Async<String, String>`. A non-generic `Yield extends Async<Unit, Never>` similarly participates using its fixed parent arguments. Multi-level inheritance and parents such as `Async<Fiber<T, E>, Never>` require the same substitution rule.

Because generic function declarations are monomorphized (full-erasure-design.md §3.1), the expected type at each `is` / `as` site is always fully concrete and known at compile time. The actual type\_id is unknown at the site, but the set of *all possible* type\_ids is known (all registered construction types, including class descendants). Completeness nevertheless requires explicit discovery of all reachable concrete construction types, including generated constructors and non-generic classes in ID-bearing hierarchies, before sets are finalized. An erased TypeDef alone does not enumerate those instantiations. Expected types appearing only at checks must also be collected, even if they have no construction ID. No later codegen step may introduce an unregistered construction.

### 2.4 Storage

In the baseline, sets are stored as WASM-GC byte arrays — one byte per type\_id, value 0 or 1. Each unique expected type at an `is` / `as` site produces one global array. Sites checking the same expected type share the array.

The baseline uses the single global namespace from §2.2 and sizes every array to the full assigned ID range. This guarantees that every valid object passing the base check has an in-bounds ID, including descendants of a checked class. Arrays are initialized once and never modified.

The example below assumes the module has only the four illustrated construction IDs. If it also constructs `Result` or other ID-bearing types, the array has additional zero entries.

Storage is an implementation detail: an empty valid set can become constant false, a singleton can become ID equality, and larger sets can use byte arrays. Dense IDs per record/enum definition or inheritance-connected class hierarchy, and packed bitsets, are possible later optimizations; they do not change membership semantics.

```wasm
;; Subtype set for "is Option<Animal>" (4 Option instantiations in program)
(global $set_option_animal (ref $i8_array)
    (array.new_fixed $i8_array 4
        i32.const 0    ;; id 0 (Option<Int32>)    → no
        i32.const 0    ;; id 1 (Option<String>)   → no
        i32.const 1    ;; id 2 (Option<Animal>)   → yes
        i32.const 1    ;; id 3 (Option<Dog>)      → yes
    )
)
```

### 2.5 Runtime check

An `is` / `as` / type-annotated match on a generic type instantiation uses the following short-circuit sequence. Steps 2–3 run only if step 1 succeeds; a failed base check must not cast or index the set:

1. **Base type check:** `ref.test` with the erased WASM struct type (e.g., "is this an `Option`?"). This is what full-erasure-design.md §6.2 already does. **Sound only under [nominal emission](nominal-type-identity.md):** erased shapes are uniform (`struct { i32 }` bases), so with per-type rec groups `$Option` and a same-shape `$Maybe` would canonicalize to the same WASM type and this test could not discriminate — with the single rec group they are distinct members and it can.
2. **Type-ID read:** `struct.get` field 0 → `i32` type\_id.
3. **Set membership:** `array.get_u` from the pre-computed set → 0 or 1.

```wasm
;; x is Option<Animal>
;; Step 1: base type check (ref.test $Option) already done
;; Steps 2+3: type_id lookup
global.get $set_option_animal     ;; (ref $i8_array)
local.get $x
ref.cast (ref $Option)
struct.get $Option 0              ;; type_id → i32
array.get_u $i8_array             ;; set[type_id] → 0 or 1
```

A failed membership check returns false for `is`, skips the arm for typed match, and follows the existing cast-failure path for `as`, before payload access. Successful casts preserve the original object and ID.

For **non-generic record / enum / class expected types**, the existing single `ref.test` with the concrete WASM type index is unchanged — no type\_id is read, even when the object carries an ID because of its class hierarchy. It is precise under [nominal emission](nominal-type-identity.md).

### 2.6 Why cross-instantiation assignment doesn't update the type\_id

The type\_id records the **original instantiation** (e.g., `Option<Dog>` = id 3). When assigned to `Option<Animal>` (covariant `T`) or — under full erasure — to an `Async<Int32, String>` supertype slot from `Succeed<Int32>` (full-erasure-design.md §8), the type\_id stays unchanged. This is correct because the subtype set for each expected type already includes every type\_id that satisfies the variance/inheritance relationship at compile time.

```dovetail
function checkAnimal(value: Any): Bool = value is Option<Animal>
function checkString(value: Any): Bool = value is Option<String>

let x: Option<Dog> = Some(myDog)     // type_id = 3
let y: Option<Animal> = x            // no-op (same WASM type, type_id stays 3)
checkAnimal(y)                   // set_option_animal[3] → 1 ✓
checkString(y)                   // set_option_string[3]  → 0 ✓ (Dog ≠ String)
```

```dovetail
let s: Succeed<Int32> = Succeed(42)        // type_id = TYPE_ID_SUCCEED_INT32
let a: Async<Int32, String> = s            // no-op (WASM upcast, type_id stays)
a is Succeed<Int32>                         // set lookup includes that type_id ✓
```

No copying, no mutation — the sets encode the relationship at compile time.

### 2.7 Construction identity and inference

“Original instantiation” means the concrete type of the constructor after type inference and substitution, before subsequent widening conversions. It does not mean the narrowest type recoverable from payload values, nor the type of the variable receiving the object.

For `let x: Option<Animal> = Some(myDog)`, the ID depends on the constructor type resolved by contextual inference: if it resolves to `Option<Animal>`, that is the ID; if it resolves to `Option<Dog>` followed by widening, the ID is `Option<Dog>`. This extension does not change inference. Lowering must preserve the resolved constructor type explicitly rather than recover it from an erased name or a destination slot.

Unconstrained arguments such as those in `None` and `Ok(99)` follow the existing inference rules. If `None` resolves to `Option<Never>`, it satisfies both `Option<Int32>` and `Option<String>` by covariance. This is a subtype check, not a test of exact instantiation equality. Likewise, an `Ok(99)` resolved as `Result<Int32, Never>` satisfies `Result<Int32, String>`. Any cached zero-payload enum values must preserve the resolved instantiation ID; a single shared `None` cannot stand for distinct resolved construction types.

---

## 3. Changes from Full-Erasure Base

### 3.1 Struct layout (extends full-erasure-design.md §2.5)

Add immutable `i32` type\_id as field 0 to generic records/enums and to the root of each ID-bearing class hierarchy (§2.1). All existing fields in those layouts shift by +1. Descendants inherit the ID field, including non-generic descendants.

```
;; Enum example:
struct $Option { f0: i32 }                              ;; type_id
struct $None   <: $Option { f0: i32 }                   ;; inherited type_id, no payload
struct $Some   <: $Option { f0: i32, f1: anyref }       ;; type_id + erased T

;; Record example:
struct $Pair { f0: i32, f1: anyref, f2: anyref }        ;; type_id + A, B erased

;; Class example (with vtable):
struct $Box { f0: i32, f1: ref $BoxVtable, f2: i32, f3: anyref } ;; ID + vtable + identity hash + T

;; Sealed class hierarchy:
struct $Async   { f0: i32, f1: ref $AsyncVtable, f2: i32 }
struct $Succeed <: $Async { f0: i32, f1: ref $SucceedVtable, f2: i32, f3: anyref }
struct $Yield   <: $Async { f0: i32, f1: ref $YieldVtable, f2: i32 } ;; non-generic, inherits header
```

### 3.2 Construction (extends full-erasure-design.md §2.6)

Emit `i32.const TYPE_ID` as the first field value. The type\_id constant comes from the resolved concrete construction type (§2.7), including the most-derived class for class construction. Base-class initialization must not replace that ID with a parent ID.

```wasm
;; Some(42) as Option<Int32>
i32.const TYPE_ID_OPTION_INT32   ;; type_id
i32.const 42
struct.new $Int32Box             ;; box the primitive
struct.new $Some                 ;; (type_id, boxed ref)
```

### 3.3 Field access (extends full-erasure-design.md §2.7)

Existing field indices for ID-bearing layouts shift by +1. Cast-back-on-read is unchanged except for the index.

```wasm
;; Reading payload from Some<Int32> (field 1 is the erased T, after type_id)
struct.get $Some, 1        ;; → anyref
ref.cast (ref $Int32Box)   ;; → (ref $Int32Box)
struct.get $Int32Box, 0    ;; → i32
```

Class vtable access shifts similarly: `struct.get $Box, 1` (was 0) for the vtable, `struct.get $Box, 3` (was 2) for the first user field. The existing mutable identity-hash field shifts from 1 to 2; it remains separate from the immutable type ID. Central layout helpers account for flattened user fields.

### 3.4 `is` / `as` / match (replaces full-erasure-design.md §6.2)

The base design's "base type only" check and compiler warning are replaced by the full type\_id + subtype set mechanism (§2.5). For checks in this extension’s scope, the erasure warning is no longer needed because type arguments are checked precisely. Unsupported representations retain their existing warnings or restrictions.

The compile-time resolution rules from full-erasure-design.md §6.1 (for `is` / `as` / match on type parameters *inside* generic functions) are unchanged: compile-time type queries can still be folded. A value test such as `x is Option<T>` with runtime `x` resolves its expected type during monomorphization and then uses the runtime check described here; knowing `T` does not reveal the dynamic type of `x`.

### 3.5 Compiler metadata requirements

The final compilation must retain a registry of concrete construction types, distinct from erased layout definitions, and a collection of concrete expected types used in runtime checks. Keys must include the nominal definition and all normalized type arguments; an erased `MangledName` is insufficient. The registry uses structural `instance_key(&Type)` keys from [codegen/instance_key.rs](../dovetail/src/compiler/codegen/instance_key.rs), which retain nominal definitions and recursively encode arguments independently of erased layout names.

After discovery is complete, assign module-local IDs and compute each expected type's membership over the registered concrete construction types using §2.3. Emit membership data and construction constants from that same registry. Cached package artifacts retain symbolic type identities; numeric IDs and sets belong to the final compilation. Discovery conservatively includes concrete types referenced by emitted code and signatures as well as construction sites. It walks finite type expressions rather than expanding hypothetical recursive generic fields indefinitely. WASI/WIT constructors retain complete concrete types through their emission helpers.

`Any` remains the universal top type for already-boxed values, including interface objects. Covariant widening to `Any` (and the corresponding contravariant parameter relation) does not create a wrapper. Other interface-containing generic arguments retain exact matching: a concrete value implementing a trait, or an interface-subset conversion, does not establish container subtype membership.

The standard Async interpreter keeps its internal values erased, but typed boundaries must reconstruct containers such as `Cause<E>` and `Result<T, Cause<E>>` from checked payloads. A cast cannot relabel an `Any` instantiation. Effect nodes retain their declared result types, and fork nodes create typed fiber handles through factories captured at their typed construction sites so completion state stays on the same object.

### 3.6 Record updates and generic match coverage

A record `with` expression allocates a new value with the update expression's resolved instantiation. It does not copy the source object's ID: a covariant `Box<Dog>` viewed as `Box<Animal>` can be updated with an `Animal`, so the new object must record `Box<Animal>`.

Exhaustiveness uses symbolic instantiations of the declared sealed leaves, independently of module-local construction IDs. Parent substitution and variance produce constraints for each leaf; typed arms partition those constraints into matching and remaining regions. The pattern matrix retains correlations with other pattern columns. Unrelated nominal leaves cannot overlap, regardless of their parameters. Guarded arms do not establish coverage, and a typed test on `Any` does not cover arbitrary values.

Finite non-generic sealed type-argument domains include abstract class types and `Never`, as well as their declared descendants. Open or generic descendant domains stay symbolic. Interface-containing arguments with unresolved parameters retain their subtype constraints; uncertainty cannot eliminate a possible leaf, including a `Never` instantiation.

Complete generic hierarchies such as `Succeed<T>` / `FailCause<E>` over `Async<T, E>` can be proved exhaustive. Partial or unproved coverage requires a fallback, with missing leaf constraints reported when available. Analysis bounds partition growth; exceeding the bound requires a fallback rather than silently accepting uncertain coverage.

The existing rejection of provably redundant class tests remains, using complete subtyping instead of ancestry alone. Runtime regression examples widen to `Any` when testing a relation already guaranteed by the static type. Monomorphization preserves dynamic nominal, interface-object, and `Any` match arms after substituting their target arguments.

---

## 4. Examples

The ID comments assume constructors resolve to the concrete types shown. Contextual inference must be preserved as specified in §2.7; a destination annotation alone does not determine the construction ID.

### 4.1 Covariant — Option

```dovetail
enum Option<out T> = None | Some(T)

let x: Option<Dog> = Some(myDog)      // type_id = TYPE_ID_OPTION_DOG
let y: Option<Animal> = x             // no-op (same WASM type, type_id unchanged)

function check(a: Any): Bool =
    a is Option<Animal>                // ref.test $Option + set lookup → true for Dog, Animal
```

### 4.2 Contravariant — Consumer

For a class `Consumer<in T>` whose use of `T` passes the language's variance rules, a value constructed as `Consumer<Animal>` retains that ID when widened to `Consumer<Dog>`. A runtime check for `Consumer<Dog>` includes the `Consumer<Animal>` ID because `Dog <: Animal`.

This example concerns classes. Interface objects have a separate representation and currently invariant generic arguments; this extension does not introduce contravariant interface-object conversions or runtime checks.

### 4.3 Multiple covariant parameters — Result

```dovetail
enum Result<out T, out E> = Ok(T) | Err(E)

let r: Result<Dog, IOError> = Ok(myDog)    // assume constructor resolves as Result<Dog, IOError>
let r2: Result<Animal, Error> = r          // no-op

function check(a: Any): Bool =
    a is Result<Animal, Error>             // set includes TYPE_ID_RESULT_DOG_IOERROR
                                           // because Dog <: Animal AND IOError <: Error
```

### 4.4 Invariant generic — Pair

Even when no type parameter is variant, the type\_id is what distinguishes instantiations at runtime — full erasure has collapsed them all to the same `$Pair` WASM struct:

```dovetail
record Pair<A, B> =                          // A, B both invariant
    first: A
    second: B

let p1: Pair<Int32, String> = Pair { first = 1; second = "hi" }
                                            // type_id = TYPE_ID_PAIR_INT32_STRING
let p2: Pair<Bool, Float64> = Pair { first = true; second = 3.14 }
                                            // type_id = TYPE_ID_PAIR_BOOL_FLOAT64

function check(a: Any): Bool =
    a is Pair<Int32, String>                    // set has 1 only at TYPE_ID_PAIR_INT32_STRING
                                                // p1 → true, p2 → false
```

Without the type\_id, `ref.test $Pair` would succeed for both `p1` and `p2` — they share the same WASM struct. The type\_id is the only thing that lets the check distinguish them.

### 4.5 Sealed class hierarchy — Async

Full-erasure-design.md §8 enables cross-instantiation subtyping for sealed hierarchies. Type-IDs extend that with precise `is` checks:

```dovetail
sealed abstract class Async<out T, out E>()
final class Succeed<out T>(public value: T) extends Async<T, Never>()
final class FailCause<out E>(public cause: Cause<E>) extends Async<Never, E>()

let s: Succeed<Int32> = Succeed(42)             // type_id = TYPE_ID_SUCCEED_INT32
let a: Async<Int32, String> = s                 // pure no-op (full-erasure §8)
let erased: Any = a
erased is Async<Int32, String>                 // ancestor projection + variance → true
erased is Async<String, String>                // ancestor argument mismatch → false
a is Succeed<Int32>                              // set lookup → true
a is Succeed<String>                             // set lookup → false (Int32 ≠ String)
a is FailCause<String>                           // ref.test $FailCause fails → false
```

---

## 5. Trade-offs vs Full-Erasure Base

### 5.1 What this extension adds

- **Full runtime type discrimination** for erased parameters. `x is Option<Int32>` and `x is Option<String>` are correctly distinguished.
- **Variance-aware checks.** `x is Option<Animal>` correctly matches `Option<Dog>` (covariant) and rejects `Option<String>`.
- **Sealed-hierarchy reification.** `a is Succeed<Int32>` for an `Async<T, E>` reference works precisely, building on the cross-instantiation subtyping that full erasure already gave us for free.
- **Precise checks no longer need an erasure warning.** Remove the warning from full-erasure-design.md §6.2 only for checks covered by this extension.

### 5.2 What it costs

- **Type-ID overhead:** One extra `i32` field per generic record/enum value and per object in an ID-bearing class hierarchy. Actual allocation overhead depends on engine layout and alignment.
- **Subtype set storage:** One byte array per unique expected type at `is` / `as` sites (sizing per §2.4). The baseline requires N × M bytes for N assigned IDs and M distinct expected types, plus array/global overhead; size must be measured rather than assumed small.
- **Runtime check overhead:** a short-circuit base check, ID read, and set lookup instead of `ref.test` alone. Still O(1) with byte-array membership.
- **Field index shift:** All existing field accesses on ID-bearing layouts shift by +1. Must be handled consistently in codegen (struct.new, struct.get, struct.set, vtable offsets).
- **Codegen complexity:** Subtype set computation requires complete construction discovery, precise concrete subtyping through inheritance, and consistent layout metadata. Baseline set computation performs N × M subtype queries, whose individual cost depends on type structure.

### 5.3 What stays the same

- Cross-instantiation subtyping remains a pure no-op at the WASM level (type\_id is never modified).
- Cast-back-on-read for erased fields is unchanged (just shifted index).
- Box-on-write for primitives in erased slots is unchanged.
- Generic function declarations remain monomorphized.
- Arrays remain specialized per primitive element type / shared `anyref` for reference elements.
- Closures (full-erasure-design.md §3.2) are unaffected — this extension addresses data types only. Type discrimination on closure values, if ever needed, is out of scope here.

---

## 6. Validation requirements

The five formerly ignored generic `is` / `as` tests in [any_type.rs](../dovetail/tests/any_type.rs) are enabled. Additional behavioral regressions live in [runtime_type_ids.rs](../dovetail/tests/runtime_type_ids.rs), [runtime_type_patterns.rs](../dovetail/tests/runtime_type_patterns.rs), and [runtime_type_coverage.rs](../dovetail/tests/runtime_type_coverage.rs); codegen structural checks cover immutable IDs and shared membership tables. [Library identity tests](../standard-io/test/runtimeTypeIdentityTest.dove) exercise typed causes, results, and fiber handles. Existing class identity, nominal identity, field layout, and ABI tests remain part of validation.

The existing tests are only the starting point. Validation must also cover:

- Positive and negative covariance, contravariance, invariant, and nested-generic checks.
- Ancestor checks with correct and incorrect arguments, multi-level substitution, and parents with reordered or nested arguments.
- Non-generic descendants of generic bases, generic descendants of non-generic roots, and ordinary non-generic checks on ID-bearing objects.
- Constructor inference versus later widening, `Never` arguments, and zero-payload variants with distinct resolved construction types.
- Wrong-argument `as` failure before payload access, and typed-match fallthrough on failed membership.
- Same-shape unrelated generic types failing the base check without indexing; class descendants safely indexing ancestor sets.
- Types appearing only as check targets, constructions in generated or monomorphized code, and cached-package inputs included in final discovery.

---

## 7. Boundaries and deferred decisions

- **Cross-package assignment:** cached packages contribute symbolic concrete types to final whole-program discovery. IDs need only be consistent within the emitted module; they are not a persistent ABI. Separate modules exchanging GC values or dynamic loading would require a new coordination design.
- **Subtype set growth:** the baseline is N × M bytes. Measure representative programs before selecting dense per-family IDs, bitsets, or other representations. Empty/singleton membership optimization does not affect semantics.
- **Runtime type values:** exposing types through `typeof` or reflection would require additional metadata and a language-level contract. Module-local IDs alone do not provide that API.
- **Arrays:** array demonomorphization has already landed. Reference-element arrays share one WASM array representation, and some primitive types share storage representations too; direct runtime checks cannot recover all element-type distinctions. WASM arrays have no user-defined header field for an ID. Wrapping arrays would require a separate design covering construction, aliases, and conversions; wrapping only at an `Any` boundary must not mistake a widened static type for the original construction type. This extension leaves that limitation unresolved. `String` currently uses a struct wrapping its backing byte array, so it is already distinguishable from `Array<Int8>` through its outer representation.
- **Other erased representations:** interface objects, tuples, closures, and generic newtypes need separate scope and representation decisions before claiming complete runtime discrimination for all Dovetail values. Warnings or restrictions associated with unsupported checks must remain; only checks made precise by this extension can lose their erasure warning.
