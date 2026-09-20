# Types and collections

## Data declarations

```dovetail
record Point =
    x: Int32
    y: Int32

enum State =
    Waiting
    Ready(Point)

newtype CustomerId = Int64

type Coordinates = (Int32, Int32)

module Point =
    function make(x: Int32, y: Int32): Point = Point { x = x; y = y }
    function moved(self, dx: Int32): Point = self with { x = self.x + dx }
```

Records have named immutable fields; construction uses braces, field assignments,
and semicolons or layout. `with` makes a shallow updated record. Arrays/classes
inside a record remain shared mutable objects. Enums carry variant-specific data;
match them exhaustively. Tuple fields are `_0`, `_1`, etc., or destructure them.
Aliases are not nominal boundaries; a newtype is distinct, constructed as
`CustomerId(42i64)`, with a wrapped `.value` when visible.

`public record Account private = ...` and `public enum State private = ...`
reserve construction for the associated module. Consumers can still read record
fields and pattern-match; record `with` updates also require construction access.
`newtype Amount private = Int32` additionally restricts wrapped-value inspection
and destructuring. Implementations, extensions, same-package callers, and derives
do not gain privileged access. Expose validated factories and queries/transitions.

## Modules and extensions

A module named for a type supplies static functions and instance functions taking
`self`; consumers can use instance syntax. Standalone modules group qualified
functions. Generic type modules put their type parameters in scope; globals are
per instantiation (use explicit type arguments for globals).

```dovetail
extension ArrayHelpers<T> for Array<T> =
    function first(self): Option<T> =
        if self.length == 0 then None else Some(self[0])
```

Every extension has a name. Import that name to enable its members, including in
the same package. Prefer the associated module for a type's own core operations.

## Lists, arrays, slices

| Need | Form and behavior |
|---|---|
| Immutable linked sequence | `List<T>`, `[1, 2]`, `[]`, `head :: tail`, `left ++ right` |
| Fixed-length mutable indexed storage | `Array<T>`, `[|1, 2|]`, `[||]`, `values[i]` |
| Writable shared view | `Slice<T>`, `values[|start..end|]` |
| Read-only view | `ReadonlySlice<T>`; prevents writes through that view, not other aliases |

Use `array[|..|]` for the full view, `[|start..|]`/`[|..end|]` for open bounds,
and `.readonly` for a read-only view without copying. `slice(start, end)` uses an
exclusive end; prefer checked constructors when range validation is needed.
Slice ends are exclusive; omitted bounds select the start/end. Slicing retains
shared storage, not a copy. Invalid bounds or indexing panic; validate untrusted
indices. Copy when independent snapshots are required. Array length is fixed;
use collection APIs for growth. List `map`, `filter`, `foldLeft`, `find`, and
pattern matching support immutable processing. Import relevant library extensions
before relying on operations. Inspect the actual type's methods; similarly named
collections can expose different folds/conversions. Strings are UTF-8: distinguish
byte offsets, characters, and lengths when interacting with byte slices.

## Runtime tests and casts

Use `value is ConcreteType` before `value as ConcreteType` when recovering from
`Any`. `as` panics on failure; there is no `as?`. Concrete generic instantiations
are distinguished (e.g. `Option<Int32>` versus `Option<String>`). These operators
are not numeric conversions between unrelated concrete types. `Any`, `Never`,
and interface targets are unsupported recovery targets from `Any`. Class casts
and interface upcasts have their own subtype rules; see contracts.

## Checked private construction

<!-- book-example: {"name": "aitypes", "depends": [], "stdout": ""} -->
```dovetail
package aitypes

newtype Quantity private = Int32

module Quantity =
    public function make(value: Int32): Result<Quantity, String> =
        if value > 0 then Ok(Quantity(value)) else Error("quantity must be positive")

    public function read(self): Int32 = self.value

function main(): Unit = assert Quantity.make(2).require.read() == 2

test "invalid quantity is rejected" = assert Quantity.make(0).isError
```
