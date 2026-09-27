# Projects and library selection

Read [Getting started](https://dovetaillang.org/book/getting-started.md), [Tool commands](https://dovetaillang.org/book/tool-commands.md), [Packages](https://dovetaillang.org/book/packages.md), [Standard library](https://dovetaillang.org/book/stdlib.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

- Read the actual manifest before choosing a project or API. Package namespaces
  and dependency project names differ. Keep package dependencies acyclic.
- Use `dovetail init` / `projects add` for scaffolding and query installed APIs via
  [API discovery](api-discovery.md). Import named extensions explicitly.
- Select a real accessible standard-library tag compatible with the compiler; a
  compiler release does not prove a matching library tag exists.
- Pin intended dependency revisions and commit Dovetail.toml and Dovetail.lock.
  `--locked` prevents lock changes but can download; `--offline` prevents downloads
  and needs cached dependencies. Do not edit cached/generated sources.
- Inspect existing collection bounds: validated values may deliberately lack Default,
  while mutable collections can require it for keys and/or values.
- Verify library availability and readiness; do not invent a familiar HTTP/database
  framework. Keep time, random values, and generated IDs explicit in business logic.
