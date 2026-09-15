# Part 11: Packages and Modules

Dovetail organizes code into **packages** and **modules**. Every source file declares its package, and you bring types and functions from other packages in with **imports**. The compiler enforces that package names match the directory structure and that dependencies form a directed acyclic graph—no circular imports.

---

## 11.1 Package Declaration

### Every File Has a Package

Every Dovetail source file must start with a **package declaration**. This identifies which package the file belongs to:

```dovetail
package hello

function main() =
    println("Hello, World!")
```

For a library with a deeper structure:

```dovetail
package com.example.users

record User =
    name: String
    age: Int32
```

The package path uses **dot notation**. Each segment typically corresponds to a directory: `com.example.users` matches a path like `com/example/users/` under the project’s source root.

### Package Must Match Directory Structure

The compiler enforces that the declared package matches the file’s location. If your project’s root package is `com.example.myapp`, then:

- `src/main.dove` → `package com.example.myapp`
- `src/utils/helpers.dove` → `package com.example.myapp.utils`

This keeps the codebase predictable: you can infer the package from the path and the path from the package.

### Relationship to Projects and `Dovetail.toml`

When you use the `dovetail` CLI, each **project** in the workspace has a **root package** defined in `Dovetail.toml`:

```toml
compiler-version = "0.1.0"

[[project]]
name = "api"
root_package = "com.example.api"
packages = ["."]
```

All source files under that project’s `src/` directory belong to packages under that root. For example, `api/src/handlers.dove` would declare `package com.example.api`, and `api/src/auth/validate.dove` would declare `package com.example.api.auth`.

Single-file compilation with `dovetail` does not use `Dovetail.toml`; you still declare a package at the top of the file (e.g. `package myapp`).

### Module file shorthand

Instead of writing `package` and then an inline `module Name = ...`, you can make the **entire file** a module by starting with:

```dovetail
module package_path.ModuleName
```

or, for a generic type, **`module package_path.TypeName<T>`** (e.g. `module standard.prelude.Array<T>`). The rest of the file is the module body; the effective package is `package_path`. The **kind** of module (standalone vs module for a type) is determined by whether that name is an existing type in the package. Different files in a package may define different modules. A type and its associated same-name module may also be declared in separate files of that package. See [Part 6: Type System — Modules](06-type-system.md#68-modules) for the two kinds of modules and what each may contain.

---

## 11.2 Imports

### Importing Types and Functions

To use a type or function from another package, add an **import** after the package declaration. Imports come at the top of the file, one per line:

```dovetail
package api

import users.User
import users.Email
import users.validate
import http.Request
import http.Response

function handleGetUser(req: Request): Response =
    let user = User.find(req.params.id) orReturn
    Ok(user.toJson())
```

You import by **qualified name**: the package path followed by the declaration name. There are no wildcard imports (`import users.*` is not allowed), so it’s always clear where each name comes from.

### Aliased Imports

If a name would clash with a local type or you prefer a shorter name, use an alias:

```dovetail
package api

import users.User as AppUser
import http.Request as HttpRequest

function handle(req: HttpRequest): Unit =
    let u: AppUser = AppUser.fromRequest(req)
    ...
```

The alias is used everywhere in that file instead of the original name.

### Full Package Paths

For packages with multiple segments, use the full path in the import:

```dovetail
package com.example.myapp

import com.example.core.types.Id
import com.example.core.utils.formatDate
```

The compiler resolves imports using the packages exposed by the current project and its dependencies (as specified in `Dovetail.toml` for workspace builds).

### Modules and extension methods across packages

**Same package:** Instance methods on a type in the same package come from a **module for that type** (a module with the same name as the type, if the package defines one). You do not need to import the module to use instance members — they are available whenever the type is in scope. Static members and globals require the module to be in scope (same package or imported). For a generic type you must supply type arguments when accessing statics or globals: e.g. `Array<Int32>.empty`, `Array<Int32>.defaultCapacity`. See [Modules](06-type-system.md#68-modules).

**Cross package:** To add methods to a type from another package, use a **named extension** and **import it explicitly**:

```dovetail
// File: json/encoders.dove
package json
import users.User

extension UserJson for User =
    function toJson(self): Json = ...
```

Consumers must import the extension by name:

```dovetail
package api

import users.User
import json.UserJson

function serialize(u: User): Json =
    u.toJson()   // UserJson is in scope via import
```

Trait implementations for types from another package follow the **orphan rule** (see Part 8): you may only implement a trait if your package defines either the trait or the type. When the trait is in your package and the type is imported, the implementation is discovered when the trait is used; you don’t import the implementation itself.

---

## 11.3 Visibility

Declarations can be restricted by **visibility**. This controls whether other packages (or other files in the same package) can see a type, function, or member.

### Visibility Levels

| Modifier   | Meaning |
|-----------|--------|
| `public`  | Visible from any package that depends on this package. |
| `internal`| Visible within the same **package** only. This is the **default**. |
| `private` | Visible within the declaring **file** only. |
| `protected`| Visible within the declaring class and its subclasses (see Part 9). |

If you don't write a modifier, the declaration is `internal` — visible to the rest of its own package and to nothing else. That makes each package's public surface an explicit choice, which is what lets you use packages as architectural layers.

### Making Declarations Public

Use `public` when you want to expose a type or function to other packages:

```dovetail
package math

public function add(a: Int32, b: Int32): Int32 =
    a + b

public record Point =
    x: Int32
    y: Int32
```

Without `public`, `add` and `Point` would only be visible inside the `math` package.

### Type Visibility and Private Construction

A leading `private` hides a declaration from other files. A `private` after a type's name restricts operations on its values. These are separate choices:

```dovetail
package users

public newtype Email private = String

module Email =
    public function parse(s: String): Result<Email, String> =
        if s.contains("@") then
            Ok(Email(s))
        else
            Error("Invalid email")

    public function text(self): String = self.value
```

Other packages can import `users.Email`, call `Email.parse`, and use `email.text()`. Direct construction, `.value`, and unwrapping patterns require `module Email` in package `users`. Another function in `users`, a trait implementation, an extension, or a same-named module in another package has no such privilege.

For `public record Account private = ...` and `public enum Status private = ...`, callers can inspect fields and match patterns. Only their associated modules in the defining package can construct values; record `with` updates have the same restriction. Put the modifier after any type parameters and before any `where` clause. See [private records](06-type-system.md#private-construction), [private enums](06-type-system.md#private-construction-of-enums), and [private newtypes](06-type-system.md#private-newtypes).

### Internal Visibility

`internal` is the default, so most declarations are already package-scoped: visible to every file of their own package, invisible everywhere else. Write it explicitly when you want that intent to be obvious. Use `private` when a helper should not even escape the file it is declared in.

---

## 11.4 No Circular Dependencies

### Dependency Direction

Packages must not depend on each other in a cycle. If package **A** imports from package **B**, then **B** must not import from **A**. The dependency graph must be a **directed acyclic graph (DAG)**.

The compiler enforces this. You’ll get an error if you introduce a circular dependency.

### Why No Cycles?

- **Compilation order:** The compiler processes packages in dependency order. Cycles would make that order undefined.
- **Clear boundaries:** Acyclic dependencies encourage a clear layering of your code (e.g. core → domain → api → app).
- **Faster builds:** The build system can schedule work along the DAG; cycles would complicate caching and parallelization.

### Project-Level Dependencies

In a workspace, dependencies are declared per **project** in `Dovetail.toml`:

```toml
compiler-version = "0.1.0"

[[project]]
name = "core"
root_package = "com.example.core"
packages = ["."]

[[project]]
name = "api"
root_package = "com.example.api"
depends = ["core"]
packages = ["."]

[[project]]
name = "myapp"
root_package = "com.example.myapp"
depends = ["core", "api"]
packages = ["."]
```

Here, `core` has no project dependencies; `api` depends on `core`; `myapp` depends on both. Packages under `api` can import from packages under `core`, and packages under `myapp` can import from both. The reverse is not allowed: `core` cannot depend on `api` or `myapp`.

### Resolving Cycles

If you discover a cycle, you usually need to:

1. **Extract shared code** into a new package that both sides depend on (e.g. move common types into `core`).
2. **Invert the dependency** (e.g. use callbacks or traits so that the lower layer doesn’t reference the higher layer by type).
3. **Restructure** so that the dependency flows in one direction (e.g. merge two packages or split one).

---

## 11.5 GitHub Dependencies

Projects can use libraries from public or private Git repositories, including repositories containing several Dovetail projects. Declare each repository and revision once in `Dovetail.toml`, select the projects to make available, and use their names in a local project's `depends` list.

### Declare a Repository and Select Projects

The following example imports two projects from one repository. Replace the example URL and tag with those of your library:

```toml
compiler-version = "0.1.0"

[[dependencies]]
git = "https://github.com/acme/database.git"
tag = "v1.2.0"
projects = ["postgres", "sqlite"]

[[project]]
name = "api"
root_package = "com.example.api"
packages = ["."]
depends = ["postgres", "sqlite"]
```

Use `[[dependencies]]` with **double brackets** for each repository declaration. It has no user-assigned section name. The `projects` list selects project names from the repository's `Dovetail.toml`; those names are available throughout the declaring manifest. Each local project chooses which ones it uses through `depends`, just as it does for local sibling projects.

The compiler resolves the complete transitive dependency tree. If `postgres` depends on another project in its own repository or on a project from another Git repository, Dovetail resolves that dependency using the library's manifest. Your application does not need to repeat those declarations. Cycles remain errors, including cycles spanning repositories.

### Pin the Compiler and Library Revision

Every manifest must declare `compiler-version`. Run `dovetail --version` to find your binary's version; the examples here assume `0.1.0`. The workspace and every reachable dependency manifest must require that exact version. Build, check, run, and test reject a root version mismatch before fetching dependencies. Editor analysis performs the same compatibility checks.

The compiler version also pins the bundled prelude. Dependencies use that prelude, and cannot supply a conflicting replacement. A published tag without `compiler-version` cannot be used until its publisher adds the field in a new revision.

Choose at most one revision selector per repository declaration:

| Field | Selects |
|-------|---------|
| `tag = "v1.2.0"` | A Git tag |
| `branch = "1.2"` | A Git branch named `1.2` |
| `rev = "<commit-sha>"` | A specific Git commit |

Omitting all three initially selects the repository's default branch. Dovetail records the resolved full commit in `Dovetail.lock`; normal builds keep that commit even if the branch or tag moves upstream. These selectors are Git references, not semantic-version ranges.

For a repository with a manifest below its root, add `manifest = "libraries/Dovetail.toml"` to the repository declaration. The selected project names and their directories are resolved relative to that manifest.

### Keep Package Imports Unchanged

Repository project names select dependencies; source imports use the library's declared package names. For example, if the `postgres` project exposes a public `Connection` type in `acme.postgres`, your application imports it as:

```dovetail
package com.example.api

import acme.postgres.Connection
```

Neither the repository URL nor its revision appears in the import. The library still controls which declarations are `public`.

### Aliases and Multiple Versions

By default, an imported project keeps its original name. Use `{ project, alias }` to choose another name in the consuming manifest. For example, add this top-level declaration alongside the first one to make an older revision available as `postgres-1.1`:

```toml
[[dependencies]]
git = "https://github.com/acme/database.git"
tag = "v1.1.0"
projects = [{ project = "postgres", alias = "postgres-1.1" }]
```

A local project can then select it with `depends = ["postgres-1.1"]`. Aliases are scoped to the manifest declaring them and must not collide with local project names or other imported aliases. They do not rename package FQNs or source imports; this is different from an `import ... as ...` alias, which renames a declaration within one source file.

Different local targets may use different revisions. However, two distinct providers of the same package FQN cannot coexist in one target's dependency tree, including through transitive dependencies. Adding both Postgres revisions to one target produces an error with the conflicting dependency paths. A diamond that reaches the same resolved project twice is deduplicated.

### Fetch, Build, and Update

A normal `dovetail build` fetches missing dependencies as needed. You can also manage them explicitly:

```bash
dovetail deps fetch
dovetail build --locked
dovetail check --locked --offline

dovetail deps update postgres
dovetail deps update
```

- `deps fetch` materializes declared selections and their transitive dependencies, including selections not yet used by a local target.
- `--locked` requires `Dovetail.lock` to remain unchanged. Use it after resolving dependencies to detect changes that would require a new lockfile.
- `--offline` prevents network fetching. Dependencies must already be available locally.
- `deps update postgres` updates the repository declaration containing that name or alias. All projects selected by that declaration move together, so in the first example both `postgres` and `sqlite` update.
- `deps update` updates dependencies across the graph.

Updating re-resolves the declared Git selector. To switch from `v1.2.0` to a newer release tag, edit `tag` in `Dovetail.toml` and run `dovetail deps fetch` or build again.

**Commit `Dovetail.lock` and ignore `.dovetail/`.** Dovetail shares source checkouts at `.dovetail/deps/<repository-digest>/<full-commit>/` across aliases and project selections. The root workspace's lockfile controls resolution; a fetched library's lockfile does not override it.

Workspace builds and tests select local projects. Remote projects supply source and assets without becoming independent output targets, and their tests are not run implicitly.

### Private Repositories, Assets, and Editor Navigation

Private repositories use the installed Git executable and your configured HTTPS credential helper or SSH agent. An SSH declaration can use `git = "git@github.com:acme/private-library.git"`. Configure access through Git; keep credentials out of `Dovetail.toml`. Editor fetching is noninteractive.

A dependency can carry prebuilt WebAssembly components, resources, and macro scripts, including transitively. Commit the actual artifact files in Git. Automatic Git LFS hydration, submodule fetching, native build scripts, and release-asset downloading are not supported.

The language server resolves the same dependency graph as the CLI. Go-to-definition opens the checked-out source, and semantic requests use the consuming project's dependency revision. Fetched files remain available for offline navigation and are read-only dependency sources. Generated component bindings are navigable under `.dovetail/generated/`.

### Select Standard Libraries with One Tag

Use top-level `standard-tag` to make all standard library projects available at one revision:

```toml
compiler-version = "0.1.0"
standard-tag = "<tag>"

[[project]]
name = "api"
root_package = "com.example.api"
packages = ["."]
depends = ["standard-collection", "standard-json"]
```

Replace `<tag>` with a tag whose manifest declares the matching compiler version. Currently this shorthand uses `https://github.com/somdoron/dovetail.git`. It exposes projects rooted at `standard` or `standard.*`, retaining their actual project names such as `standard-json`, while excluding `standard.prelude`. Each local project still selects the libraries it needs through `depends`.

The [Git dependencies design](../docs/github-dependencies-design.md) describes the resolver and compatibility rules in more detail.

---

## Summary

- Every file starts with a **package** declaration; the package path must match the directory structure.
- Use **imports** to bring in types and functions from other packages; one import per line, no wildcards. Use **aliased imports** when names clash or you want a shorter name.
- **Extensions** and **trait implementations** in the same package are available automatically with the type; cross-package extensions must be named and explicitly imported.
- **Visibility** is controlled by `public`, `private`, and `internal` so you can hide implementation details and expose a clear API.
- **Git dependencies** are declared once per repository and revision, selected through project `depends`, and pinned in `Dovetail.lock`. Package imports keep their original names.
- **Circular dependencies** are disallowed; the compiler enforces a DAG of package (and project) dependencies, which keeps builds and design predictable.

In the next part, we’ll look at async programming and the `.andWait` operator.
