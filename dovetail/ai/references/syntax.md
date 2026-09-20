# Syntax and expressions

## Layout and bindings

Use spaces, normally four per level. Layout after `=`, `then`, `else`, `with`,
`do`, and `=>` defines blocks. Indent continuations. Explicit `begin ...; ... end`
also exists. `//` comments and `///` documentation are supported; doc examples
are not executed as tests. The final expression determines a block's value.

```dovetail
function larger(left: Int32, right: Int32): Int32 =
    if left > right then left else right

function countPositive(values: List<Int32>): Int32 =
    let mutable count = 0
    for value in values do
        if value > 0 then
            count = count + 1
    count
```

`let` bindings are immutable; `let mutable` enables reassignment. Shadowing creates
a new binding. `Unit` is `()`, `Never` has no values, and `Any` is the top type.
Integer types are `Int8/16/32/64` and `Uint8/16/32/64`; floats are `Float32/64`.
`Bool`, `Char`, and `String` are distinct. Integers support decimal, `0x`, `0b`,
`0o`, and suffixes such as `u8`/`i64`; floats support exponents and `f32`/`f64`.

`BigInt` (`123big`) and `Decimal` (`19.99dec`) are exact prelude types. Operands
must have matching types: `amount * 2dec`. Unsuffixed numbers do not become Decimal
through an annotation. No digit separators, decimal division/rounding API, or
BigInt/Decimal literal match patterns exist; use equality guards. Decimal normalizes
trailing zeros; `Decimal.of(123big, 2)` constructs 1.23. Invalid runtime scales panic.

## Operators and strings

Use `and`, `or`, `not` for short-circuit logic; `==`, `!=`, `<`, `<=`, `>`, `>=`
for comparison; `+ - * / %` for ordinary arithmetic. Bitwise operators are
`& | ^ ~ << >>`. `++` concatenates strings/sequences; `::` prepends a list element.

Precedence low to high: `or`, `and`, binary `~`, equality, comparison, `::`,
`|`, `^`, `&`, shifts, `+ - ++`, `* / %`, unary operators, postfix operations.
`::` is right-associative. Parenthesize uncertain expressions.

Strings interpolate `$name` and `${expression}`. Use `\$` for a literal dollar;
other escapes include `\n`, `\r`, `\t`, `\\`, `\"`, `\0`, and `\u{2764}`.
Characters use single quotes. Triple-quoted strings drop one initial newline
but do not strip indentation. Prefixed literals are covered in integrations.

## Control flow and patterns

```dovetail
function describe(values: List<Int32>): String =
    match values with
        case [] => "empty"
        case [only] => "one: $only"
        case first :: rest if first > 0 => "positive first"
        case _ => "other"
```

`if` without `else` returns Unit. Match branches must cover all possibilities;
guards do not generally prove exhaustive coverage. Patterns include literals,
enum payloads, tuples, records (`Point { x; y }`), lists, bindings, and `_`.
Arrays have no list-pattern syntax. Records use semicolon-separated fields.

`for item in iterable do` requires `Iterable<T>`; `while condition do` also
returns Unit. `break`/`continue` are supported in synchronous loops. `..` is
slice syntax, not a standalone numeric range; use the collection Range API.
Async loops have extra restrictions described in effects.

## Functions and closures

```dovetail
function applyTwice(operation: Int32 => Int32, value: Int32): Int32 =
    operation(operation(value))

let twice = (value: Int32) => value * 2
let add: (Int32, Int32) => Int32 = (left, right) => left + right
```

Functions use typed parameters and an optional return annotation; annotate public
contracts explicitly. Top-level declarations default to package visibility.
`public` exports; `private` top-level helpers are file-local.

Named arguments use `name = expression`; positional arguments must precede named
ones. Every parameter receives exactly one argument. Receiver evaluation precedes
eager argument evaluation, which follows written order, even for reordered names.
`ByName` arguments stay deferred. Accepted names belong to the declaration visible
at the call site, so public parameter renaming can break callers.

Stored function/method references take positional arguments only. Bound references
such as `point.getX` capture the receiver; `Point.getX` takes it explicitly.
An expected function type can resolve overloads and generic instantiations.
Closures can capture state; mutable captures are shared. Default parameter values
are not implemented: use a configuration record and factory for defaults.

## Checked example

<!-- book-example: {"name": "aisyntax", "depends": [], "stdout": ""} -->
```dovetail
package aisyntax

function positiveTotal(values: List<Int32>): Int32 =
    let mutable total = 0
    for value in values do
        if value > 0 then
            total = total + value
    total

function main(): Unit = assert positiveTotal([1, -2, 3]) == 4

test "positive values contribute" = assert positiveTotal([0, -1, 2]) == 2
```
