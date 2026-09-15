# Part 9: Classes and Object-Oriented Programming

Classes in Dovetail provide object-oriented programming with encapsulation, inheritance, and polymorphism. Unlike records (which are pure data), classes combine data with behavior and support mutable state.

---

## 9.1 Class Definitions

A class is defined with the `class` keyword, followed by the class name, constructor parameters, and methods:

```dovetail
class Point(x: Int32, y: Int32) =
    function getX(self): Int32 = self.x
    function getY(self): Int32 = self.y
    function sum(self): Int32 = self.x + self.y
```

Key points:
- Constructor parameters define the class fields
- Methods are defined in the class body
- Classes must have at least one method

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
class Point(x: Int32, y: Int32) =
    function dummy(self): Int32 = 0

function getX(p: Point): Int32 = p.x  // Direct field access
```

---

## 9.2 Methods

### Instance Methods

Instance methods take `self` as the first parameter:

```dovetail
class Counter(mutable value: Int32) =
    function getValue(self): Int32 = self.value
    function increment(self): Unit = self.value = self.value + 1
```

### Static Methods (Smart Constructors)

Methods without `self` are static and called on the class itself. Static methods are commonly used as "smart constructors" to provide alternative ways to create instances:

```dovetail
class Point(x: Int32, y: Int32) =
    function getX(self): Int32 = self.x
    function origin(): Point = Point(0, 0)           // Smart constructor
    function fromSingle(v: Int32): Point = Point(v, v)  // Smart constructor
    function zero(): Int32 = 0                        // Static utility

function main(): Int32 =
    let p1 = Point.origin()       // (0, 0)
    let p2 = Point.fromSingle(5)  // (5, 5)
    let p3 = Point(3, 7)          // Primary constructor
    0
```

Smart constructors can also perform validation:

```dovetail
class PositiveInt private (value: Int32) =
    function get(self): Int32 = self.value
    
    function create(n: Int32): Option<PositiveInt> =
        if n > 0 then Some(PositiveInt(n)) else None

function main(): Int32 =
    match PositiveInt.create(42) with
        Some(p) => p.get()
        None => 0
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

    function getColor(self): String = self.color
    function getName(self): String = self.name

function main(): Int32 =
    let r = Rectangle(3, 4)
    r.getColor()  // "blue"
    r.getName()   // "rectangle"
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

    function getArea(self): Int32 = self.area      // Calls generated getter
    function getPerimeter(self): Int32 = self.perimeter

function main(): Int32 =
    let r = Rectangle(3, 4)
    r.getArea()      // 12 - computed on access
    r.getPerimeter() // 14 - computed on access
```

Key differences between stored fields and properties:
- **Stored fields**: Evaluated once during construction, stored in memory
- **Properties**: Evaluated every time they're accessed, no storage

### Static Stored Fields

Static fields belong to the class itself, not individual instances:

```dovetail
class Config(name: String) =
    let static defaultTimeout: Int32 = 30
    let static maxRetries: Int32 = 3

    function dummy(self): Int32 = 0  // Need at least one method

function main(): Int32 =
    Config.defaultTimeout  // 30 - accessed on class, not instance
```

Static fields are initialized once when the class is first loaded.

### Static Property Fields

Static properties are like static fields but computed each time they're accessed:

```dovetail
class Math(value: Int32) =
    let static property pi: Float64 = 3.14159265359
    let static property e: Float64 = 2.71828182846

    function dummy(self): Int32 = 0

function main(): Int32 =
    Math.pi  // 3.14159265359 - computed on access
```

### Mutable Fields

Use `let mutable` for fields that can be changed after construction:

```dovetail
class Counter(initial: Int32) =
    let mutable count: Int32 = initial

    function increment(self): Unit = self.count = self.count + 1
    function getCount(self): Int32 = self.count
```

### Chained Fields

Fields can depend on each other (in declaration order):

```dovetail
class Stats(base: Int32) =
    let doubled: Int32 = base * 2
    let quadrupled: Int32 = doubled * 2  // Uses earlier field

    function getQuadrupled(self): Int32 = self.quadrupled
```

### Side Effects in Constructor Body

Any expression in the class body runs during construction:

```dovetail
class Logger(name: String) =
    let _ = println("Creating logger: " ++ name)  // Runs at construction

    function log(self, msg: String): Unit = println(name ++ ": " ++ msg)
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

    function dummy(self): Int32 = 0
```

---

## 9.4 Private Constructors

Use the `private` keyword after the class name to make the constructor private:

```dovetail
class Singleton private (value: Int32) =
    function get(self): Int32 = self.value
    
    function instance(): Singleton = Singleton(42)  // Only accessible here

function main(): Int32 =
    // let s = Singleton(10)  // Error: constructor is private
    let s = Singleton.instance()  // OK - use smart constructor
    s.get()
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
    function getValue(self): Int32 = self.value
    function increment(self): Unit = self.value = self.value + 1
    function add(self, amount: Int32): Unit = self.value = self.value + amount

function main(): Int32 =
    let c = Counter(0)
    c.increment()
    c.increment()
    c.add(10)
    c.getValue()  // 12
```

You can mix mutable and immutable fields:

```dovetail
class Entity(id: Int32, mutable health: Int32) =
    function getId(self): Int32 = self.id           // id is immutable
    function getHealth(self): Int32 = self.health
    function damage(self, amount: Int32): Unit = 
        self.health = self.health - amount          // health is mutable
```

Attempting to assign to an immutable field results in a compile error.

---

## 9.6 Inheritance

Classes can extend other classes using `extends`:

```dovetail
class Animal(name: String) =
    function getName(self): String = self.name

class Dog(name: String, breed: String) extends Animal(name) =
    function getBreed(self): String = self.breed

function main(): Int32 =
    let d = Dog("Rex", "German Shepherd")
    d.getName()   // "Rex" - inherited from Animal
    d.getBreed()  // "German Shepherd" - defined in Dog
    0
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
    function getName(self): String = self.name    // Concrete method
    abstract function area(self): Int32           // Abstract method

class Square(name: String, side: Int32) extends Shape(name) =
    function area(self): Int32 = self.side * self.side  // Must implement

function main(): Int32 =
    // let s = Shape("test")  // Error: cannot instantiate abstract class
    let sq = Square("square", 5)
    sq.area()  // 25
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
    function get(self): T = self.value

function main(): Int32 =
    let intBox = Box(42)
    let strBox = Box("hello")
    intBox.get()  // 42
```

### Multiple Type Parameters

```dovetail
class Pair<A, B>(first: A, second: B) =
    function getFirst(self): A = self.first
    function getSecond(self): B = self.second

function main(): Int32 =
    let p = Pair(1, "one")
    p.getFirst()   // 1
    p.getSecond()  // "one"
    0
```

### Mutable Generic Fields

```dovetail
class MutableBox<T>(mutable value: T) =
    function get(self): T = self.value
    function set(self, newValue: T): Unit = self.value = newValue

function main(): Int32 =
    let box = MutableBox(10)
    box.set(20)
    box.get()  // 20
```

### Constrained Generic Classes

Use `where` clauses or inline constraints to restrict type parameters:

```dovetail
trait Countable =
    function count(self): Int32

class Counter<T: Countable>(value: T) =
    function getCount(self): Int32 = self.value.count()
```

Or with `where` clause:

```dovetail
class Wrapper<T>(value: T)
    where T: Printable
=
    function fmt(self): String = self.value.format()
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
    function speak(self): String

class Dog(name: String) implements Speakable =
    function speak(self): String = "Woof"
    function getName(self): String = self.name
```

### Multiple Traits

Use `and` to implement multiple traits:

```dovetail
trait Speakable =
    function speak(self): String

trait Nameable =
    function getName(self): String

class Dog(name: String) implements Speakable and Nameable =
    function speak(self): String = "Woof"
    function getName(self): String = self.name
```

### Combined with Inheritance

Classes can extend a parent and implement traits:

```dovetail
class Dog(name: String) extends Pet(name) implements Speakable and Comparable =
    function speak(self): String = "Woof"
    function compare(self, other: Dog): Int32 = 0
```

### External Implementation

You can also implement traits for classes externally:

```dovetail
trait Speakable =
    function speak(self): String

class Dog(name: String) =
    function bark(self): String = "Woof"

implement Speakable for Dog =
    function speak(self): String = self.bark()
```

This is useful when:
- You don't own the class definition
- You want to add trait implementations later
- You're implementing traits from other packages

---

## 9.11 Extension Properties

Extensions can add not just methods, but also properties to classes, records, and enums. Extension properties generate getter functions just like class body properties.

### Basic Extension Properties

```dovetail
class Point(x: Int32, y: Int32) =
    function getX(self): Int32 = self.x
    function getY(self): Int32 = self.y

extension for Point =
    let property magnitude: Float64 =
        let x = self.x.toFloat64()
        let y = self.y.toFloat64()
        (x * x + y * y).sqrt()

function main(): Int32 =
    let p = Point(3, 4)
    p.magnitude  // 5.0 - computed via extension property
```

Key points about extension properties:
- Use `let property name: Type = expression` syntax
- Properties are evaluated every time they're accessed
- They can access public fields and methods of the type
- They respect the extension's visibility modifiers

### Extension Properties for Records

Extension properties work the same way for records:

```dovetail
record Rectangle =
    width: Int32
    height: Int32

extension for Rectangle =
    let property area: Int32 = self.width * self.height
    let property perimeter: Int32 = 2 * (self.width + self.height)

function main(): Int32 =
    let r = Rectangle { width = 3; height = 4 }
    r.area       // 12
    r.perimeter  // 14
```

### Extension Properties for Enums

Extension properties can add computed values to enums:

```dovetail
enum Color =
    Red
    Green
    Blue

extension for Color =
    let property hexCode: String =
        match self with
            case Red => "#FF0000"
            case Green => "#00FF00"
            case Blue => "#0000FF"

function main(): Int32 =
    let c = Color.Red
    println(c.hexCode)  // "#FF0000"
    0
```

### Visibility of Extension Properties

Extension properties follow the extension's visibility:

```dovetail
public extension for Point =
    public let property distance: Float64 = self.magnitude
    private let property internal: Int32 = 0  // Only visible in this package
```

### Combining with Extension Methods

Extensions can have both properties and methods:

```dovetail
extension for Point =
    let property magnitude: Float64 =
        let x = self.x.toFloat64()
        let y = self.y.toFloat64()
        (x * x + y * y).sqrt()

    function scale(self, factor: Int32): Point =
        Point(self.x * factor, self.y * factor)

    function normalize(self): Point =
        let mag = self.magnitude
        Point(
            (self.x.toFloat64() / mag).toInt32(),
            (self.y.toFloat64() / mag).toInt32()
        )
```

---

## 9.12 Extension Methods for Classes

You can add methods to classes using extensions:

```dovetail
class Point(x: Int32, y: Int32) =
    function getX(self): Int32 = self.x
    function getY(self): Int32 = self.y

extension for Point =
    function distance(self): Float64 =
        let x = self.x.toFloat64()
        let y = self.y.toFloat64()
        (x * x + y * y).sqrt()
    
    function scale(self, factor: Int32): Point =
        Point(self.x * factor, self.y * factor)
```

Extensions can also be generic:

```dovetail
class Container<T>(value: T) =
    function getValue(self): T = self.value

extension for Container<T> =
    function map<U>(self, f: (T) => U): Container<U> =
        Container(f(self.getValue()))
```

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
    internal data: Int32        // Within this package (default)
) =
    function getId(self): Int32 = self.id
    function getSecret(self): String = self.secret  // OK - inside class
```

Private fields are not accessible outside the class:

```dovetail
function main(): Int32 =
    let e = Entity(1, "secret", 42)
    e.id      // OK - public
    // e.secret  // Error - private field
    0
```

### Method Visibility

```dovetail
class Calculator(value: Int32) =
    private function helper(self): Int32 = self.value * 2
    public function compute(self): Int32 = self.helper() + 1
    function internal(self): Int32 = 0  // Default: internal
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
| Methods | Defined in body | Via extensions only |
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
- **Extension Properties**: Add properties to classes, records, and enums with `let property` in extensions
- **Extension Methods**: Add methods to classes with `extension for ClassName`
- **Visibility**: Control access with `public`, `private`, `internal` on classes, fields, methods, and properties
