# Slice Design

`Slice<T>` is a prelude type representing a writable view of a contiguous array region. It replaces the parser's heap-allocated slice record and the array/offset arguments in the migrated IO, WASI, time, TLS, and collection APIs.

## Type and representation

```dovetail
public newtype Slice<T> private = (Array<T>, Int32, Int32)
// backing array, start, length
```

Construction establishes `0 <= start`, `0 <= length`, and `length <= data.length - start`, after checking `start <= data.length`. Subtraction-based checks avoid signed overflow. Re-slicing checks bounds against the current view; element access checks `0 <= index < length`. Violations panic with a message.

Slices share their backing array. Writes through an array or overlapping view are immediately visible through every alias. Copying a slice copies the view, not its elements. The private inner type keeps the constructor and backing-array accessor out of the public API. As with other transparent newtypes, explicit dynamic casts can recover a slice from a compatible tuple representation; recovery validates its bounds and traps on invalid views. Dynamic `is` tests and typed patterns reject malformed views without trapping.

The existing transparent-newtype and tuple-flattening machinery represents a slice as three WASM values in locals, direct-call parameters and returns, fields declared as `Slice<T>` in records and classes, concrete enum payloads, and immutable closure captures. Mutable class fields store and update the three components directly. Single-slot boundaries—including `Any`, erased generic positions such as `Option<Slice<T>>`, array elements, globals, mutable closure captures, and indirect-call arguments/results—box the view. Containers and closures still allocate their own objects. Parser results store their `remaining: Slice<Char>` directly in the result struct; parser closure calls box slice arguments. Iteration allocates a stateful iterator.

Ordinary slice operations reuse existing representation lowering. Spliced field reads restore concrete leaf types after erasure, including the array reference inside a generic slice field. Recovery from boxed storage validates slice bounds, including slices nested in flattened tuples. The WASI write emitter recognizes the private triple to marshal its bounded contents into linear memory.

## API

```dovetail
module Slice<T> =
    public function make(data: Array<T>, start: Int32, length: Int32): Slice<T>
    public function full(data: Array<T>): Slice<T>
    public property length(self): Int32
    public function isEmpty(self): Bool
    public function get(self, index: Int32): T
    public function set(self, index: Int32, value: T): Unit
    public function slice(self, start: Int32, end: Int32): Slice<T>
    public function sliceInclusive(self, start: Int32, end: Int32): Slice<T>
    public function drop(self, count: Int32): Slice<T>
    public function take(self, count: Int32): Slice<T>
    public function toArray(self): Array<T>
    public function copyTo(self, dest: Slice<T>): Unit
```

`slice` uses a half-open interval; `make` uses a start and length. There is no length-based overload of `slice`. `sliceInclusive` validates the end before incrementing it, preventing overflow in the inclusive syntax.

`copyTo` requires sufficient destination length and uses `Array.copy`, including its overlap-safe behavior. `toArray` produces independent storage and handles empty views without an element default.

`Iterable<T>` supplies `for x in slice`. `Equatable` compares elements, including across different arrays; equality operators honor explicit newtype trait implementations. `Display` preserves the parser's existing format (`[1,2,]` and `[]`) using only prelude facilities.

## Syntax

Slicing uses the array delimiters `[|` and `|]`. Element indexing continues to use `[` and `]`.

| Expression | Meaning |
|---|---|
| `a[\|i..j\|]` | Half-open view from `i` to `j` |
| `a[\|i..\|]` | View from `i` through the end |
| `a[\|..j\|]` | View from zero to `j` |
| `a[\|..\|]` | Whole view |
| `a[\|i..=j\|]` | View including element `j` |
| `a[\|..=j\|]` | View from zero through element `j` |
| `s[i]` | Checked element access |
| `s[i] = value` | Checked write through the view |

Every range form works on `Array<T>`, `Slice<T>`, and `ReadonlySlice<T>`. Ranges on a read-only view remain read-only. Bounds are `Int32`. Empty half-open intervals are valid, including an empty view at the array's end. Inclusive ends must designate an existing element.

The lexer recognizes `..` and `..=` independently of the existing `.` token. The decimal lexer only consumes a dot followed by a digit, so `1..3` is unambiguous. The parser adds `Expr::SliceIndex` with optional start/end, an inclusive flag, and a source span. Ranges are accepted only in postfix `[| … |]`.

Range inference recognizes `standard.prelude.Slice` and `standard.prelude.ReadonlySlice` by fully qualified identity. Element reads and writes on slices resolve through the indexing traits below. Arrays first lower to `Slice.full`; the resulting view lowers to `slice`, `sliceInclusive`, `drop`, or `take`. A full slice of a slice is the view itself. Each receiver and explicit bound appears once in the call tree and is evaluated in source order. Omitted bounds are handled inside methods, so no expression is duplicated.

`a[|i|]`, `a[i..j]`, an omitted inclusive end, and slice-range assignment are errors. Array and list literals retain their existing syntax. String element indexing retains its existing meaning; String slicing is unsupported. Convert explicitly with `Slice.full(text.chars())`, paying for the character-array copy once.

## Library migration and WASI boundary

The old `standard.parser.Slice` record is removed. Parser inputs and remaining views use the prelude type; string conversion is explicit. Public signature replacements are direct, with repository callers migrated together and no deprecated adapters.

`Async.streamWrite`, `WriteStream`, and `ComponentStreamWrite.start` carry `Slice<Uint8>`. Retry loops advance the pending view with `remaining[|accepted..|]`. Total-write counters remain where cancellation and framing contracts need them. `CopyOperation` remains the packed host operation token; it never held an array offset.

TCP, stdout/stderr, filesystem write, and append intrinsics accept slices. The compiler evaluates the stream and slice once, extracts the flattened triple internally, then allocates and copies exactly `length` bytes beginning at `start`. No public parts accessor is exposed. Host ABI, pinned-buffer lifetime, cancellation, and completion handling retain their existing contracts.

Time byte readers and TLS handshake-header inspection use slice-relative indexing. Incomplete TLS headers still return `None`. Map entry filling accepts a destination slice and returns the number of entries written relative to that view.

## Validation and exclusions

Tests cover range forms, nested and empty slices, checked mutation, aliasing, overlap-safe copying, iteration, equality, display, private representation, evaluation order, overflow, and flattened/boxed storage boundaries. Host write tests use interior slices with sentinel bytes outside both bounds. Existing partial-write and cancellation suites exercise the migrated IO paths.

First-class range values, direct String slicing, a byte alias, growth operations, and changes to Array's representation remain out of scope.

Related designs: [tuple lowering](tuple-multivalue-codegen-design.md), [newtypes](newtypes-design.md), [arrays](arrays-design.md), and [tuples](tuples-design.md).

## Read-only views and indexing capabilities

```dovetail
public type ReadonlySlice<out T> = intrinsic

public trait Index<K> =
    type Output
    function get(self: Self, index: K): Output

public trait IndexSet<K> =
    type Value
    function set(self: Self, index: K, value: Value): Unit
```

`Slice<T>` implements `Index<Int32, Output = T>` and `IndexSet<Int32, Value = T>`. `ReadonlySlice<T>` implements only the read capability. The traits are independent: a user type can provide write-only access or use non-integer keys. Ordinary methods named `get` or `set` alone do not enable indexing. Existing array/string element-index intrinsics remain built in; array and string types do not gain these trait implementations in this change.

For trait-based access, the compiler selects an implementation by receiver and key compatibility before checking the assigned value or expected result. Keys use ordinary parameter assignability, including bottom-typed expressions, `Any`, interface conversions, and subclasses; this does not change the invariance of generic trait bounds. Multiple compatible key applications remain ambiguous. Generic bounds specify the associated type explicitly. Receiver, key, and assigned value evaluate once in source order. Trait implementations delegate to the public module methods, preserving existing `.get` and `.set` calls. Range syntax remains separate from these element-access traits.

`array.readonly` returns a whole-array view; `slice.readonly` preserves its window; `readonlySlice.readonly` returns itself. These operations share backing storage without copying elements. Public `ReadonlySlice.make` and `full` use the same checked bounds as mutable slices. The intrinsic type is covariant; mutable `Array<T>` and `Slice<T>` remain invariant. Its compiler-owned representation is three flattened values: backing array reference, offset, and length. Construction, widening, and reslicing allocate no view object or reader closure. Existing single-slot boundaries box the three-field tuple. Dynamic recovery validates backing storage and bounds; `is` and typed patterns reject malformed views without trapping.

Each primitive has a distinct WASM array type in the explicit recursive type group, even when storage layouts match. Reference elements share one array type, and strings share Uint8 backing storage. Known primitive reads cast to that array type and read a native value without boxing. Reads through `ReadonlySlice<Any>` dispatch on the backing array and use the original primitive's existing box; Uint128 elements are already boxed. Compatible copies use overlap-safe `array.copy`; copying widened primitive storage into an Any array converts elements while copying.

`ReadonlySlice` exposes `length`, `isEmpty`, `get`, `slice`, `sliceInclusive`, `drop`, `take`, `toArray`, `copyTo`, and `map`. Subviews preserve the read-only type. `toArray` always produces an independent mutable copy; `copyTo` writes to a mutable `Slice`, including overlapping views. `map` evaluates its callback once per element in order and returns fresh backing storage as a read-only view. Iteration, display, and element-wise equality follow `Slice`.

Read-only access is shallow: existing mutable aliases can still change backing elements, and element objects may themselves be mutable. There is no public backing-array accessor or indexed assignment through a `ReadonlySlice`. This is an access capability, not a freezing or ownership mechanism; explicit dynamic casts can recover compatible three-field views, but the module API does not expose the intrinsic representation.


Output byte transports accept `ReadonlySlice<Uint8>` through
`AsyncOutputStream.write`, its subclass hook, `Async.streamWrite`, and the
WASI write-start intrinsics. Array and writable-slice callers use `.readonly`.
The host boundary casts the opaque backing reference to the Uint8 array type
before copying the selected window into pinned memory. Empty-write health
checks, partial-write accounting, and cancellation semantics are unchanged.
