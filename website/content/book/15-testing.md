# Part 15: Testing

Testing is a first-class feature in Dovetail. Tests are written directly in your source files alongside your code, making it easy to keep tests close to the implementation they verify.

---

## 15.1 Unit Tests

### Basic Test Syntax

Tests are declared using the `test` keyword followed by a string name and a body:

```dovetail
function add(a: Int32, b: Int32): Int32 = a + b

test "addition works" =
    assert add(1, 2) == 3
```

The test name is a descriptive string that appears in test output. The body contains assertions that verify expected behavior.

### The assert Expression

The `assert` expression checks that a condition is true. If the condition is false, the test fails with a panic:

```dovetail
test "basic assertions" =
    assert 1 + 1 == 2
    assert "hello".length == 5
    assert true
```

Multiple assertions can appear in a single test. The test passes only if all assertions succeed:

```dovetail
test "multiple assertions" =
    let result = calculateTotal(100.0, 5)
    assert result > 0.0
    assert result == 540.0  // 100 * 5 * 1.08 (with tax)
```

### Testing with Let Bindings

Tests can use `let` bindings to set up test data:

```dovetail
record User =
    name: String
    age: Int32

test "user creation" =
    let user = User { name = "Alice"; age = 30 }
    assert user.name == "Alice"
    assert user.age == 30
```

### Testing Functions

Tests typically call functions and verify their output:

```dovetail
function fibonacci(n: Int32): Int32 =
    if n <= 1 then n
    else fibonacci(n - 1) + fibonacci(n - 2)

test "fibonacci base cases" =
    assert fibonacci(0) == 0
    assert fibonacci(1) == 1

test "fibonacci sequence" =
    assert fibonacci(5) == 5
    assert fibonacci(10) == 55
```

---

## 15.2 Test Organization

Dovetail supports two testing models: **unit tests** in source files and **integration tests** in a separate `test/` directory.

### Unit Tests in Source Files

Unit tests are placed in the same file as the code they test. This keeps tests close to implementation and allows tests to access private members:

```dovetail
package myapp.math

function square(x: Int32): Int32 = x * x

function cube(x: Int32): Int32 = x * x * x

// Tests for this module - can access private functions
test "square of positive" =
    assert square(3) == 9

test "square of negative" =
    assert square(-4) == 16

test "cube of positive" =
    assert cube(2) == 8
```

Unit tests have direct access to all functions in the file, including private ones.

### Integration Tests in test/ Directory

Integration tests live in a `test/` directory at the project root. These tests use `package test` and import symbols from the main packages:

```
project/
├── src/
│   └── math.dove       # Contains unit tests inline
└── test/
    ├── mathTest.dove   # Integration tests (package test)
    └── utils/
        └── helpersTest.dove  # package test.utils
```

Integration test files use `package test` (or `package test.<subpath>` for subdirectories) and import from the main package:

```dovetail
// test/mathTest.dove
package test

import myapp.math.square
import myapp.math.cube

test "integration square" =
    assert square(5) == 25

test "integration cube" =
    assert cube(3) == 27
```

Integration tests:
- Use `package test` as their package declaration (the `test` package prefix is reserved and cannot be used in `src/`)
- Must import functions and types from the main packages — can access **public and internal** (default visibility) symbols, but not private members
- Are discovered automatically when running `dovetail test`

### When to Use Each Model

**Use unit tests** (in `src/`) when:
- Testing internal implementation details
- Testing private helper functions
- Tests need access to module internals

**Use integration tests** (in `test/`) when:
- Testing the public API of your package
- Verifying behavior from an external perspective
- Tests should only use public and internal interfaces

### Testing Enums and Pattern Matching

Tests can verify enum behavior and pattern matching:

```dovetail
enum Result<T, E> =
    Ok(T)
    Error(E)

function divide(a: Int32, b: Int32): Result<Int32, String> =
    if b == 0 then
        Error("Division by zero")
    else
        Ok(a / b)

test "successful division" =
    match divide(10, 2) with
        case Ok(result) => assert result == 5
        case Error(_) => assert false

test "division by zero" =
    match divide(10, 0) with
        case Ok(_) => assert false
        case Error(msg) => assert msg == "Division by zero"
```

### Testing Classes

Tests work with classes just like any other type:

```dovetail
class Counter() =
    let mutable count: Int32 = 0

    public function increment(self): Unit =
        self.count = self.count + 1

    public function getCount(self): Int32 = self.count

test "counter increments" =
    let counter = Counter()
    assert counter.getCount() == 0
    counter.increment()
    assert counter.getCount() == 1
    counter.increment()
    counter.increment()
    assert counter.getCount() == 3
```

---

## 15.3 Test Attributes

Test attributes modify test behavior. They are placed before the `test` keyword using `@` syntax.

### @skip - Skipping Tests

Use `@skip` to temporarily disable a test:

```dovetail
@skip
test "work in progress" =
    assert newFeature() == expected
```

Provide a reason for skipping:

```dovetail
@skip("Waiting for API v2")
test "new api test" =
    assert callNewApi() == expected
```

Skipped tests appear in the output but don't run:

```
    ○ new api test (Waiting for API v2)
```

### @panics - Expecting Panics

Use `@panics` when a test should cause a panic:

```dovetail
function divideUnsafe(a: Int32, b: Int32): Int32 = a / b

@panics
test "division by zero panics" =
    let result = divideUnsafe(10, 0)
    assert result == 0  // Never reached
```

A test marked with `@panics`:
- **Passes** if the code panics
- **Fails** if the code completes without panicking

You can optionally specify an expected panic message substring:

```dovetail
@panics("out of bounds")
test "array access panics with message" =
    let arr = [|1, 2, 3|]
    let x = arr[100]  // Should panic with "out of bounds"
```

### @timeout - Test Timeouts

Use `@timeout` to limit how long a test can run (in milliseconds):

```dovetail
@timeout(1000)
test "completes quickly" =
    let result = fastOperation()
    assert result == expected
```

If the test takes longer than the timeout, it fails with a timeout diagnostic.

### Combining Attributes

Multiple attributes can be combined:

```dovetail
@timeout(5000)
@skip("Flaky on CI")
test "network test" =
    let response = fetchRemoteData()
    assert response.status == 200
```

---

## 15.4 Running Tests

### The dovetail test Command

Run all tests in your project:

```bash
dovetail test
```

Individual results are shown by default, grouped by project and source file.
There is no `test --verbose` flag. The summary reports passed, failed, and skipped
tests. A filter matching no tests exits successfully, so check the count in CI.

### Filtering Tests

Run only tests whose fully qualified test name (FQTN) contains a substring:

```bash
dovetail test --filter "addition"
```

The `--filter` (or `-f`) flag matches against the full FQTN (`<package_path> <test_name>`). Multiple `--filter` flags are OR'd — a test runs if it matches any filter:

```bash
dovetail test --filter "addition" --filter "subtraction"
```

### Filtering by File

Run only tests defined in a specific source file:

```bash
dovetail test --file src/math.dove
```

### Testing Specific Projects

In a multi-project workspace, run tests for a specific project using a positional argument:

```bash
dovetail test myapp
```

Filters can be combined with the project argument:

```bash
dovetail test myapp --filter "math" --file src/math.dove
```

### Exit Codes

The `dovetail test` command returns:
- **0** if all tests pass (or are skipped)
- **1** if any test fails
- **2** if there is a compilation error

This makes it easy to use in CI/CD pipelines:

```bash
dovetail test
```

### Test Output Format

Results use `✓` for passed tests, `✗` for failures, and `○` for skipped tests.
Failures include diagnostics and a final failure summary.

---

## 15.5 Testing Best Practices

### Write Descriptive Test Names

Good test names describe the expected behavior:

```dovetail
// Good
test "returns None when user not found" =
    assert findUser(999).isNone

// Less clear
test "test1" =
    assert findUser(999).isNone
```

### One Concept Per Test

Each test should verify one specific behavior:

```dovetail
// Good: separate tests for separate behaviors
test "empty list has zero length" =
    assert [].length == 0

test "single element list has length one" =
    assert [42].length == 1

// Avoid: multiple unrelated assertions
test "list operations" =
    assert [].length == 0
    assert [1].length == 1
    assert [1, 2].contains(1)
    assert [1, 2].sum() == 3
```

### Test Edge Cases

Include tests for boundary conditions and error cases:

```dovetail
function safeDivide(a: Int32, b: Int32): Option<Int32> =
    if b == 0 then None else Some(a / b)

test "divide positive numbers" =
    assert safeDivide(10, 2).require == 5

test "divide by zero returns None" =
    assert safeDivide(10, 0).isNone

test "divide with negative numbers" =
    assert safeDivide(-10, 2).require == -5

test "divide zero by nonzero" =
    assert safeDivide(0, 5).require == 0
```

### Use Helper Functions

Extract common setup into helper functions:

```dovetail
function createTestUser(name: String): User =
    User { name = name; age = 25; email = "$name@test.com" }

test "user greeting" =
    let user = createTestUser("Alice")
    assert user.greet() == "Hello, Alice!"

test "user email" =
    let user = createTestUser("Bob")
    assert user.email == "Bob@test.com"
```

### Keep Tests Fast

Tests should run quickly. Use `@timeout` to catch slow tests:

```dovetail
@timeout(100)
test "lookup is fast" =
    let result = cache.lookup("key")
    assert result == Some("value")
```

---

## Summary

- Tests are declared with `test "name" = body` and always return `Unit`
- Use `assert` to verify conditions
- Use `@skip` to disable tests temporarily
- Use `@panics` for tests that should panic
- Use `@timeout` to limit test duration
- Run tests with `dovetail test`
- Use `--filter` to run specific tests
- Use `--file` to run tests from a specific source file
- Write descriptive names and test one concept per test
