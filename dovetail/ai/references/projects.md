# Projects, packages, and library discovery

Run `dovetail init myapp` to create a workspace and starter project. AI support is
optional; `--ai generic,claude` selects it explicitly and `--no-ai` skips its prompt.
Use `dovetail projects add model` to add a local project.

```toml
compiler-version = "{{DOVETAIL_VERSION}}"

[[project]]
name = "myapp"
root_package = "myapp"
packages = ["."]
```

A workspace may contain several projects. A project defaults to a directory named
for it; `path` overrides that. `root_package` is its source root package. Package
names follow directories below `src/`; all files in a package are checked together.
List packages in dependency order and keep the dependency graph acyclic. Select
an application explicitly for `run` in multi-project workspaces.

Import declarations with `import package.Type`, optionally `as Alias`. Top-level
`public` exports across packages, `internal` is package-local, and `private` is
file-local. Import named extensions explicitly. Keep imports consistent with actual
package declarations, not project names (which commonly contain hyphens).

## Dependencies

`depends = ["standard-json", "standard-io"]` declares project dependencies.
For published standard libraries, set workspace `standard-tag` to a real accessible
tag whose manifest matches the compiler; do not assume a tag exists merely because
the compiler version exists. Explicit Git dependencies support branch/tag/rev and
project selection; use a pinned revision for reproducible integration:

```toml
[[dependencies]]
git = "https://github.com/acme/libraries.git"
rev = "<full-commit-id>"
projects = ["validation"]
```

Replace placeholders with actual accessible inputs. Add selected projects to the
consumer's `depends`. `dovetail deps fetch` resolves/fetches; `deps update [alias]`
deliberately updates selections. Commit `Dovetail.toml` and `Dovetail.lock`.
`--locked` forbids lock changes but can download pinned dependencies; `--offline`
forbids downloads and requires cache. They can be combined.

Caches and generated sources are in `.dovetail/`; artifacts are in `build/` by
default. Do not edit dependency checkouts or generated bindings to change APIs.

## Standard-library map

Use [API discovery](api-discovery.md) to query actual declarations when choosing
an API; existing constructors and bounds are more authoritative than guessed idioms. Dependency project names and
package namespaces differ. Useful starting points:

| Project | API area |
|---|---|
| Prelude (automatic) | Option, Result, Array/List, slices, BigInt, Decimal, core traits |
| `standard-collection` | ArrayList, MutableMap/Set/Queue/Stack, Range, StringBuilder |
| `standard-encoding` | Base64, Hex and typed decode failures |
| `standard-parser` | Parser composition; tuple extension can accumulate outputs |
| `standard-math` | RoundingMode; does not imply implemented decimal rounding |
| `standard-io` | Async, resources, console, filesystem, networking |
| `standard-io-stream` | Stream descriptions, chunking and transport adapters |
| `standard-json` | JSON values, encoding/decoding, derives |
| `standard-time` | Durations, clocks, date/time and zones |
| `standard-sqlite` | Resource-scoped connections/statements, typed async SQL, bundled component |

Inspect workspace projects for HTTP, logging, randomness, crypto, TLS, and other
available libraries before adding a dependency. Do not infer availability from
another language's standard library. Several mutable collections require Default
elements (maps also require Default keys/values and Equatable/Hashable keys);
validated types may intentionally lack defaults.

Keep incomplete UTF-8/protocol input across byte chunks. Use parameterized SQL and
scoped database resources. Pass time/IDs/random facts into pure business functions
rather than hiding nondeterminism inside them.
