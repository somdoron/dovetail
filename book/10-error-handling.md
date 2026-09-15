# Part 10: Error Handling

Dovetail takes a different approach to error handling than many mainstream languages. Instead of exceptions and try/catch blocks, Dovetail uses **values** to represent errors. This makes error handling explicit, composable, and checked by the compiler.

---

## 10.1 No Exceptions

In many languages, functions can throw exceptions at any point, and callers may or may not handle them. This leads to hidden control flow and runtime surprises.

Dovetail has **no exceptions**. Instead, it provides two types for representing the absence of a value or the possibility of failure:

- **`Option<T>`** — a value that may or may not exist
- **`Result<T, E>`** — an operation that may succeed with `T` or fail with `E`

Both are regular enum types defined in the prelude. There's nothing magical about them — they follow the same rules as any other enum. The compiler ensures you handle both cases.

**Benefits of errors-as-values:**

- **Explicit** — the function signature tells you whether it can fail
- **Composable** — chain operations with methods like `map`, `andThen`, and `try`/`orReturn`
- **Compiler-checked** — you can't accidentally ignore an error case in a `match`

---

## 10.2 Option Type

### Definition

`Option<T>` represents a value that may or may not be present:

```dovetail
public enum Option<T> =
    Some(T)
    None
```

### When to Use Option

Use `Option` when the absence of a value is a **normal, expected** situation — not an error:

- Looking up a key in a map
- Finding the first element that matches a condition
- Parsing an optional configuration field

### Creating Option Values

```dovetail
let found: Option<Int32> = Some(42)
let missing: Option<Int32> = None
```

### Pattern Matching on Option

The most fundamental way to work with `Option` is pattern matching:

```dovetail
function describe(maybe: Option<Int32>): String =
    match maybe with
        case Some(value) => "Found: $value"
        case None => "Nothing here"
```

### Useful Methods

**`or`** — unwrap the value or use a default:

```dovetail
let x: Option<Int32> = Some(5)
let y: Option<Int32> = None

assert x.or(0) == 5
assert y.or(0) == 0
```

**`expect`** — unwrap or panic with a message:

```dovetail
let x: Option<Int32> = Some(42)
let value = x.expect("value must exist")
assert value == 42
```

**`require`** — unwrap or panic with a default message:

```dovetail
let x: Option<Int32> = Some(7)
assert x.require == 7
```

**`isSome` / `isNone`** — test which variant:

```dovetail
let x: Option<Int32> = Some(1)
assert x.isSome
assert x.isNone == false
```

**`map`** — transform the inner value if present:

```dovetail
let x: Option<Int32> = Some(3)
let doubled = x.map(v => v * 2)   // Some(6)

let y: Option<Int32> = None
let result = y.map(v => v * 2)    // None
```

**`andThen`** — chain operations that themselves return `Option`:

```dovetail
function half(x: Int32): Option<Int32> =
    if x % 2 == 0 then Some(x / 2) else None

let result = Some(10).andThen(half)  // Some(5)
let failed = Some(3).andThen(half)   // None
```

**`filter`** — keep the value only if a predicate holds:

```dovetail
let x = Some(10).filter(v => v > 5)   // Some(10)
let y = Some(3).filter(v => v > 5)    // None
```

### Converting: toResult

Convert an `Option` to a `Result` by providing an error value for the `None` case:

```dovetail
let x: Option<Int32> = Some(42)
let r = x.toResult("not found")   // Ok(42)

let y: Option<Int32> = None
let s = y.toResult("not found")   // Error("not found")
```

---

## 10.3 Result Type

### Definition

`Result<T, E>` represents an operation that can succeed or fail:

```dovetail
public enum Result<T, E> =
    Ok(T)
    Error(E)
```

### When to Use Result

Use `Result` when an operation can **fail** and you want the caller to handle the failure:

- Parsing user input
- Reading from a file
- Validating data
- Network requests

### Creating Result Values

```dovetail
let success: Result<Int32, String> = Ok(42)
let failure: Result<Int32, String> = Result.error("something went wrong")
```

Note that `Ok` is a variant constructor and can be used directly, while `Error` uses `Result.error()` to avoid ambiguity with the variant name.

### Pattern Matching on Result

```dovetail
function describe(result: Result<Int32, String>): String =
    match result with
        case Ok(value) => "Success: $value"
        case Error(msg) => "Failed: $msg"
```

### Useful Methods

**`or`** — unwrap the success value or use a default:

```dovetail
let ok: Result<Int32, String> = Ok(5)
let err: Result<Int32, String> = Result.error("oops")

assert ok.or(0) == 5
assert err.or(0) == 0
```

**`expect`** — unwrap or panic with a message:

```dovetail
let r: Result<Int32, String> = Ok(42)
assert r.expect("must succeed") == 42
```

**`require`** — unwrap or panic with a default message:

```dovetail
let r: Result<Int32, String> = Ok(7)
assert r.require == 7
```

**`isOk` / `isError`** — test which variant:

```dovetail
let ok: Result<Int32, String> = Ok(1)
assert ok.isOk
assert ok.isError == false
```

**`map`** — transform the success value:

```dovetail
let r: Result<Int32, String> = Ok(3)
let doubled = r.map(v => v * 2)   // Ok(6)
```

**`mapError`** — transform the error value:

```dovetail
let r: Result<Int32, String> = Result.error("fail")
let wrapped = r.mapError(e => "wrapped: $e")  // Error("wrapped: fail")
```

**`andThen`** — chain operations that return `Result`:

```dovetail
function parseAge(input: String): Result<Int32, String> =
    // ... parsing logic

function validateAge(age: Int32): Result<Int32, String> =
    if age >= 0 then Ok(age) else Result.error("age must be non-negative")

let result = parseAge("25").andThen(validateAge)
```

### Converting: toOption

Convert a `Result` to an `Option`, discarding the error information:

```dovetail
let ok: Result<Int32, String> = Ok(42)
assert ok.toOption().isSome       // Some(42)

let err: Result<Int32, String> = Result.error("fail")
assert err.toOption().isNone      // None
```

---

## 10.4 Railway-Oriented Programming

### The Problem

When chaining multiple operations that can fail, pattern matching quickly becomes deeply nested:

```dovetail
function processUser(id: Int32): Result<String, String> =
    match lookupUser(id) with
        case Error(e) => Result.error(e)
        case Ok(user) =>
            match validateUser(user) with
                case Error(e) => Result.error(e)
                case Ok(valid) =>
                    match formatUser(valid) with
                        case Error(e) => Result.error(e)
                        case Ok(output) => Ok(output)
```

This is sometimes called the "pyramid of doom." Each level of nesting adds visual noise without adding meaning.

### try and orReturn

Dovetail provides two equivalent syntactic forms to flatten this pattern:

- **`try expr`** — prefix syntax
- **`expr.orReturn`** — postfix syntax

Both do the same thing: **unwrap the success value, or early-return the error** from the enclosing function.

The example above becomes:

```dovetail
function processUser(id: Int32): Result<String, String> =
    let user = try lookupUser(id)
    let valid = try validateUser(user)
    try formatUser(valid)
```

Or using the postfix form:

```dovetail
function processUser(id: Int32): Result<String, String> =
    let user = lookupUser(id).orReturn
    let valid = validateUser(user).orReturn
    formatUser(valid).orReturn
```

### How They Work

When you write `try expr` (or `expr.orReturn`):

1. The expression is evaluated — it must produce an `Option` or `Result`
2. If it's `Some(value)` or `Ok(value)`, the value is unwrapped and execution continues
3. If it's `None` or `Error(e)`, the enclosing function **immediately returns** with the failure value

### Return Type Compatibility

The enclosing function's return type must be compatible with the early return:

- If you `try` a `Result<T, E>`, the function must return `Result<_, E>` (same error type)
- If you `try` an `Option<T>`, the function must return `Option<_>`

```dovetail
// Works: both return Result<_, String>
function process(): Result<Int32, String> =
    let x = try getResult()   // getResult(): Result<Int32, String>
    Ok(x + 1)

// Works: both return Option<_>
function find(): Option<Int32> =
    let x = try lookup()      // lookup(): Option<Int32>
    Some(x + 1)
```

### Chaining Multiple Operations

You can freely mix `try`/`orReturn` with regular code in a single function:

```dovetail
function validateAndProcess(input: String): Result<Output, String> =
    let parsed = try parse(input)
    let validated = try validate(parsed)
    let enriched = enrich(validated)   // this one can't fail
    let result = try save(enriched)
    Ok(result)
```

### Chaining After orReturn

Because `orReturn` is a postfix operator, you can chain further operations directly on the unwrapped value — accessing tuple fields, calling methods, or accessing properties:

```dovetail
function getUser(): Result<(String, Int32), String> = Ok(("Alice", 30))

function getUserName(): Result<String, String> =
    let name = getUser().orReturn._0
    Ok(name)
```

```dovetail
function getConfig(): Result<Option<Int32>, String> = Ok(Some(42))

function process(): Result<Bool, String> =
    let hasSetting = getConfig().orReturn.isSome
    Ok(hasSetting)
```

This is one of the advantages of the postfix `orReturn` form over the prefix `try` — it reads naturally left-to-right without needing parentheses.

### Complete Example

Here's a multi-step validation pipeline:

```dovetail
function validateEmail(email: String): Result<String, String> =
    if email.length == 0 then
        Result.error("email cannot be empty")
    else if email.contains("@") == false then
        Result.error("email must contain @")
    else
        Ok(email)

function validateAge(age: Int32): Result<Int32, String> =
    if age < 0 then
        Result.error("age cannot be negative")
    else if age > 150 then
        Result.error("age is unrealistic")
    else
        Ok(age)

function createProfile(name: String, email: String, age: Int32): Result<Profile, String> =
    let validEmail = try validateEmail(email)
    let validAge = try validateAge(age)
    Ok(Profile { name = name; email = validEmail; age = validAge })
```

If any validation fails, `createProfile` immediately returns the first error. No nesting required.

---

## 10.5 Panics and Assertions

### Panic

The `panic` expression causes an **immediate, unrecoverable** program termination:

```dovetail
panic "something went terribly wrong"
```

Panics are for truly unexpected situations — bugs, violated invariants, or states that "should never happen." They are **not** for expected error conditions.

### Assert

The `assert` expression checks a condition and panics if it's false:

```dovetail
assert x > 0
assert list.length == expectedLength
```

Assertions are commonly used in tests and to check invariants during development.

### When to Use Panics vs Result

| Use `panic`/`assert` when... | Use `Result` when... |
|-------------------------------|----------------------|
| A bug has been detected | An operation can legitimately fail |
| An invariant is violated | User input might be invalid |
| "This should never happen" | A resource might be unavailable |
| In tests | The caller should decide how to handle failure |

### expect and require — Bridging Option/Result to Panics

Sometimes you're certain a value exists or an operation succeeded, and a missing value would be a bug. The `expect` and `require` methods let you unwrap with a panic:

```dovetail
// With a custom message
let config = loadConfig().expect("config file must exist")

// With a default message
let value = maybeValue.require
```

Use these sparingly. In production code, prefer `or`, `map`, or `try`/`orReturn` to handle failures gracefully. Reserve `expect`/`require` for cases where failure genuinely indicates a programming error.

---

## Summary

- Dovetail uses **errors as values** — no exceptions, no hidden control flow
- **`Option<T>`** represents optional values (`Some` or `None`)
- **`Result<T, E>`** represents operations that can fail (`Ok` or `Error`)
- Both types provide methods for safe access: `or`, `expect`, `require`, `isSome`/`isNone`, `isOk`/`isError`
- **`map`** and **`andThen`** transform values without unwrapping
- **`try`** (prefix) and **`orReturn`** (postfix) enable railway-oriented programming — flat, readable chains of fallible operations
- **`toResult`** and **`toOption`** convert between the two types
- **`panic`** and **`assert`** are for unrecoverable errors and invariant checking
- **`expect`** and **`require`** bridge `Option`/`Result` to panics when failure is a bug
