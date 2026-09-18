# Future Dovetail Linter

Status: proposal and rule backlog. Dovetail does not yet have a linter. This
document records desired rules; it does not introduce a command, configuration
format, or implemented diagnostics.

## Responsibilities

The formatter controls layout, spacing, and wrapping while preserving program
structure. The linter will identify readability and maintainability issues,
using resolved declarations and types when needed, and offer explicit fixes.
Adding names to positional arguments belongs to the linter, not the formatter.

Language errors remain compiler errors. Existing compiler warnings, including
discarded `Result`, `Async`, and `Resource` values and unsafe casts from `Any`,
remain in place. A future linter should not duplicate those diagnostics.

Fixes must preserve binding, evaluation order, and side effects. Offer automatic
fixes only when that can be established; otherwise provide a suggestion.
Rule severity, suppression, configuration, and CLI/LSP integration will be
designed when linter implementation is scheduled.

## Named arguments for larger signatures

Proposed rule: `preferNamedArguments`.

Suggest names for positional arguments when the resolved callable declares more
than two explicit value parameters. Exclude an instance method's receiver and
generic type parameters from that count. Apply to declared functions, methods,
and class constructors once named arguments are supported there.

```dovetail
// Before
createUser("John", 30, "john@example.com")

// Suggested fix
createUser(name = "John", age = 30, email = "john@example.com")
```

- Use the parameter names from the declaration selected by call resolution.
- For mixed calls, name the remaining positional arguments and preserve
  existing named arguments. Exclude receivers from this rule: an explicitly
  passed receiver stays positional. Already fully named calls need no diagnostic.
- Preserve argument order, expressions, and comments; do not reorder arguments
  to match declaration order or insert omitted default arguments.
- Skip calls that cannot be resolved reliably and function values whose types
  do not expose parameter names. Do not invent names for positional enum payloads.
- Offer a fix only if the rewritten call selects the same callable and preserves
  type inference. This matters for overloads and generic calls.

This replaces the book's earlier proposal that the formatter add argument names
automatically. Named arguments remain optional language syntax.

## Require names for Boolean and numeric literals

Planned rules: `requireNamedBooleanArguments` and `requireNamedNumericArguments`.

Require a named argument when a caller supplies a Boolean or numeric literal,
regardless of the number of parameters in the signature. Receivers are exempt,
including explicitly passed literal receivers, because receivers must remain
positional. These are future lint requirements, not language errors.

```dovetail
// Before
save(true)
retry(3)
move(-10, 20)

// Required by the future rules
save(overwrite = true)
retry(attempts = 3)
move(horizontal = -10, vertical = 20)
```

The Boolean rule covers `true` and `false`. The numeric rule covers all numeric
literal forms, including integer, floating-point, arbitrary-precision integer,
and decimal literals. Parentheses and unary negation do not hide a literal from
these rules. Variables, named constants, and computed expressions are outside
their scope; string and character literals are also outside their scope.

Use the same callable coverage and resolution safeguards as
`preferNamedArguments`. Function values without parameter names and positional
enum payloads are exempt. Already named literal arguments satisfy the rules.

A fix must respect the positional-before-named call rule. If naming a literal
would leave later positional arguments, name those arguments too, preserving
their order and expressions. For example, `send(true, message)` becomes
`send(urgent = true, message = message)`. Combine overlapping named-argument
diagnostics into one call-level diagnostic and fix rather than reporting the
same argument under multiple rules.

## Additional candidate rules

These are discussion candidates, not committed defaults or implementation work.

| Candidate | Intended behavior | Fix considerations |
|-----------|-------------------|--------------------|
| `unusedBinding` | Report unused local bindings and parameters, excluding intentional discards. | Never remove an initializer that may have side effects; parameter removal can change public APIs and contracts. |
| `unnecessaryMutable` | Suggest immutable bindings for locals never assigned after initialization. | Account for assignments from captured closures; object mutation alone does not require a mutable binding. |
| `redundantDiscardBinding` | Suggest a standalone expression instead of `let _ = expression` when explicit discard acknowledgment is unnecessary. | Preserve intentional acknowledgment for discarded `Result`, `Async`, and `Resource` values. |
| `identifierCasing` | Flag identifiers that violate Dovetail's camelCase value names and established type naming conventions. | Renames require symbol-aware edits; exported names need explicit review. |
| `constructorFactoryName` | Encourage `make` for conventional static construction factories. | Do not flag arbitrary functions named `new` or `create`; semantic intent may need a suggestion rather than an automatic fix. |

## Future validation

For named-argument rules, cover ordinary and generic functions, methods,
constructors, overloads, mixed calls, defaults when available, receiver exclusion,
and function values without labels. Verify that applying a fix preserves the
resolved target and runtime evaluation order, retains comments, and produces no
further diagnostic on a second lint pass. Calls with unresolved types or names
must not receive speculative fixes.

For the literal rules, cover one- and two-parameter calls, all numeric literal
forms, negative and parenthesized literals, already named literals, mixed calls,
and nonliteral expressions. Verify that fixes name subsequent positional
arguments when necessary and that overlapping rules produce one diagnostic.
