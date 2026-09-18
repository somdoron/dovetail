# Part 4: Control Flow

This part covers how to control the flow of execution in Dovetail programs.

---

## 4.1 If Expressions

In Dovetail, `if` is an expression - it always returns a value. This makes it more powerful than traditional if statements.

### Basic Syntax

```dovetail
if condition then
    expression1
else
    expression2
```

### Examples

**Simple if/else:**

```dovetail
let max = if a > b then a else b
```

**Multi-line blocks:**

```dovetail
let result = if user.isActive then
    debug("User is active")
    user.score * 2
else
    debug("User is inactive")
    0
```

**Single-line form:**

```dovetail
let sign = if x > 0 then "positive" else "non-positive"
```

### Else If Chains

Use `else if` for multiple conditions:

```dovetail
let grade = if score >= 90 then
    "A"
else if score >= 80 then
    "B"
else if score >= 70 then
    "C"
else if score >= 60 then
    "D"
else
    "F"
```

### If Without Else

When you don't need a value, you can omit `else`. The expression returns `Unit`:

```dovetail
if shouldLog then
    debug("Logging enabled")
```

### Nested If

If expressions can be nested:

```dovetail
let category = if age < 13 then
    "child"
else
    if age < 20 then "teenager" else "adult"
```

But prefer `else if` chains or `match` for better readability.

---

## 4.2 Match Expressions

`match` is Dovetail's powerful pattern matching construct. It's like `switch` in other languages, but much more expressive.

### Basic Syntax

```dovetail
match value with
    case pattern1 => result1
    case pattern2 => result2
    case _ => defaultResult
```

### Matching Literals

```dovetail
let dayName = match dayNumber with
    case 1 => "Monday"
    case 2 => "Tuesday"
    case 3 => "Wednesday"
    case 4 => "Thursday"
    case 5 => "Friday"
    case 6 => "Saturday"
    case 7 => "Sunday"
    case _ => "Invalid day"
```

### Matching Enums

Pattern matching shines with enums:

```dovetail
enum Color =
    Red
    Green
    Blue
    RGB(Uint8, Uint8, Uint8)

let description = match color with
    case Red => "Pure red"
    case Green => "Pure green"
    case Blue => "Pure blue"
    case RGB(r, g, b) => "RGB($r, $g, $b)"
```

### Matching Option

```dovetail
let message = match maybeUser with
    case Some(user) => "Hello, ${user.name}!"
    case None => "Hello, guest!"
```

### Matching Result

```dovetail
match fetchData() with
    case Ok(data) => processData(data)
    case Error(err) => debug("Error: $err")
```

### Destructuring Records

```dovetail
record Point =
    x: Int32
    y: Int32

let quadrant = match point with
    case Point { x; y } if x > 0 and y > 0 => "Q1"
    case Point { x; y } if x < 0 and y > 0 => "Q2"
    case Point { x; y } if x < 0 and y < 0 => "Q3"
    case Point { x; y } if x > 0 and y < 0 => "Q4"
    case _ => "Origin or on axis"
```

Commas are also accepted between record-pattern fields, but `dovetail fmt`
normalizes them to semicolons. In a multiline pattern, semicolons appear
between fields but not after the last field.

### Matching Tuples

```dovetail
let description = match (x, y) with
    case (0, 0) => "Origin"
    case (0, _) => "On Y axis"
    case (_, 0) => "On X axis"
    case (a, b) if a == b => "On diagonal"
    case _ => "Somewhere else"
```

### Matching Lists

A list matches either as a fixed set of elements or as a head and a tail:

```dovetail
let description = match xs with
    case [] => "empty"
    case [only] => "exactly one"
    case [a, b] => "exactly two"
    case h :: t => "at least one more"
```

`h :: t` binds `h` to the first element and `t` to the rest, which makes
recursion over a list read directly:

```dovetail
function sum(xs: List<Int32>): Int32 =
    match xs with
        case [] => 0
        case h :: t => h + sum(t)
```

`[]` and `h :: t` together cover every list, so the pair needs no wildcard:

```dovetail
let first = match xs with
    case [] => "none"
    case h :: t => h.format()
```

Fixed-length patterns do **not** cover everything, though — `case []` plus
`case [a, b]` leaves a one-element list unmatched, and the compiler says so:

```
non-exhaustive match: missing case for [_]
```

These patterns match a `List` only. Arrays have no pattern form; index them or
iterate them instead.

### Guard Conditions

Add `if` after a pattern for additional conditions:

```dovetail
let category = match age with
    case n if n < 0 => "Invalid"
    case n if n < 13 => "Child"
    case n if n < 20 => "Teenager"
    case n if n < 65 => "Adult"
    case _ => "Senior"
```

### The Wildcard Pattern

Use `_` to match anything you don't care about:

```dovetail
match triple with
    case (first, _, _) => first  // Only care about first element
```

### Variable Binding

Patterns can bind values to names:

```dovetail
match result with
    case Ok(value) => debug("Got: $value")
    case Error(e) => debug("Error: $e")
```

### Exhaustiveness

The compiler ensures all cases are covered. This won't compile:

```dovetail
enum Direction = North | South | East | West

// Error: non-exhaustive match
let name = match dir with
    case North => "N"
    case South => "S"
    // Missing East and West!
```

Use `_` as a catch-all if needed:

```dovetail
let name = match dir with
    case North => "N"
    case South => "S"
    case _ => "Other"
```

---

## 4.3 Loops

Dovetail provides `for` and `while` loops for iteration.

### For Loops

Iterate over a value that implements `Iterable<T>`, including arrays, lists, slices, and collection ranges:

```dovetail
for item in list do
    debug(item)
```

**Over a slice:**

```dovetail
let values = [|10, 20, 30, 40|]
let mutable total = 0
for value in values[|1..3|] do
    total = total + value
assert total == 50
```

See [Lists, Arrays, and Slices](06-type-system.md#67-lists-arrays-and-slices) for slicing forms and shared-mutation behavior. The `..` notation is only valid inside slicing brackets; use the collection `Range` API for a numeric loop.

**With index using enumerate:**

```dovetail
for (index, item) in list.zipWithIndex() do
    debug("$index: $item")
```

**Over a range:**

```dovetail
for i in Range.new(0, 10) do
    debug(i)  // 0 to 9

for i in Range.inclusive(0, 10) do
    debug(i)  // 0 to 10 (inclusive)
```

**Iterating over maps:**

```dovetail
for (key, value) in map do
    debug("$key -> $value")
```

### While Loops

Execute while a condition is true:

```dovetail
let mutable count = 0
while count < 10 do
    debug(count)
    count = count + 1
```

### Loop as Expression

Loops return `Unit`:

```dovetail
let result: Unit = for i in [|0, 1, 2, 3, 4|] do
    debug(i)
```

### Early Exit

Use `break` to exit a loop early:

```dovetail
let mutable found = false
for item in list do
    if item == target then
        found = true
        break
```

Use `continue` to skip to the next iteration:

```dovetail
for item in list do
    if item < 0 then
        continue  // Skip negative numbers
    debug(item)
```

### Functional Alternatives

For many use cases, functional methods are cleaner than loops:

```dovetail
// Instead of a loop to transform
let doubled = numbers.map(x => x * 2)

// Instead of a loop to filter
let positive = numbers.filter(x => x > 0)

// Instead of a loop to sum
let total = numbers.fold(0, (acc, x) => acc + x)

// Instead of a loop to find
let found = numbers.find(x => x > 100)
```

These functional approaches are often more readable and eliminate the need for mutable variables.

---

## Summary

Dovetail provides expressive control flow constructs:

- **If expressions** - Always return a value, can be used anywhere an expression is expected
- **Match expressions** - Powerful pattern matching with destructuring, guards, and exhaustiveness checking
- **Loops** - `for` and `while` for iteration, with `break` and `continue` for control

A key principle: prefer pattern matching over long if/else chains, and prefer functional methods (`map`, `filter`, `fold`) over imperative loops when it makes the code clearer.

In the next part, we'll explore functions in depth.

For restrictions on loops that suspend, see [Async Programming](12-async.md).
