# Part 9: Classes and Object-Oriented Programming

Classes in Dovetail provide object-oriented programming with encapsulation, inheritance, and polymorphism. Unlike records (which are pure data), classes combine data with behavior and support mutable state.

---

## 9.1 Class Definitions

A class is defined with the `class` keyword, followed by the class name, constructor parameters, and methods:

```dovetail
class Point(x: Int32, y: Int32) =
    public function getX(self): Int32 = self.x
    public function getY(self): Int32 = self.y
    public function sum(self): Int32 = self.x + self.y
```

Key points:
- Constructor parameters define the class fields
- Methods are defined in the class body

### Creating Instances

Create instances by calling the class name like a function:

```dovetail
let p = Point(3, 4)
let x = p.getX()     // 3
let total = p.sum()  // 7
```

### Field Access

Fields can be accessed directly (based on visibility):

```dovetail
class Point(public x: Int32, public y: Int32) =
    public function dummy(self): Int32 = 0

function getX(p: Point): Int32 = p.x  // Direct field access
```

---

## 9.2 Methods

### Instance Methods

Instance methods take `self` as the first parameter:

```dovetail
class Counter(mutable value: Int32) =
    public function getValue(self): Int32 = self.value
    public function increment(self): Unit = self.value = self.value + 1
```

### Static Methods (Smart Constructors)

Methods without `self` are static and called on the class itself. Static methods are commonly used as "smart constructors" to provide alternative ways to create instances:

```dovetail
class Point(x: Int32, y: Int32) =
    public function getX(self): Int32 = self.x
    public function origin(): Point = Point(0, 0)           // Smart constructor
    public function fromSingle(v: Int32): Point = Point(v, v)  // Smart constructor
    public function zero(): Int32 = 0                        // Static utility

function main(): Unit =
    let p1 = Point.origin()       // (0, 0)
    let p2 = Point.fromSingle(5)  // (5, 5)
    let p3 = Point(3, 7)          // Primary constructor
    ()
```

Smart constructors can also perform validation:

```dovetail
class PositiveInt private (value: Int32) =
    public function get(self): Int32 = self.value
    
    public function create(n: Int32): Option<PositiveInt> =
        if n > 0 then Some(PositiveInt(n)) else None

function main(): Unit =
    match PositiveInt.create(42) with
        case Some(p) => p.get()
        case None => 0
    ()
```

---

## 9.3 Class Body Fields

Classes can include field declarations in the body using a unified `let` syntax. These fields can be stored fields, properties (computed getters), static fields, or static properties.

### Stored Fields

Basic stored fields allocate memory in each instance:

```dovetail
class Rectangle(width: Int32, height: Int32) =
    let color: String = "blue"
    let name: String = "rectangle"

    public function getColor(self): String = self.color
    public function getName(self): String = self.name

function main(): Unit =
    let r = Rectangle(3, 4)
    r.getColor()  // "blue"
    r.getName()   // "rectangle"
    ()
```

Key points about stored fields:
- They are evaluated when the instance is created
- They are **private** by default (not accessible outside the class)
- They are **immutable** by default
- They can reference constructor parameters and other fields
- Non-let expressions in the class body run as side effects during construction

### Property Fields

Properties generate getter functions that are evaluated each time they're accessed:

```dovetail
class Rectangle(width: Int32, height: Int32) =
    let property area: Int32 = self.width * self.height
    let property perimeter: Int32 = 2 * (self.width + self.height)

    public function getArea(self): Int32 = self.area      // Calls generated getter
    public function getPerimeter(self): Int32 = self.perimeter

function main(): Unit =
    let r = Rectangle(3, 4)
    r.getArea()      // 12 - computed on access
    r.getPerimeter() // 14 - computed on access
    ()
```

Key differences between stored fields and properties:
- **Stored fields**: Evaluated once during construction, stored in memory
- **Properties**: Evaluated every time they're accessed, no storage

### Static Stored Fields

Static fields belong to the class itself, not individual instances:

```dovetail
class Config(name: String) =
    public let static defaultTimeout: Int32 = 30
    public let static maxRetries: Int32 = 3


function main(): Unit =
    Config.defaultTimeout  // 30 - accessed on class, not instance
    ()
```

Static fields are initialized once when the class is first loaded.

### Static Property Fields

Static properties are like static fields but computed each time they're accessed:

```dovetail
class Math(value: Int32) =
    public let static property pi: Float64 = 3.14159265359
    public let static property e: Float64 = 2.71828182846

    public function dummy(self): Int32 = 0

function main(): Unit =
    Math.pi  // 3.14159265359 - computed on access
    ()
```

### Mutable Fields

Use `let mutable` for fields that can be changed after construction:

```dovetail
class Counter(initial: Int32) =
    let mutable count: Int32 = initial

    public function increment(self): Unit = self.count = self.count + 1
    public function getCount(self): Int32 = self.count
```

### Chained Fields

Fields can depend on each other (in declaration order):

```dovetail
class Stats(base: Int32) =
    let doubled: Int32 = base * 2
    let quadrupled: Int32 = doubled * 2  // Uses earlier field

    public function getQuadrupled(self): Int32 = self.quadrupled
```

### Side Effects in Constructor Body

Any expression in the class body runs during construction:

```dovetail
class Logger(name: String) =
    let _ = debug("Creating logger: " ++ name)  // Runs at construction

    public function log(self, msg: String): Unit = debug(name ++ ": " ++ msg)
```

### Field Visibility

Fields can have visibility modifiers:

```dovetail
class Entity(id: Int32) =
    public let name: String = "entity"          // Accessible everywhere
    private let secret: String = "classified"   // Only within class
    let internal: String = "data"               // Package-level (default)

    public let property publicProp: Int32 = self.id * 2
    private let property privateProp: Int32 = self.id * 3

    public function dummy(self): Int32 = 0
```

---

## 9.4 Private Constructors

Use the `private` keyword after the class name to make the constructor private:

```dovetail
class Singleton private (value: Int32) =
    public function get(self): Int32 = self.value
    
    public function instance(): Singleton = Singleton(42)  // Only accessible here

function main(): Unit =
    // let s = Singleton(10)  // Error: constructor is private
    let s = Singleton.instance()  // OK - use smart constructor
    s.get()
    ()
```

Private constructors are useful for:
- Singleton patterns
- Factory patterns where validation is required
- Ensuring instances are created only through smart constructors

---

## 9.5 Mutable Fields

By default, class fields are immutable. Use `mutable` to allow modification:

```dovetail
class Counter(mutable value: Int32) =
    public function getValue(self): Int32 = self.value
    public function increment(self): Unit = self.value = self.value + 1
    public function add(self, amount: Int32): Unit = self.value = self.value + amount

function main(): Unit =
    let c = Counter(0)
    c.increment()
    c.increment()
    c.add(10)
    c.getValue()  // 12
    ()
```

You can mix mutable and immutable fields:

```dovetail
class Entity(id: Int32, mutable health: Int32) =
    public function getId(self): Int32 = self.id           // id is immutable
    public function getHealth(self): Int32 = self.health
    public function damage(self, amount: Int32): Unit =
        self.health = self.health - amount          // health is mutable
```

Attempting to assign to an immutable field results in a compile error.

---

## 9.6 Inheritance

Classes can extend other classes using `extends`:

```dovetail
class Animal(name: String) =
    public function getName(self): String = self.name

class Dog(name: String, breed: String) extends Animal(name) =
    public function getBreed(self): String = self.breed

function main(): Unit =
    let d = Dog("Rex", "German Shepherd")
    d.getName()   // "Rex" - inherited from Animal
    d.getBreed()  // "German Shepherd" - defined in Dog
    ()
```

The `extends` clause specifies:
1. The parent class name
2. Arguments to pass to the parent's constructor

Child classes inherit all methods from the parent and can access parent fields through inherited methods.

---

## 9.7 Abstract Classes

Abstract classes cannot be instantiated directly and can contain abstract methods:

```dovetail
abstract class Shape(name: String) =
    public function getName(self): String = self.name    // Concrete method
    public abstract function area(self): Int32           // Abstract method

class Square(name: String, side: Int32) extends Shape(name) =
    public override function area(self): Int32 = self.side * self.side  // Must implement

function main(): Unit =
    // let s = Shape("test")  // Error: cannot instantiate abstract class
    let sq = Square("square", 5)
    sq.area()  // 25
    ()
```

Key points:
- Use `abstract class` to define an abstract class
- Use `abstract function` to declare methods without implementation
- Abstract methods can only appear in abstract classes
- Concrete subclasses must implement all abstract methods

---

## 9.8 Sealed Abstract Classes

A `sealed abstract class` is an abstract class whose subclasses are restricted to the same package. This allows the compiler to know all possible subclasses and perform **exhaustiveness checking** on pattern matches — just like enums.

### Defining a Sealed Hierarchy

```dovetail
sealed abstract class Shape()
final class Circle(public radius: Float64) extends Shape()
final class Rectangle(public width: Float64, public height: Float64) extends Shape()
```

Rules for subclasses of a sealed class:
- Must be in the **same package**
- Must be either `final class` (leaf — cannot be extended) or `sealed abstract class` (intermediate — itself sealed)

### Exhaustive Pattern Matching

Because the compiler knows all subclasses, it can verify you've handled every case:

```dovetail
function area(s: Shape): Float64 =
    match s with
        case c: Circle => 3.14 * c.radius * c.radius
        case r: Rectangle => r.width * r.height
```

If you forget a case, the compiler reports an error:

```dovetail
function area(s: Shape): Float64 =
    match s with
        case c: Circle => 3.14 * c.radius * c.radius
        // Error: non-exhaustive match: missing case for Rectangle
```

You can still use a wildcard to catch remaining cases:

```dovetail
function describe(s: Shape): String =
    match s with
        case c: Circle => "circle"
        case _ => "other shape"
```

### Deep Sealed Hierarchies

Sealed abstract classes can form multi-level hierarchies. Matching on an intermediate sealed class covers all its leaf subclasses:

```dovetail
sealed abstract class Shape()
sealed abstract class Polygon() extends Shape()
final class Triangle(public base: Float64) extends Polygon()
final class Rectangle(public width: Float64) extends Polygon()
final class Circle(public radius: Float64) extends Shape()

function describe(s: Shape): String =
    match s with
        case p: Polygon => "polygon"   // Covers both Triangle and Rectangle
        case c: Circle => "circle"
```

### Sealed vs Enums

Both sealed classes and enums represent a fixed set of alternatives with exhaustive matching. Choose based on your needs:

- **Enums**: Best for simple discriminated unions where all variants share the same type parameters. Variants are lightweight (no methods or trait implementations).
- **Sealed classes**: Best when subclasses need their own type parameters, methods, trait implementations, or mutable state. Each subclass is a full class with its own capabilities.

---

## 9.9 Generic Classes

Classes can have type parameters:

```dovetail
class Box<T>(value: T) =
    public function get(self): T = self.value

function main(): Unit =
    let intBox = Box(42)
    let strBox = Box("hello")
    intBox.get()  // 42
    ()
```

### Multiple Type Parameters

```dovetail
class Pair<A, B>(first: A, second: B) =
    public function getFirst(self): A = self.first
    public function getSecond(self): B = self.second

function main(): Unit =
    let p = Pair(1, "one")
    p.getFirst()   // 1
    p.getSecond()  // "one"
    ()
```

### Mutable Generic Fields

```dovetail
class MutableBox<T>(mutable value: T) =
    public function get(self): T = self.value
    public function set(self, newValue: T): Unit = self.value = newValue

function main(): Unit =
    let box = MutableBox(10)
    box.set(20)
    box.get()  // 20
    ()
```

### Constrained Generic Classes

Use `where` clauses or inline constraints to restrict type parameters:

```dovetail
trait Countable =
    public function count(self): Int32

class Counter<T: Countable>(value: T) =
    public function getCount(self): Int32 = self.value.count()
```

Or with `where` clause:

```dovetail
class Wrapper<T>(value: T)
    where T: Printable
=
    public function fmt(self): String = self.value.format()
```

---

## 9.10 Implementing Traits for Classes

Generic classes can inherit trait method and property defaults. For methods with
their own type parameters, see
[Generic Trait Defaults](26-advanced-generics.md#267-generic-trait-defaults).

Classes can implement traits in two ways:

### Inline Implementation

Use `implements` in the class declaration:

```dovetail
trait Speakable =
    public function speak(self): String

class Dog(name: String) implements Speakable =
    public function speak(self): String = "Woof"
    public function getName(self): String = self.name
```

### Multiple Traits

Use `and` to implement multiple traits:

```dovetail
trait Speakable =
    public function speak(self): String

trait Nameable =
    public function getName(self): String

class Dog(name: String) implements Speakable and Nameable =
    public function speak(self): String = "Woof"
    public function getName(self): String = self.name
```

### Combined with Inheritance

Classes can extend a parent and implement traits:

```dovetail
class Dog(name: String) extends Pet(name) implements Speakable and Comparable =
    public function speak(self): String = "Woof"
    public function compare(self, other: Dog): Int32 = 0
```

### External Implementation

You can also implement traits for classes externally:

```dovetail
trait Speakable =
    public function speak(self): String

class Dog(name: String) =
    public function bark(self): String = "Woof"

implement Speakable for Dog =
    public function speak(self): String = self.bark()
```

This is useful when:
- You don't own the class definition
- You want to add trait implementations later
- You're implementing traits from other packages

---

## 9.11 Extension Properties

Named extensions add computed properties without adding stored fields. Import the
extension explicitly, even when it is declared in the same package. Extension
properties use `property name(self): Type`, while class-body computed fields use
`let property name: Type`.

**Complete example (checked in CI)** — no extra dependencies:

<!-- book-example: {"name": "classes", "depends": []} -->
```dovetail
package classes

import classes.RectangleHelpers

class Rectangle(public width: Int32, public height: Int32) =
    public function dimensions(self): (Int32, Int32) = (self.width, self.height)

extension RectangleHelpers for Rectangle =
    property area(self): Int32 = self.width * self.height
    function scaled(self, factor: Int32): Rectangle =
        Rectangle(self.width * factor, self.height * factor)

function main(): Unit =
    let rectangle = Rectangle(3, 4)
    assert rectangle.area == 12
    assert rectangle.scaled(2).area == 48

test "extension properties compute from public fields" = main()
```

Properties are evaluated on access. Extensions follow normal visibility rules and
cannot use private construction to bypass a type's smart constructors. Records
and enums can also be extended; see [Extension Methods](06-type-system.md#69-extension-methods).

---

## 9.12 Extension Methods for Classes

`RectangleHelpers` above defines both a property and a method. Call its method with
ordinary receiver syntax, such as `rectangle.scaled(2)`, after importing the named
extension. An extension does not change the class's inheritance hierarchy.

Generic extensions declare their parameters on the extension name:

```dovetail
class Container<T>(value: T) =
    function getValue(self): T = self.value

extension ContainerHelpers<T> for Container<T> =
    function map<U>(self, transform: T => U): Container<U> =
        Container(transform(self.getValue()))
```

Import `yourPackage.ContainerHelpers` at the call site. See
[Advanced Generics](26-advanced-generics.md#261-generic-extensions) for bounds.

---

## 9.13 Visibility Modifiers

Control access to classes, fields, and methods with visibility modifiers:

### Class Visibility

```dovetail
public class PublicPoint(x: Int32, y: Int32) =
    function getX(self): Int32 = self.x

private class PrivateHelper(value: Int32) =
    function get(self): Int32 = self.value

internal class InternalUtil(data: String) =  // Default visibility
    function getData(self): String = self.data
```

### Field Visibility

```dovetail
class Entity(
    public id: Int32,           // Accessible everywhere
    private secret: String,     // Only within this class
    internal data: Int32        // Within this package; fields default to private
) =
    function getId(self): Int32 = self.id
    function getSecret(self): String = self.secret  // OK - inside class
```

Private fields are not accessible outside the class:

```dovetail
function main(): Unit =
    let e = Entity(1, "secret", 42)
    e.id      // OK - public
    // e.secret  // Error - private field
    0
    ()
```

### Method Visibility

Class fields and methods default to `private`. Expose operations with `public` or
`internal` explicitly. This differs from top-level declarations, which default to
`internal`.

```dovetail
class Calculator(value: Int32) =
    private function helper(self): Int32 = self.value * 2
    public function compute(self): Int32 = self.helper() + 1
    internal function packageHelper(self): Int32 = 0  // Explicit package visibility
```

### Combining with Mutable

Visibility can be combined with mutable:

```dovetail
class Counter(private mutable count: Int32) =
    function increment(self): Unit = self.count = self.count + 1
    function getCount(self): Int32 = self.count
```

---

## 9.14 Classes vs Records

| Feature | Class | Record |
|---------|-------|--------|
| Methods | Defined in body | Via associated modules or named extensions |
| Mutability | Mutable fields supported | Always immutable |
| Inheritance | Supported | Not supported |
| Abstract | Supported | Not supported |
| Identity | Reference identity | Value equality |
| Use case | Stateful objects | Pure data |

**When to use classes:**
- You need mutable state
- You need inheritance hierarchies
- You're modeling entities with identity
- You need encapsulation with private fields

**When to use records:**
- You're modeling pure data
- You want value semantics
- You don't need inheritance
- Immutability is preferred

---

## 9.15 Class Identity

`ClassIdentity.equals` asks whether two references name the same instance, independently of its fields or any `Equatable` implementation:

```dovetail
class IdentityCounter(public mutable value: Int32)

function checkIdentity(): Unit =
    let first = IdentityCounter(1)
    let alias = first
    let second = IdentityCounter(1)
    assert ClassIdentity.equals(first, alias)
    assert !ClassIdentity.equals(first, second)
    let hash = ClassIdentity.hash(first)
    first.value = 2
    assert ClassIdentity.equals(first, alias)
    assert ClassIdentity.hash(alias) == hash
```

Both functions are available for every class, including generic classes and abstract base references. Upcasts preserve identity and the hash. To compare related types explicitly, use `ClassIdentity.equals<Base>(left, right)`. Unrelated classes have no implicit common object type for this call.

Every class instance reserves one hidden hash slot. Its value is assigned on the first `ClassIdentity.hash` call and stays fixed for that object's lifetime. Equality does not initialize the hash. Hashes may collide: equal hashes do not prove that objects are identical. They are not persistent identifiers and must not be used as unique handles.

These primitives require no annotation. Classes still need explicit `Equatable` and `Hashable` implementations to use identity semantics through those traits; see [Identity Equality and Hashing](08-traits.md#identity-equality-and-hashing). Generic functions can call both primitives under `where T: class`.

## Summary

- **Class Definitions**: Combine constructor parameters with methods using `class Name(fields) = methods`
- **Instance Methods**: Take `self` parameter; **Static Methods**: No `self`, called on class
- **Smart Constructors**: Use static methods to provide alternative ways to create instances with validation
- **Class Body Fields**: Use unified `let` syntax for stored fields, properties, static fields, and static properties
- **Property Fields**: Use `let property` for computed getters evaluated on each access
- **Static Fields**: Use `let static` for class-level fields; `let static property` for class-level computed getters
- **Private Constructors**: Use `class Name private (fields)` to restrict instantiation to smart constructors
- **Mutable Fields**: Use `mutable` keyword to allow field modification
- **Inheritance**: Use `extends Parent(args)` to inherit from another class
- **Abstract Classes**: Use `abstract class` and `abstract function` for unimplemented methods
- **Sealed Abstract Classes**: Use `sealed abstract class` to restrict subclasses to the same package with exhaustive pattern matching
- **Generic Classes**: Type parameters with `class Name<T>`, constraints with `where T: Trait`
- **Trait Implementation**: Inline with `implements Trait` or external with `implement Trait for Class`
- **Extension Properties**: Add computed properties with `property name(self): Type` in named extensions
- **Extension Methods**: Add methods to classes with `extension Helpers for ClassName`
- **Visibility**: Control access with `public`, `private`, `internal` on classes, fields, methods, and properties
