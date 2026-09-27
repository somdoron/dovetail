# Trait Defaults, Associated Outputs, and Inherited Overloads

**Status:** Implemented. Final integration validation is recorded below.
The agreed scope is items 1, 2, 4, 5, and 6 from the trait discussion. Associated
type references use `P.Output` and `W.Wrapped<T>`; associated-type defaults
(item 3) remain unsupported.

This document reopens five limitations recorded in
[the trait implementation audit](trait-implementation-status.md): default members
on generic classes, default bodies on generic methods, ordinary associated
outputs and generic associated types in generic code, and overloads inherited
through `extends`.

## 1. Intended behavior

### 1.1 Generic classes inherit defaults

```dovetail
trait Greeter =
    function greet(self): Int32 = 1

class Box<T>(value: T) implements Greeter =
    public function get(self: Box<T>): T = self.value

function main(): Unit =
    assert Box(42).greet() == 1
```

Adding class type parameters must not remove the ability to inherit an instance
method or property default. A matching explicit class member takes precedence.
Existing rules for conflicting defaults, abstract declarations, and virtual
overrides continue to apply to generic classes.

Defaults must work through direct calls, generic trait bounds, and eligible
interface values. Subclass overrides must be observed by calls made inside a
default body. The compiler substitutes class and trait parameters independently,
including renamed parameters inherited through a parent class or supertrait.

### 1.2 Generic methods may have default bodies

```dovetail
trait Picker =
    function pick<T>(self, value: T): T = value
```

The compiler checks the body once against the method's declared parameters and
bounds. It specializes the body when a concrete call supplies those parameters.
Implementors may omit the method or supply a matching implementation.

This applies to implementation blocks and inline class implementations, including
generic classes. Class trait matching must therefore recognize methods with
their own type parameters, rather than skip them. Method parameters are compared
up to renaming and retain their own identity when an enclosing parameter has the
same spelling. Implementations cannot strengthen the declared bounds.

Allowing these bodies does not make generic methods safe for interface-object
dispatch. Existing interface declaration restrictions remain in force.

### 1.3 Preserve ordinary associated outputs in generic code

```dovetail
trait Producer =
    type Output
    function produce(self): Output

function read<P>(producer: P): P.Output where P: Producer =
    producer.produce()
```

The type checker must preserve the meaning “the Output chosen by P's Producer
implementation” without requiring a separate output parameter or equality
binding. The same representation is needed for inferred intermediate results,
arguments, nested type applications, and default bodies that use associated
outputs. An unknown output is a specific associated type, not `Any` and not a
fresh freely assignable inference variable on each use.

Existing explicit equalities remain supported:

```dovetail
function read<P, O>(producer: P): O
    where P: Producer<Output = O> =
    producer.produce()
```

An equality binding normalizes the projection to its bound type. A concrete
implementation normalizes it to that implementation's associated definition.
Without either, the projection remains symbolic and can only be used in ways
justified by the available bounds. In particular, an unknown Output cannot be
assumed to be `Int32`, or to implement arithmetic or any other trait.

The internal identity includes the receiver, declaring trait application, and
associated member. Distinct trait applications and distinct receivers must not
accidentally share a projection. Inherited declarations reached through a
diamond must retain the same identity. An unqualified `P.Output` is ambiguous
when multiple available trait applications provide distinct matching members;
report that ambiguity rather than select the first bound. Existing equality
syntax provides a way to state the intended relationship without introducing
another qualification syntax in this phase.

Associated-type declarations still cannot supply defaults.

### 1.4 Preserve generic associated types in generic code

The following complete example is covered by the compiler regression suite.

```dovetail
package example

trait Wrapper =
    type Wrapped<T>
    function wrap<T>(self, value: T): Wrapped<T>

record OptionalWrapper =
    name: String

record ArrayWrapper =
    name: String

implement Wrapper for OptionalWrapper =
    type Wrapped<T> = Option<T>
    function wrap<T>(self, value: T): Option<T> = Some(value)

implement Wrapper for ArrayWrapper =
    type Wrapped<T> = Array<T>
    function wrap<T>(self, value: T): Array<T> = [|value|]

function wrapValue<W, T>(wrapper: W, value: T): W.Wrapped<T>
    where W: Wrapper =
    wrapper.wrap(value)

function main(): Unit =
    let optionalWrapper = OptionalWrapper { name = "optional" }
    let arrayWrapper = ArrayWrapper { name = "array" }
    let numberOption: Option<Int32> = wrapValue(optionalWrapper, 42)
    let numberArray: Array<Int32> = wrapValue(arrayWrapper, 42)
    let textOption: Option<String> = wrapValue(optionalWrapper, "hello")
    let textArray: Array<String> = wrapValue(arrayWrapper, "hello")
    assert numberOption.require == 42
    assert numberArray == [|42|]
    assert textOption.require == "hello"
    assert textArray == [|"hello"|]
```

Use one projection representation for ordinary and generic associated types.
In addition to receiver, trait application, and member identity, it carries the
associated type's argument list (empty for an ordinary associated type).
Check arity at the reference. The projection stays symbolic until its receiver
and available evidence allow normalization.

When `W` becomes `OptionalWrapper`, normalize `W.Wrapped<Int32>` by selecting
its `Wrapper` implementation, then substituting `Int32` for the associated
definition's own parameter to obtain
`Option<Int32>`. Substitute block, trait, method, and associated-definition
parameters without capturing one another, even when their names coincide.
Normalize nested projections recursively, with cycle detection and a diagnostic
for recursive definitions that cannot normalize.

Do not assume a GAT is injective: an implementation can define `Wrapped<T>`
without using `T`. Equality of two results does not by itself prove equality of
their input parameters. When inference cannot determine an argument, report the
missing information instead of guessing. Matching identical symbolic projections
with equal arguments is still valid.

Resolve projection roots in the generic type scope before treating the dotted
name as a package-qualified type. Preserve existing qualified type names. Test
projections in signatures, bounds, aliases, nested generic arguments, and
inferred results. A member-name collision between distinct trait applications
uses the ambiguity rule from section 1.3.

The new representation must also reach existing consumers of GATs. In particular,
revisit abstract `use` with a `Usable` bound: infer its continuation and result
using the bound's symbolic `Wrapped` application, then normalize for concrete
calls. Exercise the real resource library and async consumers rather than only
the standalone wrapper example. Existing effect and resource-lifetime semantics
remain unchanged.

### 1.5 Inherit distinct method overloads

```dovetail
trait TextPrinter =
    function print(self, value: String): Unit

trait Printer extends TextPrinter =
    function print(self, value: Int32): Unit
```

`Printer` requires both methods. Calls select an overload using the existing
argument compatibility and ambiguity rules. The same behavior applies when
the two declarations are inherited from separate parents.

Use the declaring member's identity throughout flattening, implementation
matching, default selection, bound lookup, qualification, and runtime dispatch.
Do not identify an overload solely by its name or by a signature after concrete
substitution: distinct declarations can acquire identical concrete parameter
types. Such calls must follow ambiguity rules rather than silently merge slots.

Retain the existing rejection of return-type-only conflicts, redundant inherited
declarations without bodies, and static/instance member-kind conflicts. Exact
signature redeclarations with bodies still override defaults. Repeated paths to
the same declaration in a diamond do not create extra requirements.

Implementation completeness must check every signature. One implemented overload
does not satisfy all same-name requirements. Explicit implementations and default
bodies are selected separately for each overload. Interface vtables and super
upcasts must preserve distinct slots and the selected declaring member.

This phase enables overloads introduced through inheritance. It does not need to
change the current policy on duplicate names declared directly in one trait.

## 2. Compiler work

1. Establish a shared declaration identity for trait methods. Audit name-only
   keys and lookups, including default templates and synthesized class members.
   Preserve identity across trait argument substitution and inherited copies.
2. Carry overload identity through collection, inference, typed calls,
   specialization, interface construction, and code generation. Then remove
   the two distinct-parameter inheritance rejection paths.
3. Extend default templates with method-level type parameters and bounds.
   Injected implementation methods must retain those parameters instead of
   constructing an empty parameter list.
4. Register generic class default signatures and templates before class method
   specialization and vtable construction. Reuse the ordinary generic class
   method pipeline and preserve virtual receiver metadata. Replace the matcher
   that skips methods with their own parameters with signature/contract matching.
5. Add a shared ordinary/GAT projection to the compiler type model and
   normalize it through explicit equalities or implementation selection. Cover
   substitution, unification, assignability, type traversal, display, cache
   serialization, mangling, and specialization. Replace unresolved-output call
   rejection with a symbolic output where valid. Resolve concrete projections
   before emitting executable code; unresolved codegen types are diagnostics,
   not guessed representations. Add dotted projection syntax, argument-arity
   checks, recursion detection, and abstract GAT handling in resource/async
   consumers. Do not infer argument equality from GAT result equality.
6. Update the trait audit, core design, appendix, TOC, and book to describe the
   tested behavior. Put the projection/default-generic details in advanced
   generics material, keeping the introductory chapters simple.

Useful starting points:

- `typechecker/collect/trait_flatten.rs`: inherited overload rejection.
- `typechecker/collect/traits.rs`: generic default rejection.
- `typechecker/collect/classes.rs`: generic class completeness and member matching.
- `typechecker/infer/trait_defaults.rs`: default-body scope and template keys.
- `typechecker/infer/function_expressions.rs`: unresolved associated-output rejection.
- `monomorphize/implement_blocks.rs`: default injection and class materialization.
- `common/types.rs`: implementation/default symbol construction.

Compiler paths above are relative to `dovetail/src/compiler`, except
`common/types.rs`, which is relative to `dovetail/src`.

## 3. Implementation stages

### Stage A: Declaration identity and inherited overloads (item 6)

Introduce stable member identity while preserving current behavior, then enable
inherited overloads end-to-end. Track the original declaration independently of
the trait application and default-body provider. Audit all name-only resolution
paths, including method references and explicit qualification, not just calls.
Keep implementation symbols, default templates, and interface slots distinct.

Exit criterion: inherited overloads execute correctly through concrete, generic,
class, and interface receivers; missing overloads and genuinely ambiguous calls
are diagnosed. Existing trait/interface dispatch regressions pass.

### Stage B: Generic default methods and class defaults (items 2 and 1)

Build on Stage A's member identity. First preserve method parameters and bounds
through default inference and injection. Then synthesize generic class members
and specialize their templates through the existing class pipeline. Include
inline generic trait-method matching so support is consistent across implementors.

Exit criterion: defaults work across generic/non-generic implementation blocks
and classes, including inherited overloaded defaults, explicit overrides,
renamed parameters, and virtual calls from default bodies. Negative bound and
default-conflict tests still reject invalid programs.

### Stage C: Ordinary associated projections (item 4)

Introduce the full projection representation, initially exercise its empty
associated-argument case. Resolve `P.Output` against direct and inherited bounds;
preserve it through inference and normalize it through existing equalities and
concrete implementations. Update cache compatibility if the stored type format
changes. Existing package-qualified names must keep their meaning.

Exit criterion: the `read` example works with different concrete outputs and
through another generic helper; explicit equality syntax continues to work;
unknown outputs do not gain unproven capabilities. No unresolved projection
reaches concrete code generation.

### Stage D: Generic associated projections and consumers (item 5)

Enable the associated-argument case, including nested projections and independent
parameter scopes. Exercise non-injective definitions and recursive-definition
diagnostics. Integrate abstract resource and async uses where their existing
contracts provide the required information.

Exit criterion: the complete wrapper example runs, generic forwarding preserves
its result type, and real GAT consumers work through sufficiently constrained
bounds. Wrong arity, ambiguous members, incompatible projections, and cyclic
normalization produce diagnostics rather than crashes or invalid WASM.

### Stage E: Integration and documentation

Run the combined matrix below and the actual Dovetail workspace. Update the audit
and feature status only after validation. Replace the old restriction examples
in the book; put advanced examples in the advanced generics chapter. Preserve
the existing uncommitted enclosing-bound and book changes while doing this work.

Exit criterion: all five features have positive, negative, and interaction
coverage; affected suites and workspace tests pass; the docs describe the final
behavior and explicitly retain the unsupported associated-type defaults.

## 4. Acceptance and regression coverage

| Area | Required evidence |
|------|-------------------|
| Generic class defaults | Methods and properties; multiple concrete class arguments; generic supertraits; explicit override precedence; subclass virtual override observed inside defaults; direct, bound, and interface calls; conflicting defaults rejected. |
| Generic method defaults | Multiple method arguments; method bounds enforced; enclosing/method parameter renaming and shadowing; omitted and explicit implementations; generic/non-generic blocks and classes; inherited default; signature/contract mismatch rejected. |
| Ordinary associated outputs | Output forwarded without equality; output used as another call's argument; nested container output; repeated projection identity; equalities normalize; inherited outputs; concrete specialization; unrelated output types and unsupported operations rejected; missing and ambiguous members diagnosed. |
| Generic associated outputs | Complete wrapper example with multiple element types; nested projections; forwarding through generic helpers; multiple associated parameters; parameter-name collisions; inherited declarations; non-injective definitions; abstract resource/async consumers; wrong arity and cyclic normalization diagnosed. |
| Inherited overloads | Own plus inherited methods; multiple parents; different arities and argument types; defaults per overload; generic bounds; explicit qualification; class implementations; interface calls and super upcasts; diamond deduplication; missing overload and ambiguous call diagnostics. |
| Interactions | Generic default body calling an inherited overload; default on a generic class using an ordinary or generic associated output; overloaded default specialized through a generic supertrait; projection-bearing generic methods across package boundaries. |

Use focused compiler regressions for type errors and dispatch behavior, then run
the affected trait, default, inheritance, interface, class, generic, associated
type, and enclosing-bound suites. Run the actual Dovetail workspace through
`cargo run --bin dovetail -- test` for library compatibility. Review changed Rust
for readability and structure. Record actual results before marking any feature
complete.

## 5. Separate features

Associated-type defaults remain unsupported by explicit user decision.
Blanket implementations, specialization,
trait variance, and automatic static defaults on classes also remain separate
features; this document does not declare them implemented.

## 6. Implementation and validation

The implementation carries ordinary and generic associated outputs in one
`AssociatedProjection` type. Its identity includes the receiver, declaring
trait application, associated member, and associated parameters. References
resolve through generic bounds; equality evidence or concrete implementations
normalize them during substitution. Distinct declarations remain ambiguous even
when they happen to choose the same concrete output. Recursive normalization
has a depth/cycle guard and produces diagnostics.

Inherited overloads receive distinct dispatch symbols. Parent-trait routing,
implementation calls, class signature matching, and interface slots preserve
the selected method. Generic default templates carry their method parameters
independently of the class and trait parameters. Class default collection checks
explicit and inherited members before supplying a default.

The generic resource check follows uses inside expressions and requires their
continuation to preserve the resource's symbolic wrapper. Blocks and closures
check their own continuation result.

Regression coverage lives in [trait_completion.rs](../dovetail/tests/trait_completion.rs)
and [multi_package.rs](../dovetail/tests/multi_package.rs). It includes executable
examples, rejected invalid programs, generic bounds on overloads, explicit
qualification, class/interface dispatch, equality normalization, aliases, nested
non-injective wrappers, and a generic default with a projected result crossing a
package boundary. Existing tests for retired restrictions now assert the new
behavior.

The book explains these features with complete programs in
[Advanced Generics](../website/content/book/26-advanced-generics.md#267-generic-trait-defaults).
Introductory trait, class, and resource chapters link to that material.

Review regressions additionally cover inherited binder renaming, overload
identity, class method/property dispatch, inherited implementation contracts,
dependent bounds hidden behind aliases, and generic associated parameters in
implementation signatures. Concrete alias normalization refreshes implementation
evidence until stable, then validates declarations against the complete registry.
Missing providers and recursive definitions still produce diagnostics.

Initial implementation validation, before the subsequent review fixes:

- `cargo check -p dovetail-lang`: passed.
- Eighteen affected integration suites: **1,147 passed**, no failures,
  1 pre-existing ignored class test. This includes the 24 new feature tests
  and cross-package, closure, and alias coverage.
- `git diff --check`: passed.
- `cargo run --bin dovetail -- test`: **1,608 workspace tests passed** with
  network access. The initial sandboxed run's 49 network failures disappeared
  when the same tests ran with network access.

Post-review validation:

- Three reviewers cross-reviewed the fixes; the final source reviews reported
  no remaining findings.
- Twenty-four affected integration suites: **1,187 passed**, no failures,
  1 pre-existing ignored class test. This includes 40 review regressions in
  the six `trait_*_review.rs` suites added during review.
- `git diff --check`: passed.
- `cargo run --bin dovetail -- test`: **1,608 workspace tests passed** against
  the final review fixes, including network-dependent tests.
