# Part 2: Dovetail Tool Commands

## 2.1 The Dovetail CLI

Run `dovetail --help` for commands and `dovetail <command> --help` for that command's
arguments. `dovetail --version` reports the compiler version. From a compiler
checkout, use `cargo run -- <command>` to avoid a stale installed binary.

| Command | Behavior |
|---|---|
| `dovetail init hello` | Create a workspace and optionally install coding-agent support |
| `dovetail ai install` | Install or refresh workspace-local coding-agent guidance |
| `dovetail projects add model` | Add a project and starter `main` to an existing workspace |
| `dovetail query <search/package/definition>` | Discover typed declarations in projects, dependencies, and the prelude |
| `dovetail check [project]` | Type-check all projects or one selected project and its dependencies |
| `dovetail build [project]` | Compile; writes artifacts under `build/` by default |
| `dovetail image build [-p project]` | Build precompiled Linux AMD64/ARM64 OCI images for configured projects |
| `dovetail image push [-p project]` | Publish existing image archives using Docker credentials |
| `dovetail run [project]` | Build and run an application's `main` |
| `dovetail test [project]` | Build and execute discovered tests |
| `dovetail fmt [files...]` | Format local workspace sources/tests, or explicit `.dove` files |
| `dovetail deps fetch` | Fetch declared Git selections and their transitive dependencies |
| `dovetail deps update [alias]` | Explicitly update all dependencies or one selected alias |
| `dovetail lsp-server` | Start the editor language server over stdio |

Run manifest-based commands from the workspace root. `fmt` and `query` also find a
manifest in an ancestor directory; explicit file formatting needs no manifest.
Specify the project when running a multi-project workspace.

### Coding-agent support

`dovetail ai install` installs a progressive language skill and two review profiles.
On first interactive use, select generic agents (including Codex), Claude, or both:

```sh
dovetail ai install --agent generic,claude
dovetail ai install --agent claude --dry-run
dovetail init hello --ai generic,claude
dovetail init hello --no-ai
```

Generic guidance installs under `.agents/skills/dovetail`; Claude uses
`.claude/skills/dovetail` and two `.claude/agents` wrappers. Reviews run only when
requested. Consumer `AGENTS.md` and `CLAUDE.md` files are left untouched.
Commit installed files, including their installation metadata, to share with a team.

The skill assets ship inside the `dovetail-lang` Cargo package and are embedded in
the executable, so `cargo install dovetail-lang` includes AI support. Installation
is offline and uses the executing compiler's bundled guidance. A later
bare `dovetail ai install` refreshes remembered targets; the skill recommends it if
its version differs from the compiler. Edited managed files cause a conflict report
before writing: preserve or resolve those edits and rerun. Explicit targets add or
refresh those targets without removing others. `--dry-run` previews changes.

The command finds the nearest ancestor workspace. Noninteractive first installation
requires `--agent`; noninteractive `init` skips the optional prompt unless `--ai` is
provided. Interactive `init` offers support after workspace creation. If optional
installation fails, the project remains created and the error gives a retry command.

### Checking, building, and testing

```sh
dovetail check hello
dovetail build hello --output-dir build
dovetail test hello --filter "greets"
dovetail test hello --file hello/src/main.dove
```

`--output-dir` also has the short form `-o`. Test filters match the fully qualified test name; repeated `--filter` arguments
are OR'd. `--file` selects a source path (workspace-relative paths are the least
ambiguous). Test output already includes individual results; there is no test
`--verbose` option. Tests can live in `src/` or the project's `test/` directory;
see [Testing](15-testing.md).

Compiler warnings appear during checking, building, running, testing, and in the
editor. They do not fail compilation. Handle discarded `Result`, `Async`, and
`Resource` values or acknowledge an intentional discard with `let _ = ...`.
That acknowledgement does not execute deferred work.

### Container images

Configure `[project.image]` in `Dovetail.toml`, then use `dovetail image build`
and `dovetail image push`. Both select all image-configured local projects unless
`-p` selects one. Building produces local archives; pushing never rebuilds.
See the [container image guide](../docs/container-images.md) for base images,
runtime releases, project settings, and GitHub Actions publishing.

Image WASI grants (`allow-network`, `inherit-env`, and `allow-path` under
`[project.image.wasi]`) default to denied and are fixed at image build time.
There are no startup flag or environment-variable overrides; change the manifest
and rebuild. Mounts, port mappings, and environment values still need corresponding
WASI grants. See [Production Deployment and CI](27-production-deployment.md) for
the complete workflow.

### Formatting

```sh
dovetail fmt
dovetail fmt --check
dovetail fmt hello/src/main.dove
```

The formatter uses four spaces and a 100-column target, preserves comments and
literal text, and checks the structure of its output before accepting it. It does
not fetch or format Git dependencies. It does not insert named arguments or
rewrite synchronous functions into async functions. See [Formatting](../docs/formatting.md).

### Runtime permissions

`run` denies filesystem, network, and inherited environment access unless granted.
Standard input/output are available. Grant only the capabilities the program uses:

```sh
dovetail run hello --allow-cwd
dovetail run server --allow-network
dovetail run worker --env MODE=development -- first-argument second-argument
```

| Run option | Grants |
|---|---|
| `--allow-cwd` | Current directory mounted at `.`; useful for relative file paths |
| `--allow-path PATH` | A specific host path, mounted at that path; repeatable |
| `--allow-root` | Entire host filesystem root |
| `--allow-network` | TCP, UDP, and DNS access |
| `--env KEY=VALUE` | One environment entry; repeatable |
| `--inherit-env` | All host environment entries; explicit `--env` entries override them |
| `-- ARGS...` | Application arguments |

These are options of `run`, not of `test`. The test runner supplies its own host
configuration. Built components also need a compatible host and that host's
permission configuration when executed outside `dovetail run`.

### Discover APIs with `query`

Use compiler-backed declaration queries to explore local projects, dependencies,
and the embedded prelude:

```bash
dovetail query search Connection
dovetail query search transaction --package standard.sqlite
dovetail query package
dovetail query package standard.sqlite
dovetail query definition standard.sqlite.Connection
dovetail query definition standard.sqlite.Connection.transaction
```

`search` returns matching declaration/member names, kinds, projects, and source
locations. Matching is case-insensitive, with exact matches ranked first. Use
`--limit` (default 50) and `--offset` to page results. `--package` restricts search
to that exact package, including members.

`package` lists available packages by project. With a name, it lists immediate
declarations and child packages. `definition` accepts a fully qualified name and
shows documentation and Dovetail signatures; a type and its same-name module are
shown together, and a member query includes its overloads.

Use `--project <name>` to select a local consumer's dependency context. Local
private/internal declarations are included; dependencies show public APIs and
protected subclass members by default. `--all` also exposes nonpublic dependency
declarations for inspection without changing their accessibility. If multiple
contexts provide different versions of a definition, select the intended consumer
with `--project`.

Definition output is a declaration view, **not compilable source**: executable
bodies, initializers, and superclass argument expressions are omitted. It preserves
bounds, modifiers, aliases, and associated types. Type definitions include related
implementation signatures and links to named extensions; importing an extension
remains necessary. Conditional implementation bounds still need to be satisfied.
Read source and tests when behavior cannot be established from signatures.

The embedded prelude can be queried outside a workspace. Dependency queries use
normal resolution, honor `--offline` and `--locked`, and do not request upgrades.
Normal resolution can populate caches and lockfiles. Queries do not emit WASM.

On analysis errors, recoverable declarations are marked `INCOMPLETE`, diagnostics
are printed to stderr, and the exit status is nonzero. Unavailable inferred types
are marked explicitly. A healthy library definition does not require its consumer
application to typecheck. Empty searches succeed; missing targets and ambiguous
or incomplete results fail.

### Language server

Editors normally launch `dovetail lsp-server` over stdio. For a TCP client, use
`dovetail lsp-server --tcp --port 9257`. `--verbose` enables server logging to stderr;
it is a language-server option, not a test option.

### Exit status

Successful commands return zero. Check/build failures return nonzero. Tests return
1 for test failures and 2 for compilation/setup failures; no matching tests returns
zero, so do not use an unchecked filter as proof that tests ran. `fmt --check`
returns 1 when files need formatting; formatter input or processing errors return 2.
CLI usage errors also return nonzero. Preserve exit status in CI rather than
swallowing failures with `|| echo ...`.

## 2.2 Project Structure

A workspace holds projects. Each project lists its source packages and project
dependencies. The compiler supplies the prelude automatically.

```text
my-project/
  Dovetail.toml
  hello/
    src/main.dove
    test/greetingTest.dove
  model/
    src/types.dove
```

To add a library, run `dovetail projects add model`, replace its generated `main`
with library declarations, and add `"model"` to the consuming project's `depends`:

```toml
compiler-version = "0.1.0"

[[project]]
name = "model"
root_package = "example.model"
packages = ["."]

[[project]]
name = "hello"
root_package = "hello"
packages = ["."]
depends = ["model"]
```

`model/src/types.dove` starts with `package example.model`. A declaration used by
`hello` must be `public` and imported there. A library has no `main`; there is no
application/library `type` field. Packages within a project are listed in dependency
order. See [Packages and Modules](11-packages.md) for visibility and dependency rules.

## 2.3 Dependency Management

`compiler-version` is an exact version, not a range. The installed compiler and every
dependency manifest must agree. The compiler and its bundled prelude form one
compatibility unit.

To add standard libraries, set `standard-tag` to an **existing tag with the matching
compiler version**, then select project names through `depends`:

```toml
compiler-version = "0.1.0"
standard-tag = "<matching-tag>"

[[project]]
name = "hello"
root_package = "hello"
packages = ["."]
depends = ["standard-json", "standard-io"]
```

`<matching-tag>` is a placeholder, not a published version promise. For an arbitrary
Git library, use an explicit dependency declaration:

```toml
[[dependencies]]
git = "https://github.com/acme/libraries.git"
rev = "<full-commit-id>"
projects = ["validation"]
```

Use a repository and revision you can access; add `"validation"` to your project's
`depends` to consume it. Git selections can use a branch, tag, or revision, and
can alias selected projects. See [GitHub Dependencies](11-packages.md#115-github-dependencies)
for the full syntax.

```sh
dovetail deps fetch
dovetail check --locked
dovetail deps update
dovetail check --locked --offline
```

Commit `Dovetail.toml` and the generated `Dovetail.lock`. `--locked` forbids a lockfile
change but may download the already-pinned revision. `--offline` forbids network
access and needs cached sources. Combining them requires both an unchanged lock
and sufficient local cache. Updating a dependency is deliberate work: run `deps update`,
review the lock change, and check/test consumers before committing it.

Generated sources, dependency checkouts, and caches live under `.dovetail/`.
Build artifacts live under `build/` unless overridden. Neither directory replaces
the lockfile as the reproducibility record.

## 2.4 CI and Local Compiler Development

For an installed, version-matched compiler, a minimal consumer CI sequence is:

```sh
dovetail deps fetch --locked
dovetail fmt --check
dovetail check --locked
dovetail test --locked
```

Cache `.dovetail/deps/` if useful, but retain the lockfile checks on cached builds.
This is a command sequence, not a dedicated setup action; CI must install the
compiler first.

To run a local compiler checkout against a separate workspace, enter the consumer
workspace and use Cargo's manifest option:

```sh
cargo run --manifest-path /absolute/path/to/dovetail/Cargo.toml -- check
cargo run --manifest-path /absolute/path/to/dovetail/Cargo.toml -- test
```

Cargo selects the compiler repository; Dovetail reads `Dovetail.toml` in the current
working directory. No installed executable is used.

Book maintainers can run `python3 tools/check-book.py` from the compiler checkout.
It checks local book links and runs the explicitly marked complete examples through
the local compiler. See [Book validation](validation.md) for its scope and how to
add examples.
