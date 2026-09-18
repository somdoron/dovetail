# Part 5: Functions

Functions are the primary building blocks in Dovetail. They are first-class values, meaning you can pass them around, return them from other functions, and store them in variables.

---

## 5.1 Function Definitions

### Basic Syntax

Functions are defined using the `function` keyword:

```dovetail
function greet(): String =
    "Hello, World!"
```

The function body follows the `=` sign. Since Dovetail is expression-based, the last expression in the body is the return value.

### Parameters and Return Types

Functions can take parameters with type annotations. The return type follows the parameter list:

```dovetail
function add(a: Int32, b: Int32): Int32 =
    a + b

function formatUser(name: String, age: Int32): String =
    "$name is $age years old"
```

### Unit Return Type

Functions that don't return a meaningful value return `Unit`:

```dovetail
function greet(name: String): Unit =
    debug("Hello, $name!")
```

You can omit the return type annotation for `Unit`:

```dovetail
function greet(name: String) =
    debug("Hello, $name!")
```

### Multi-Line Function Bodies

Function bodies can span multiple lines. Each line is an expression, and the last expression is returned:

```dovetail
function calculateTotal(price: Float64, quantity: Int32): Float64 =
    let subtotal = price * quantity.toFloat64()
    let tax = subtotal * 0.08
    subtotal + tax
```

### Visibility

Top-level functions default to `internal` (package visibility). Use `public` to
make them accessible from other packages, or `private` for file-local helpers:

```dovetail
public function add(a: Int32, b: Int32): Int32 =
    a + b
```

---

## 5.2 Named and Default Arguments

### Named Arguments

When calling a function, you can use named arguments for clarity:

```dovetail
function createUser(name: String, age: Int32, email: String): User =
    User { name = name; age = age; email = email }

// Using named arguments
let user = createUser(name = "Alice", age = 30, email = "alice@example.com")
```

Named arguments can be in any order:

```dovetail
let user = createUser(email = "bob@example.com", name = "Bob", age = 25)
```

You can mix positional and named arguments, but positional arguments must come first:

```dovetail
let user = createUser("Charlie", age = 28, email = "charlie@example.com")
```

Every parameter must receive exactly one argument. Unknown names, duplicate
arguments (including a positional and named argument for the same parameter),
missing arguments, and positional arguments after named ones are errors.

Named arguments also work with methods and class constructors. The receiver is
evaluated first, then eager arguments are evaluated once in the order written,
even when that differs from parameter order. `ByName` arguments remain deferred.

Accepted names come from the declaration visible at the call site. Calls through
a trait bound or interface use that contract's names; calls through a base class
use its visible method declaration. Calls through a concrete class use its own
method declaration, or the inherited declaration when it does not override the
method. An implementation may rename parameters, but those names are not aliases
for the contract's names. Runtime dispatch does not affect argument binding.
Renaming a public parameter can therefore break callers that use its name.

Function values, including stored function and bound-method references, accept
only positional arguments: their types do not contain parameter names. Positional
enum payloads and newtype constructors also have no argument labels. Record
construction continues to use its existing field-initializer syntax.

### Default Parameter Values (Planned)

Default parameter values are planned but are not implemented yet. For optional
settings today, use a configuration record and a function that constructs it:

```dovetail
record Config =
    host: String
    port: Int32

function defaultConfig(host: String): Config =
    Config { host = host; port = 8080 }

let local = defaultConfig("localhost")
let custom = local with { port = 3000 }
```

Record field assignments are distinct from function arguments. The formatter
preserves that distinction; it does not introduce named arguments.

The proposed default-parameter syntax is:

```dovetail
function greet(name: String, greeting: String = "Hello"): String =
    "$greeting, $name!"

greet("Alice")           // "Hello, Alice!"
greet("Bob", "Hi")       // "Hi, Bob!"
greet("Charlie", greeting = "Hey")  // "Hey, Charlie!"
```

Parameters with defaults must come after required parameters:

```dovetail
function createConfig(
    host: String,
    port: Int32 = 8080,
    timeout: Int32 = 30,
    debug: Bool = false
): Config =
    Config { host = host; port = port; timeout = timeout; debug = debug }

// Use defaults
let config1 = createConfig("localhost")

// Override some defaults
let config2 = createConfig("localhost", port = 3000, debug = true)
```

### Future Linter Rule for Named Arguments

The formatter preserves the caller's choice of positional or named arguments.
Adding argument names requires resolving the called declaration and belongs in
a semantic linter.

Dovetail does not yet have a linter. A planned rule would suggest named arguments
when the called declaration has more than two explicit parameters, excluding
the method receiver:

```dovetail
// You write:
createUser("John", 30, "john@example.com")

// A future linter could suggest this fix:
createUser(name = "John", age = 30, email = "john@example.com")
```

Additional planned lint rules require named arguments for Boolean and numeric
literals, even in calls with only one or two parameters. For example,
`retry(3)` would be written as `retry(attempts = 3)`. These lint rules are not
implemented yet. See the [future linter design and rule backlog](../docs/linter-design.md).

---

## 5.3 Closures and Lambdas

### Basic Closure Syntax

Closures (anonymous functions) use the `=>` arrow:

```dovetail
// With type annotations (via expected type)
let multiply: (Int32, Int32) => Int32 = (a, b) => a * b

// Single parameter - no parentheses needed
let double = (x: Int32) => x * 2

// Multiple parameters - parentheses required
let add = (a: Int32, b: Int32) => a + b
```

### Multi-Line Closures

Closure bodies can span multiple lines using indentation:

```dovetail
let process = (x: Int32) =>
    let y = x + 1
    let z = y * 2
    z + 10
```

### Environment Capture

Closures can capture variables from their surrounding scope:

```dovetail
let multiplier = 3
let scale = (x: Int32) => x * multiplier

scale(5)  // 15
```

Closures capture variables by reference:

```dovetail
function makeCounter(): () => Int32 =
    let mutable count = 0
    () =>
        count = count + 1
        count

let counter = makeCounter()
counter()  // 1
counter()  // 2
counter()  // 3
```

### Function Types

Function types describe the signature of a function:

```dovetail
// A function that takes two Int32s and returns an Int32
let operation: (Int32, Int32) => Int32 = (a, b) => a + b

// A function that takes no arguments and returns Unit
let action: () => Unit = () => debug("Hello!")

// A function that takes a String and returns a Bool
let predicate: (String) => Bool = s => s.length > 0
```

---

## 5.4 First-Class Function References

Named functions can be used as values wherever a function type is expected. Simply use the function name without calling it:

```dovetail
function double(x: Int32): Int32 = x * 2

let f: Int32 => Int32 = double
f(5)  // 10
```

### Passing Functions as Arguments

```dovetail
function identity(x: Int32): Int32 = x

function apply(f: Int32 => Int32, value: Int32): Int32 = f(value)

apply(identity, 42)  // 42
```

### Module Functions

Module functions work the same way:

```dovetail
module Math =
    function square(x: Int32): Int32 = x * x

let f: Int32 => Int32 = Math.square
f(4)  // 16
```

### Generic Functions

Generic functions can be used as values when the expected type provides enough information to instantiate the type parameters:

```dovetail
function identity<T>(x: T): T = x

let f: Int32 => Int32 = identity  // T inferred as Int32
f(42)  // 42
```

### Overload Disambiguation

When multiple overloads exist, the expected type disambiguates:

```dovetail
function convert(x: Int32): String = "$x"
function convert(x: String): Int32 = 0

let f: Int32 => String = convert  // selects first overload
```

---

## 5.5 Method References

### Bound Method References

When you access a method on an instance without calling it, you get a **bound method reference** — a function value with `self` already captured:

```dovetail
record Point =
    x: Int32
    y: Int32

module Point =
    function getX(self): Int32 = self.x
    function addX(self, dx: Int32): Int32 = self.x + dx

let p = Point { x = 42; y = 0 }

// Bound reference — self (p) is captured
let f = p.getX       // type: Unit => Int32
f()                   // 42

let g = p.addX        // type: Int32 => Int32
g(10)                 // 52
```

The resulting function type excludes `self` from the parameter list since it is already bound to the object.

### Unbound Method References

You can also reference a method via the type name. This produces a function that takes `self` as its first parameter:

```dovetail
let f: Point => Int32 = Point.getX
let p = Point { x = 42; y = 0 }
f(p)  // 42
```

### Class Methods

Method references work with class instance methods too:

```dovetail
class Counter(public value: Int32) =
    public function getValue(self: Counter): Int32 = self.value

let c = Counter(99)
let f = c.getValue
f()  // 99
```

### Using Method References with Higher-Order Functions

Method references are especially useful when passing to higher-order functions:

```dovetail
function apply(f: Int32 => Int32, arg: Int32): Int32 = f(arg)

let p = Point { x = 42; y = 0 }
apply(p.addX, 10)  // 52
```

---

## 5.6 Higher-Order Functions

Higher-order functions are functions that take other functions as parameters or return functions.

### Functions as Parameters

```dovetail
function applyTwice(f: (Int32) => Int32, x: Int32): Int32 =
    f(f(x))

let double = (x: Int32) => x * 2
applyTwice(double, 5)  // 20 (5 * 2 * 2)
```

### Common Higher-Order Functions

Dovetail's standard library provides common higher-order functions for collections:

**map** - Transform each element:

```dovetail
let numbers = [1, 2, 3, 4, 5]
let doubled = numbers.map(x => x * 2)  // [2, 4, 6, 8, 10]
```

**filter** - Keep elements matching a predicate:

```dovetail
let numbers = [1, 2, 3, 4, 5]
let evens = numbers.filter(x => x % 2 == 0)  // [2, 4]
```

**foldLeft** - Reduce to a single value:

```dovetail
let numbers = [1, 2, 3, 4, 5]
let sum = numbers.foldLeft(0, (acc, x) => acc + x)  // 15
```

### Returning Functions

Functions can return other functions:

```dovetail
function makeMultiplier(factor: Int32): (Int32) => Int32 =
    x => x * factor

let triple = makeMultiplier(3)
triple(7)  // 21
```

### Pipeline Style

Higher-order functions enable a clean pipeline style:

```dovetail
let result = numbers
    .filter(x => x > 0)
    .map(x => x * 2)
    .foldLeft(0, (acc, x) => acc + x)
```

This style is often more readable than nested function calls:

```dovetail
// Equivalent but harder to read
let result = numbers.filter(x => x > 0).map(x => x * 2).foldLeft(0, (acc, x) => acc + x)
```

---

## Summary

- Functions are defined with `function name(params): ReturnType = body`
- Use named arguments for clarity: `createUser(name = "Alice", age = 30)`
- Default parameter values are planned; use configuration records for defaults today
- Closures use `=>` syntax: `x => x * 2` or `(a, b) => a + b`
- Closures capture variables from their environment
- Function types describe signatures: `(Int32, Int32) => Int32`
- Named functions can be used as values: `let f: Int32 => Int32 = double`
- Bound method references capture self: `let f = point.getX`
- Unbound method references include self: `let f: Point => Int32 = Point.getX`
- Higher-order functions take or return other functions
- Use `map`, `filter`, and `foldLeft` for collection processing
