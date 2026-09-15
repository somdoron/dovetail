# Part 8: Traits and Implementations

Traits define shared behavior that types can implement and generic functions can
require through bounds. When a contract must also be a value type, use an
[interface](06-type-system.md#611-interfaces-and-interface-types). Traits and
interfaces share implementation blocks, inheritance, defaults, and explicit
member qualification.

---

## 8.1 Defining Traits

### Basic Trait Definition

A trait declares a set of methods that types must implement:

```dovetail
trait Printable =
    function format(self): String
```

Traits can have multiple methods:

```dovetail
trait Describable =
    function describe(self): String
    function summary(self): String
```

### Traits with Parameters

Trait methods can take parameters:

```dovetail
trait Comparable =
    function compare(self, other: Self): Int32
```

The `Self` type refers to the implementing type.

### Generic Traits

Traits can have type parameters:

```dovetail
trait Container<T> =
    function get(self): T
    function set(self, value: T): Self
```

---

## 8.2 Implementing Traits

### Basic Implementation

Use `implement` to make a type conform to a trait:

```dovetail
record Point =
    x: Int32
    y: Int32

trait Printable =
    function format(self): String

implement Printable for Point =
    function format(self): String =
        "(${self.x}, ${self.y})"

let p = Point { x = 3; y = 4 }
p.format()  // "(3, 4)"
```

### Implementing Multiple Traits

A type can implement multiple traits:

```dovetail
trait Printable =
    function format(self): String

trait Describable =
    function describe(self): String

record User =
    name: String
    age: Int32

implement Printable for User =
    function format(self): String = self.name

implement Describable for User =
    function describe(self): String =
        "${self.name} is ${self.age} years old"
```

### Implementing Traits for Enums

```dovetail
enum Color =
    Red
    Green
    Blue

trait Printable =
    function format(self): String

implement Printable for Color =
    function format(self): String =
        match self with
            case Red => "red"
            case Green => "green"
            case Blue => "blue"
```

### Implementations and Private Types

An `implement` block does not gain access to a type's private operations. For private records and enums, it may read fields and match patterns, but must call associated-module functions to construct values or update a record with `with`. A private newtype also requires module functions to inspect its wrapped value:

```dovetail
newtype Amount private = Int32

module Amount =
    public function make(value: Int32): Amount =
        assert value >= 0
        Amount(value)

    public function read(self): Int32 = self.value

trait Counted =
    function count(self): Int32

implement Counted for Amount =
    public function count(self): Int32 = self.read()
```

Writing `self.value` or an `Amount(value)` pattern directly in that implementation is an error. The rule also applies to generic implementations, extensions, and generated implementations; sharing the defining package does not grant access.

### Implementing Generic Traits

```dovetail
trait Container<T> =
    function get(self): T

record Box<T> =
    value: T

implement <T> Container<T> for Box<T> =
    function get(self): T = self.value
```

### Trait Properties

Traits can declare properties that types must provide. Properties in traits define getters that implementations must satisfy:

```dovetail
trait Measurable =
    property size(self): Int32

record Box =
    width: Int32
    height: Int32
    depth: Int32

implement Measurable for Box =
    property size(self): Int32 = self.width * self.height * self.depth

function main(): Unit =
    let b = Box { width = 2; height = 3; depth = 4 }
    assert b.size == 24  // calls the property getter
```

Key points about trait properties:
- Declared with `property name(self): Type`; an optional body provides a default
- Implementations without a default provide the property using `property name(self): Type = expression`
- Properties are accessed like fields but call getter functions
- Properties are evaluated every time they're accessed

### Implementing Trait Properties

When implementing a trait with properties, provide the property implementation(self):

```dovetail
trait Stats =
    property average(self): Int32
    property total(self): Int32

record DataSet =
    values: (Int32, Int32, Int32)

implement Stats for DataSet =
    property total(self): Int32 =
        self.values._0 + self.values._1 + self.values._2

    property average(self): Int32 =
        self.total / 3

function main(): Unit =
    let data = DataSet { values = (10, 20, 30) }
    assert data.total == 60
    assert data.average == 20
```

Trait properties can reference each other in implementations, just like class body properties can reference other fields.

### Combining Trait Methods and Properties

Traits can have both methods and properties:

```dovetail
trait Shape =
    property area(self): Int32
    property perimeter(self): Int32
    function describe(self): String

record Rectangle =
    width: Int32
    height: Int32

implement Shape for Rectangle =
    property area(self): Int32 = self.width * self.height
    property perimeter(self): Int32 = 2 * (self.width + self.height)

    function describe(self): String = "Rectangle(${self.width}x${self.height})"
```

### Using Trait Methods

Once a trait is implemented, you can call its methods:

```dovetail
let user = User { name = "Alice"; age = 30 }
println(user.format())     // "Alice"
println(user.describe())   // "Alice is 30 years old"
```

### Trait Bounds in Functions

Use traits as constraints in generic functions:

```dovetail
function printAll<T>(items: Array<T>): Unit
    where T: Printable =
    for item in items do
        println(item.format())
```

### Automatic Import of Implementations

Trait implementations are automatically imported along with types and traits. You never need to explicitly import implementations.

**Importing a type brings its implementations:**

When you import a type, all trait implementations for that type (defined in its package) come with it.

```dovetail
// In package "users"
record User =
    name: String
    email: String

implement Equatable for User = ...
implement Comparable for User = ...
implement JsonSerializable for User = ...
```

```dovetail
// In your code
import users.User

// All implementations from the users package come with User
let u1 = User { name = "Alice"; email = "alice@example.com" }
let u2 = User { name = "Bob"; email = "bob@example.com" }
u1 == u2           // Equatable works
u1 < u2            // Comparable works
u1.toJson()        // JsonSerializable works
```

**Importing a trait brings its implementations:**

When you import a trait, all implementations of that trait (defined in its package) come with it.

```dovetail
// In package "serialization"
trait JsonWriter =
    function writeJson(self, value: Any): String

implement JsonWriter for FileOutput = ...
implement JsonWriter for StringBuffer = ...
implement JsonWriter for HttpResponse = ...
```

```dovetail
// In your code
import serialization.JsonWriter

// All implementations from the serialization package come with JsonWriter
// You can now use JsonWriter methods on FileOutput, StringBuffer, HttpResponse
```

---

## 8.3 Core Traits

Dovetail provides several built-in traits that enable common operations.

### Equatable

Enables `==` and `!=` comparisons:

```dovetail
trait Equatable =
    function equals(self, other: Self): Bool
```

Primitive types have prelude implementations of `Equatable`. Records and enums can request structural equality with `@derive(Equatable)` when their fields/variants are equatable.

```dovetail
@derive(Equatable)
record Point =
    x: Int32
    y: Int32

// Derived equality compares the Int32 fields
let p1 = Point { x = 1; y = 2 }
let p2 = Point { x = 1; y = 2 }
p1 == p2  // true
```

### Identity Equality and Hashing

`ClassIdentity` uses **reference identity**, not the contents of an object. Two separately created instances of the same class have different identities, even when all their fields contain equal values. Two references to the same instance share its identity.

Classes can implement equality and hashing explicitly using reference identity:

```dovetail
class IdentityBox<T>(public value: T)

implement <T> Equatable for IdentityBox<T> =
    public function equals(self, other: IdentityBox<T>): Bool =
        ClassIdentity.equals(self, other)

implement <T> Hashable for IdentityBox<T> =
    public function hash(self): Int64 = ClassIdentity.hash(self)

function checkBoxIdentity(): Unit =
    let first = IdentityBox(42)
    let second = IdentityBox(42)
    let alias = first
    assert !ClassIdentity.equals(first, second)  // Same contents, different instances
    assert ClassIdentity.equals(first, alias)    // Two references to one instance
    assert first != second                     // The Equatable implementation above uses identity
```

The element type needs no equality or hashing bounds: these implementations use the box's identity. Neither implementation is automatic, and an implementation for a base class does not automatically provide one for every subclass.

`Hashable.hash` returns an `Int64`. Whenever two values compare equal through `Equatable`, their trait hashes must match. Collisions between unequal values are permitted. A class using structural equality should therefore hash its equality-relevant contents; using identity hashing for structurally equal but distinct objects would generally violate the contract.

Raw `ClassIdentity.equals` and `ClassIdentity.hash` remain available even if a class defines structural trait implementations. They do not call those implementations. The current mutable hash collections also require `Default` for keys; providing these two traits alone does not remove that separate requirement. A class can implement `Default` using a shared placeholder instance when that is safe for its API. For example, the runtime's `Waiter` uses one global instance with an empty suspended-fiber stack for unused hash-table slots, while real scheduler conditions always allocate fresh instances. The collections track occupied slots separately and do not mutate this placeholder's suspended-fiber stack.

### Indexed reads and writes

`Index<K>` enables `value[key]` on user types. Its associated `Output` is the read result. `IndexSet<K>` independently enables `value[key] = item`; its associated `Value` is the accepted item type. Keys need not be integers.

```dovetail
function read<C, K, T>(collection: C, key: K): T where C: Index<K, Output = T> = collection[key]

function write<C, K, T>(collection: C, key: K, item: T): Unit where C: IndexSet<K, Value = T> = collection[key] = item
```

`Slice<T>` implements both capabilities with `Int32` keys. `ReadonlySlice<T>` implements only reads. A method named `get` or `set` does not grant the corresponding syntax without the trait. Array and String indexing retain their built-in behavior; they do not implement these traits. Range slicing (`[|start..end|]`) is a separate operation.

Implementations are selected by receiver and compatible key parameter type, using ordinary argument assignability. Neither the expected read result nor the assigned value resolves ambiguous implementations. Receiver, key, and value are evaluated once, in that order.


### Arithmetic operators and associated output constraints

`Add<R>`, `Sub<R>`, `Mul<R>`, and `Div<R>` overload `+`, `-`, `*`, and `/`.
Each declares an associated `Output` and a method named `add`, `sub`, `mul`, or `div`:

```dovetail
public trait Add<R> =
    type Output
    function add(self: Self, rhs: R): Output
```

The operands select the implementation, whose `Output` determines the result.
Numbers implement these traits with native arithmetic; `Uint128` supports addition,
subtraction, and multiplication, but not division. Strings concatenate with `++`.

Generic functions constrain associated outputs using equality bindings in bounds:

```dovetail
function add<L, R, O>(left: L, right: R): O where L: Add<R, Output = O> =
    left + right
```

The compiler can infer `O` from the matching implementation. A function returning
its left operand's type instead uses `where L: Add<R, Output = L>`; `Output = Self`
is never assumed. The same bindings work for ordinary trait methods and for
associated types nested inside other types. Positional trait arguments precede
bindings. A helper can also name an implementation's output as `T.Output`.
See [Associated Outputs](26-advanced-generics.md#268-associated-outputs) for a
complete example. Equality bindings for generic associated types remain unsupported.

Newtypes must implement each capability explicitly, including `Equatable` and
`Comparable`. Wrapping a number or String does not grant its operators. Public
newtypes may opt into structural equality with `@derive(Equatable)`. Private
newtypes delegate through their associated modules to preserve access rules.

### Unary negation

`Neg` lets library types overload unary `-` without changing their result type:

```dovetail
public trait Neg =
    function negate(self: Self): Self

function opposite<T>(value: T): T where T: Neg = -value
```

`BigInt` and `Decimal` implement `Neg`. Primitive signed integers and floats
retain their native unary negation; this trait does not add implicit conversions.

### Concat

Overloads `++`, the concatenation operator:

```dovetail
trait Concat<R> =
    type Output
    function concat(self: Self, rhs: R): Output
```

`++` is deliberately separate from `+`: `+` is arithmetic, while `++` is "join these two things". Implementing `Concat` does not overload `+`, and vice versa.

`String` implements `Concat<String>` with `Output = String`, backed by native concatenation. Other types opt in:

```dovetail
record Line =
    text: String

implement Concat<Line> for Line =
    type Output = Line
    function concat(self: Line, other: Line): Line =
        Line { text = self.text ++ other.text }

let joined = Line { text = "a" } ++ Line { text = "b" }
```

Like `Div`, the right-hand type is a parameter and `Output` is an associated type, so the operands need not match and the result need not be `Self`:

```dovetail
implement Concat<String> for Line =
    type Output = Line
    function concat(self: Line, other: String): Line = Line { text = self.text ++ other }
```

`++` is left-associative and binds at the same level as `+`.

### Comparable

Enables ordering comparisons (`<`, `>`, `<=`, `>=`):

```dovetail
trait Comparable =
    function compare(self, other: Self): Int32  // -1, 0, or 1
```

Numeric types and strings implement `Comparable` by default.

```dovetail
function max<T>(a: T, b: T): T
    where T: Comparable =
    if a > b then a else b

max(10, 20)  // 20
```

---

## 8.4 The Orphan Rule

The orphan rule prevents conflicts when multiple packages could implement the same trait for the same type.

### The Rule

You can only implement a trait for a type if:
1. **You own the trait** (it's defined in your package), OR
2. **You own the type** (it's defined in your package)

### Why This Matters

Without this rule, two different packages could implement the same trait for the same type differently, causing conflicts:

```dovetail
// Package A
implement Printable for Int32 =
    function format(self): String = "number: $self"

// Package B (different implementation!)
implement Printable for Int32 =
    function format(self): String = "int($self)"

// Which one should be used? Conflict!
```

### Valid Implementations

```dovetail
// In your package: you own the trait
trait MyTrait =
    function doSomething(self): Int32

// OK - you own the trait
implement MyTrait for Int32 =
    function doSomething(self): Int32 = self * 2
```

```dovetail
// In your package: you own the type
record MyRecord =
    value: Int32

// OK - you own the type
implement Printable for MyRecord =
    function format(self): String = "${self.value}"
```

### Invalid Implementation

```dovetail
// Trying to implement someone else's trait for someone else's type
// ERROR: Orphan rule violation
implement SomeoneElsesTrait for SomeoneElsesType =
    function method(self): Int32 = 0
```

### Working Around the Orphan Rule

If you need to implement a foreign trait for a foreign type, wrap it in a newtype:

```dovetail
// Can't implement ThirdPartyTrait for ThirdPartyType directly
// Solution: wrap it
newtype MyWrapper = ThirdPartyType

implement ThirdPartyTrait for MyWrapper =
    function method(self): Int32 = 42
```

---

## 8.5 Trait and Interface Inheritance

`extends` includes a parent's members in a contract. A trait can extend traits
or interfaces; an interface can extend only interfaces.

```dovetail
interface Named =
    function name(self): String

interface Described extends Named =
    function description(self): String

record Item =
    title: String

implement Described for Item =
    function name(self): String = self.title
    function description(self): String = "Item: ${self.name()}"

function main(): Unit =
    let item: Described = Item { title = "Book" }
    let named: Named = item
    assert named.name() == "Book"
    assert item.description() == "Item: Book"
```

The child implementation supplies inherited requirements in the same block,
unless a default body supplies them. A separate implementation of the parent
does not fill missing members in the child implementation.

Implementing the child also makes the concrete type satisfy the parent. If the
type has a direct parent implementation, that implementation wins when the
parent contract is requested. Otherwise, one child implementation can supply the
parent; multiple possible providers are ambiguous. Inherited methods can share a
name when their parameter lists differ. See
[Inherited Overloads](26-advanced-generics.md#2610-inherited-overloads) for an example.

## 8.6 Default Methods and Properties

A trait or interface member can provide a body. An implementation may omit that
member or replace its body. Defaults can call other members of the contract:

```dovetail
trait Measured =
    property size(self): Int32
    property doubled(self): Int32 = self.size * 2
    function describe(self): String = "Size: ${self.size}"

record Batch =
    count: Int32

implement Measured for Batch =
    property size(self): Int32 = self.count

function main(): Unit =
    let batch = Batch { count = 3 }
    assert batch.doubled == 6
    assert batch.describe() == "Size: 3"
```

A child contract can override an inherited default by providing a new body.
Repeating only the inherited signature is not an override. Conflicting defaults
from parents require an explicit implementation to resolve the conflict.

Defaults work in implementation blocks and classes, including generic ones.
Methods with their own type parameters can also provide defaults; see
[Generic Trait Defaults](26-advanced-generics.md#267-generic-trait-defaults).
Classes must implement required static trait members explicitly.

## 8.7 Method Resolution and Explicit Disambiguation

For a concrete receiver, applicable members are considered in this order:

1. Members of the receiver type's associated module.
2. Members of imported named extensions.
3. Trait or interface implementations.

Named extensions must be imported even within the same package. Multiple
applicable candidates at the same priority are ambiguous. Qualify a member to
choose its contract or extension explicitly:

```dovetail
trait ShortLabel =
    function label(self): String

trait LongLabel =
    function label(self): String

record Entry =
    title: String

implement ShortLabel for Entry =
    function label(self): String = self.title

implement LongLabel for Entry =
    function label(self): String = "Entry: ${self.title}"

function main(): Unit =
    let entry = Entry { title = "Book" }
    assert ShortLabel.label(entry) == "Book"
    assert LongLabel.label(entry) == "Entry: Book"
```

Use `Trait<Arguments>.method(receiver, arguments)` for a generic contract, or
`ExtensionName.method(receiver, arguments)` for a named extension. Properties
also support qualification as a call with the receiver argument.

Implementations must not overlap: two implementations that could apply to the
same type and contract are rejected. Different `where` bounds do not establish
that implementations are disjoint. A blanket implementation whose target is a
bare type parameter, such as `implement <T> Printable for T`, is unsupported.

---

## Summary

- **Traits** define shared behavior as a set of methods and properties
- **Trait Properties**: Declared with `property name(self): Type`, implementations provide the getter
- **Implementations** make types conform to traits using `implement Trait for Type`
- **Implementing Properties**: Use `property name(self): Type = expression` to satisfy trait property requirements
- **Automatic Imports**: Importing a type brings its trait implementations; importing a trait brings all its implementations
- **Core Traits**: `Equatable` for `==`, `Comparable` for ordering
- **Orphan Rule**: You must own either the trait or the type to implement a trait
- **Workaround**: Use newtypes to wrap foreign types when needed

- **Inheritance**: `extends` includes parent requirements; child implementations supply inherited members or use defaults
- **Defaults**: Methods and properties can supply reusable bodies
- **Disambiguation**: `TraitName.method(receiver)` selects a contract explicitly
