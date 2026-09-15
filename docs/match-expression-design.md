# Match Expression Design (First Iteration)

This document designs the **basic** match expression for Dovetail: literals, variable and wildcard patterns, guard conditions, and exhaustiveness checking. It is the first iteration; the design is kept forward-compatible with enums, generics, records, and tuples.

---

## 1. Overview and goals

- **Match expression:** `match scrutinee with` followed by a list of `case pattern [if guard] => body` arms. The expression evaluates the scrutinee and selects the first arm whose pattern matches and whose guard (if present) is true; the body of that arm is the value of the match.
- **First iteration scope:** Only **literal** patterns, **variable** patterns, and **wildcard** (`_`) patterns, plus optional **guard** conditions. No constructor, tuple, or record patterns yet.
- **Exhaustiveness:** The typechecker (Rules phase) must check that the match is exhaustive when the scrutinee type is known and finite (e.g. boolean, or future enums). For types that are effectively infinite (e.g. `Int32`), exhaustiveness is satisfied by a catch-all: either a wildcard or a variable pattern (possibly with a guard that is not provably always true only for a subset).
- **Future-proofing:** AST and typechecker design should accommodate later addition of constructor patterns (enums), type-aware matching (generics), record patterns, and tuple patterns without breaking this basic version.

**Implementation status:** Done (literals, variable, wildcard, guards, exhaustiveness).

---

## 2. Syntax (aligned with grammar)

From [grammar.md](grammar.md):

```
match_expr          = "match" expression "with" match_body
match_body          = BEGIN { match_arm SEP } match_arm [ SEP ] END
match_arm           = "case" pattern [ "if" expression ] "=>" block_expr
```

For the **first iteration**, the only patterns we implement and typecheck are:

| Pattern kind   | Grammar            | Example   | Description                          |
|----------------|--------------------|-----------|--------------------------------------|
| Literal        | `literal_pattern`  | `0`, `1`, `"ok"`, `true` | Matches that value only.         |
| Variable       | `ident_pattern`    | `x`, `n`  | Binds scrutinee to name; always matches. |
| Wildcard       | `wildcard_pattern` | `_`       | Matches anything; does not bind.     |

Constructor, tuple, and record patterns are in the grammar but **out of scope** for this iteration; they can be rejected in the typechecker with a clear “not yet supported” error if we prefer, or left for a later phase.

---

## 3. Examples

### 3.1 Literals and wildcard

```dovetail
match x with
    case 0 => "zero"
    case 1 => "single"
    case _ => "multiple"
```

- Scrutinee `x` is matched in order: first `0`, then `1`, then any other value with `_`.
- The wildcard arm makes the match exhaustive for any type (we do not require the compiler to enumerate all integer literals).

### 3.2 Variable pattern

```dovetail
match x with
    case 0 => "zero"
    case 1 => "single"
    case x2 => x2.toString()
```

- `x2` binds the scrutinee value in the last arm; that arm matches any value not matched by the literal arms.
- Variable pattern also acts as catch-all for exhaustiveness.

### 3.3 Guard condition (`if` guard)

```dovetail
match x with
    case 0 => "zero"
    case n if n < 0 => "negative"
    case n if n < 10 => "small"
    case _ => "large"
```

- After a pattern matches, the optional `if expression` is evaluated; the arm is chosen only if the guard is true. Otherwise matching continues to the next arm.
- Guards are boolean expressions and can refer to variables bound by the pattern (e.g. `n`).

### 3.4 Typo note

In examples, use `case` (not `caee`). The grammar and lexer require the `case` keyword.

---

## 4. Semantics

- **Evaluation order:** Evaluate scrutinee once; then consider arms in order. For each arm: try to match the pattern; if it matches, evaluate the guard (if any); if there is no guard or the guard is true, evaluate the body and that is the result of the match. If the guard is false, continue to the next arm.
- **Literal pattern:** The literal’s type must match the scrutinee type (e.g. we cannot match a string literal to an `Int32` scrutinee—that is a typecheck error). At runtime, match succeeds iff the scrutinee value equals the literal (same as equality for the type).
- **Variable pattern:** Always matches and binds the scrutinee to that name in the body (and in the guard, if present). Shadowing of outer names is allowed.
- **Wildcard pattern:** Always matches; no binding. Used when the value is not needed in the body.
- **Scope:** In `case pat if guard => body`, the variable bound by `pat` is in scope in both `guard` and `body`.

---

## 5. Typechecker

### 5.1 Inference (phase 2)

- **Scrutinee:** The scrutinee expression is inferred as usual; its type is the “match type” `T`.
- **Arms:** Each arm’s pattern is **typechecked against the scrutinee type** `T`. We cannot match a literal of one type against a scrutinee of another type (e.g. string literal with `Int32` scrutinee is a type error).
  - **Literal:** The literal has a fixed type (e.g. `42` is `Int32`, `"hi"` is `String`, `true` is `Bool`). That type must be **equal to or compatible with** `T`. If the literal’s type is not compatible with `T`, report an error (e.g. “literal pattern type String does not match scrutinee type Int32”).
  - **Variable:** Introduces a binding of type `T` in the arm’s scope.
  - **Wildcard:** No type constraint beyond `T`.
- **Guard:** Must have type `Bool`.
- **Body:** Body type is inferred; all arms must have the same body type (or we unify to a common type) so that the match expression has a well-defined type.

**Example of invalid match (arm type error):**

```dovetail
let x: Int32 = 3
match x with
    case 0    => "zero"   // ok: Int32 literal
    case "one" => "one"   // error: String literal cannot match Int32 scrutinee
    case _    => "other"
```

### 5.2 Rules (phase 3): exhaustiveness

We add an **exhaustiveness** rule in the Rules phase. The rule ensures that the set of arms covers all possible values of the scrutinee type, so that the match never “falls through” without choosing an arm.

**When is a match exhaustive?**

- For **infinite or unbounded** types (e.g. `Int32`, `String`): having a **catch-all** arm makes the match exhaustive. A catch-all is either:
  - a **wildcard** pattern (`_`), or
  - a **variable** pattern (which matches any value).
- For **finite** types (e.g. `Bool`, or in the future a fixed set of enum variants): we can either require a catch-all or require that every value is covered by some arm. For `Bool`, that means either two arms (e.g. `case true` and `case false`) or one literal arm plus a catch-all.

**Algorithm (first iteration):**

1. **If scrutinee type is finite (e.g. `Bool`):**
   - Enumerate the values (e.g. `true`, `false`).
   - For each value, check that some arm matches it. An arm matches a value if: (a) the pattern is a literal equal to that value, or (b) the pattern is variable or wildcard (and optionally a guard might filter; for exhaustiveness we consider that variable/wildcard “covers” that value if the guard is not statically false for it).
   - If a value has no covering arm, report “non-exhaustive match: value … not covered”.
2. **If scrutinee type is not finite (e.g. `Int32`):**
   - Exhaustiveness is satisfied if and only if there is at least one catch-all arm (wildcard or variable) that can be reached. “Reached” means: not preceded by a sequence of literal arms that already cover all values of the type (which we don’t have for Int32), so in practice we only require one catch-all arm at the end. If there is no catch-all, report “non-exhaustive match: possible values not covered (add a wildcard or variable arm)”.
3. **Guards:** A variable or wildcard arm with a guard does not by itself guarantee exhaustiveness for infinite types, because the guard might be false for some values. So for the first iteration we can either: (a) treat “variable with guard” as catch-all for exhaustiveness (conservative: we assume the guard can be true for remaining values), or (b) not treat it as catch-all and require a final arm without guard. Option (a) is simpler and matches common practice (e.g. “case n if n < 0 => …” then “case _ => …”); we can document that a final variable/wildcard arm (with or without guard) is considered catch-all for exhaustiveness of infinite types.

**Recommendation:** For the first iteration, consider a match exhaustive for an infinite type if there exists at least one arm whose pattern is variable or wildcard (whether or not it has a guard). That avoids forcing users to add a redundant `case _ => ...` when they already have `case n if n >= 0 => ...`. For finite types, we still require every value to be covered (by literal or by catch-all).

---

## 6. Exhaustiveness: research and evolution

- **Generic algorithm:** Exhaustiveness can be modelled by viewing types and patterns as “spaces” of values; one checks whether the union of pattern spaces covers the type space. This generalises to enums (constructor spaces), GADTs, and guards (see e.g. Liu’s “A Generic Algorithm for Checking Exhaustivity of Pattern Matching”, EPFL; Rust’s “usefulness” algorithm in the rustc guide).
- **Guards:** With guards, a branch can be “taken” only when the guard is true, so exhaustiveness becomes “for every value, some arm both matches and has a true guard”. For the first iteration we keep the rule simple: variable/wildcard counts as covering remaining values even with a guard; we can tighten later with a proper usefulness/space algorithm.
- **Placement in pipeline:** Exhaustiveness is a **semantic** rule that depends on the scrutinee type. It belongs in the **Rules** phase (phase 3) of the typechecker, after Inference has attached types. It does not require codegen changes.

---

## 7. Future extensions (out of scope for first iteration)

The following are explicitly **not** implemented in the first iteration but should be kept in mind so that we don’t block them.

- **Enums:** Constructor patterns (e.g. `case Some(x) =>`, `case None =>`). Exhaustiveness will require covering every variant; the same “space” or “usefulness” approach applies.
- **Generics / type-info:** Matching on type parameters or type-level information may require GADT-style constraints. The AST and exhaustiveness rule should avoid hard-coding only value patterns.
- **Records:** Record patterns (e.g. `case Point { x, y } =>`) and possibly record field guards. Exhaustiveness for records is usually trivial (one constructor) unless we add “open” records.
- **Tuples:** Tuple patterns (e.g. `case (0, _) =>`). Exhaustiveness for product types is a product of per-component coverage.

The current design uses only literal, variable, and wildcard patterns so that the first iteration is simple and the exhaustiveness rule is easy to state; adding constructor/tuple/record patterns later will extend the pattern language and the exhaustiveness algorithm in a well-understood way.

---

## 8. Summary

| Item | First iteration |
|------|------------------|
| Syntax | `match expr with` + `case pattern [if guard] => body` |
| Patterns | Literal, variable, wildcard only |
| Guard | Optional `if expression` (must be `Bool`) |
| Typechecker | Inference: pattern vs scrutinee type; body type unification. Rules: exhaustiveness. |
| Exhaustiveness | Finite type: every value covered. Infinite type: at least one catch-all arm (variable or wildcard). Implemented in Rules phase. |
| Future | Enums, generics, records, tuples: extend patterns and exhaustiveness algorithm. |

This document is the design for **Backlog item 8**: match expression with literals, variable and wildcard, guard condition, and exhaustiveness rule.
