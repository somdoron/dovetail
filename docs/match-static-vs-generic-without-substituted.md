# Match static vs generic without `Type::Substituted`

## Goal

Today we use `Type::Substituted(inner, type_param_name)` only to distinguish:

- **Static case** (e.g. `foo(x: Option<Int32>)`): match arm `case x: Option<String>` → unreachable; if we see a record pattern with wrong FQN we **error**.
- **Generic case** (e.g. `bar<T>(x: Option<T>)`): after specialization, `bar<Int32>` has subject type `Option<Int32>`. We treat that as “generic” so we **skip** the `Option<String>` arm (unreachable) and **do not error**.

So the only reason for `Substituted` is: “this concrete type came from a type parameter,” so we use “generic” semantics (skip unreachable, no error) instead of “static” (error on wrong base type).

The cost: we must strip `Substituted` everywhere before storing typed functions and in codegen, which adds complexity.

## Why “subject from type param” must be on the type

The subject of a match is **any expression**, not just a variable that refers to a parameter:

- **Function call:** `match (id x) with case ...` or `match (getOpt p) with ...` — the call’s return type might be the instantiation of a type parameter. We need to treat that as “generic” when the pattern doesn’t match.
- **Let-bound variable:** `let y = x; match y with ...` — the subject is `VarRef(y)`, but the type of `y` came from `x` (the param). So we’d have to track “this variable’s type came from a type parameter” through the scope chain.
- **Field access, other expressions:** same idea — the type can flow through arbitrarily many steps.

So we cannot decide “subject from type param” by:

- **VarRef-only:** only treating “subject is VarRef to a param” as generic fails for `match y with` (y bound from param) and `match (f x) with`.
- **Param-name / scope heuristics:** we’d have to propagate “from type param” through every binding (let, pattern), every call (return type), every field access, etc. That’s full type-flow or provenance tracking — complex and easy to get wrong.

The only robust way to know “this value’s type came from a type parameter” after arbitrary flow is for the **type itself** to say so. Then:

- Substitution produces `Option<Substituted(Int32, T)>`; that type flows into any let, return, or field.
- When we match on any expression with that type, we already have the answer.

So **a type-level marker (like `Substituted`) that propagates with the type is the right abstraction.** Removing it would require either incomplete heuristics (VarRef/param only) or full provenance tracking.

## Why the earlier “alternatives” don’t work

**Option A (rules phase + VarRef heuristic):**

- Assumed we could infer “subject from type param” in rules by: subject is `VarRef(name)` and that name is a parameter whose type in the generic def contained a type parameter.
- That only covers `match x with` when `x` is a param. It does **not** cover `match y with` (y from let), `match (f x) with`, etc.
- Using mangled-name parsing to detect specializations is a string heuristic the project is not allowing anyway.
- So Option A is **not viable** as a full replacement.

**Option B (inference-time “param origin” set):**

- Assumed we could set `is_generic` when subject is VarRef to a param whose type in the generic def contained a type parameter.
- Same limitation: subject is often not a direct param (let-bound, call result, etc.). To cover those we’d need to propagate “from type param” through every expression form — i.e. we’d be reimplementing what the type marker already does, but in a separate flow analysis.
- So Option B is **not** a full replacement; it only works for the narrow case “subject is exactly a parameter.”

## Conclusion (before dry-run approach)

- **Keeping `Type::Substituted`** (or an equivalent type-level marker that propagates with types) is the correct design for “subject from type parameter” in the presence of arbitrary subject expressions and type flow.
- Alternatives that rely only on VarRef or param names don’t scale to function calls, let bindings, and other subject forms; doing it properly would require full type-flow/provenance, which is heavier than a type wrapper and strip-at-boundaries.

If the goal is to reduce the cost of Substituted, the direction would be to **minimize and centralize** the strip logic (e.g. a single strip point before storing typed functions and at codegen entry) rather than to remove the marker.

---

## Solution: Type-check generic code in main inference (two birds, one stone)

**Problem 1:** Generic code (function bodies, generic types) is only inferred when we **specialize** it. If nothing in the project ever instantiates a generic (e.g. library API never called), we never type-check that body → **we never report type errors in that generic code**.

**Problem 2:** We need `Substituted` only to decide in the **specialization** run: "error on invalid match" (static) vs "skip unreachable arm only" (generic).

**Solution:** Infer generic code in the **same** inference pass as today — don't skip it and wait for instantiation. Use a **single** flag: `typechecking_only`. When true we type-check and report errors; when false we assume we're not in a generic body (instantiation or non-generic) and for matches we only skip unreachable arms, never error on pattern mismatch.

1. **Type-checking generic code (main inference):**
   - Today we skip generic function bodies and only infer them when we instantiate. **Change:** infer them during normal inference like any other code.
   - Set `typechecking_only: true` when we're inferring a generic body (type params in scope, no concrete type args). In this mode we **don't instantiate** anything (e.g. we don't resolve and instantiate generic function calls). We can't anyway — we have type params, not concrete types. **Do not** set any "instantiation run" flag; we only need `typechecking_only`.
   - Match inference: when `typechecking_only` is true, report all errors (wrong base type, etc.). Generic bodies get fully type-checked.

2. **When typechecking_only is false:**
   - We're either in a non-generic function or in an instantiation run (we're inferring a body with concrete types after substitution). In both cases, for match inference we **skip** unreachable arms and **do not** emit pattern/record mismatch errors (either we're static and skip is enough, or we're instantiation and errors were already reported when we type-checked the generic). So one flag is enough: if `typechecking_only` is false, we assume "instantiation" semantics for matches (skip only, no errors).

**Result:**

- **Bird 1:** All generic code is type-checked when we infer it with `typechecking_only == true`.
- **Bird 2:** Remove `Type::Substituted`; when `typechecking_only` is false, match inference does "skip only, no errors."

**Implementation sketch:**

- **Single flag:** `typechecking_only: bool` on the inference context. When we infer a generic function (or generic type) body during main inference, set it to `true` for that inference; we don't instantiate other generics. When we later run inference for an instantiation, `typechecking_only` is false (we're not in "type-check only" mode), so we get "skip only, no errors" for matches without a second flag.
- **Inference (match):** When `typechecking_only` is true, in `infer_type_annotated_pattern` and `infer_record_pattern` (and anywhere that today uses `Substituted` to choose error vs skip): error when appropriate (wrong base type, etc.). When `typechecking_only` is false: **never** emit "type mismatch in pattern" / "different base type" errors; only return `None` (skip arm).
- **Instantiation:** Run body inference with concrete types; `typechecking_only` is already false (we didn't set it for this run), so match inference automatically does skip-only. No `Substituted` in the substitution map; no strip step.
- **Remove:** `Type::Substituted`, `strip_substituted`, `strip_all_substituted`, `from_pairs_substituted`, and all strip points.
