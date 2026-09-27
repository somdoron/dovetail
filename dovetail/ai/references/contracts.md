# Contracts: choosing and preserving guarantees

Read [Traits](https://dovetaillang.org/book/traits.md), [Classes](https://dovetaillang.org/book/classes.md), [Interfaces](https://dovetaillang.org/book/type-system.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

- A trait is a generic bound, not a runtime value type. Use an interface for
  runtime-selected implementations, or an enum for a closed set of alternatives.
- Interfaces require receiver methods and prohibit method generics and associated
  types. Consult the book for the precise restrictions on `Self` and inheritance.
- Implementations must obey package ownership and non-overlap rules. Importing an
  unrelated implementation cannot satisfy missing parent obligations.
- An override/implementation cannot strengthen promised preconditions. Preserve
  signature bounds and statically visible argument names.
- Qualify an ambiguous member with its trait/extension rather than relying on imports.
- Structural `==` is not automatic for records/enums. Equality and hashing must agree;
  private construction/inspection restrictions also apply to generated derives.
- Class identity is stable across mutation. Do not combine structural equality with
  identity hashing. A `class` bound grants identity operations, not constructors or
  an automatic Equatable/Hashable implementation.
