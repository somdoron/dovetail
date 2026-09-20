# Generics and tuple extension

## Parameters and bounds

```dovetail
function identity<T>(value: T): T = value

record Box<T> =
    value: T

function larger<T>(left: T, right: T): T where T: Comparable =
    if left > right then left else right
```

Infer arguments from inputs/expected types or specify `identity<Int32>(1)`.
Use `where T: A + B` or separate comma-delimited constraints; inline `<T: A>`
also works. A generic body's operations must follow its declared bounds.
Specializations are monomorphized; do not assume unbounded dynamic operations.

Generic modules/extensions/implementations introduce enclosing parameters.
A method's `where` may constrain an enclosing parameter without redeclaring it.
The extra bound applies to that method and references to it, not sibling methods
or construction of the container. Do not shadow enclosing type-parameter names.
An implementation or override cannot add bounds absent from the promised method;
restrict the implementation block instead when appropriate.

`where T: class` proves class identity support; superclass bounds imply it.
Arrays, primitives, records, Any, and interface values do not satisfy it.
Primitive subtype bounds are not numeric conversions.

## Variance and runtime patterns

Plain parameters are invariant. Use `out T` for produced values and `in T` for
consumed values only when the API requires widening/narrowing. The compiler checks
positions, including mutable fields. Conditional bounds on variant class parameters
can invalidate virtual dispatch; do not assume such bounds are accepted.

Typed patterns such as `case box: Box<Int32> => box.value` distinguish concrete
instantiations. Include a fallback where the subject remains open-ended. Use
ordinary generic bounds when runtime type discrimination is unnecessary.

## Associated types and defaults

```dovetail
trait Producer =
    type Output
    function produce(self): Output

record NumberProducer =
    value: Int32

implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value

function produce<P>(producer: P): P.Output where P: Producer = producer.produce()
```

Associated outputs belong to an implementation, not a caller-selected parameter.
Use an equality bound such as `where P: Producer<Output = Int32>` when a caller
requires a specific associated output. Associated types have no declaration defaults;
implementations supply them. Duplicate associated names across bounds are ambiguous.
Generic associated types use declarations such as `type Rebind<U>` and projections
such as `A.Rebind<T>`. Preserve their declared bounds in implementations. Traits
can supply generic defaults whose method parameters are independent of implementing
type parameters. Interfaces cannot expose generic methods or associated types.
Inherited overloads still require unambiguous signatures and satisfied bounds.

## Tuple extension

Binary `~` appends one element: `1 ~ true ~ "x"` is `(1, true, "x")`.
The right operand stays one element; a tuple on the left grows. `(T, U)` remains
a two-element type even when T is a tuple; `T ~ U` extends T's shape.

```dovetail
function append<T, U>(left: T, right: U): T ~ U = left ~ right
```

`.init` returns the prefix (the first element for a pair); `.last` returns the
last element. Reconstructing `((1, true), "x")` with `.init ~ .last` flattens the
left tuple; use `(.init, .last)` to retain that pair shape.

`Tuple` is a structural bound for actual tuples of at least two elements, not
an implementable trait or runtime interface. Recursive implementation heads may
use exactly `T ~ U where T: Tuple` with declared T/U parameters. Combine a pair
case `(A, B)` with that recursive case; the tuple bound establishes disjoint arity.
Ordinary user-trait bounds do not prove disjointness. Unconstrained/chained symbolic
implementation heads are unsupported. Reverse inference of extension operands
requires the supported tuple-constrained form.
