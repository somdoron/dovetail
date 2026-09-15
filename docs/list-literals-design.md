# List literals, `::`, and the Array/List split

## What changed

- `[| 1, 2, 3 |]` is the array literal; `[||]` is the empty array.
- `[1, 2, 3]` is the list literal; `[]` is the empty list.
- `::` prepends to a list, in expressions and in patterns.
- `[]`, `[a, b]` and `h :: t` work as patterns.
- `List` is the default sequence across the standard library; `Array` is kept
  for byte buffers, indexed or mutable storage, fixed-size data, and the WASI
  boundary.

Giving `[ ]` to the list follows every language that has both — Haskell, OCaml,
F#, Scala, Erlang. `[| |]` for the array is F#/OCaml's spelling.

## Why `::` and `[…]` share one AST node

`Expr::ListLiteral { elements, tail }` covers both forms: `[a, b]` is
`{[a, b], None}`, `h :: t` is `{[h], Some(t)}`, and `a :: b :: []` *flattens*
to `{[a, b], None}` — byte-identical to what `[a, b]` produces.

The obvious alternative — desugar `h :: t` in the parser straight to
`List.Cons(h, t)` — is wrong, and not subtly. Enum construction binds a type
parameter from the first payload that mentions it and then checks the rest
against that binding:

- `TypeParamSubstitution::unify` inserts a binding the first time it sees the
  type variable and afterwards demands exact equality
  (`infer/type_param_substitution.rs`).
- `infer_enum_variant_call` discards `unify`'s result and calls
  `check_assignable` on the substituted payload types
  (`infer/function_expressions.rs`).

So for `Cons(T, List<T>)`, `T` binds to `typeof(head)` and the tail must then be
exactly `List<typeof(head)>`. Given `Dog <: Animal`:

```dovetail
let animals: List<Animal> = [Animal("generic")]
let all = Dog("rex") :: animals      // rejected under a parser-level desugar
let ok  = [Dog("rex"), Animal("g")]  // accepted — the literal joins elements
```

That asymmetry is not shippable: `x :: xs` is *the* idiomatic prepend. Keeping
both forms in one node lets `infer_list_literal` run the same
lowest-common-type join the array literal has always used, over the elements
and the tail's element type together.

## Why patterns desugar in the parser instead

Patterns cannot dispatch through a trait or run an inference join — a pattern
has to name a concrete constructor. So `[]`, `[a, b]` and `h :: t` are rewritten
in the parser into ordinary `Nil` / nested `Cons` variant patterns, using the
*bare* (empty `type_name`) form so they resolve against the scrutinee's type and
cannot be shadowed by a user-defined `List`.

Everything downstream — inference, exhaustiveness, codegen — therefore never
learns that list patterns exist. In particular exhaustiveness needed no
list-specific rule: `case [] / case h :: t` is `Nil`/`Cons`, which it already
knew how to complete.

The cost is that a diagnostic about a list pattern names the variant rather than
the sugar. Witness *rendering* compensates: `render_witness` prints an uncovered
`List` value in list notation, so the missing case for `case [] / case [a, b]`
reads `[_]`, not `Cons(_, Nil)`.

## `[]` needs no annotation; `[||]` does

`Array<T>` is invariant, so an empty array literal has nothing to infer its
element type from and still errors without an annotation.

`List<out T>` is covariant, so `[]` infers `List<Never>`, which is assignable to
every `List<T>` — exactly how `None : Option<Never>` already works. `[]` is
therefore accepted bare, and `expected_type` only sharpens the displayed type.

## Zero codegen changes

`List` is an ordinary prelude enum. Literals fold into `EnumCreate` chains and
patterns into nested variant patterns, both of which codegen already handled in
full generality.

## A compiler bug this surfaced

`for` desugars to `while … match next() … case None => break`. When the body
awaits, `desugar_await` lowers the loop into `Async.whileLoop(cond, body)` and
lifts the body into a closure — orphaning that generated `break`, which aborted
codegen with "break outside loop". No code in the repo had ever written `for`
with an `await` in the body (75 `while`+`await` sites, zero `for`+`await`), so
the bug was latent until this migration wrote one.

`desugar_for` now ends the loop by clearing a flag in its condition rather than
by `break`. A user-written `break` inside an awaiting loop body remains
unsupported and would hit the same assertion.
