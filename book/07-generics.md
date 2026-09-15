# Part 7: Generics

Generics allow you to write code that works with multiple types while maintaining type safety. Instead of writing separate functions for each type, you write one generic version.

---

## 7.1 Generic Functions

### Basic Generic Functions

Use angle brackets to declare type parameters:

```dovetail
function identity<T>(x: T): T = x

// Use with different types
let a = identity(42)        // T is Int32
let b = identity("hello")   // T is String
let c = identity(true)      // T is Bool
```

The compiler infers the type argument from the value you pass.

### Multiple Type Parameters

Functions can have multiple type parameters:

```dovetail
function first<A, B>(a: A, b: B): A = a
function second<A, B>(a: A, b: B): B = b

let x = first(10, "hello")   // A is Int32, B is String, returns 10
let y = second(10, "hello")  // returns "hello"
```

### Explicit Type Arguments

Sometimes you need to specify type arguments explicitly:

```dovetail
function create<T>(): Array<T> = [||]

// Must specify T since it can't be inferred
let numbers = create<Int32>()
let names = create<String>()
```

### Type Parameter Scope

Type parameters are in scope for the entire function:

```dovetail
function swap<T>(pair: (T, T)): (T, T) =
    let (a, b) = pair
    (b, a)

let swapped = swap((1, 2))  // (2, 1)
```

---

## 7.2 Generic Types

### Generic Records

Records can have type parameters:

```dovetail
record Box<T> =
    value: T

let intBox = Box { value = 42 }
let stringBox = Box { value = "hello" }
```

### Generic Records with Multiple Parameters

```dovetail
record Pair<A, B> =
    first: A
    second: B

let pair = Pair { first = 1; second = "one" }
let x = pair.first   // Int32
let y = pair.second  // String
```

### Generic Enums

Enums can also be generic:

```dovetail
enum Option<T> =
    Some(T)
    None

function divide(a: Int32, b: Int32): Option<Int32> =
    if b == 0 then
        None
    else
        Some(a / b)

let result = divide(10, 2)
match result with
    case Some(value) => println("Result: $value")
    case None => println("Cannot divide by zero")
```

### Result Type

The `Result` type uses two type parameters:

```dovetail
enum Result<T, E> =
    Ok(T)
    Error(E)

function divideOrError(a: Int32, b: Int32): Result<Int32, String> =
    if b == 0 then
        Error("Cannot divide by zero")
    else
        Ok(a / b)

let result = divideOrError(10, 2)  // Ok(5)
```

### Generic Classes

Classes can have type parameters:

```dovetail
class Container<T>(value: T) =
    function get(self): T = self.value
    
    function set(self, newValue: T): Container<T> =
        Container(newValue)

let container = Container(42)
let value = container.get()  // 42
```

---

## 7.3 Constraints

A constraint says what a type must support before it can be used with your generic code. For example, a function that compares two values needs a type that supports comparison.

### Basic Constraints

Use the `where` clause to add constraints:

```dovetail
trait Printable =
    function format(self): String

function printIt<T>(x: T): String
    where T: Printable
= x.format()
```

Now `printIt` can only be called with types that implement `Printable`:

```dovetail
implement Printable for Int32 =
    function format(self): String = "$self"

printIt(42)      // OK - Int32 implements Printable
// printIt([1, 2])  // Error - List doesn't implement Printable
```

### Multiple Constraints

You can require multiple traits using `+`:

```dovetail
trait Showable =
    function show(self): String

trait Countable =
    function count(self): Int32

function describe<T>(x: T): String
    where T: Showable + Countable
= "${x.show()} (count: ${x.count()})"
```

Or list them separately:

```dovetail
function describe<T>(x: T): String
    where T: Showable, T: Countable
= "${x.show()} (count: ${x.count()})"
```

### Inline Constraints

For simple cases, you can put the constraint inline:

```dovetail
function printIt<T: Printable>(x: T): String = x.format()
```

This is equivalent to using a `where` clause.

### Constraints on Multiple Type Parameters

```dovetail
function combine<A, B>(a: A, b: B): String
    where A: Showable, B: Showable
= "${a.show()} and ${b.show()}"
```

### Constraints on Generic Types

Records, enums, and classes can also have constraints:

```dovetail
record SortedPair<T>
    where T: Comparable
=
    first: T
    second: T

// Only types that implement Comparable can be used
let pair = SortedPair { first = 1; second = 2 }  // OK
```

### Common Trait Constraints

Some commonly used trait constraints:

- `Equatable` - allows `==` and `!=` comparisons
- `Comparable` - allows `<`, `>`, `<=`, `>=` comparisons
- `Printable` - can be formatted as a string

```dovetail
function findMax<T>(a: T, b: T): T
    where T: Comparable =
    if a > b then a else b

let bigger = findMax(10, 20)  // 20
```

---

## Summary

- Use `<T>` to write a function or type that works with different types.
- The compiler usually infers type arguments from the values you pass.
- Use explicit type arguments, such as `<Int32>`, when needed.
- Use `where T: Trait` to require operations your generic code needs.

For generic extensions, class constraints, method-specific bounds, variance,
and runtime type matching, continue with [Part 26: Advanced Generics](26-advanced-generics.md).
