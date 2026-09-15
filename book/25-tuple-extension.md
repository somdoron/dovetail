# Part 25: Tuple Extension (Advanced)

Tuple extension lets generic code build tuples and define behavior across tuple
arities. This chapter assumes familiarity with [tuples](06-type-system.md#64-tuples),
[generics](07-generics.md), and [trait implementations](08-traits.md).

## 25.1 Extending a Tuple

Use binary `~` to append one element. A non-tuple left operand forms a pair;
a tuple left operand grows by one element:

```dovetail
let pair = 1 ~ true                 // (1, true)
let triple = pair ~ "hello"         // (1, true, "hello")
let nested = pair ~ ("a", "b")      // (1, true, ("a", "b"))
```

The right operand always remains one element. Chaining associates to the left,
so `1 ~ 2 ~ 3` produces `(1, 2, 3)`. Both operands execute once, left to right.

`~` binds below comparisons, so compare an extension result with parentheses:
`(1 ~ true) == (1, true)`. Prefix `~` still means bitwise negation.

## 25.2 Extension in Generic Types

The same operator works in types and generic functions:

```dovetail
function append<T, U>(left: T, right: U): T ~ U = left ~ right
```

The compiler determines the result shape from `T`; no bound or overload is
required. With `T = Int32` and `U = Bool`, the result is `(Int32, Bool)`.
With `T = (Int32, Bool)` and `U = String`, it is `(Int32, Bool, String)`.
While the outer shape of `T` is unknown, the compiler preserves `T ~ U` until
substitution reveals that shape.

Ordinary pair types behave differently: `(T, U)` always has two outer elements.
Substituting a tuple for `T` preserves it as the first element of that pair.

Infer extension operands from ordinary function arguments or supply explicit
type arguments. Reverse inference from an extension result is currently
supported only for the tuple-constrained form described below.

## 25.3 Tuple Shape and Accessors

`Tuple` is a built-in constraint for actual tuples of at least two elements.
Aliases to tuples satisfy it; records and nominal wrappers around tuples do
not. It is usable in `where` bounds, and cannot be implemented, inherited, or
used as a runtime interface type.

Every tuple has `.init` and `.last`:

```dovetail
(1, true).init              // 1
(1, true).last              // true
(1, true, "x").init         // (1, true)
(1, true, "x").last         // "x"
((1, true), "x").init       // (1, true), the first element intact
```

Each accessor evaluates its receiver once. For a pair, `.init` returns the
first element. For a larger tuple, it returns the prefix tuple. `.last` always
returns the final element.

For an unknown `T: Tuple`, the compiler defers the accessor's result type until
it knows the shape. A tuple bound alone does not prove that `.init` returns a
tuple, and it says nothing about the traits implemented by individual elements.

For an actual extension `T ~ U`, `.init` has type `T` and `.last` has type `U`,
even without a bound on `T`:

```dovetail
function prefix<T, U>(left: T, right: U): T = (left ~ right).init
```

Reconstructing an arbitrary tuple with these accessors requires care. If a
pair's first element is itself a tuple, extension flattens that first element:

```dovetail
let pair = ((1, true), "x")
let preserved = (pair.init, pair.last) // ((1, true), "x")
let extended = pair.init ~ pair.last  // (1, true, "x")
```

## 25.4 Pair and Recursive Implementations

A trait can handle tuple arities with a pair case `(A, B)` and a recursive
case `T ~ U where T: Tuple`. The tuple bound guarantees that the recursive case
has at least three outer elements. The pair case matches exactly two, including
pairs whose first element is another tuple.

This example counts outer elements:

```dovetail
trait Arity =
    function arity(self): Int32

implement <A, B> Arity for (A, B) =
    function arity(self: (A, B)): Int32 = 2

implement <T, U> Arity for T ~ U where T: Tuple, T: Arity =
    function arity(self: T ~ U): Int32 = self.init.arity() + 1
```

| Actual type | Implementation | Bindings |
|---|---|---|
| `(Int32, Bool)` | Pair | `A = Int32`, `B = Bool` |
| `(Int32, Bool, String)` | Recursive | `T = (Int32, Bool)`, `U = String` |
| `((Int32, Bool), String)` | Pair | `A = (Int32, Bool)`, `B = String` |

Selection does not depend on declaration order or implementation priority.
A concrete triple implementation of `Arity` would overlap the recursive case
and is rejected. Ordinary user-trait bounds cannot establish that two
implementations are disjoint; `Tuple` supplies a specific structural arity fact.

Supported symbolic implementation heads have exactly the form `T ~ U`, with
both operands declared as implementation parameters and `T: Tuple`. Unconstrained
or chained symbolic heads are not supported.

The prelude's `Equatable`, `Hashable`, `Default`, `Comparable`, and `Display`
use pair/recursive implementations. Equality stops at the first unequal element;
hashing combines elements in order; defaults are constructed element by
element. Ordering is lexicographic, and formatting preserves flat versus nested
tuple punctuation.

## 25.5 Parser Composition

In `standard.parser`, `left.append(right)` and `left ++ right` sequence parsers
and extend their successful results. For example:

```dovetail
let parser = parseChar('(') ++ parseDigit() ++ parseChar(')')
```

Its successful result has type `(Char, Char, Char)`. As with value extension,
a tuple returned by the right parser remains one element.

`zip` keeps both results intact as a pair. Use `zipLeft` or `zipRight` when
one result, such as a delimiter, should be discarded.

## 25.6 Enclosing Parameter Bounds

An individual method can require that an enclosing parameter is a tuple:

```dovetail
record TupleBox<T> = value: T

module TupleBox<T> =
    function append<U>(self: TupleBox<T>, other: U): T ~ U where T: Tuple = self.value ~ other
```

This method returns a tuple of at least three elements. A `TupleBox<Int32>`
can still be constructed, but its `append` method cannot be called because
`Int32` does not satisfy `Tuple`.

The bound is a requirement chosen by this method; the `~` operator itself
continues to accept a scalar left operand. The method's signature, body, and
closures can all use its tuple proof. A generic caller must carry a matching
proof, and taking a method reference checks the same requirement as a call.
The [advanced generics chapter](26-advanced-generics.md#263-bounds-on-enclosing-parameters) explains
the rules for modules, classes, implementations, and extension blocks.

For the compiler design and implementation milestones, see the
[tuple extension design](../docs/tuple-extension-design.md).
