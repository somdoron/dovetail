# Part 3: Language Basics

This part covers the fundamental syntax and building blocks of Dovetail.

---

## 3.1 Layout-Sensitive Syntax

Dovetail uses indentation to define code blocks - no curly braces or semicolons needed. If you've used Python, you'll feel right at home.

### The Basics

- Use **spaces** for indentation (tabs cause a compile error)
- **Four spaces** is the formatter standard
- Code at the same indentation level belongs to the same block
- Lines can be continued by indenting more than the current block

### Where Blocks Are Used

Indented blocks appear in many places in Dovetail:

**After `=` in definitions:**

```dovetail
function greet(name: String): String =
    let message = "Hello, $name!"
    message

let result =
    let x = 10
    let y = 20
    x + y
```

**After `then` and `else` in conditionals:**

```dovetail
if x > 0 then
    debug("positive")
    x * 2
else
    debug("non-positive")
    0
```

**After `with` in match expressions:**

```dovetail
match color with
    case Red => "red"
    case Green => "green"
    case Blue => "blue"
```

**After `do` in loops:**

```dovetail
for item in list do
    debug(item)
```

**After `=>` in lambdas:**

```dovetail
let process = x =>
    let doubled = x * 2
    doubled + 1
```

### Single-Line Expressions

For short expressions, you can keep everything on one line:

```dovetail
function double(x: Int32): Int32 = x * 2

if x > 0 then x else -x
```

### Multi-Line Continuations

Lines indented more than the block continue the previous expression:

```dovetail
let result = someFunction(
    arg1,
    arg2,
    arg3)
```

### Explicit Delimiters

You can use explicit `begin`/`end` and `;` instead of relying on layout:

```dovetail
// Using layout (implicit)
function foo() =
    let x = 5
    x + 1

// Using explicit delimiters
function foo() = begin let x = 5; x + 1 end
```

### Record Construction

Record fields use semicolons as separators:

```dovetail
let point = Point { x = 10; y = 20 }
```

Or with layout:

```dovetail
let point = Point {
    x = 10
    y = 20
}
```

---

## 3.2 Comments and Documentation

### Single-Line Comments

Use `//` for regular comments:

```dovetail
// This is a comment
let x = 5  // Inline comment
```

### Documentation Comments

Use `///` for documentation comments. They support Markdown formatting:

```dovetail
/// Creates a greeting message for the given name.
///
/// Returns a friendly greeting string that can be displayed
/// to the user.
///
/// ## Example
///
/// ```
/// greet("Bob")  // "Hello, Bob!"
/// ```
function greet(name: String): String =
    "Hello, $name!"
```

Documentation comments can be attached to:
- Functions
- Types (records, enums, classes, traits)
- Type fields
- Packages (via `README.md` in package directory)

### Testing Documented Behavior

Documentation comments are displayed by tooling, but their code blocks are not
executed by `dovetail test`. Add an explicit test for documented behavior:

```dovetail
/// Adds two numbers together.
function add(a: Int32, b: Int32): Int32 = a + b

test "documented addition" = assert add(2, 3) == 5
```

The book separately checks marked complete examples in CI; see [Book validation](validation.md).
The synchronous `debug(value)` calls in examples print diagnostic output. For
application console I/O, see [the async Console API](22-stdlib.md#225-io).

---

## 3.3 Variables and Bindings

### Immutable Variables (Default)

Variables are immutable by default using `let`:

```dovetail
let name = "Alice"
let age = 30

// This would be a compile error:
// name = "Bob"
```

### Mutable Variables

Use `let mutable` when you need to reassign a variable:

```dovetail
let mutable counter = 0
counter = counter + 1
counter = counter + 1
// counter is now 2
```

### Variable Shadowing

You can shadow a variable by declaring a new one with the same name:

```dovetail
let x = 5
let x = x + 1    // shadows the previous x, now x = 6
let x = "hello"  // shadows again, x is now a String
```

Shadowing creates a new variable rather than mutating the existing one.

### Type Annotations

Types are usually inferred, but you can add explicit annotations:

```dovetail
let count: Int32 = 0
let name: String = "Alice"
let ratio: Float64 = 3.14
```

---

## 3.4 Primitive Types

Dovetail provides the following primitive types:

| Type | Description |
|------|-------------|
| `Int8` | Signed 8-bit integer |
| `Int16` | Signed 16-bit integer |
| `Int32` | Signed 32-bit integer |
| `Int64` | Signed 64-bit integer |
| `Uint8` | Unsigned 8-bit integer |
| `Uint16` | Unsigned 16-bit integer |
| `Uint32` | Unsigned 32-bit integer |
| `Uint64` | Unsigned 64-bit integer |
| `Float32` | 32-bit floating point |
| `Float64` | 64-bit floating point |
| `Bool` | Boolean (`true` or `false`) |
| `Char` | Unicode character |
| `String` | UTF-8 encoded text |
| `Unit` | Empty type (like `void`) |
| `Never` | Bottom type (no values) |
| `Any` | Top type (all values) |

### Literals

```dovetail
// Integers
let decimal = 42
let hex = 0xFF
let binary = 0b1010
let octal = 0o755
let byte = 255u8       // with type suffix
let long = 1000000i64

// Floats
let pi = 3.14159
let scientific = 1.5e10
let typed = 3.14f32

// Exact numbers (available from the prelude)
let large = 123456789012345678901234567890big
let amount = 19.99dec
let exactScientific = 1.25e3dec

// Booleans
let active = true
let disabled = false

// Characters
let letter = 'A'
let emoji = '🎉'
let escaped = '\n'

// Strings
let greeting = "Hello, World!"
let multiline = """
    This is a
    multi-line string
    """

// Unit
let nothing: Unit = ()
```

`BigInt` literals use the `big` suffix and accept integer digits in any supported
base, including `0xFFbig`. `Decimal` literals use `dec` and accept base-10
integers, fractions, and scientific notation. Both are constructed exactly from
source digits, without floating-point conversion. Suffixes are lowercase and
must touch the number; digit separators are not supported.

These prelude types support `+`, `-`, `*`, unary `-`, equality, and ordering.
Operands must have the same type: write `amount * 2dec`, not `amount * 2`.
Unsuffixed literals keep their existing types, even with a `Decimal` annotation.
Literal patterns in `match` are not supported for these types; use an equality
guard instead. Division and rounding APIs are not provided yet.

Decimal uses a `BigInt` coefficient and a nonnegative `Int32` scale. Values are
normalized: `1.200dec` formats as `"1.2"`, and `-0.00dec` as `"0"`. Scientific
notation adjusts scale exactly: `1.25e3dec` is `1250dec`. Use
`Decimal.of(123big, 2)` for explicit construction, or `fromInt32`/`fromInt64`.
Negative constructor scales and arithmetic scale overflow panic. Invalid
literal scales are compile-time errors. Compiler literal construction has a
resource limit of 1,000,000 coefficient digits, including exponent expansion;
this does not limit runtime BigInt arithmetic.

`BigInt` and `Decimal` have moved from `standard.math` to `standard.prelude`;
remove their old imports. `Decimal.unscaled` and the first argument to
`Decimal.of` now use `BigInt` instead of `Int64`. `RoundingMode` remains in
`standard.math`.

### The Never Type

`Never` is special - it represents computations that never complete normally:

```dovetail
function fail(message: String): Never =
    panic message
```

It's useful for functions that always panic, unreachable code branches, and infallible async operations (`Async<T, Never>`).

### The Any Type

`Any` is the top type — every type is assignable to `Any`:

```dovetail
let a: Any = "Hello"
let b: Any = 42
let c: Any = true
```

To use a value of type `Any`, you need to test or cast it back to a concrete type using `is` and `as` (see [Type Testing and Casting](06-type-system.md#610-the-any-type-and-type-casting)).

---

## 3.5 Operators

### Arithmetic Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `+` | Addition | `a + b` |
| `-` | Subtraction | `a - b` |
| `*` | Multiplication | `a * b` |
| `/` | Division | `a / b` |
| `%` | Modulo | `a % b` |
| `-` | Negation (unary) | `-x` |

### Comparison Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `==` | Equal | `a == b` |
| `!=` | Not equal | `a != b` |
| `<` | Less than | `a < b` |
| `<=` | Less or equal | `a <= b` |
| `>` | Greater than | `a > b` |
| `>=` | Greater or equal | `a >= b` |

### Logical Operators

Dovetail uses keywords for logical operators to improve readability:

| Operator | Description | Example |
|----------|-------------|---------|
| `and` | Logical AND | `a and b` |
| `or` | Logical OR | `a or b` |
| `not` | Logical NOT | `not x` |

Short-circuit evaluation applies: `and` stops if the left side is false, `or` stops if the left side is true.

### Bitwise Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `&` | Bitwise AND | `a & b` |
| `\|` | Bitwise OR | `a \| b` |
| `^` | Bitwise XOR | `a ^ b` |
| `~` | Bitwise NOT | `~x` |
| `<<` | Left shift | `a << 2` |
| `>>` | Right shift | `a >> 2` |

### Sequence Operators

| Operator | Description | Example |
|----------|-------------|---------|
| `::` | Prepend one element to a list | `x :: xs` |
| `++` | Join two sequences | `xs ++ ys` |

`::` is right-associative, so `1 :: 2 :: rest` prepends both in order. `++`
concatenates; it works on `String` natively and on any type implementing the
`Concat` trait, including `List`.

```dovetail
let xs = 1 :: [2, 3]        // [1, 2, 3]
let ys = [1, 2] ++ [3, 4]   // [1, 2, 3, 4]
```

### Operator Precedence

From lowest to highest:

1. `or` (logical or)
2. `and` (logical and)
3. binary `~` ([tuple extension, advanced](25-tuple-extension.md))
4. `==`, `!=` (equality)
5. `<`, `<=`, `>`, `>=` (comparison)
6. `::` (cons — **right**-associative)
7. `|` (bitwise or)
8. `^` (bitwise xor)
9. `&` (bitwise and)
10. `<<`, `>>` (shift)
11. `+`, `-`, `++` (additive and concatenation)
12. `*`, `/`, `%` (multiplicative)
13. `not`, `-`, `~` (unary prefix)
14. Function calls, field access, indexing, `is`, `as` (postfix)

So `x :: xs ++ ys` groups as `x :: (xs ++ ys)`, and `x :: xs == ys` as
`(x :: xs) == ys`.

Use parentheses when precedence is unclear:

```dovetail
let result = (a + b) * c
let check = (x > 0) and (y < 10)
```

---

## 3.6 String Interpolation

Dovetail supports string interpolation in all strings - no special prefix needed.

### Simple Variable Interpolation

Use `$` followed by a variable name:

```dovetail
let name = "Alice"
let greeting = "Hello, $name!"  // "Hello, Alice!"
```

### Expression Interpolation

Use `${}` for complex expressions:

```dovetail
let x = 10
let y = 20
let message = "Sum: ${x + y}"  // "Sum: 30"

let user = getUser()
let info = "User ${user.name} is ${user.age} years old"
```

### Escape Sequences

| Sequence | Meaning |
|----------|---------|
| `\n` | Newline |
| `\r` | Carriage return |
| `\t` | Tab |
| `\\` | Backslash |
| `\"` | Double quote |
| `\$` | Literal dollar sign |
| `\0` | Null character |
| `\u{XXXX}` | Unicode code point |

```dovetail
let escaped = "Line 1\nLine 2"
let price = "Cost: \$99.99"  // Literal $, not interpolation
let heart = "\u{2764}"       // ❤
```

### Prefixed Literals

A string may carry an identifier on its opening quote — `sql"SELECT * FROM t WHERE id = $id"`. The prefix hands the literal to a library, which decides what interpolation means: in a `sql"..."` literal, `$id` binds a parameter rather than splicing text. Prefixed literals also add a spread form, `$..values`, which exists nowhere else. See [Part 24](24-prefixed-literals.md).

### Multi-line Strings

Use triple quotes for multi-line strings:

```dovetail
let poem = """
    Roses are red,
    Violets are blue,
    Dovetail is great,
    And so are you!
    """
```

Interpolation works in multi-line strings too:

```dovetail
let name = "World"
let html = """
    <html>
      <body>
        <h1>Hello, $name!</h1>
      </body>
    </html>
    """
```

### String Concatenation

Use `++` to concatenate strings:

```dovetail
let first = "Hello"
let second = "World"
let combined = first ++ ", " ++ second ++ "!"
```

For building strings dynamically, consider using `StringBuilder` from the standard library for better performance.

---

## Summary

You now understand Dovetail's basic syntax:

- **Layout-sensitive** - Indentation defines blocks, no braces or semicolons needed
- **Comments** - `//` for regular, `///` for documentation with Markdown
- **Variables** - Immutable by default (`let`), use `let mutable` for mutation
- **Primitive types** - Integers, floats, booleans, characters, strings, unit, never, any
- **Operators** - Standard arithmetic/comparison, keywords for logical (`and`, `or`, `not`)
- **String interpolation** - `$variable` and `${expression}` in any string

In the next part, we'll explore control flow with if expressions, pattern matching, and loops.
