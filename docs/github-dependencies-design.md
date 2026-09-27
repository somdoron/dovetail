# GitHub Dependencies Design

Status: implemented; validation results are recorded with the implementation delivery.

Dovetail workspaces can depend on projects in public or private Git repositories. Each repository and revision is declared once in `Dovetail.toml`, alongside the projects to import from it. Builds and the language server use the same locked dependency graph and local source checkouts.

This extends [multi-project support](multi-project-multi-package-design.md). A workspace contains projects; each project contains Dovetail packages. The remote selection unit is a project, including its packages, resources, macros, components, and dependency closure.

## 1. Agreed constraints

- Declare repositories and revisions centrally in each `Dovetail.toml`.
- Import several projects from one repository declaration.
- Allow custom dependency aliases and multiple revisions of a repository in a workspace.
- Reject conflicting package FQNs within a single build target's dependency tree. Keep compiler FQNs and source import syntax unchanged.
- Support public and private repositories through normal Git authentication.
- Carry prebuilt WebAssembly components, resources, and macro scripts with dependencies.
- Store dependency source locally so the LSP can navigate and inspect it.
- Pin the compiler and its bundled prelude with a required workspace `compiler-version`; reject mismatches with the running binary or any reachable dependency manifest.
- Support `standard-tag` as workspace-level shorthand exposing all projects from the canonical standard library repository at one locked revision.

The manifest shape and constraints below capture the implemented design. `Dovetail.lock` uses a versioned TOML format; source checkout identities include repository, commit, manifest path, and project.

## 2. Manifest format

Use an unnamed array of repository declarations. TOML requires double brackets to repeat the table:

```toml
compiler-version = "0.8.0"

[[dependencies]]
git = "https://github.com/acme/standard.git"
branch = "1.2"
projects = ["collection", "time", "json", "sqlite"]

[[dependencies]]
git = "https://github.com/acme/database.git"
branch = "1.2"
projects = ["postgres"]

[[dependencies]]
git = "https://github.com/acme/database.git"
branch = "1.1"
projects = [
    { project = "postgres", alias = "postgres-1.1" },
]

[[project]]
name = "api"
root_package = "app.api"
packages = ["."]
depends = ["collection", "json", "postgres"]

[[project]]
name = "legacy"
root_package = "app.legacy"
packages = ["."]
depends = ["postgres-1.1"]
```

Each `[[dependencies]]` entry has:

| Field | Meaning |
|-------|---------|
| `git` | Required repository URL; support HTTPS and SSH. |
| `branch` | Optional branch to track. |
| `tag` | Optional release tag to select. |
| `rev` | Optional specific commit to select. |
| `manifest` | Optional path to a manifest within the repository; defaults to `Dovetail.toml`. |
| `projects` | Required nonempty list of remote projects to expose as dependencies. |

At most one of `branch`, `tag`, and `rev` may be present. With none, initially resolve the default branch. All projects selected by one declaration use one resolved commit.

A string in `projects` selects the remote `[[project]]` with that name and uses the same name as its local dependency alias. An inline table supplies an explicit alias. Aliases are arbitrary manifest names: `postgres-1.1` does not imply any revision or version constraint.

Aliases must be unique within the declaring manifest and must not collide with local project names. They affect `depends` lookup only; they do not rename package declarations or source imports. Existing local project dependencies continue to work.

For a repository containing multiple manifests, selection is explicit:

```toml
[[dependencies]]
git = "https://github.com/acme/platform.git"
tag = "v1.2.3"
manifest = "libraries/database/Dovetail.toml"
projects = ["postgres"]
```

Resolve `manifest` relative to the checkout root. Do not recursively guess which manifest to use. Project directories remain relative to the selected manifest's directory, following existing workspace conventions.

### 2.1 Compiler and prelude version

Every workspace manifest, including manifests used by dependencies, must declare a top-level exact compiler version:

```toml
compiler-version = "0.8.0"
```

The compiler and its bundled prelude are one compatibility unit. The running binary must match the root manifest's `compiler-version` exactly. `dovetail build`, `dovetail check`, and `dovetail test` must reject a mismatch with a nonzero exit before dependency fetching or compilation. A matching lockfile or cached output cannot bypass this check. Missing declarations are manifest errors; do not silently infer the running version.

For example, running Dovetail 0.9.0 against a manifest requiring 0.8.0 fails:

```text
error: compiler version mismatch in Dovetail.toml
  workspace requires Dovetail 0.8.0
  running binary is Dovetail 0.9.0
```

During recursive dependency resolution, check each reachable dependency's owning manifest against the same version, including the standard library manifest. Reject mismatches before typechecking and show the full dependency path:

```text
error: incompatible compiler requirement
  workspace and running binary use Dovetail 0.8.0
  app -> postgres -> protocol requires Dovetail 0.7.0
```

All dependencies use the running compiler's bundled prelude. Do not load a separate dependency-provided prelude. Exact versions are required initially; compatibility ranges and automatic toolchain installation or switching are outside this design.

The LSP checks its own compiler version against the workspace and dependencies too. Report mismatches as workspace configuration errors and do not serve semantic results from an incompatible compiler or stale cache. Include compiler version in compilation and LSP cache keys; source checkouts can remain shared across compiler versions.

Existing manifests must add `compiler-version` when adopting this schema. Project initialization must write the creating binary's version, and manifest-rewriting commands must preserve it. The examples use illustrative release versions. Compatibility uses the declared Cargo package version (currently `0.1.1`); development binaries with that same declared version are treated as compatible. No Git revision is appended.

### 2.2 Standard library shorthand

An optional top-level `standard-tag` selects the standard libraries centrally:

```toml
compiler-version = "0.8.0"
standard-tag = "1.1"

[[project]]
name = "api"
root_package = "app.api"
packages = ["."]
depends = ["collection", "json", "sqlite"]
```

Dovetail knows the canonical standard repository URL. Resolve the specified tag exactly and lock it to one commit. Expose all standard library projects from its manifest by their project names throughout the declaring workspace, without requiring an explicit `[[dependencies]]` entry or `projects` list. Projects still declare their needs through `depends`; compile only the target's reachable closure. The compiler's bundled prelude is excluded from repository-provided projects.

These implicit aliases follow the same uniqueness rules as explicit dependencies; report collisions rather than silently preferring either declaration. The shorthand uses ordinary checkout storage, transitive resolution, assets, and LSP navigation. Its scope is the declaring manifest, so a dependency's own `standard-tag` is resolved in that dependency's context and does not override the root selection. Different resolved standard revisions exposing the same package FQN in one build tree remain a conflict.

`standard-tag` selects a library release independently of `compiler-version`. The selected standard repository manifest must require the same compiler version as the workspace and binary. The canonical repository currently is `https://github.com/somdoron/dovetail.git`. The shorthand exposes projects rooted under `standard.*`, excluding `standard.prelude`, using their actual project names (for example, `standard-json`). Moving the libraries to a dedicated repository is deferred.

## 3. Dependency resolution and scope

Transitive dependency resolution is required from the first implementation. The consumer declares only its direct dependencies; Dovetail recursively resolves and fetches their dependencies until the complete reachable graph is available. This applies across any number of repository boundaries, including private repositories, and includes each dependency's components, resources, and macros.

Each manifest owns its local project names and imported aliases. A fetched project's `depends` is resolved against its own manifest, never against the application's aliases.

For example, selecting `json` from the standard repository brings its required local sibling projects from that same checkout, even if those siblings are absent from the application's `projects` selection. It also brings any external dependencies declared by the remote manifest that the selected project's closure requires. Those internal aliases do not become aliases in the application manifest.

The `projects` selection exposes projects for use in local `depends` lists. A build compiles only its target and dependency closure, not every project in every fetched repository. Preserve existing language visibility rules within that closure.

For example, if `app -> postgres` in repository A, `postgres -> protocol` in repository B, and `protocol -> encoding` in repository C, declaring `postgres` in the application's manifest is sufficient. Dovetail reads the relevant manifests, resolves each edge in its owning manifest, fetches all three repositories at their locked commits, and orders compilation as `encoding`, `protocol`, `postgres`, then `app`. All reachable nodes and edges are recorded in the root lockfile and available to LSP navigation. The application does not need to repeat `protocol` or `encoding` declarations.

Missing transitive projects, authentication failures, cycles, and FQN conflicts fail resolution or validation with the dependency path from the build target. Never proceed with a partial graph or silently omit an unresolved dependency. Existing compatible transitive lock entries remain pinned during ordinary builds; an upstream update that changes dependencies triggers resolution of the affected closure.

The resolver needs an internal identity for each resolved project:

```text
repository identity + full commit + manifest path + remote project name
```

Local projects have an equivalent identity based on their owning workspace and project. This identity belongs to dependency resolution and orchestration; it does not change compiler FQNs.

Resolution must deduplicate identical project instances reached through multiple paths, detect cycles across manifest boundaries, and retain dependency paths for diagnostics. Project-output caches and lookup maps must use resolved identities rather than globally assuming project names are unique.

Source identity should normalize supported equivalent URL forms conservatively. Do not assume unrelated hosts, forks, or arbitrary SSH host aliases refer to the same repository merely because project names or commits match.

## 4. Multiple versions and FQN conflicts

Different revisions may coexist on disk and be used by separate local build targets. A workspace-wide build checks each target's closure independently.

Within one target's closure:

- The same resolved project reached through a diamond is included once.
- Distinct providers declaring the same package FQN cause a compilation error, even if their current symbols do not overlap.
- Distinct dependencies with disjoint package FQNs may coexist.
- Local packages and generated component bindings participate in the same collision checks.

In the manifest example, `api` and `legacy` may both build. A third project depending on both fails if the two Postgres revisions declare the same package FQN. No version-qualified import syntax or compiler symbol identity change is needed.

Report the conflicting package and both paths, for example:

```text
error: package `database.postgres` has multiple providers in target `combined`
  combined -> api -> postgres
    database.git, commit <new-commit>, project postgres
  combined -> legacy -> postgres-1.1
    database.git, commit <old-commit>, project postgres
```

Validate before registry merging, macro expansion, or code generation. Never silently select one provider.

## 5. Revisions and lockfile

Branches provide the requested maintenance-version model, such as `branch = "1.2"`. Tags and commits support release and exact-revision selection. A branch or tag is a selection rule; the lockfile records the actual commit used.

Commit `Dovetail.lock` beside `Dovetail.toml`. Its versioned format should record:

- Repository sources, requested selectors, and full resolved commits.
- Selected manifest paths and project identities.
- Dependency edges and scoped alias mappings for the transitive graph.
- Enough information to detect declarations incompatible with the existing lock.
- The root compiler requirement and the standard repository selection introduced by `standard-tag`, when present. Manifest compatibility is still validated against the running binary even when the lock is unchanged.

The root lockfile controls the consuming workspace's resolved graph. A fetched repository's lockfile does not override it. Changing a root dependency declaration does not implicitly override dependencies requested by another manifest.

Normal builds reuse compatible locked revisions and may fetch missing checkouts. They do not advance branches or tags merely because upstream moved. Initial resolution or changed declarations may update the lock; preserve unaffected compatible entries. Write lockfile changes atomically after successful resolution.

CLI behavior:

| Operation | Behavior |
|-----------|----------|
| `dovetail deps fetch` | Materialize required dependencies using the lock; resolve missing entries when permitted. |
| `dovetail deps update postgres` | Advance the repository declaration containing alias `postgres`; all projects selected by that declaration move together. |
| `dovetail deps update` | Update repository selections across the graph. |
| `--locked` | Reject any operation requiring a lockfile change; missing locked checkouts may still be fetched. |
| `--offline` | Use available local data only; report missing dependencies without network access. |

An update must also resolve any changed transitive requirements. Targeted updates accept root aliases; use the untargeted update command to advance transitive-only selections. Semantic-version ranges and automatic root overrides are deferred.

## 6. Local dependency storage

Use workspace-local checkouts:

```text
Dovetail.toml
Dovetail.lock
.dovetail/
  deps/
    <repository-id>/
      <full-commit>/
        Dovetail.toml
        collection/
          src/
        sqlite/
          src/
          artifacts/sqlite.wasm
```

`repository-id` is a stable filesystem-safe digest of repository identity, not an alias or credential-bearing URL. Several declarations resolving to the same repository and commit share a checkout, including selections from different manifests in that checkout. Different commits occupy different directories.

All transitive checkouts belong to the consuming workspace's `.dovetail/deps`; do not create nested dependency stores inside fetched repositories. Add `.dovetail/` to the workspace's ignore configuration. Keep `Dovetail.lock` tracked.

Treat completed checkouts as immutable dependency sources. Fetch into staging locations and publish atomically; coordinate concurrent CLI and LSP access. Validate canonical paths, including symlinks, so manifest and artifact paths cannot escape their allowed roots. Do not run repository installation or native build scripts to materialize a dependency.

A future explicit local-path override can support editing a dependency. A global download cache can later reduce duplication across workspaces while preserving local navigation paths. Cache cleanup should be explicit and avoid removing revisions in active use.

## 7. WebAssembly, resources, and macros

Dependencies carry the same project assets as local projects: source, embedded resources, derive-macro scripts, and prebuilt WebAssembly components. Resolve every asset relative to its owning fetched project.

The existing file-based component declaration provides the initial distribution mechanism:

```toml
[[project]]
name = "standard-sqlite"
root_package = "standard.sqlite"
packages = ["."]

[[project.component]]
path = "artifacts/sqlite.wasm"
package = "sqlite.raw"
```

Commit the prebuilt component with the source so one Git commit pins both. Consumers do not need the dependency's C or Rust toolchain. Continue validating the component format, selected interface, and compatibility with Dovetail's supported component ABI.

SQLite ships `artifacts/sqlite.wasm` inside the `standard-sqlite` project and declares it with `path`. The compiler embeds no SQLite binary. Consumers depend on `standard-sqlite` to inherit its artifact and bindings.

Propagate required assets transitively. Deduplicate repeated references to the same resolved component declaration, and reject distinct providers of the same bindings package. The current deduplication by bindings package alone must not silently discard a conflicting component. Distinct bindings packages do not automatically resolve competing providers of the same component import interface; report ambiguous composition rather than arbitrarily choosing one.

For the initial scope, require ordinary Git-tracked component files. Detect Git LFS pointer files and report that the actual artifact is unavailable. Explicit LFS support and authenticated GitHub release downloads are future extensions; externally downloaded artifacts would require pinned checksums and source/artifact association.

## 8. Public and private repositories

Use the installed Git executable for transport. HTTPS uses configured credential helpers; SSH uses the user's SSH configuration, agent, and known-host verification. Public HTTPS dependencies need no account configuration.

Keep credentials outside manifests, lockfiles, cache directory names, and diagnostics. Network operations initiated by the LSP must not hang waiting for an invisible interactive prompt. Report the affected repository and actionable authentication failure, with a retry after credentials are configured.

CI must supply credentials with access to private dependency repositories. GitHub Actions' default `GITHUB_TOKEN` is scoped to the workflow repository; another private repository requires suitable separate access, such as a GitHub App token, PAT, or SSH credential. See [GitHub token scope](https://docs.github.com/en/actions/concepts/security/github_token).

The format uses Git URLs and does not require a GitHub-specific repository API for ordinary source or artifact fetching.

## 9. LSP support

The compiler and LSP consume the same resolver output, lockfile, identities, and absolute source paths. Avoid a separate editor dependency resolver.

Required behavior:

- Go-to-definition opens the actual checked-out source file.
- Hover, completion, diagnostics, and references use the dependency revision selected by the requesting project's graph.
- Navigation inside a dependency resolves its imports in its owning manifest context.
- Macros and generated component bindings remain available to analysis. Generated declarations need stable virtual-document locations or an equivalent navigable representation where no source file exists.
- References distinguish identical FQN text in different revisions; editor index/cache context includes resolved ownership even though compiler FQNs are unchanged.
- Opening a fetched file does not accidentally create a separate dependency workspace or lockfile beneath the checkout.
- Cached dependencies remain explorable offline.

On workspace open, load the lockfile and fetch missing locked checkouts in the background with progress and cancellation. Do not advance existing branch selections. If initial resolution or changed declarations require a lock update, use the same policy as the CLI and expose failures clearly.

Watch the root manifest and lockfile. After changes, rebuild affected graphs and invalidate affected analysis. Avoid recursively watching or globally merging every cached revision. Treat dependency documents as read-only for editor-managed refactoring and edits; navigation should explain their dependency origin. Exact enforcement through the editor client remains an implementation detail.

Separate build targets may use separate versions without creating editor-wide FQN errors. A target whose closure conflicts should receive the same diagnostic as the CLI, while unrelated targets remain analyzable.

## 10. Implementation sequence

1. Extend manifest parsing and serialization with `compiler-version`, `standard-tag`, unnamed repository declarations, project selections, aliases, selectors, and manifest paths. Add early binary/manifest version validation and recursive dependency version checks. Preserve declarations when existing project-management commands rewrite the manifest.
2. Add Git fetching, workspace-local checkout storage, recursive transitive resolution across manifests and repositories, and the root lockfile.
3. Integrate resolved identities into orchestration and caches; validate each build target's closure and package/component collisions without changing FQNs.
4. Load remote source and assets through the existing compilation pipeline; preserve dependency source paths in diagnostics.
5. Integrate the shared resolver into the LSP, including navigation, graph-scoped indexing, background fetching, and invalidation.
6. Add CLI fetch/update and locked/offline behavior, diagnostics, and documentation.

Existing entry points include `dovetail/src/manifest/{toml_schema,resolve,validate}.rs`, `dovetail/src/compiler/pipeline.rs`, `dovetail/src/components.rs`, component codegen, and `dovetail/src/lsp/`. The resolver, commands, and LSP integration are implemented. Git executable injection supports deterministic transport tests.

## 11. Acceptance criteria

- Existing manifests containing only local projects retain their dependency behavior after adding the required `compiler-version` field.
- Missing `compiler-version` is a manifest error. A running binary with a different version fails build/check/test before fetching or compilation, including with populated caches or `--locked`/`--offline`.
- Direct, transitive, and standard library compiler-version mismatches fail with the owning manifest and dependency path. Matching versions use the bundled prelude.
- The LSP reports binary/manifest mismatches and does not reuse incompatible semantic results; compiler-version changes invalidate compilation and analysis caches.
- `standard-tag` exposes all standard library projects at one locked commit while compiling only the selected target's closure. Its aliases participate in collision checks and its sources remain navigable in the LSP.
- One repository declaration imports several projects from one checkout and one locked commit.
- Updating through any alias of that declaration moves all its selected projects together.
- Custom aliases resolve in `depends` without changing source import paths.
- Duplicate aliases and local-name collisions produce clear manifest errors.
- Remote sibling dependencies resolve in their own manifest, even when the consumer has an identically named project or alias.
- A dependency chain spanning at least three remote repositories resolves, fetches, locks, compiles, and supports LSP navigation with only the first dependency declared by the application. Include a private transitive repository and verify an authentication failure reports the full dependency path.
- Nested manifests resolve correctly; escaped manifest and asset paths are rejected.
- Public and private repositories work in the CLI and editor; failed authentication does not leave a usable-looking partial checkout.
- Locked builds reproduce selected commits after upstream branches move; offline builds succeed with cached dependencies and fail clearly when data is missing.
- Diamonds deduplicate identical resolved projects; transitive cycles produce dependency-path diagnostics.
- Separate targets using two Postgres revisions both build and navigate correctly. A combined target with overlapping package FQNs fails before compilation merges providers.
- A fetched SQLite project carries its binary, resources, and macros transitively into a consumer without requiring a native toolchain.
- Conflicting component providers are rejected instead of silently deduplicated.
- LSP definitions and references reach the correct revision, including from within fetched source. Lock changes refresh affected results.
- Concurrent CLI and LSP resolution cannot expose partial checkouts or corrupt the lockfile.

## 12. Research references

Cargo provides relevant precedent for Git branch/tag/revision selection, locked commits, and renamed dependencies: [Specifying dependencies](https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html). Central declarations with per-member selection are described in [Cargo workspaces](https://doc.rust-lang.org/cargo/reference/workspaces.html). Dovetail adapts these ideas to its single-manifest project model and explicitly rejects overlapping package providers per build tree.

[Cargo Git authentication](https://doc.rust-lang.org/cargo/appendix/git-authentication.html) explains credential helpers and the Git CLI option for SSH configurations unsupported by its built-in transport. [WebAssembly component composition](https://component-model.bytecodealliance.org/composing-and-distributing/composing.html) describes import/export wiring. [Git LFS documentation](https://docs.github.com/en/repositories/working-with-files/managing-large-files/about-git-large-file-storage) explains why a repository pointer is insufficient as an artifact.
