# Part 6: Type System

Dovetail has a powerful type system that catches errors at compile time while keeping your code clean through type inference.

---

## 6.1 Type Inference

Dovetail infers types whenever possible, so you don't need to write them explicitly:

```dovetail
let name = "Alice"        // inferred as String
let age = 30              // inferred as Int32
let price = 19.99         // inferred as Float64
let active = true         // inferred as Bool
```

You can add explicit type annotations when needed:

```dovetail
let count: Int64 = 100    // explicitly Int64, not Int32
let ratio: Float32 = 0.5  // explicitly Float32, not Float64
```

Function return types must be explicit (for readability), except for `Unit`:

```dovetail
function double(x: Int32): Int32 = x * 2

function calculateTotal(price: Float64, quantity: Int32): Float64 =
    price * quantity.toFloat64()

// Unit return type can be omitted
function greet(name: String) =
    println("Hello, $name!")
```

---

## 6.2 Records

Records are data-only types with named fields. They're similar to structs in other languages.

### Defining Records

```dovetail
record Point =
    x: Int32
    y: Int32

record User =
    name: String
    email: String
    age: Int32
```

### Creating Records

Use the record name followed by field initializers in braces:

```dovetail
let origin = Point { x = 0; y = 0 }
let user = User { name = "Alice"; email = "alice@example.com"; age = 30 }
```

Multi-line syntax:

```dovetail
let user = User {
    name = "Alice"
    email = "alice@example.com"
    age = 30
}
```

### Accessing Fields

Use dot notation:

```dovetail
let userName = user.name
let userAge = user.age
```

### Updating Records with `with`

Records are immutable. Use `with` to create a modified copy:

```dovetail
let older = user with age = user.age + 1
let renamed = user with name = "Bob"
```

### Private Construction

Put `private` after the record name to reserve construction and `with` updates for its associated module:

```dovetail
public record Account private =
    balance: Int32

module Account =
    public function make(balance: Int32): Result<Account, String> =
        if balance < 0 then Error("balance must not be negative")
        else Ok(Account { balance = balance })

    public function deposit(self, amount: Int32): Result<Account, String> =
        if amount < 0 then Error("deposit must not be negative")
        else Ok(self with balance = self.balance + amount)
```

Any caller that can see `Account` can read `account.balance` and destructure an account in a pattern. Only `module Account` in the defining package can write `Account { ... }` or `account with ...`. Sharing the package or file is not enough; trait implementations and extensions must call the module's functions too.

Leading `public` controls visibility of the type itself. For generic records, write `record Box<T> private where T: Equatable = ...`: the construction modifier follows type parameters and precedes constraints. Empty private records use `record Token private`, with the modifier on the same line as the name.

### Records are Equatable

Records with equatable fields are automatically equatable:

```dovetail
let p1 = Point { x = 1; y = 2 }
let p2 = Point { x = 1; y = 2 }
p1 == p2  // true
```

---

## 6.3 Enums (Discriminated Unions)

Enums represent values that can be one of several variants. Each variant can optionally carry data.

### Simple Enums

```dovetail
enum Color =
    Red
    Green
    Blue

let color = Red
```

### Enums with Data

Variants can carry payload data:

```dovetail
enum Shape =
    Circle(Float64)           // radius
    Rectangle(Float64, Float64)  // width, height
    Point

let circle = Circle(5.0)
let rect = Rectangle(10.0, 20.0)
let point = Point
```

### Pattern Matching on Enums

Use `match` to handle different variants:

```dovetail
function area(shape: Shape): Float64 =
    match shape with
        case Circle(radius) => 3.14159 * radius * radius
        case Rectangle(w, h) => w * h
        case Point => 0.0
```

### Named Record Payloads

A variant with exactly one record payload supports both positional and record syntax:

```dovetail
record Placement =
    placedAt: Int32
    total: Int32

enum OrderStatus =
    Draft
    Placed(Placement)

let placement = Placement { placedAt = 1; total = 100 }
let first = OrderStatus.Placed(placement)
let second = OrderStatus.Placed { placedAt = 1; total = 100 }
```

The brace construction is shorthand for `OrderStatus.Placed(Placement { placedAt = 1; total = 100 })`. The payload remains a `Placement` value that can be passed around independently.

Patterns can bind that whole value or inspect its fields:

```dovetail
function total(status: OrderStatus): Int32 =
    match status with
        case Placed { total } => total
        case Draft => 0
```

`case Placed { total }` means `case Placed(Placement { total })`; `case Placed(placement)` binds the whole record. Ordinary record pattern rules apply, including omitted fields and nested patterns. Extracting an inline record variant into a named payload can therefore preserve its brace construction and patterns.

Braces unwrap exactly one record payload. They do not unwrap newtypes, classes, or multiple positional payloads. Generic payloads work when their record type is known: `Some { total = 100; placedAt = 1 }` needs an expected type such as `Option<Placement>`. Fields alone do not identify a nominal record type. Custom construction uses the enum qualifier; patterns can infer it from the matched value.

Construction checks both the enum's and the payload record's construction permissions. A private record constructor does not prevent inspecting its public fields through a variant pattern or wrapping an existing record value in a publicly constructible enum.

### Private Construction of Enums

The same modifier reserves every variant's construction for the enum's associated module:

```dovetail
public enum Status private =
    Open
    Closed

module Status =
    public function open(): Status = Status.Open
    public function close(self): Status = Status.Closed

function isClosed(status: Status): Bool =
    match status with
        case Status.Open => false
        case Status.Closed => true
```

Outside `module Status`, `Status.Open` is allowed in a pattern but rejected as a value expression, because that would construct a value. This applies to variants with no payload, tuple payloads, and named record payloads. Patterns still expose their payloads, and exhaustiveness checking is unchanged. The `with` expression remains a record operation; this does not add enum updates.

### Option Type

The prelude's `Option<T>` type represents optional values. It is available without an import:

```dovetail
enum Option<T> =
    Some(T)
    None

function findUser(id: Int32): Option<User> =
    if id == 1 then
        Some(User { name = "Alice"; email = "a@b.com"; age = 30 })
    else
        None
```

See [Option Type in chapter 10](10-error-handling.md#102-option-type) for its methods and guidance on handling missing values.

### Result Type

The prelude's `Result<T, E>` type represents success or failure. It is also available without an import:

```dovetail
enum Result<+T, +E> =
    Ok(T)
    Error(E)

function divide(a: Int32, b: Int32): Result<Int32, String> =
    if b == 0 then
        Error("Division by zero")
    else
        Ok(a / b)
```

See [Result Type in chapter 10](10-error-handling.md#103-result-type) for its methods and [the `try` expression](10-error-handling.md#try-and-orreturn) for propagating failures.

---

## 6.4 Tuples

Tuples group multiple values of different types.

### Creating Tuples

```dovetail
let pair = (1, "hello")
let triple = (10, 20.5, true)
```

### Accessing Tuple Elements

Use pattern matching or numbered accessors:

```dovetail
let (x, y) = pair
// x is 1, y is "hello"

let first = triple._0   // 10
let second = triple._1  // 20.5
```

### Tuples vs Records

- Use tuples for quick grouping of a few values
- Use records when you need meaningful field names
- Records are better for public APIs

For generic tuple construction, shape constraints, and implementations across
tuple arities, see [Part 25: Tuple Extension (Advanced)](25-tuple-extension.md).

---

## 6.5 Newtypes

Newtypes create distinct types from existing types, preventing accidental mixing.

### Defining Newtypes

```dovetail
newtype UserId = Int32
newtype OrderId = Int32
newtype Email = String
```

### Using Newtypes

```dovetail
let userId = UserId(123)
let orderId = OrderId(456)

// This won't compile - different types!
// let wrong: UserId = orderId  // Error!
```

### Why Newtypes?

Newtypes prevent bugs by making the type system work for you:

```dovetail
function getUser(id: UserId): Option<User> = ...
function getOrder(id: OrderId): Option<Order> = ...

let userId = UserId(123)
let orderId = OrderId(456)

getUser(userId)   // OK
getUser(orderId)  // Compile error! Can't pass OrderId where UserId expected
```

### Private Newtypes

A private newtype reserves both construction and inspection for its associated module:

```dovetail
public newtype Password private = String

module Password =
    public function parse(value: String): Result<Password, String> =
        if value.length < 8 then
            Error("Password must be at least 8 characters")
        else
            Ok(Password(value))

    public function matches(self, other: String): Bool =
        self.value == other
```

Only `module Password` in the defining package may call `Password(...)`, access `.value`, or unwrap a `Password(value)` pattern. Other code can store, pass, and compare passwords and call the module's public functions. Trait implementations and extensions have no special access, even in the same package.

Private records and enums expose their fields and patterns; private newtypes keep the wrapped value opaque. All three use `Name<T> private where ... = ...`, but their inspection rules differ. The old `newtype Name = private Type` syntax is rejected.

---

## 6.6 Type Aliases

Type aliases create alternative names for existing types. Unlike newtypes, they're interchangeable with the original type.

### Defining Type Aliases

```dovetail
type Name = String
type Age = Int32
type Coordinate = (Int32, Int32)
type UserMap = Map<UserId, User>
```

### Using Type Aliases

```dovetail
function greet(name: Name): String =
    "Hello, $name!"

let name: Name = "Alice"
greet(name)    // OK
greet("Bob")   // Also OK - Name is just String
```

### When to Use Type Aliases vs Newtypes

- **Type aliases**: For convenience and readability (great for tuples and function types)
- **Newtypes**: When you need type safety and distinction

```dovetail
// Type aliases make complex types readable
type Coordinate = (Int32, Int32)
type Handler = (Request) => Response
type Callback<T> = (T) => Unit

// Newtypes create distinct types
newtype UserId = Int32
newtype OrderId = Int32
```

---

## 6.7 Lists, Arrays, and Slices

`List<T>`, `Array<T>`, and `Slice<T>` are available from the prelude without imports. Lists are immutable linked sequences; arrays are fixed-size mutable collections; slices are writable views into arrays.

### Lists and List Literals

Square brackets create a `List<T>`. The element type is inferred from the contents or the surrounding context:

```dovetail
let numbers: List<Int32> = [1, 2, 3]
let names = ["Alice", "Bob"]
let empty: List<Int32> = []
let nested: List<List<Int32>> = [[1, 2], [3]]
assert numbers.length == 3
assert empty.isEmpty
```

A list is an immutable singly linked sequence, defined as `List<out T> = Nil | Cons(T, List<T>)` in the prelude and used as the standard library's default sequence type. It is represented by the enum variants `Nil` (empty) and `Cons(head, tail)`. `[]` is the literal spelling of `Nil`; `[1, 2]` is equivalent to `Cons(1, Cons(2, Nil))`. `List.empty` also creates an empty list.

Use `::` to prepend an element or `++` to concatenate lists. These operations leave the original lists unchanged. Prepending shares the existing tail and takes constant time; concatenation copies the left list's nodes and takes time proportional to its length. `::` associates to the right:

```dovetail
let original = [2, 3]
let extended = 1 :: original
assert extended == [1, 2, 3]
assert original == [2, 3]
assert 1 :: 2 :: [] == [1, 2]
assert [1, 2] ++ [3, 4] == [1, 2, 3, 4]
```

`List` is covariant (`out T`), so an empty list fits wherever a `List<T>` is expected without an annotation at the call site:

```dovetail
function total(xs: List<Int32>): Int32 = xs.foldLeft(0, (a, b) => a + b)
total([])
```

`::` binds looser than `+` and `++`, and tighter than comparison — so `x :: xs ++ ys` is `x :: (xs ++ ys)`, and `x :: xs == ys` is `(x :: xs) == ys`.

The list structure is immutable; objects stored as elements may still have mutable state.

### Accessing and Matching Lists

`head` and `tail` return the first element and the remaining list, and panic on an empty list. `headOption` and `get(index)` return `Option<T>` instead; `get` returns `None` for a negative or out-of-bounds index. Lists use `.get(i)` for indexed lookup. Both indexed lookup and `length` traverse the list, so use an array when frequent random access matters.

```dovetail
let numbers = [10, 20, 30]
assert numbers.head == 10
assert numbers.tail == [20, 30]
assert numbers.headOption.or(0) == 10
assert numbers.get(1).or(0) == 20
assert numbers.get(3).isNone
```

Patterns can match an empty list, an exact number of elements, or a head and tail:

```dovetail
function describeList(values: List<Int32>): String =
    match values with
        case [] => "empty"
        case [only] => "one element"
        case first :: rest => "multiple elements"
```

### Transforming and Iterating Lists

`map`, `filter`, `take`, `drop`, and `reverse` return lists without changing the original. `foldLeft` combines the elements into a single value. `toArray()` copies the elements into a new mutable array:

```dovetail
let numbers = [1, 2, 3]
assert numbers.map(x => x * 2) == [2, 4, 6]
assert numbers.filter(x => x > 1) == [2, 3]
assert numbers.take(2) == [1, 2]
assert numbers.drop(1) == [2, 3]
assert numbers.reverse() == [3, 2, 1]
assert numbers.find(n => n > 1) == Some(2)
assert numbers.contains(2)
assert numbers.foldLeft(0, (total: Int32, value: Int32) => total + value) == 6
let array = numbers.toArray()
array[0] = 99
assert numbers.head == 1

let mutable sum = 0
for number in numbers do
    sum = sum + number
assert sum == 6
```

Lists compare by contents with `==` and `!=` when their elements implement `Equatable`. They compare lexicographically when their elements implement `Comparable`.

### Choosing Between Array and List

|  | `List<T>` | `Array<T>` |
|---|---|---|
| Literal | `[1, 2]` | `[\| 1, 2 \|]` |
| Prepend | O(1) | O(n) |
| Index / length | O(n) | O(1) |
| Mutable | no | yes |
| Variance | covariant (`out T`) | invariant |
| Pattern matching | yes | no |

Reach for a **list** when you build a sequence once and read it front to back,
which is most business logic. Reach for an **array** when you index it, mutate
it, know its size up front, or are holding bytes.

### Creating Arrays

Arrays are fixed-size, mutable, indexed collections. Array literals use `[| ... |]`; list literals use `[ ... ]`:

```dovetail
let numbers = [| 1, 2, 3, 4, 5 |]
let names = [| "Alice", "Bob", "Charlie" |]
let empty: Array<Int32> = [||]
```

The plain `[ ... ]` form is a *list* literal (§6.7). Arrays get the marked
syntax because they are the specialised choice — contiguous, mutable, indexed —
while a list is the everyday sequence.

An empty array literal needs a type annotation. `Array` is invariant, so there
is nothing for the compiler to infer the element type from.

Use `Array.fill(length, value)` to create an array of a given size filled with a value:

```dovetail
let zeros = Array.fill(10, 0)        // [|0, 0, 0, ..., 0|]
let greetings = Array.fill(3, "hi")  // [|"hi", "hi", "hi"|]
```

### Accessing Elements

Use the index operator `arr[i]` or the `.get(i)` method:

```dovetail
let first = numbers[0]       // 1
let second = numbers.get(1)  // 2
```

### Setting Elements

Use the index assignment `arr[i] = value` or the `.set(i, value)` method:

```dovetail
numbers[0] = 99
numbers.set(1, 42)
```

### Array Length

```dovetail
let count = numbers.length  // 5
```

### Cloning

`arr.clone()` creates an independent copy of the array:

```dovetail
let a = [|1, 2, 3|]
let b = a.clone()
b[0] = 99
assert a[0] == 1   // original unchanged
```

### Array Type Syntax

The type of an array is written as `Array<ElementType>`:

```dovetail
function sum(numbers: Array<Int32>): Int32 =
    numbers[0] + numbers[1] + numbers[2]
```

### Nested Arrays

Arrays can contain other arrays:

```dovetail
let matrix: Array<Array<Int32>> = [| [| 1, 2 |], [| 3, 4 |] |]
let value = matrix[0][1]  // 2
```

### Equality and Comparison

Primitive-element arrays support `==`, `!=`, `<`, `>`, `<=`, `>=` (lexicographic comparison):

```dovetail
assert [| 1, 2, 3 |] == [| 1, 2, 3 |]
assert [| 1, 2 |] < [| 1, 3 |]
```

### Iterating Over Arrays

```dovetail
for item in numbers do
    println(item)
```

### Slices: Shared Array Views

`Slice<T>` is available from the prelude without an import. It is a writable view of part of an `Array<T>`. Creating or re-slicing a view does not copy its elements.

Slicing uses `[| ... |]`, matching the array literal delimiters. Element access and assignment still use `[i]`:

```dovetail
let values = [|10, 20, 30, 40, 50|]
let middle: Slice<Int32> = values[|1..4|]
assert middle.length == 3
assert middle[0] == 20

middle[0] = 99
assert values[1] == 99   // writes through to the original array
values[2] = 77
assert middle[1] == 77   // observes writes through the array too

let tail = middle[|1..|]
assert tail[0] == 77     // bounds are relative to middle
```

All slicing forms work on both arrays and slices:

| Form | Elements included |
|---|---|
| `values[\|i..j\|]` | From `i` up to, but excluding, `j` |
| `values[\|i..\|]` | From `i` through the end |
| `values[\|..j\|]` | From zero up to, but excluding, `j` |
| `values[\|..\|]` | The whole array or slice |
| `values[\|i..=j\|]` | From `i` through `j`, including `j` |
| `values[\|..=j\|]` | From zero through `j`, including `j` |

`values[|i|]` and `values[i..j]` are invalid. Use `values[i]` for one element. Range assignment is unsupported; use `copyTo` to copy several elements. The range notation belongs to slicing syntax: `let range = 1..3` is not a valid expression.

### Slice Bounds and Constructors

Bounds are `Int32` values. A half-open interval requires `0 <= start <= end <= length`. An inclusive end must refer to an existing element. Invalid construction, re-slicing, or element access panics; negative indices do not count back from the end. Empty half-open slices are valid, including one at the array's end:

```dovetail
let empty = values[|values.length..|]
assert empty.isEmpty()

let whole = Slice.full(values)
let window = Slice.make(values, 1, 3)  // start and length
assert window == whole.slice(1, 4)    // start and exclusive end
assert window == whole.sliceInclusive(1, 3)
assert window == whole.drop(1).take(3)
```

`make` takes a **length**; `slice` takes an **exclusive end**. The backing array, start, and length are private. Copying a slice value shares the same view of the data. Ordinary locals, parameters, and returns carry the view directly; storing it at an erased or boxed boundary, such as an array element or `Option<Slice<T>>` payload, can allocate a box for the view.

### Copying, Comparing, and Iterating

`toArray()` produces an independent copy. `copyTo(destination)` copies all source elements into a destination slice whose length must be at least the source length. It also works when the views overlap:

```dovetail
let copied = middle.toArray()
copied[0] = 0
assert middle[0] == 99

let shifted = [|1, 2, 3, 4|]
shifted[|..3|].copyTo(shifted[|1..|])
assert shifted == [|1, 1, 2, 3|]
```

Read-only views use the private tuple newtype `ReadonlySlice<T>`:

```dovetail
let array = [|10, 20, 30|]
let view = array[|1..|].readonly
assert view[0] == 20
let subview = view[|..1|]          // also ReadonlySlice<Int32>
let copy = view.toArray()         // independent mutable array
array[1] = 99
assert view[0] == 99              // read-only does not freeze other aliases
assert copy[0] == 20
```

Arrays and slices expose `.readonly` without copying elements. Read-only views support the same checked ranges, iteration, equality, and display as mutable slices. `map` returns fresh backing storage as a read-only view; `copyTo` accepts a mutable destination slice. Indexed writes and `.set` are unavailable. Both slice types use trait-based element access: `Index<Int32>` for reads, and `IndexSet<Int32>` for mutable writes.

Slices compare by contents with `==` and `!=` when their elements implement `Equatable`, even if they view different arrays. They support `Display` when their elements do. Use `for` to iterate over a slice:

```dovetail
let mutable total = 0
for value in middle do
    total = total + value
assert total == 216
```

### Slices and Strings

Strings do not support slicing syntax. Convert explicitly to a character array and then create a view. `chars()` copies the characters once; subsequent slicing shares that array:

```dovetail
let characters = Slice.full("hello".chars())
let greetingStart = characters[|..2|]
assert greetingStart[0] == 'h'
assert greetingStart[1] == 'e'
```

---

## 6.8 Modules

A **module** is a named container that groups **functions**, **properties**, and **top-level `let`** (globals) under one name (similar to F# modules). Modules do not contain type declarations. You use its contents via the qualifier: `ModuleName.member`. There are **two kinds** of modules, depending on whether a type with the same name exists in the package. In a **generic** module (e.g. `module Array<T> =`), **all** members and globals have the type parameter in scope, and globals are **instantiated per type argument** (like C#): each `Array<Int32>`, `Array<String>`, etc. has its own copy of each static and each top-level `let`.

### Standalone modules

When **no type** in the package has the same name as the module, it's a **standalone module**. It can contain only static functions, static properties (`property name(): Type = ...`), and top-level `let` (static). No instance members.

```dovetail
package standard

module Math =
    let Pi: Float64 = 3.141592653589793
    property pi(): Float64 = 3.141592653589793
    function floor(x: Float64): Int32 = ...
    function ceil(x: Float64): Int32 = ...
```

You import the module as a whole and use qualified names:

```dovetail
import standard.Math

let x = Math.floor(3.14)
let p = Math.Pi
```

You cannot import a single member (e.g. `import standard.Math.floor` is not allowed).

### Modules for a type

When a **type with the same name** as the module exists in the package (e.g. type `Array<T>` and module `Array`), the module is a **module for that type**. It may contain instance methods (functions with `self`), instance properties (`property name(self): Type = ...`), static methods and static properties, and top-level `let` (static/global). For a generic type, the module must declare the type parameters: `module Array<T> =`.

In a **generic** module, **all** members and top-level `let` have the type parameter `T` in scope. Static members and globals are **instantiated per type argument** (like C#): `Array<Int32>.defaultCapacity` and `Array<String>.defaultCapacity` are different storage locations.

```dovetail
package standard.prelude

// Type Array<T> is declared elsewhere (e.g. intrinsic).
module Array<T> =
    let defaultCapacity: Int32 = 16
    property empty(): Array<T> = ...
    function fill(size: Int32, value: T): Array<T> = ...
    function get(self, index: Int32): T = ...
    function set(self, index: Int32, value: T): Unit = ...
    property length(self): Int32 = ...
```

**Instance members are always available on the type.** You do not need to import the module to call `arr.length` or `arr.get(0)` — whenever the type is in scope, the instance methods and properties from that type's module are available.

**Static methods** can infer the module's type arguments from call arguments or an expected return type: `Array.fill(5, 42)` infers `Int32`, and `let empty: Array<Int32> = Array.empty()` uses the annotation. You can also supply them explicitly, as in `Array<Int32>.fill(5, 42)`. For a generic global, qualify its instantiation explicitly, such as `Array<Int32>.defaultCapacity`. Static use requires the module to be in scope (same package, prelude, or imported).

A package may have **at most one** module for a given module name (standalone or for a type).

---

## 6.9 Extension Methods

**Extension methods** let you add methods to types you don't own — especially types from another package. Every extension has a name (e.g. `UserHelpers`) and defines methods (and static methods) on a type. Extensions must be **imported** to be used; they are not attached automatically.

```dovetail
package myapp

import users.User  // User is defined in another package

extension UserHelpers for User =
    function displayName(self): String =
        "${self.firstName} ${self.lastName}"
```

Why give extensions a name and require import?

1. **Explicit imports** — You must import the extension to use its methods, so it's clear where they come from.
2. **Avoid conflicts** — Different packages can define extensions with different names for the same type.
3. **Clear ownership** — The extension name shows which package or layer provides the behavior.

Using extensions:

```dovetail
package app

import users.User
import myapp.UserHelpers  // Must import the extension

function main() =
    let user = User { firstName = "Alice"; lastName = "Smith" }
    println(user.displayName())  // Works because UserHelpers is imported
```

For **types in the same package**, instance methods are usually provided by a **module for that type** (a module with the same name as the type), not by an extension. See [Modules](#68-modules) above. Use an **extension** when you want to add methods to a type from another package or when you want a separate, explicitly imported set of methods.

---

## 6.10 The Any Type and Type Casting

### The Any Type

`Any` is the **top type** in Dovetail — every type is assignable to `Any`:

```dovetail
let a: Any = "Hello"
let b: Any = 42
let c: Any = true
let d: Any = Point { x = 1; y = 2 }
```

This makes `Any` useful for heterogeneous collections, generic APIs that accept values of unknown type, and interop scenarios. Under the hood, primitive values (integers, floats, booleans, etc.) are automatically **boxed** when stored as `Any`, and **unboxed** when cast back to their concrete type.

### Type Testing with `is`

The `is` expression checks at runtime whether a value has a specific type. It returns `Bool`:

```dovetail
function describe(x: Any): String =
    if x is String then "a string"
    else if x is Int32 then "an integer"
    else if x is Bool then "a boolean"
    else "something else"
```

`is` supports concrete target types, including primitives, records, enums, generic types, and arrays:

```dovetail
let x: Any = Some(42)
x is Option<Int32>    // true
x is Option<String>   // false
x is String           // false
```

### Type Casting with `as`

The `as` expression casts a value to a specific type. If the cast fails at runtime, it **panics** (there is no safe `as?` variant):

```dovetail
let x: Any = 42
let n: Int32 = x as Int32   // succeeds, n is 42

let y: Any = "hello"
let s: String = y as String  // succeeds, s is "hello"
```

For primitive types, `as` automatically unboxes the value:

```dovetail
let x: Any = 3.14
let f: Float64 = x as Float64   // unboxes to 3.14
```

For records and enums, `as` returns the reference directly:

```dovetail
record Point =
    x: Int32
    y: Int32

let x: Any = Point { x = 1; y = 2 }
let p: Point = x as Point
assert p.x == 1
```

### The `is` + `as` Pattern

Since `as` panics on failure, the idiomatic pattern is to test first with `is`, then cast:

```dovetail
function tryGetInt(x: Any): Option<Int32> =
    if x is Int32 then Some(x as Int32)
    else None
```

Or in a more complex example:

```dovetail
function processValue(x: Any): String =
    if x is Int32 then
        let n = x as Int32
        "integer: ${n * 2}"
    else if x is String then
        let s = x as String
        "string of length ${s.length}"
    else
        "unknown"
```

### Restrictions

For the concrete type recovery shown here, the subject is `Any`. These operators
also support [class type tests and casts](09-classes.md) and
[interface upcasts](#611-interfaces-and-interface-types), subject to their own rules.
They do not convert unrelated concrete types:

```dovetail
let x: Int32 = 42
// x is String    // compile error: 'is' requires subject of type Any
// x as String    // compile error: 'as' requires subject of type Any
```

For recovery from `Any`, the target must be concrete; `Any`, `Never`, and
interface targets are unsupported:

```dovetail
let x: Any = 42
// x is Any       // compile error: target must be a concrete type
// x is Never     // compile error: target must be a concrete type
```

### Working with Generic Types

`is` and `as` work with generic type instantiations:

```dovetail
let x: Any = Some(42)
if x is Option<Int32> then
    let opt = x as Option<Int32>
    match opt with
        case Some(n) => assert n == 42
        case None => panic "unexpected"
```

Different instantiations are distinguished at runtime:

```dovetail
let a: Any = Some(42)
let b: Any = Some("hello")

a is Option<Int32>     // true
a is Option<String>    // false
b is Option<String>    // true
```

### Working with Arrays

Array element types are also distinguished:

```dovetail
let x: Any = [|1, 2, 3|]
x is Array<Int32>      // true
x is Array<String>     // false
```

---

## 6.11 Interfaces and Interface Types

An `interface` describes behavior and can also be the type of a value. It lets a
parameter, return value, field, or collection hold different implementations of
the same contract. A plain `trait` describes behavior for generic bounds; it
cannot appear as a value type. Both use `implement` blocks, and an interface can
also be used as a generic bound.

### Declaring and Implementing Interfaces

```dovetail
interface Label =
    function text(self): String

record Person =
    name: String

record NumberLabel =
    value: Int32

implement Label for Person =
    function text(self): String = self.name

implement Label for NumberLabel =
    function text(self): String = "${self.value}"

record Badge =
    label: Label

function readLabel(label: Label): String = label.text()

function chooseLabel(person: Bool): Label =
    if person then Person { name = "Ada" }
    else NumberLabel { value = 42 }

function main(): Unit =
    let label: Label = Person { name = "Ada" }
    assert label.text() == "Ada"
    assert readLabel(NumberLabel { value = 42 }) == "42"
    assert chooseLabel(false).text() == "42"
    let badge = Badge { label = Person { name = "Grace" } }
    assert badge.label.text() == "Grace"
    let labels: List<Label> = [Person { name = "Ada" }, NumberLabel { value = 42 }]
    for item in labels do
        assert item.text().length > 0
```

The compiler implicitly converts each concrete value to `Label` where that type
is expected. Calling `text` through `Label` dispatches to the implementation of
the value held at runtime. Records, enums, primitives, and classes can implement
interfaces. Only the interface's members are exposed through an interface value.

Type annotations make the intended common interface explicit. They also allow a
list literal to contain different concrete implementations. An existing
`List<Person>` does not automatically become `List<Label>`: convert the elements
when building the new collection. The same boundary applies to containers such
as `Option<T>` and to function types; coercion does not recursively adapt them.

### Generic Interfaces and Bounds

An interface can have type parameters:

```dovetail
interface Source<T> =
    function get(self): T

record Fixed =
    value: Int32

implement Source<Int32> for Fixed =
    function get(self): Int32 = self.value

function read<T>(source: T): Int32 where T: Source<Int32> = source.get()

function main(): Unit =
    let source: Source<Int32> = Fixed { value = 7 }
    assert source.get() == 7
    assert read(Fixed { value = 9 }) == 9
    assert read(source) == 7
```

`Source<Int32>` fixes the element type while hiding the implementation. A generic
bound retains the caller's type: passing `Fixed` allows static dispatch, while
passing an interface value still calls through that value's interface. Individual
interface methods cannot introduce their own type parameters.

### Intersections and Upcasts

`A and B` is a value type requiring both interfaces. Its components must all be
interfaces, and the concrete value must implement every component:

```dovetail
interface Named =
    function name(self): String

interface Counted =
    function count(self): Int32

record Team =
    title: String
    members: Int32

implement Named for Team =
    function name(self): String = self.title

implement Counted for Team =
    function count(self): Int32 = self.members

function main(): Unit =
    let team: Named and Counted = Team { title = "Core"; members = 3 }
    assert team.name() == "Core"
    assert team.count() == 3
    let named: Named = team
    let counted = team as Counted
    assert named.name() == "Core"
    assert counted.count() == 3
```

An intersection implicitly upcasts to a component or a smaller intersection;
`as` can also express an upcast. An interface extending another interface can
likewise upcast to its parent. These conversions expose fewer capabilities and
do not discover new ones at runtime. If components declare the same member,
qualify the call, such as `Named.name(team)`, to select the contract explicitly.

### Inheritance, Defaults, and `Self`

Interfaces share inheritance, default methods and properties, and explicit
member qualification with traits. See [Part 8, sections 8.5–8.7](08-traits.md#85-trait-and-interface-inheritance)
for the implementation rules. An interface can extend only other interfaces;
a trait can extend traits or interfaces.

An interface method may return bare `Self`:

```dovetail
interface Stepper =
    function step(self): Self
    function value(self): Int32

record Counter =
    n: Int32

implement Stepper for Counter =
    function step(self): Counter = Counter { n = self.n + 1 }
    function value(self): Int32 = self.n

function main(): Unit =
    let counter: Stepper = Counter { n = 4 }
    let next: Stepper = counter.step()
    assert next.value() == 5
```

Through an interface value, the result has the interface type that declares the
method. For an intersection receiver, it has that declaring component's type,
so calling a `Self`-returning method does not preserve the whole intersection.

### Interface Restrictions and `Any`

The compiler checks these rules when the interface is declared:

- Every method takes `self`; static methods are not allowed.
- Methods cannot declare their own generic parameters.
- Associated types, including generic associated types, are not allowed.
- `Self` is allowed as the receiver and as a bare return type. Parameters such as
  `other: Self` and nested returns such as `Option<Self>` are not allowed.
- Parent contracts in `extends` must themselves be interfaces.

Use a trait when a contract needs these excluded features. For example,
`equals(self, other: Self)` belongs in a trait because both arguments must have
the same concrete type.

Interface values and [`Any`](#610-the-any-type-and-type-casting) serve different
purposes. An interface exposes a known contract. `Any` requires a concrete type
test or cast before using the value. Testing `value is Label` or casting an
`Any` value to `Label` is unsupported. Storing an interface value in `Any` retains
the interface wrapper: testing that `Any` for the underlying record type returns
false. Store the concrete record directly in `Any` when concrete type recovery
is needed.

### Choosing a Contract

| Need | Use |
|------|-----|
| Heterogeneous collection, runtime-selected return, or injected dependency field | Interface |
| One contract usable both as a generic bound and a value type | Interface, within the declaration restrictions |
| Same-type arguments, static methods, associated types, or generic methods | Trait |
| Generic behavior where static dispatch is sufficient | Trait |
| A closed, known set of alternatives with pattern matching | Enum |
| Shared state and a single class inheritance hierarchy | Abstract class |

---

## Private Construction for Records and Enums

A public type can expose its shape while reserving construction for its associated
module:

```dovetail
public record Account private =
    balance: Int32

module Account =
    public function make(balance: Int32): Account =
        assert balance >= 0
        Account { balance = balance }

    public function deposit(self, amount: Int32): Account =
        assert amount >= 0
        self with balance = self.balance + amount

public enum Status private =
    Open
    Closed

module Status =
    public function open(): Status = Status.Open
    public function close(self): Status = Status.Closed
```

Outside `module Account`, reading `account.balance` and matching record fields
are allowed; record construction and `with` expressions are rejected. Outside
`module Status`, variants can appear in patterns but cannot construct values.
The module must belong to the type's defining package. Trait implementations and
extensions must call module functions for construction and updates.

Place the modifier after type parameters and before constraints, for example
`record Box<T> private where T: Equatable = ...`. Empty private records use
`record Token private`. Leading `public` controls visibility of the type itself.

Private newtypes also use this placement, but retain stronger inspection rules:
only their module can access `.value` or unwrap a constructor pattern. Trait
implementations must use module functions for those operations too. Derive macros
receive no exemption from these restrictions. The old `newtype Name = private Type`
syntax is rejected.

Choose privacy per type. If `OrderStatus` is private, `module Order` cannot directly
construct `OrderStatus.Placed(...)`; it must use `module OrderStatus` functions.
An aggregate can instead keep `Order` construction private while leaving its
status enum publicly constructible.

---

## Summary

- **Type Inference**: Dovetail infers types when possible, but you can be explicit
- **Records**: Data-only types with named fields, immutable by default
- **Enums**: Discriminated unions that can carry data per variant
- **Tuples**: Positional grouping of values (anonymous types; access via _0, _1, or destructuring)
- **Newtypes**: Distinct types for type safety
- **Type Aliases**: Convenient names for existing types
- **Lists**: Immutable linked sequences, with `[ ... ]` literals, `::` prepending, and `++` concatenation
- **Arrays**: Fixed-size indexed collections, with `[| ... |]` literals
- **Slices**: Writable shared array views, with `[|start..end|]` slicing and `[i]` element access
- **Modules**: Named containers (standalone or for a type); qualified use; instance members of a module-for-type are available on the type. In generic modules, all members and globals have T in scope and are instantiated per type argument (C#-style); static methods can infer type arguments; generic globals use an explicit instantiation such as `Array<Int32>.defaultCapacity`
- **Extension methods**: Add methods to types (e.g. from other packages); always named and must be imported to use
- **Any type**: Top type; every value is assignable to `Any`; use `is` to test and `as` to cast back to concrete types

- **Interfaces**: Contracts usable as value types and generic bounds; implicit coercion enables runtime dispatch, heterogeneous collections, and interface intersections
