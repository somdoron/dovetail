# Generics: preserve the declared contract

Read [Generics](https://dovetaillang.org/book/generics.md), [Advanced generics](https://dovetaillang.org/book/advanced-generics.md), [Tuple extension](https://dovetaillang.org/book/tuple-extension.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

- Every operation in a generic body must follow declared bounds. Do not add a
  stronger bound to an override or implementation than its contract permits.
- Method bounds can constrain enclosing parameters without redeclaring them; they
  do not constrain sibling methods or construction. Avoid parameter shadowing.
- Plain parameters are invariant. Choose `out`/`in` based on the API's actual
  production/consumption needs, and preserve restrictions on virtual dispatch.
- Associated outputs belong to the implementation, not the caller. Preserve bounds
  on associated types and projections; interfaces cannot expose them.
- Binary `~` appends one element and grows a tuple on the left. Reconstructing a
  nested pair with `.init ~ .last` may flatten it; ordinary pair syntax preserves it.
- `Tuple` is a structural bound for tuples of at least two elements. Recursive
  implementation heads and reverse inference support specific tuple-constrained
  forms; consult the chapter before inventing an unconstrained symbolic head.
