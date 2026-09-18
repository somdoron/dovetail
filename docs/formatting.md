# Formatting Dovetail

`dovetail fmt` formats local workspace source files and project tests. Run it
from the workspace root or a subdirectory. It finds the nearest ancestor
`Dovetail.toml`, reads local project and package paths, and includes each
project's `test/` tree. It does not fetch dependencies, read their sources,
generate bindings, or update the lockfile.

Pass explicit `.dove` files to format without a manifest:

```sh
dovetail fmt src/example.dove src/anotherExample.dove
dovetail fmt --check
```

`--check` lists files needing formatting without writing them. Exit codes are
0 for success, 1 for formatting differences in check mode, and 2 for input,
formatting, or filesystem errors. All selected files are parsed and formatted
before writing begins. Each changed file is replaced atomically, retaining its
permissions; replacement is not a transaction across multiple files.

During compiler development, use `cargo run -- fmt` to run the local compiler.

## Canonical style

The formatter chooses indentation, spacing, and wrapping. It preserves one
blank line between statements or declarations, collapsing larger gaps to one.
Other original layout does not influence its choices. There are no style settings or
formatting-disable directives.

- Four spaces per indentation level and a 100-column target.
- A short function body stays after `=`, including after a wrapped signature.
- A multiline conditional chain puts each `else if` and its final `else` on
  a separate line. If any branch body wraps, every branch body starts on a
  new indented line. A whole conditional that fits can remain inline.
- Lists wrap at grammar-supported boundaries. Record fields use semicolons
  inline and layout separators when multiline. Record-pattern fields use
  semicolons inline and between multiline fields, with no trailing semicolon.
  Commas in record patterns are accepted as input and normalized to semicolons.
- A `where` clause stays inline when the whole signature fits within 100
  columns, even if the body is multiline. Otherwise it starts on the next
  line, with wrapped conditions aligned beneath the first condition.
- Continuations indent one level. Long type annotations can wrap after the
  colon. Generic closing angles stay attached to the last type parameter.
- Explicit parentheses are preserved, including redundant grouping and
  parentheses inside interpolations. The formatter can add parentheses where
  wrapping requires them to preserve precedence or block boundaries.
- Comments retain their text and their leading/trailing attachment. Comment
  prose and literal text are not reflowed and can exceed the width target.
- Literal spelling and string text are preserved. Expressions inside string
  interpolation are formatted.
- Imports and declarations retain their order. Generated line breaks use LF,
  and documents end with a newline. Line breaks inside literal text remain data.

Syntax errors leave files unchanged. The formatter reparses its output and
checks the program structure before accepting it. It does not require a
successful typecheck.

## VS Code

The existing Dovetail language server handles **Format Document** using the
same formatter and the current unsaved editor buffer. Configure optional
format-on-save as described in the [extension README](../vscode-dovetail/README.md).
Formatting uses the canonical style regardless of editor indentation settings.

## Implementation

The compiler lexer optionally captures original byte ranges, comments, and
interpolation boundaries. The parser optionally records syntax-production
ranges over its token stream. Normal compilation keeps using the same grammar.

The formatter builds documents from this source syntax, retaining spelling
that the compiler AST discards. A small document printer chooses legal line
breaks. The ordinary compiler parser provides structural validation, with
source locations and scope-neutral single-expression block wrappers ignored.
`use` and binding scopes remain significant.

Tests cover representative formatting rules, CLI behavior, LSP buffer handling,
and structural equivalence plus idempotence across local workspace sources.
