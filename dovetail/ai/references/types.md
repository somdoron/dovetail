# Types: boundaries and aliases

Read [Type system](https://dovetaillang.org/book/type-system.md), [Standard library](https://dovetaillang.org/book/stdlib.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

- A type alias does not establish nominal identity; a newtype does.
- Private construction restricts construction, not validation. Implement a validated
  factory and expose meaningful queries. Private records/enums still support public
  observation; private newtypes also restrict wrapped-value inspection.
- `with` updates require construction access. Implementations, extensions, derives,
  and same-package callers gain no special access to private constructors.
- Records and read-only slices can still refer to mutable arrays/classes. A shallow
  update or read-only view is not an independent snapshot.
- Slices share storage and have exclusive end bounds. Invalid indexing/bounds panic;
  validate untrusted indices. Strings use UTF-8, so byte and character offsets differ.
- Import named extensions explicitly, even within their own package. Query the actual
  collection API; similar names do not imply identical folds or conversions.
- `as` panics on failure; there is no `as?`. Type tests/casts are not numeric
  conversions. Do not assume Any/interface recovery supports every target type.

## Minimal checked construction example

<!-- book-example: {"name": "aitypes", "depends": [], "stdout": ""} -->
```dovetail
package aitypes

newtype Quantity private = Int32

module Quantity =
    public function make(value: Int32): Result<Quantity, String> =
        if value > 0 then Ok(Quantity(value)) else Error("quantity must be positive")

    public function read(self): Int32 = self.value

function main(): Unit = assert Quantity.make(2).require.read() == 2

test "invalid quantity is rejected" = assert Quantity.make(0).isError
```
