# Tuple Extension Design

This document specifies the complete tuple extension feature: the `~` type and value operator, the built-in `Tuple` constraint, pair and recursive tuple operations, inductive trait implementations, enclosing type parameter bounds, and parser integration through the existing `Concat` / `++` mechanism. Implementation milestones are separate; the semantics below apply to the whole feature.

It builds on the [tuples design](tuples-design.md) and [traits design](traits-design.md). Tuples continue to have at least two elements; there are no empty or singleton tuples.

## 1. Motivation

Sequencing parsers with the existing `zip` produces nested pairs:

```dovetail
parseChar('(').zip(parseDigit()).zip(parseChar(',')).zip(parseDigit())
// Parser<(((Char, Char), Char), Char)>
```

Tuple extension lets a parser accumulate a flat sequence:

```dovetail
parseChar('(') ++ parseDigit() ++ parseChar(',') ++ parseDigit()
// Parser<(Char, Char, Char, Char)>
```

The same operation supports builders and accumulators. Tuple decomposition also lets libraries implement element-wise traits using a pair base case and a recursive case for arity three and above.

## 2. The `~` Operator

`~` is a built-in binary operator at both the type and value levels. It appends one element to a tuple, or constructs a pair when the left operand is a non-tuple. **Concrete and generic code follow the same rules.** No `Tuple` bound is required to use extension.

### 2.1 Type-Level Semantics

| Expression | Result |
|---|---|
| `Int32 ~ Bool` | `(Int32, Bool)` |
| `(A, B) ~ C` | `(A, B, C)` |
| `(A, B, C) ~ D` | `(A, B, C, D)` |
| `(A, B) ~ (C, D)` | `(A, B, (C, D))` |
| `((A, B), C) ~ D` | `((A, B), C, D)` |

Normalization is defined by the left operand's outer type shape:

```text
Extend((T1, ..., Tn), U) = (T1, ..., Tn, U)   for n >= 2
Extend(T, U)           = (T, U)              for known non-tuple T
```

- The right operand is always one element, even if it is a tuple.
- Only the outer tuple on the left is extended. Nested elements retain their types.
- `~` is left-associative: `A ~ B ~ C` means `(A ~ B) ~ C`. This equals `(A, B, C)` when `A` is a non-tuple; if `A` is a tuple, its outer elements are extended instead.
- Transparent aliases are resolved before deciding the outer shape. Records, classes, and nominal newtypes remain single elements, including a newtype whose payload is a tuple.
- Pair construction does not create an intermediate singleton tuple.

### 2.2 Value-Level Semantics

```dovetail
let pair = 1 ~ true                    // (1, true)
let triple = pair ~ "hello"            // (1, true, "hello")
let nested = pair ~ ("a", "b")         // (1, true, ("a", "b"))
let four = 1 ~ 2 ~ 3 ~ 4               // (1, 2, 3, 4)
```

Evaluate the left operand once, then the right operand once. Construct the result from those values. Lowering must not duplicate an operand expression when extracting tuple fields. Extension is immutable and preserves nested tuple values.

The decision to extend or construct a pair follows the operand's static type after substitution, not a runtime inspection of an erased value.

### 2.3 Grammar and Precedence

At the expression level, binary `~` binds more tightly than logical `&&` / `||` and less tightly than equality and ordering comparisons. It is left-associative. Existing prefix `~` remains unary bitwise negation and retains its current precedence; parser position distinguishes the two uses.

```text
tuple_extension_expr = equality_expr { "~" equality_expr }
```

The logical-and grammar consumes `tuple_extension_expr` operands. For example, `a ~ b == c` means `a ~ (b == c)`; comparing an extension result requires `(a ~ b) == c`.

At the type level, `~` binds more tightly than the function arrow and less tightly than atomic type forms (including generic application and parenthesized types):

```text
tuple_extension_type = atomic_type { "~" atomic_type }
```

Thus `A ~ B => C` means `(A ~ B) => C`, and `A ~ (B => C)` appends a function type. Commas and closing delimiters terminate an extension in type argument and tuple element lists. These productions describe the new precedence layer; existing type forms remain available with parentheses for grouping.

### 2.4 Deferred Normalization and Inference

A concrete normalized extension is an ordinary tuple, with existing destructuring, positional access, matching, and trait behavior. An unresolved generic extension needs a symbolic compiler representation, such as `TupleExtend(T, U)`.

- Preserve this representation while the outer shape of `T` is unknown. An unconstrained type parameter or inference variable must not be assumed to be a non-tuple.
- Apply substitutions recursively and normalize whenever the left shape becomes known. For example, substituting `T = (Int32, Bool)` into `T ~ String` yields `(Int32, Bool, String)`.
- Normalize known tuple prefixes even when their element types remain generic: `(A, B) ~ U` is `(A, B, U)`.
- Forward unresolved extensions through generic calls, return types, associated `Output` definitions, and closure signatures. Do not prematurely replace `T ~ U` with `(T, U)`.
- Equality and unification compare normalized forms when available. Identical symbolic forms are equal; solving unresolved operands must retain their shape constraints. Extension is injective, but not every tuple is an extension result: a pair with a tuple-valued first element has no extension preimage (see §7.2). Unrestricted reverse inference remains outside this phase.
- Symbolic extension keeps its left operand invariant and permits covariance in its right operand. For example, `T ~ Int32` can widen to `T ~ Any`; widening `T` could change the result's arity.
- In the initial implementation, infer extension operands from argument types and explicit type arguments. Permit reverse decomposition only for the tuple-constrained form in §7.2. If an expected result alone would require unrestricted reverse inference, report that explicit type arguments are required instead of guessing.
- Before emitting concrete tuple construction or field access, resolve the shape through specialization or the compiler's existing generic lowering machinery. An erased single reference alone cannot determine the statically selected extension shape. This requirement includes generic operations inside parser closures.

The implementation must carry the symbolic form through substitution, type traversal, trait matching, and specialization. After normalization, use the existing tuple representation and boxing rules; extension does not introduce a separate runtime tuple kind.

## 3. The `Tuple` Bound

`Tuple` is a compiler-recognized structural constraint:

- Every actual tuple of arity at least two satisfies it.
- Non-tuples do not satisfy it. Nominal wrappers around tuples are not tuples; transparent aliases to tuples are.
- Users cannot implement or inherit `Tuple`, use it as an interface object, or override its meaning. It is a bound-only structural constraint.
- It constrains shape; it does not enable extension or implicitly wrap values.

This distinction makes the recursive tuple case precise: if `T: Tuple`, then `T ~ U` has arity at least three. An unconstrained `T ~ U` remains valid as a type or expression, but is not an allowed unresolved implementation head (§7.2).

## 4. Where Clauses Referencing Enclosing Type Parameters

Functions and methods can constrain type parameters from enclosing module, class, implementation, or extension scopes. This is independent of parser extension, which requires no `Tuple` bound.

Resolve bound names against the function's type parameters and visible enclosing parameters, using the language's existing name and shadowing rules. Do not introduce a new shadowing exception as part of this feature.

- Make the bounds available when checking both the signature and body, including nested closures.
- Check bounds after substituting both enclosing and function type arguments at calls.
- Generic callers must prove bounds from their own available constraints; the enclosing parameter need not already be concrete.
- Method-local bounds do not become unconditional bounds on the enclosing module or type, and cannot strengthen an implemented trait method's contract.
- Class overrides likewise cannot strengthen the inherited method's requirements after substituting parent type arguments and aligning method parameters.
- Match overloaded members by their complete substituted parameter signatures. Expand implementation-associated types, including generic associated types nested within other types, before comparing method contracts.
- Taking a method reference checks the same requirements as calling the method. An unavailable constrained method does not prevent constructing the enclosing class or using its other members.
- Class parameters occurring in conditional virtual-method bounds are invariant, including occurrences in bound arguments and associated-type equalities. Bounds already implied by the class do not add this restriction. Parent type arguments must respect variance too, so inheritance cannot bypass the rule.

Collection and inference use the complete visible parameter scope. Method-local evidence is restored after each body so it cannot leak into sibling members. Virtual methods whose bounds are unsatisfied retain their class layout slot without specializing the unavailable body. These rules are implemented independently of `Parser.append`.

## 5. Existing `Concat` and Parser Integration

### 5.1 Reuse the Existing Trait

[`dovetail/prelude/src/Concat.dove`](../dovetail/prelude/src/Concat.dove) already defines:

```dovetail
public trait Concat<R> =
    type Output
    function concat(self: Self, rhs: R): Output
```

Associated types and `++` lowering already exist, including string concatenation. Reuse the associated name `Output`. The operator selects the implementation from both operand types and obtains its result from that implementation's `Output`. The existing coherence rules determine uniqueness.

`++` remains left-associative at additive precedence, alongside `+` and `-`. No new operator-trait framework is needed.

### 5.2 Parser API

Preserve `zip<U>` as pair-producing (`Parser<(T, U)>`) and add `append<U>` returning `Parser<T ~ U>`. Parser `++` delegates to `append`. This lets existing code retain intentional nesting and provides an explicit flat-accumulation operation.

```dovetail
implement <T, U> Concat<Parser<U>> for Parser<T> =
    type Output = Parser<T ~ U>
    function concat(self, other: Parser<U>): Parser<T ~ U> = self.append(other)
```

**Each parser combinator has one generic implementation.** No per-arity overloads or scalar/pair overloads are required: `v1 ~ v2` handles both cases through type normalization. Separate pair and recursive implementations are needed for traits that process tuple elements, not for parser sequencing.

If one parser intentionally returns a tuple, using it on the left of `append` extends that tuple. On the right, its result remains one nested element. Use `zip` when both parser results should remain intact as a pair.

### 5.3 Full Example

The following extends the existing parser module and its `ParserFn` and `ParseResult` definitions:

```dovetail
module Parser<T> =
    function append<U>(self, other: Parser<U>): Parser<T ~ U> =
        let f: ParserFn<T ~ U> = input =>
            match self.run(input) with
                case ParseResult.Success {value = v1; remaining = r1} =>
                    match other.run(r1) with
                        case ParseResult.Success {value = v2; remaining = r2} =>
                            ParseResult.Success {value = v1 ~ v2; remaining = r2}
                        case ParseResult.Failure(m) => ParseResult.Failure(m)
                case ParseResult.Failure(m) => ParseResult.Failure(m)
        Parser(f)

let digitPair =
    parseChar('(') ++ parseDigit() ++ parseChar(',') ++ parseDigit() ++ parseChar(')')
// Parser<(Char, Char, Char, Char, Char)>
// Parsing "(3,7)" produces ('(', '3', ',', '7', ')').

let firstDigit = parseChar('(').zipRight(parseDigit())
let secondDigit = parseChar(',').zipRight(parseDigit()).zipLeft(parseChar(')'))
let justDigits = firstDigit ++ secondDigit
// Parser<(Char, Char)>
```

Sequencing retains the existing failure and remaining-input behavior: run the right parser only after left success, pass it the remaining input, and return the first failure encountered.

## 6. Pair and Recursive Tuple Accessors

Pairs are the base case across tuple operations. Built-in `init` and `last` are available on every tuple:

| Receiver type | `init` type | `last` type |
|---|---|---|
| `(A, B)` | `A` | `B` |
| `(A, B, C)` | `(A, B)` | `C` |
| `(T1, ..., Tn)`, n >= 3 | `(T1, ..., Tn-1)` | `Tn` |

```dovetail
(1, true).init                  // 1
(1, true).last                  // true
(1, true, "x").init             // (1, true)
(1, true, "x").last             // "x"
((1, 2), true).init             // (1, 2), the first element intact
```

`init` on a pair returns its first element, which may itself be a tuple. It does not return a singleton tuple. `last` always returns the final element unchanged. Both operations evaluate their receiver exactly once.

For `self: T ~ U`, `self.init` has type `T` and `self.last` has type `U`, even without a bound on `T`: extension either creates a pair or appends to a tuple prefix. For an otherwise unknown `T: Tuple`, retain symbolic accessor result types until the arity is known; the bound alone does not prove that `self.init` is a tuple. Access on a type with neither a known tuple shape nor a `Tuple` proof is rejected.

**Reconstruction laws:**

- Arity two: `(t.init, t.last)` reconstructs `t`.
- Arity three and above: `t.init ~ t.last` reconstructs `t`.
- There is no universal reconstruction law using `~` for pairs. For `t = ((1, 2), 3)`, `t.init ~ t.last` is `(1, 2, 3)`, not `t`.

These are compiler-provided shape rules; libraries do not need to implement accessor overloads.

## 7. Inductive Trait Implementations

### 7.1 Pair Base Case and Recursive Case

Replace arity enumeration with two disjoint implementations:

```dovetail
// Base case: exactly two elements.
implement <A, B> Equatable for (A, B) where A: Equatable, B: Equatable =
    public function equals(self: (A, B), other: (A, B)): Bool =
        self.init.equals(other.init) && self.last.equals(other.last)

// Recursive case: three or more elements, because T is an actual tuple.
implement <T, U> Equatable for T ~ U where T: Tuple, T: Equatable, U: Equatable =
    public function equals(self: T ~ U, other: T ~ U): Bool =
        self.init.equals(other.init) && self.last.equals(other.last)
```

For `(Int32, String, Bool)`, bind `T = (Int32, String)` and `U = Bool`. Resolve the prefix's equality through the pair implementation and the last element's equality through its existing implementation. Longer tuples repeatedly shorten the prefix until reaching a pair. Nested elements are processed using their own trait implementations.

### 7.2 Matching and Coherence

The initial supported symbolic extension implementation head is `T ~ U`, where both are declared implementation type parameters and `T` has the built-in `Tuple` constraint. Concrete extensions normalize to ordinary tuple heads. More elaborate unresolved extension heads are outside the initial feature and receive an unsupported-head diagnostic.

For the supported symbolic head:

1. Reject non-tuples and pairs as candidates.
2. For arity n >= 3, bind `T` to the tuple of the first n-1 elements and `U` to the last element. Preserve nested element types.
3. Check the remaining trait bounds with these substitutions.

Unrestricted `T ~ U` implementation heads are rejected. Extension is not a universal inverse of pair decomposition: `((A, B), C)` cannot be represented by assigning `T = (A, B)` and `U = C`, because that extension is `(A, B, C)`.

**Required amendment to trait coherence:** the built-in `Tuple` constraint is structural information the compiler may use to establish arity and disjointness. It makes the constrained recursive head disjoint from the pair head. Ordinary user-trait `where` bounds still do not establish disjointness; there is no general specialization or first-match rule.

A concrete triple implementation overlaps the recursive implementation and must be rejected. Two recursive implementations for the same trait application also overlap unless their trait application shapes establish disjointness under the existing rules. Registration order must not affect selection. Update the general traits design to record this narrow structural exception when implementing it.

### 7.3 Recursive Resolution and Lowering

Use the existing recursive trait resolver, extending its matching and coherence machinery for these heads. The prefix obligation decreases tuple arity on every step; nested tuple elements are proper structural subterms. Retain cycle detection and resource limits for other obligations, with diagnostics rather than hangs or stack overflow. There is no language-level tuple arity cap, although compiler resource limits still apply.

Lower `init` through existing tuple construction/field operations after the shape is known. The initial implementation may construct a prefix tuple; recursive equality and ordering can therefore copy O(n²) outer fields across all prefix steps without optimization. Correctness must not rely on eliminating these allocations. Avoid claiming allocation-free recursion; prefix-copy elimination is a later optimization.

### 7.4 Other Traits

`Comparable` and `Display` use the same pair/recursive implementation structure, with element trait bounds:

```dovetail
implement <A, B> Comparable for (A, B) where A: Comparable, B: Comparable = ...
implement <T, U> Comparable for T ~ U where T: Tuple, T: Comparable, U: Comparable = ...

implement <A, B> Display for (A, B) where A: Display, B: Display = ...
implement <T, U> Display for T ~ U where T: Tuple, T: Display, U: Display = ...
```

Ordering remains lexicographic: compare prefixes first and compare the last elements only when prefixes compare equal. Equality retains short-circuiting. Display must preserve flat tuple punctuation; blindly formatting the prefix as a complete tuple and wrapping it again would introduce unwanted nested parentheses. Use a formatting helper that separates element formatting from outer delimiters, while retaining parentheses for actual nested tuple elements.

## 8. Acceptance Criteria

Implementation tests must cover:

| Area | Required cases |
|---|---|
| Extension | Non-tuple to pair; pair to triple; longer chains; right tuple preserved; nested left elements preserved; nominal tuple wrapper treated as one element; transparent alias normalized. |
| Parsing | Left associativity; comparison/logical precedence; prefix bitwise `~`; function-type arrows; parenthesized operands; extension inside generic type arguments. |
| Generic normalization | Scalar and tuple instantiations of one function; generic forwarding before shapes are known; associated outputs; parser closures; unresolved reverse inference diagnosed without guessing. |
| Evaluation | Each operand and accessor receiver evaluated once; left-to-right operand order, verified with observable effects. |
| Bounds | Tuples satisfy `Tuple`; scalars and nominal wrappers fail it; user implementations rejected; enclosing-parameter bounds checked at concrete and generic calls. |
| Accessors | Pair, triple, larger tuple, nested first element; symbolic accessor results; both reconstruction laws; pair counterexample for reconstruction with `~`. |
| Traits | Pair and recursive cases coexist; triples select recursion; nested tuples; arities beyond six; missing element bound rejected; concrete triple overlap and unrestricted head rejected; selection independent of registration order. |
| Trait behavior | Equality short-circuiting; lexicographic ordering; flat Display punctuation with correct nested tuple formatting. |
| Parser | One generic `append`; scalar and tuple results on either side; chained `++`; preserved pair-producing `zip`; delimiter discarding; failure propagation and remaining input. |
| Existing behavior | String and list `++`, existing operator associated outputs, and ordinary tuple construction/access continue to work. |

## 9. Implementation Milestones

Milestones 1–6 are implemented. The previously blocked tuple regressions have
been rerun successfully, including the negative supertrait case and explicit
type-argument assertions.

The initial milestone-six validation passed all 106 Rust test executables
(3,130 tests passed, seven ignored), the workspace typecheck, and Rust
documentation checks. Repeated reviews added 31 regressions and finished with
no remaining correctness or readability findings. Final focused validation
passed 722 tests across the class, module, generic, trait, variance, and
enclosing-bound suites (two existing tests ignored), including all 50
milestone-six regressions. All 1,608 Dovetail workspace runtime tests and
`git diff --check` also passed.

| Milestone | Scope | Dependencies |
|---|---|---|
| **1 — Extension types** | Parse type-level `~`; introduce symbolic extension, substitution, normalization, and inference diagnostics. | Existing tuple types. |
| **2 — Extension values** | Parse binary `~` alongside unary `~`; typecheck and lower once-only evaluation through existing tuple representations, including generic closures. | 1. |
| **3 — Parser integration** | Add generic `append` and `Concat<Parser<U>, Output = Parser<T ~ U>>` implementation; retain `zip`; validate parser behavior. | 1–2 and existing `Concat` support. |
| **4 — Tuple shape and accessors** | Add genuine `Tuple` constraint; pair/recursive `init` and `last`, including symbolic accessor result types. | 1–2 for accessor interaction with extension. |
| **5 — Inductive traits** | Add constrained extension-head matching, structural coherence exception, and recursive resolution; migrate arity-enumerated traits and verify behavior. | 4. |
| **6 — Enclosing bounds** | Resolve enclosing-parameter where clauses; enforce call-site requirements and trait/override contracts; preserve method-local scope and class construction. | Independent; reuses 4 for Tuple-specific tests. |

All milestones belong to this design. Parser integration does not depend on inductive traits or enclosing bounds; the full feature is complete only after all milestones and their acceptance criteria are satisfied. Associated types, `Concat`, and `++` are existing infrastructure, not new milestones.

## 10. Decisions and Limits

| Topic | Decision |
|---|---|
| Extension | Same semantics in concrete and generic code; non-tuple left constructs a pair, tuple left appends. |
| Right operand | Always one element; spreading both sides is outside this feature. |
| Tuple constraint | Actual tuples only, minimum arity two; not required for extension. |
| Pair behavior | Explicit base case; `init` returns the first element and `last` the second. |
| Recursive behavior | Arity >=3; `init` returns a prefix tuple and `last` the final element. |
| Generic representation | Deferred extension and accessor results until sufficient shape information is known. |
| Implementation heads | Initial symbolic support restricted to `T ~ U` with `T: Tuple`; narrow structural coherence exception. |
| Parser API | One generic `append`, existing `Concat.Output`, pair-producing `zip` preserved. |
| Scope | Complete language and library design with independently deliverable milestones; no singleton tuples, general specialization, or guaranteed allocation-free recursion. |
