# Contracts and classes

## Traits versus interfaces

```dovetail
trait Described =
    function describe(self): String

record Label =
    text: String

implement Described for Label =
    function describe(self): String = self.text

function render<T>(value: T): String where T: Described = value.describe()
```

Traits support generic bounds, same-type `Self` arguments, static/generic methods,
and associated types. They are not value types. `implement <T> Trait for Box<T>`
defines generic implementations. A trait or implementing type must be owned by the
implementation's package (orphan rule); importing a type/trait brings its package's
implementations. Overlapping implementations are rejected.

Use an `interface` when a field, return value, or heterogeneous collection needs
runtime dispatch. Declaration/implementation syntax parallels traits. Concrete
implementations coerce to the interface; interface bounds also work in generics.
`Named and Counted` is an intersection of interfaces. Upcasts can discard members
but cannot claim contracts the source does not prove.

Interface restrictions: every method takes self; no static or method-generic
functions; no associated types; `Self` only as receiver or bare return type,
not another argument or nested `Option<Self>`; parents must be interfaces.
A trait may extend traits/interfaces. Use enums for a closed set of variants.

## Inheritance, defaults, properties

Contracts use `extends Parent` for inheritance. Implementing a child requires its
parent contracts; do not assume an unrelated implementation supplies missing
obligations. Methods and `property size(self): Int32` may have default bodies;
explicit matching implementations override defaults. Resolve ambiguous members
with `TraitName.method(receiver, arguments)` or
`Trait<Type>.method(receiver, arguments)` rather than relying on import order.
`ExtensionName.method(receiver, arguments)` explicitly selects an extension;
properties also support qualification as a receiver-taking call.

Core contracts include `Equatable` (`equals`), `Comparable` (`compare`),
`Hashable`, `Display` (`format`), `Default`, `Iterable`, and `Concat`.
Use `@derive(Equatable)` on suitable records/enums rather than assuming structural
`==` is automatic. Derives still obey visibility. Equality and hashing must agree.

## Classes

```dovetail
class Counter private (mutable value: Int32) =
    public function make(): Counter = Counter(0)
    public function read(self): Int32 = self.value
    public function increment(self): Unit = self.value = self.value + 1
```

Constructor parameters are fields; call `Counter(...)` where permitted. Private
constructors reserve construction for the class. Members/fields default private;
expose a narrow public API. Body `let` fields initialize per instance; `let mutable`
allows mutation; `let property` computes on access; `let static` fields and `let static property` getters belong
to the class. A `self` parameter marks an instance method; its absence means static.

```dovetail
abstract class Shape(name: String) =
    public function getName(self): String = self.name
    public abstract function area(self): Int32

class Square(name: String, side: Int32) extends Shape(name) =
    public override function area(self): Int32 = self.side * self.side
```

A sealed hierarchy uses `sealed abstract class Shape()` and same-package
`final class Circle(...) extends Shape()` leaves (or sealed abstract intermediate
classes), enabling exhaustive typed pattern matches. Concrete children must satisfy
abstract methods. Class contract syntax is `class X(...) implements A and B = ...`.

Classes support single inheritance, abstract/sealed abstract declarations, overrides,
and `implements` contracts. Keep override signatures compatible, including bounds;
an implementation cannot strengthen a promised method's preconditions. Named
arguments bind against the statically visible declaration, not a runtime override.
Named extensions also work on classes and require explicit imports.

`ClassIdentity.equals(left, right)` and `ClassIdentity.hash(value)` expose identity
for classes. `where T: class` permits those operations but does not implement
Equatable/Hashable or grant constructors. Mutable fields do not make identity
change; do not mix structural equality with identity hashing.

## Checked interface dispatch

<!-- book-example: {"name": "aicontracts", "depends": [], "stdout": ""} -->
```dovetail
package aicontracts

interface Label =
    function text(self): String

record Named =
    name: String

implement Label for Named =
    function text(self): String = self.name

function render(value: Label): String = value.text()

function main(): Unit = assert render(Named { name = "Dovetail" }) == "Dovetail"

test "dispatches through interface" = main()
```
