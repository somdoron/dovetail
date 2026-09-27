# Validation workflow

Read [Tool commands](https://dovetaillang.org/book/tool-commands.md), [Testing](https://dovetaillang.org/book/testing.md), [Best practices](https://dovetaillang.org/book/best-practices.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

Use the installed compiler's `--help` for exact flags. During compiler development
use `cargo run --`. Use the intended consumer workspace as the working directory.

Format changed sources, check the selected project, and run relevant tests. `check`
does not exercise code generation or runtime behavior. Report which commands ran.
Use real Dovetail workspaces for library/language validation, including failure cases.
Async tests must run the effect; merely constructing it proves nothing.

Let the formatter own layout; see [Formatting](https://dovetaillang.org/guides/formatting.md).
Documentation examples are not automatically executable tests. Test expected input
rejection through its typed result, not a panic assertion.

Compiler warnings, formatting, and proposed lint rules are separate. There is no
`dovetail lint` command or configuration. Named arguments are supported; default
parameter values are not. [Linter design](https://github.com/somdoron/dovetail/blob/main/docs/linter-design.md)
is a proposal, not an implemented command. Never report a clean lint run.

Use [API discovery](api-discovery.md) for unfamiliar local/dependency APIs. Incomplete
query output does not establish a successful project check.
