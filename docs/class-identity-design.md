# Class Identity and the `class` Constraint

This document designs reference equality and identity hashing for Dovetail classes through a prelude module, `ClassIdentity`, and a general type-parameter constraint, `where T: class`. Every class instance carries one hidden hash slot, assigned lazily on first hash. Users implement `Equatable` and `Hashable` explicitly when they want identity semantics. There are no annotations, new derive macros, or automatic trait implementations.

**Implementation status:** Implemented. The compiler, prelude, runtime migrations, and book now follow this design. See §8 for implementation and regression-test locations.

**In scope:** `ClassIdentity.equals` and `ClassIdentity.hash`; a universal lazy identity-hash slot in class layout; the `class` constraint throughout generic declarations; explicit trait implementation examples; removal of `MutexId` and `WaiterId` from `standard-io` while preserving allocation timing and scheduler behavior.

**Out of scope:** Macro changes; automatic `Equatable`/`Hashable` implementations; changes to general trait inheritance; identity observation for records, enums, newtypes, arrays, and interface objects; weak references; new process-wide promise or queue APIs.

**Related designs:** [classes-design](classes-design.md), [traits-design](traits-design.md), [full-erasure-design](full-erasure-design.md), [interface-objects-design](interface-objects-design.md).

## 1. Motivation

Before this feature, the language had no reference-equality primitive. Code that needed to ask “is this the same object?” stored and compared an id surrogate. In `standard-io`:

- `MutexId` distinguished mutexes when validating a waiting fiber or removing a held mutex. IDs were allocated at construction using the process-wide `MutexIds` counter and `freshId()`, including an exhaustion check.
- `WaiterId` identified the runtime's condition-variable registrations. `MakeWaiter` allocated an id, and `Await`/`Wake` carried it. The runtime mapped ids to suspended-fiber stacks and tracked ids in reverse registrations and pending wakes.

Class references already identify their objects. Exposing reference equality removes these counters and lookup indirections without comparing mutable fields. Equality itself needs no extra storage; identity hashing uses one hidden slot per class instance (§5).

Allocation timing is a separate concern. `Mutex.make` deliberately returns an `Async` so that executing a reusable program allocates a fresh mutex each time. Promise and queue construction must preserve the same property after their waiter ids disappear (§6).

## 2. `ClassIdentity` Module

The name spells out the operation's subject and meaning: identity of a class instance. It avoids abbreviating “reference” and does not imply that Dovetail exposes raw pointers or a reference-wrapper type.

Prelude declaration:

```dovetail
module standard.prelude.ClassIdentity

/// True exactly when both arguments refer to the same class instance.
public function equals<T>(left: T, right: T): Bool where T: class = intrinsic

/// A per-object hash, assigned on first use and stable for its lifetime.
public function hash<T>(value: T): Int64 where T: class = intrinsic
```

`equals` compares the object itself, never its fields or an `Equatable` implementation. Aliases of the same object compare equal; separate allocations compare unequal even if every field is equal. Mutation does not change the result. Abstract base references and generic class instances are supported by both functions.

Both arguments use the same type parameter. Normal inference and assignability apply. Values from one inheritance hierarchy can be compared through a common base type; callers can choose that type explicitly with `ClassIdentity.equals<Base>(left, right)`. This API does not infer a universal object type, accept `Any`, or introduce heterogeneous equality between unrelated classes.

**Equality lowering:** evaluate each argument once, in the normal argument evaluation order, and emit `ref.eq` on the class references. It neither reads nor initializes the hash slot and invokes no trait dispatch or user code. Identity must survive existing upcasts and generic variance conversions without reconstructing the class object. If a compiler path temporarily erases a class reference, lowering must recover the underlying reference rather than compare a wrapper. Hash lowering is specified in §5.

Both intrinsics must also work when used as function values; provide callable adapters if the existing function-value lowering requires them. Constraints must be checked when those function values are specialized, just as for direct calls.

## 3. The `class` Constraint

`where T: class` guarantees that `T` is a class type. It is a built-in category constraint, available wherever a declaration already accepts type-parameter bounds. It introduces neither a trait nor a universal base class, and it grants no members or constructor operations.

```dovetail
public function sameInstance<T>(left: T, right: T): Bool where T: class =
    ClassIdentity.equals(left, right)

public function instanceHash<T>(value: T): Int64 where T: class =
    ClassIdentity.hash(value)

public function sameDisplayedInstance<T>(left: T, right: T): Bool
    where T: class + Display =
    ClassIdentity.equals(left, right)
```

Bounds compose with `+`, matching the current bound syntax. `class` has no type arguments, cannot be qualified or shadowed by a declaration, and is only valid as a bound. It is not a value type: a parameter cannot have the type annotation `value: class`.

### 3.1 Satisfaction and implication

- Any concrete class type satisfies `class`, including abstract classes and generic class applications. A `Box<T>` is a class regardless of whether its element type `T` is a class; the declaration's own bounds still apply normally.
- A type parameter constrained by `class` satisfies another function's `class` requirement. Generic forwarding is supported without knowing the eventual concrete class.
- An existing bound `T: Animal`, where `Animal` is a class, implies `T: class`. The converse does not hold: `T: class` alone provides no proof that `T` is an `Animal`.
- Type aliases follow their resolved target: an alias of a class satisfies the constraint. Newtypes remain distinct and do not acquire the constraint from a wrapped class reference.
- Records, enums, tuples, arrays, primitives, `Any`, and interface-object types do not satisfy it. An interface reference does not prove a class representation even if the particular value originated from a class.
- `T: Equatable`, `T: Hashable`, and other trait bounds alone do not imply `class`. An unconstrained generic body cannot use class identity merely because all currently observed callers pass classes.

`T: class` is sufficient for both identity intrinsics because every class has hash storage. It does not imply `T: Equatable` or `T: Hashable`; those traits still require explicit implementations. No additional marker or hash-specific bound is needed.

No new inline-bound syntax is required by this feature; the examples and required surface use `where` clauses.

### 3.2 Compiler integration

This is an ordinary checked bound, not an intrinsic-specific rule deferred until code generation. Validate it during generic body checking, call and overload resolution, generic type application, function-value specialization, and generic impl applicability. Preserve the constraint through substitution, monomorphization, and cached dependency signatures.

The parser stores `TypeBound::Named` and `TypeBound::Class` entries in `TraitConstraint`. The resolved `TraitBound` representation distinguishes `Named(NamedTraitBound)` from `IsClass`, which needs no FQN. A fabricated trait/class named `class` would incorrectly route this constraint through nominal lookup.

Collection and inference both resolve bounds today; both must recognize the new case. All consumers must distinguish trait membership, a specific class-subtype requirement, and the class category. In particular, generic impl resolution must not try to satisfy `class` by searching for a trait implementation.

Diagnostics should state the failed requirement, for example: “type 'Int32' does not satisfy 'class' required by type parameter 'T'.” For an unconstrained generic argument, identify the missing `class` or specific class bound. A later Rules/codegen assertion may defend the invariant, but must not be the first place an invalid generic program is rejected.

## 4. Explicit Trait Implementations and Inheritance

Classes receive no implicit `Equatable` or `Hashable` implementation. A user who wants identity semantics writes ordinary implementations:

```dovetail
implement Equatable for MutexState =
    public function equals(self, other: MutexState): Bool =
        ClassIdentity.equals(self, other)

implement <T> Equatable for Box<T> =
    public function equals(self, other: Box<T>): Bool =
        ClassIdentity.equals(self, other)

implement <T> Hashable for Box<T> =
    public function hash(self): Int64 =
        ClassIdentity.hash(self)
```

The `Box<T>` examples assume an otherwise unconstrained class declaration. They need no `T: Equatable`, `T: Hashable`, or `T: class` bounds: these operations use the boxes' identities, not their contents. If the class declaration has bounds, the impl must preserve those as usual.

Structural implementations remain possible. `ClassIdentity.equals` always observes identity even for a class with structural `Equatable`; it does not change the meaning of that class's `==` operator. No blanket `implement <T> Equatable for T where T: class` is added to the prelude.

`ClassIdentity.hash` likewise ignores any user-written `Hashable` implementation. Authors must keep trait equality and hashing compatible: if two objects are structurally equal, their trait hashes must be equal. Delegating `Hashable` to identity hashing generally violates this contract for structural equality. The compiler does not prove compatibility or reject a raw identity-hash call on such a class; the intrinsic is independently useful.

Upcasting preserves object identity. A base implementation can compare subclass objects passed as base references, but this feature does not make `implement Equatable for Base` automatically satisfy `Derived: Equatable`. Users follow the existing trait implementation rules when they need a subclass to satisfy a trait bound. There is no identity-specific propagation or restriction on descendant implementations.

The mutex migration below calls the equality intrinsic directly. `Waiter` explicitly implements identity-based `Equatable` and `Hashable` so the scheduler can store waiter references in mutable sets.

## 5. Universal Lazy Identity Hashing

Every class instance has a hash slot. Storage is reserved at construction; assignment of the hash value is lazy. No declaration marker, usage analysis, or trait implementation controls whether the field exists.

### 5.1 Contract

`ClassIdentity.hash` returns a hash stable for the object's lifetime, with the same value through all aliases, upcasts, and generic variance conversions. Mutating user fields does not change it. Collisions are legal; the result is neither a unique object id nor a value promised to survive process restarts or serialization. Hashing invokes no user code and allocates no object.

### 5.2 Layout and inheritance

Use a common class header: physical field 0 is the existing vtable reference, and physical field 1 is the hidden mutable `i32` identity-hash slot. User fields follow, with inherited user fields before subclass fields and the existing tuple flattening rules preserved.

Each root class introduces the slot. Subclasses inherit it at the same offset and never append a second slot. Root and subclass views therefore read and initialize the same storage. Abstract and generic classes use the same header; full type erasure keeps one layout per generic class definition. This shared header convention does not introduce a universal Dovetail superclass.

The field adds one logical `i32` per class instance, including instances never hashed; actual memory overhead depends on engine layout and alignment. All construction paths initialize it to zero, including global initializers and compiler-generated class allocations. Records, enums, and other non-class representations do not acquire a hash slot.

Centralize the class-header size and hash-field index so field access, mutation, inherited layouts, tuple flattening, and constructor emission agree. Preserve the vtable at index 0. Package layouts must encode this convention consistently; invalidate incompatible compiler caches when introducing the header change. Dependent packages need no per-class opt-in metadata.

### 5.3 Lazy assignment

Emit one mutable `i32` counter for the compiled module instance, initially 1. It is shared across classes, generic instantiations, and packages, and persists across sequential `Async.run` executions in that module instance.

`ClassIdentity.hash(value)` evaluates `value` once and then:

1. Reads its hash slot. If nonzero, returns that bit pattern zero-extended to `Int64`.
2. Otherwise, takes the counter's current nonzero value and stores it in the slot.
3. Advances the counter with wrapping `i32` addition, substituting 1 if the result is zero.
4. Returns the assigned value zero-extended to `Int64`.

The slot is never subsequently changed. Objects that are never hashed never consume a counter value, and equality never initializes the slot. The first hash performs the read, initialization, and counter update; subsequent calls only read the slot and take the initialized branch. The current single-threaded execution model permits this sequence without synchronization; the intrinsic introduces no scheduler yield or callback.

Zero is reserved for “unassigned,” so there are 2³² − 1 assignable bit patterns. Values repeat after those are consumed. This is valid for a hash: callers must resolve collisions with equality and must never use the hash as a unique handle. Unsigned extension is required even when the `i32` bit pattern has its high bit set.

## 6. `standard-io` Migration

### 6.1 `MutexId`

Remove `MutexId`, `MutexIds`, `freshId()`, the global counter, and the `MutexState.id` field. Replace `FiberState.awaitingMutex: Option<MutexId>` with `Option<MutexState>`. Change mutex comparisons in queue validation and held-mutex filtering to `ClassIdentity.equals` (negated where the old comparison was inequality).

Preserve FIFO handoff, reentrant depth handling, interruption cleanup, and the use of `FiberId` in owner/queue fields. Preserve the effectful `Mutex.make` and `ReentrantMutex.make` constructors and the internal `Mutex.makeProcessWide` exception. A process-wide mutex needs no identity counter after this change; its reference is stable across sequential runtime executions.

Update the existing id-based identity tests to assert reference identity, including separate allocations before any run and reuse across sequential runs.

### 6.2 `WaiterId`

Replace the id with a class that owns its suspended-fiber stack. The following describes the internal shape; its visibility should expose no more scheduler state than the existing public instruction signatures require:

```dovetail
class Waiter(public suspended: MutableStack<FiberId>)
```

The identity primitives need no annotations or trait implementations. Storing waiters in the scheduler's mutable sets does require explicit `Equatable`, `Hashable`, and `Default` implementations, described below. `Promise<T, E>` owns one waiter and `Queue<T>` owns two, for takers and offerers. `Await`, `Wake`, and `Frame.AwaitFrame` carry a waiter reference instead of an id.

- Remove `MakeWaiter`, `nextWaiterId`, and `waiters: MutableMap<WaiterId, MutableStack<FiberId>>`. Access the waiter's stack directly.
- Replace `fiberWaiters: MutableMap<FiberId, MutableSet<WaiterId>>` with a per-fiber `MutableSet<Waiter>` of reverse registrations. Identity-based equality and hashing make adding an existing reference idempotent.
- Replace `FiberState.pendingWakes: MutableSet<WaiterId>` with a per-fiber `MutableSet<Waiter>`. Wake recording and consumption remain idempotent through ordinary set operations.
- Keep `FiberId` entries in waiter stacks. A waiter must not retain an entire `FiberState` and its suspended program graph through a stale registration.

`Waiter` explicitly implements `Equatable` and `Hashable` by delegating to `ClassIdentity`. To satisfy mutable hash collections' `Default` requirement, its `default` property returns one global constant waiter instance with an empty suspended-fiber stack. This shared instance fills unused key slots; occupancy metadata, not the placeholder's identity, determines whether a key is present. Collection operations do not mutate its stack. Removed and cleared map slots are reset to their default keys and values so they do not retain completed waiters. Actual scheduler conditions always allocate fresh waiters through `Waiter.make()` and never use the default placeholder for registrations. Each fiber also gets its own fresh mutable sets.

**Allocation timing:** `Waiter` can be allocated by an ordinary internal helper, but public `Async.makePromise`, `Queue.make`, and `Queue.makeUnbounded` must continue returning `Async` values whose execution allocates fresh state. Replace `MakeWaiter` with allocation inside `Async.thunk`, including the enclosing promise/queue state and buffers. Allocating outside the thunk and wrapping the object in `Async.succeed` would share one object across repeated executions. Removing the `async` keyword from a function body is permissible if its returned `Async` retains this behavior; changing its return type to a plain object is a separate API decision.

**Scheduler invariants:** preserve registration before a check executes, pending wakes that arrive during the check, re-registration before retry, and selective unregistering on success, failure, or interruption. Preserve the current restriction that an `Await` check must not itself `Await`. Wake and teardown must remove references at the same logical points as today; clear per-fiber sets when their registrations or pending wakes are discarded so completed waits are not retained.

Moving queues onto waiter objects also moves their lifetime beyond a runtime map. The migration must verify that termination and interruption leave no stale registrations. `FiberId` values are runtime-local, so a retained registration from one runtime must never be looked up in another runtime that reused the same numeric id. This design adds no support for sharing a live waiter between runtimes; any future process-wide promise/queue API must settle runtime ownership separately.

### 6.3 Handles That Stay

Keep `FiberId` and `ScopeId`. Their id-plus-registry arrangements allow stale handles to fail lookup without retaining the state object, and they also support diagnostics and sentinel conventions. These uses differ from comparing two already-held class references.

Keep `Waitable` in `streamLocks`: it is a WASI host handle that must round-trip through the ABI.

Future stream-node comparisons can use `ClassIdentity.equals`. Other identity-keyed mutable collections can follow the waiter pattern when a safe `Default` placeholder exists. Removing the `Default` requirement for arbitrary class keys remains a separate follow-up.

## 7. Implementation Stages and Acceptance Tests

1. **Class constraint.** Extend bound parsing and representation, collection/inference, satisfaction and implication, generic impl applicability, substitution, and dependency caching. Test concrete and generic classes, type aliases, rejection of other type categories, bounded and unbounded forwarding, `T: Base` implying `class`, the converse rejection, mixed `class + Trait` bounds, and enforcement on each supported generic declaration kind. Check fresh builds and cached cross-package callers.
2. **Identity equality.** Add the prelude declaration, intrinsic recognition, and `ref.eq` lowering. Test aliases versus fresh allocations, equal fields on distinct objects, mutation, explicit trait delegation, comparison despite structural equality, generic forwarding without element bounds, base upcasts, variance upcasts, cross-package classes, argument evaluation exactly once/in order, and function-value use. Invalid calls must fail during typechecking rather than WASM validation.
3. **Mutex migration.** Replace the surrogate id while preserving constructor timing and lock behavior. Run identity, handoff, reentrant, interruption, and process-wide mutex regression tests.
4. **Waiter migration.** Replace all waiter-id uses, including pending wakes and reverse registrations. Test promise and queue wake behavior, wake-during-check races, interrupted/failing/timed-out waits, cleanup of both directions of registrations, and fresh state when the same constructor program executes twice. Run the scheduler-window and queue/mutex interruption regressions. Verify sequential runtime executions cannot consume stale registrations from a previous runtime. Verify that `Waiter.default` shares one empty instance and that identity-keyed mutable sets deduplicate aliases, distinguish separate instances, resize correctly, and do not treat unused default-filled slots as members or mutate the default waiter's stack.
5. **Universal lazy hashing.** Implement the common class header, initialization on every allocation path, module counter, and intrinsic lowering. Test unannotated classes without trait implementations, generic forwarding under only `T: class`, specific class bounds, function-value use, single argument evaluation, explicit `Hashable` delegation, and rejection of non-class calls. Check layouts and field accesses for generic/inherited classes and flattened tuple fields; verify one slot per instance and agreement through base/derived/variance views. Test global construction, mutation stability, module lifetime, and fresh/cached cross-package builds. Compiler-level tests must verify that allocation and equality leave the slot zero and counter untouched, that only the first hash consumes a counter value, and that high-bit values zero-extend correctly. Force counter wraparound to check zero skipping, hash stability, and collisions between distinct objects; do not require all distinct objects to have distinct hashes.

All stages have a defined semantic contract. Stages 1–2 can ship independently; stages 3–4 require equality, and stage 5 can proceed after stages 1–2 without waiting for the library migrations. Universal lazy hash storage is part of this design. The main implementation risks are consistent class-field offsets and initialization across construction paths, complete bound enforcement, and preservation of the runtime migration invariants.

## 8. Implementation and Book Coverage

- [`ClassIdentity.dove`](../dovetail/prelude/src/ClassIdentity.dove) declares both primitives. Checked generic templates preserve ordinary call and function-value constraint validation; specialized bodies lower to `ref.eq` and the lazy hash operation.
- [`class_identity.rs`](../dovetail/tests/class_identity.rs) covers positive and negative constraints, explicit trait implementations, inferred and explicit function values, evaluation order, inherited and generic layouts, and executable book examples. [`multi_project.rs`](../dovetail/tests/multi_project.rs) covers repeated dependency builds, inherited fields, generic parent layouts, and class-bound functions across packages.
- [`identity_tests.rs`](../dovetail/src/compiler/codegen/identity_tests.rs) exercises the emitted hash implementation, including counter wraparound, unsigned extension, collisions, and laziness. Its host-controlled counter export exists only in the test harness.
- [`Runtime.dove`](../standard-io/src/Runtime.dove) tests reverse-registration and pending-wake cleanup, including sequential runtimes reusing fiber IDs. [`identityTest.dove`](../standard-io/test/identityTest.dove) checks reusable constructor programs and the async book example; existing queue, promise, mutex, interruption, and scheduler-window suites cover the migrated behavior.
- The book explains [class constraints](../book/26-advanced-generics.md#262-class-constraints), [explicit identity traits](../book/08-traits.md#identity-equality-and-hashing), [class identity](../book/09-classes.md#915-class-identity), and [allocation timing](../book/12-async.md#allocation-happens-when-the-program-runs).

The compiler's current dependency/prelude caches are process-local; restarting with the updated compiler recreates them with the new bound and layout representations. There is no persistent compiled-layout cache to version in this implementation.
