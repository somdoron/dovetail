# Discover APIs with the compiler

Read this when using an unfamiliar local or dependency API, exploring a library,
or verifying signatures during a requested review. Query the compiler matching the
project; the skill's version check still applies.

## Choose the smallest useful query

```bash
dovetail query search Connection
dovetail query search transaction --package standard.sqlite
dovetail query package
dovetail query package standard.sqlite
dovetail query definition standard.sqlite.Connection
dovetail query definition standard.sqlite.Connection.transaction
```

- `search` finds declarations and members by name or qualified name. It is
  case-insensitive and ranks exact matches first. Results include kinds, owning
  projects, and source locations. Use `--package` for one exact package; use
  `--limit` and `--offset` to page results (default limit: 50).
- `package` lists available packages grouped by project. Supply a package name to
  list its immediate declarations and child packages without expanding members.
- `definition` takes a fully qualified name. It shows documentation and signatures,
  including overloads. A type and its same-name module appear together even when
  declared in different files.

Use `--project <local-project-name>` when selecting a consumer's dependency
context. Project names and package names are different: `standard-sqlite` may own
`standard.sqlite`. If different contexts resolve different versions of the same
API, select the intended consumer; do not combine their declarations.

The embedded `standard.prelude` is queryable without a workspace. Other libraries
must be available through the workspace's resolved dependencies. Existing
`--offline` and `--locked` options apply; queries do not request dependency upgrades.
Normal dependency resolution may populate caches and lockfiles.

## Interpret declaration output

This is a **declaration view**, not a compilable `.dove` file. Function and property
bodies, initializers, and superclass argument expressions are omitted. Do not paste
it into a source file and expect it to compile. Preserve the displayed bounds,
visibility, async modifiers, associated types, and constructor restrictions when
using an API.

Type definitions include implementation signatures targeting that type, with their
bounds. Named extensions are linked separately: query the extension and explicitly
import its qualified name to use it, including from its own package. Conditional
implementations are not proof that a particular instantiation satisfies all bounds.
Inherited and default members retain their origin instead of being presented as
newly declared methods.

Local declarations include internal/private APIs. Dependency output defaults to
public APIs (including protected subclass members). `--all` exposes nonpublic
members for inspection; it does **not** make them accessible to the consumer.
Compiler-built-in types are identified as such rather than invented as records.
Generated declarations carry provenance.

On analysis errors, recoverable output is marked `INCOMPLETE`, diagnostics go to
stderr, and the command exits nonzero. Missing inferred types are explicitly
unavailable. Do not guess them or treat partial output as proof that code checks.
A healthy dependency can be queried without checking its consumer's function bodies.

Declarations establish API shape. Inspect source, documentation, and tests when
behavior matters: transaction semantics, resource cleanup, failure handling,
mutability, and execution order cannot be established from signatures alone.

## Compiler checkout

During compiler development use `cargo run -- query ...`, or run
`cargo run --manifest-path /path/to/compiler/Cargo.toml -- query ...` from the
consumer workspace. An installed `dovetail` binary can be stale. If its version
differs from this skill, recommend refreshing the skill with the intended compiler's
`dovetail ai install`; do not reinstall automatically.
