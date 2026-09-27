# Syntax: common mistakes

Read [Language basics](https://dovetaillang.org/book/language-basics.md), [Control flow](https://dovetaillang.org/book/control-flow.md), [Functions](https://dovetaillang.org/book/functions.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

Use the chapters for declarations, literal forms, operator precedence, and closure
syntax. Check these frequent sources of incorrect translations from other languages:

- Blocks return their final expression. Use `let mutable` for reassignment,
  `match ... with` / `case ... =>`, and word operators `and`, `or`, `not`.
- Records use semicolon-separated fields. List patterns do not match arrays.
  Guards do not generally prove exhaustive coverage.
- `..` is slice syntax, not a standalone range expression; query the Range API.
- Named arguments use `name = expression`; positional arguments come first.
  Evaluation follows written order. Stored function references accept positional
  arguments only; accepted names come from the statically visible declaration.
- Default parameter values are not implemented. Use a configuration record/factory.
- Decimal operands must have matching types (`amount * 2dec`). An annotation does
  not turn an unsuffixed literal into Decimal. Do not invent decimal division,
  rounding, digit separators, or BigInt/Decimal literal match patterns.
- Triple-quoted strings retain indentation. Use `\$` for a literal dollar sign.
