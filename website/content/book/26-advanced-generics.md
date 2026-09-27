# Part 26: Advanced Generics

This chapter covers generic library design, method-specific requirements, and
runtime type matching. Read [Generics](07-generics.md),
[Traits and Implementations](08-traits.md), and [Classes](09-classes.md) first.
The examples build on the basic type parameters and `where` clauses from Part 7.

## 26.1 Generic Extensions

You can write extensions that work with generic types. As with other extensions,
give the extension a name and import it before using its methods:

```dovetail
package example

import example.ArrayHelpers

extension ArrayHelpers<T> for Array<T> =
    function isEmpty(self): Bool =
        self.length == 0

    function first(self): Option<T> =
        if self.isEmpty() then
            None
        else
            Some(self[0])

function main(): Unit =
    let numbers = [|1, 2, 3|]
    assert numbers.isEmpty() == false
    assert numbers.first() == Some(1)
```

---

## 26.2 Class Constraints

Use `where T: class` when an operation needs class identity:

```dovetail
function sameInstance<T>(left: T, right: T): Bool where T: class =
    ClassIdentity.equals(left, right)

function instanceHash<T>(value: T): Int64 where T: class =
    ClassIdentity.hash(value)
```

The bound permits concrete and generic classes, aliases of classes, and generic parameters already constrained by `class`. A superclass bound such as `where T: Animal` also satisfies it. A `Box<Int32>` is a class even though its element type is not.

Records, enums, arrays, newtypes, primitives, `Any`, and interface objects do not satisfy `class`. The bound grants neither constructors nor arbitrary members, and it does not automatically implement `Equatable` or `Hashable`.

Combine it with trait bounds using `where T: class + Display`. An unconstrained generic function cannot call the identity functions just because its current callers happen to pass classes. See [Class Identity](09-classes.md#915-class-identity) for the equality and hash contracts.

A primitive subtype bound can restrict a method to a particular element type.
For example, `where T: Uint8` allows `ReadonlySlice<T>` to widen to
`ReadonlySlice<Uint8>`. It accepts `Uint8` and the uninhabited subtype `Never`;
it does not convert other integer types or imply the `class` constraint.
`Stream.writeToOutputStream` uses this bound to expose byte output directly on
its generic module.

---

## 26.3 Bounds on Enclosing Parameters

A method's `where` clause can constrain type parameters declared by its
enclosing module, class, implementation block, or extension block. The method
does not need to declare its own type parameters:

```dovetail
class Holder<T>(public value: T) =
    public function render(self): String where T: Display = self.value.format()
    public function unchanged(self): T = self.value
```

`render` requires `T: Display`; constructing a `Holder<T>` or calling `unchanged`
does not. The bound applies throughout the method's signature, body, and nested
closures, and does not become available to sibling methods.

Callers must satisfy these requirements when calling the method or taking a
method reference. Generic callers prove them using their own bounds:

```dovetail
function renderHolder<T>(holder: Holder<T>): String where T: Display = holder.render()
```

A method can use the class's type parameter together with a type parameter of
its own. For example, this box can search the value it holds:

```dovetail
trait Contains<Item> =
    function contains(self, item: Item): Bool

class SearchBox<T>(public value: T) =
    public function contains<U>(self, item: U): Bool where T: Contains<U> =
        self.value.contains(item)
```

Here, `T` is the type stored in the box. `U` is the type of item passed to
`contains`. Read `where T: Contains<U>` as: **the stored type must support
searching for this kind of item**.

For a concrete example, a range can contain integers:

```dovetail
record Range = start: Int32, end: Int32

implement Contains<Int32> for Range =
    function contains(self: Range, item: Int32): Bool =
        item >= self.start && item <= self.end

let box = SearchBox(Range { start = 1, end = 10 })
box.contains(5)       // T is Range, U is Int32: allowed; returns true
box.contains(true)    // U is Bool: rejected; Range does not implement Contains<Bool>
```

The same box keeps its stored type, `Range`, across calls. Each call chooses
the method's `U` from the item being searched for and checks the corresponding
`Contains<U>` requirement.

Use different names for the two parameters: declaring `contains<T>` here would
reuse the class's `T`, which the compiler rejects.

---

## 26.4 Trait and Override Contracts

An implementation must accept the calls that its trait promises to callers.
It cannot add an extra requirement to one of those methods. For example, the
compiler rejects this implementation:

```dovetail
trait Inspect =
    function inspect(self): Unit

record Box<T> = value: T

implement <T> Inspect for Box<T> =
    function inspect(self: Box<T>): Unit where T: Display = ()
```

The implementation says every `Box<T>` implements `Inspect`, but its method
would only work for some choices of `T`. A caller that knows only `Inspect`
has no way to satisfy that extra requirement.

Putting `where T: Display` on the **implementation block** instead restricts
which boxes implement `Inspect`. Every box that does implement the trait then
has the promised method.

The same rule applies to class overrides: a child method must accept the calls
allowed by its parent method. It cannot add an extra bound. A requirement
already guaranteed by the enclosing class or implementation block can be
repeated on the method.

An implementation also inherits the bounds declared on a generic trait method.
Renaming the method's type parameter, such as from `T` to `U`, does not remove
those requirements.

---

## 26.5 Variance and Method Bounds

Variance controls whether one use of a generic type can be assigned to another.
For example, every `Int32` value can be used as `Any`, but that does not by itself
mean a `Box<Int32>` can be used as `Box<Any>`.

### Plain Parameters, `out`, and `in`

With a plain parameter, such as `Box<T>`, different type arguments stay separate:
`Box<Int32>` cannot be assigned to `Box<Any>`. This is called **invariance**.

Use `out T` when the type provides values of `T`. A reader that produces integers
can be used where a reader of any values is expected:

```dovetail
class Reader<out T>(public value: T)

let integers = Reader(42)
let anyValues: Reader<Any> = integers
```

This is called **covariance**. The compiler checks that an `out` parameter is not
used where a caller could supply a value of that type, such as a method argument
or a mutable field.

Use `in T` when the type accepts values of `T`. Something that accepts any value
can also accept integers:

```dovetail
class Consumer<in T>() =
    public function accept(self, value: T): Unit = ()

let anyValues = Consumer<Any>()
let integers: Consumer<Int32> = anyValues
```

This is called **contravariance**. An `in` parameter cannot be used where the
caller receives a value of that type, such as a method return type. Start with
plain type parameters; add `in` or `out` when your API needs these conversions.

### Conditional Bounds on Virtual Methods

There is one restriction when combining these method bounds with `in` or `out`
on a class parameter. Consider this hypothetical declaration, which the compiler
rejects:

```dovetail
class Formatter<in T>() =
    public function render(self, value: T): String where T: Display = value.format()
```

Normally, `in T` lets you use a `Formatter<Any>` as a `Formatter<Int32>`: something
that accepts any value can also accept an integer. But that would cause a problem
here:

```dovetail
let original = Formatter<Any>()
let integers: Formatter<Int32> = original
integers.render(42)
```

The last call appears valid because `Int32` implements `Display`. However, the
object is still the original `Formatter<Any>`. Its `render` method is unavailable
because `Any` does not implement `Display`. Assigning the object to a differently
typed variable does not create a new object or a new implementation of its method.
A virtual call uses the method belonging to that original object.

To prevent this, use plain `T`, without `in` or `out`, when a virtual method adds
a requirement on `T`:

```dovetail
class Formatter<T>() =
    public function render(self, value: T): String where T: Display = value.format()
```

Plain `T` is called **invariant**. It prevents the conversion from
`Formatter<Any>` to `Formatter<Int32>` shown above. The same restriction applies
to parameters mentioned inside a method's bound: for `where T: Contains<U>`,
both class parameters `T` and `U` must be invariant if the class does not already
guarantee that requirement.

Alternatively, require `Display` on the whole class:

```dovetail
class Formatter<in T>() where T: Display =
    public function render(self, value: T): String = value.format()
```

Now every allowed `Formatter<T>` has a working `render` method. You cannot
construct `Formatter<Any>` in the first place, so the problematic conversion
cannot arise. The class-level bound does not impose the extra invariance rule;
the usual rules for `in` and `out` still apply.

---

## 26.6 Pattern Matching on Generic Types

You can use pattern matching to check types at runtime. This is particularly useful in generic functions where you need to handle different types differently.

### Type Matching in Generic Functions

The most common use case is determining the actual type in a generic function:

```dovetail
record Box<T> =
    value: T

function getBoxType<T>(box: T): Int32 =
    match box with
        case _: Box<Int32> => 1
        case _: Box<String> => 2
        case _ => 0

let intBox = Box { value = 42 }
let strBox = Box { value = "hello" }

getBoxType(intBox)   // returns 1
getBoxType(strBox)   // returns 2
```

The `_: Type` syntax checks if the value has the specified type without binding it to a variable.

### Distinguishing User-Defined Types

Type patterns work with non-generic types too, letting you distinguish between different user-defined types:

```dovetail
record Cat = name: String
record Dog = name: String

function getAnimalType<T>(animal: T): Int32 =
    match animal with
        case _: Cat => 1
        case _: Dog => 2
        case _ => 0

let cat = Cat { name = "Whiskers" }
let dog = Dog { name = "Rex" }

getAnimalType(cat)  // returns 1
getAnimalType(dog)  // returns 2
```

### Matching Type Arguments

Use the type name with explicit type arguments in a pattern:

```dovetail
record Box<T> =
    value: T

function describeBox(box: Box<String>): Int32 =
    match box with
        case _: Box<String> => 1
        case _ => 0

let b = Box { value = "hello" }
describeBox(b)  // returns 1
```

### Matching with Custom Types

You can match on generic types instantiated with user-defined types:

```dovetail
record Person = name: String

record Container<T> =
    item: T

function checkContainer<T>(c: T): Int32 =
    match c with
        case _: Container<Person> => 1
        case _ => 0
```

### Matching Multiple Type Arguments

For types with multiple type parameters, specify all type arguments. This allows distinguishing between different instantiations:

```dovetail
record Pair<A, B> =
    first: A
    second: B

function getPairType<T>(p: T): Int32 =
    match p with
        case _: Pair<Int32, String> => 1
        case _: Pair<String, Int32> => 2
        case _: Pair<String, String> => 3
        case _ => 0

let p1 = Pair { first = 42; second = "hello" }
let p2 = Pair { first = "hello"; second = 42 }
let p3 = Pair { first = "a"; second = "b" }

getPairType(p1)  // returns 1
getPairType(p2)  // returns 2
getPairType(p3)  // returns 3
```

### Generic Enums

Pattern matching works with generic enums too:

```dovetail
enum Maybe<T> =
    Just(T)
    Nothing

function checkMaybe<T>(m: T): Int32 =
    match m with
        case _: Maybe<String> => 1
        case _: Maybe<Int32> => 2
        case _ => 0
```

### Generic Classes

And with generic classes:

```dovetail
class Wrapper<T>(value: T) =
    function get(self): T = self.value

function checkWrapper<T>(w: T): Int32 =
    match w with
        case _: Wrapper<String> => 1
        case _: Wrapper<Int32> => 2
        case _ => 0
```

### Distinguishing Different Generic Types

You can distinguish between different generic types that have the same type argument:

```dovetail
record Box<T> =
    value: T

record Wrapper<T> =
    inner: T

function getType<T>(x: T): Int32 =
    match x with
        case _: Box<Int32> => 1
        case _: Wrapper<Int32> => 2
        case _ => 0

let b = Box { value = 42 }
let w = Wrapper { inner = 42 }

getType(b)  // returns 1
getType(w)  // returns 2
```

### Binding Values in Type Patterns

You can bind the matched value to a variable:

```dovetail
record Box<T> =
    value: T

function processBox<T>(x: T): Int32 =
    match x with
        case box: Box<Int32> => box.value
        case box: Box<String> => box.value.length
        case _ => 0

let intBox = Box { value = 42 }
let strBox = Box { value = "hello" }

processBox(intBox)   // returns 42
processBox(strBox)   // returns 5
```

### Wildcard Fallback

The wildcard pattern `_` catches any type that doesn't match the specific patterns:

```dovetail
record Box<T> =
    value: T

function boxCode<T>(x: T): Int32 =
    match x with
        case _: Box<Int32> => 1
        case _: Box<String> => 2
        case _ => 99              // Fallback for other types

let b = Box { value = true }
boxCode(b)  // returns 99 (Box<Bool> falls through to wildcard)
```

---

## 26.7 Generic Trait Defaults

A default is a method body supplied by the trait. The implementing type can use
it by leaving that method out of its implementation block.

In this example, `pick<T>` is a **generic method**: each call chooses `T`.
The first call chooses `Int32`; the second chooses `String`. `Reader` itself has
no type parameters.

```dovetail
package example

trait Picker =
    function pick<T>(self, value: T): T = value

record Reader = id: Int32

implement Picker for Reader

function main(): Unit =
    let reader = Reader { id = 0 }
    assert reader.pick(42) == 42
    assert reader.pick("text") == "text"
```

A generic class can use the same default. Here `Reader<T>` has a class parameter,
and `pick<T>` has a separate method parameter. Creating `Reader(1)` fixes the
class parameter to `Int32`; it does not prevent `reader.pick("text")` from
choosing `String` for the method parameter.

```dovetail
package example

trait Picker =
    function pick<T>(self, value: T): T = value

class Reader<T>(id: T) implements Picker =
    public function get(self: Reader<T>): T = self.id

function choose<P, T>(picker: P, value: T): T where P: Picker = picker.pick(value)

function main(): Unit =
    assert choose(Reader(1), "bound") == "bound"
    let reader = Reader(1)
    assert reader.pick("text") == "text"
    assert reader.pick(42) == 42
```

An explicit matching method takes precedence over the default. Defaults may
also be properties. Calls from a default to another instance member observe
subclass overrides, just like calls from other class methods.

A generic default body can use only the operations justified by its declared
bounds. For example, a method comparing two values with `==` needs
`where T: Equatable`. Implementations cannot add stricter requirements than the
trait declares. Generic methods still cannot be dispatched through interface
values; use a generic function with a trait bound instead.

## 26.8 Associated Outputs

An associated type lets each implementation choose a type. `Producer` below says
that `produce()` returns its implementation's `Output`. `NumberProducer` chooses
`Int32` in its implementation block.

The generic helper does not need to choose that output type. `P.Output` means
“the output chosen by the `Producer` implementation for `P`.”

```dovetail
package example

trait Producer =
    type Output
    function produce(self): Output

record NumberProducer = value: Int32

implement Producer for NumberProducer =
    type Output = Int32
    function produce(self): Int32 = self.value

function read<P>(producer: P): P.Output where P: Producer =
    producer.produce()

function forward<P>(producer: P): P.Output where P: Producer =
    read(producer)

function main(): Unit =
    assert forward(NumberProducer { value = 42 }) == 42
```

For `NumberProducer`, both `read` and `forward` return `Int32`. Another producer
could choose `String`, and the same helpers would return `String` for it.

Inside `read`, the output is still unknown. It can be returned or passed to
another function expecting `P.Output`. It cannot be treated as `Int32` or used
in arithmetic merely because one implementation returns a number.

When a helper specifically needs a numeric output, state that requirement:

```dovetail
function readNumber<P>(producer: P): Int32
    where P: Producer<Output = Int32> =
    producer.produce()
```

Associated names are resolved through the parameter's trait bounds. If two
bounds give `P` different members named `Output`, `P.Output` is ambiguous.
Associated-type declarations cannot provide defaults: each implementation must
supply its associated definitions.

## 26.9 Generic Associated Types

Sometimes the implementation chooses a container, while each method call chooses
what goes inside it. That is the purpose of `type Wrapped<T>`.

Read the following example in three steps:

1. `Wrapper` requires a generic method `wrap<T>` and an associated type `Wrapped<T>`.
2. `OptionalWrapper` chooses `Option<T>`; `ArrayWrapper` chooses `Array<T>`.
3. `wrapValue` returns the container chosen by its `wrapper` argument, containing
   the type of its `value` argument.

```dovetail
package example

trait Wrapper =
    type Wrapped<T>
    function wrap<T>(self, value: T): Wrapped<T>

record OptionalWrapper = name: String
record ArrayWrapper = name: String

implement Wrapper for OptionalWrapper =
    type Wrapped<T> = Option<T>
    function wrap<T>(self, value: T): Option<T> = Some(value)

implement Wrapper for ArrayWrapper =
    type Wrapped<T> = Array<T>
    function wrap<T>(self, value: T): Array<T> = [|value|]

function wrapValue<W, T>(wrapper: W, value: T): W.Wrapped<T>
    where W: Wrapper =
    wrapper.wrap(value)

function main(): Unit =
    let optional = OptionalWrapper { name = "optional" }
    let array = ArrayWrapper { name = "array" }
    let numberOption: Option<Int32> = wrapValue(optional, 42)
    let numberArray: Array<Int32> = wrapValue(array, 42)
    let textOption: Option<String> = wrapValue(optional, "hello")
    let textArray: Array<String> = wrapValue(array, "hello")
    assert numberOption.require == 42
    assert numberArray == [|42|]
    assert textOption.require == "hello"
    assert textArray == [|"hello"|]
```

`W.Wrapped<T>` names the result without forcing every implementation to use the
same container. In the first call, `W` is `OptionalWrapper` and `T` is `Int32`,
so the result is `Option<Int32>`. In the second, `W` is `ArrayWrapper`, so the
result is `Array<Int32>`.

The associated definition does not have to use its parameter. An implementation
could choose `type Wrapped<T> = Int32` for every `T`. Therefore, knowing the
result type does not necessarily tell the compiler the input type. Supply type
arguments when the arguments to the call do not provide enough information.

### Generic Resource Helpers

`Usable<T, E>` uses an associated type `Wrapped<U, E2>` for the result of its
continuation. A helper accepting an unknown resource must preserve that result
contract. Its caller can provide a `finish` function returning the resource's
wrapper:

```dovetail
package example
newtype Holder = Int32
implement Usable<Int32, Never> for Holder =
    type Wrapped<U, E2> = U
    function use<U, E2>(self, f: (Int32) => U, errorF: (Never) => E2): U = f(self.value)
function scoped<R>(resource: R, finish: (Int32) => R.Wrapped<Int32, Never>): R.Wrapped<Int32, Never>
    where R: Usable<Int32, Never> =
    let value = use resource
    finish(value)
function main(): Unit = assert scoped(Holder(40), value => value + 2) == 42
```

`Holder` chooses `Wrapped<U, E2> = U`, so its continuation returns a plain
`Int32`. The generic `scoped` function cannot assume that choice for every
resource. Its `finish` parameter and return type both say
`R.Wrapped<Int32, Never>` to preserve the contract.

## 26.10 Inherited Overloads

A child trait can add a method whose name is already used by a parent when the
parameter lists differ. The implementation supplies both methods. The argument
type determines which one a call selects:

```dovetail
package example

trait TextValue =
    function value(self, input: String): Int32

trait Value extends TextValue =
    function value(self, input: Int32): Int32

record Reader = id: Int32

implement Value for Reader =
    function value(self, input: String): Int32 = 10
    function value(self, input: Int32): Int32 = input + 1

function read<R>(reader: R): Int32 where R: Value =
    reader.value("text") + reader.value(20)

function main(): Unit =
    let reader = Reader { id = 0 }
    assert reader.value("text") == 10
    assert reader.value(20) == 21
    assert read(reader) == 31
```

Here `reader.value("text")` selects the inherited `String` method, while
`reader.value(20)` selects the `Int32` method. This also works inside `read`,
where only the `R: Value` bound is known.

The overloads can supply separate default bodies. Eligible interface methods
retain separate dispatch slots, including after conversion to a parent
interface. An implementation missing either required overload is incomplete.
Calls matching more than one overload remain ambiguous; changing only the
return type does not create a valid overload.

---

## Further Reading

[Part 25: Tuple Extension (Advanced)](25-tuple-extension.md) applies generic
bounds to tuple shapes and recursive trait implementations. Its
[enclosing-parameter example](25-tuple-extension.md#256-enclosing-parameter-bounds)
shows how a method can require a tuple without restricting the whole type.
