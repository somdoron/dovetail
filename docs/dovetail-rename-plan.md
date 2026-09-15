# Domain → Dovetail rename plan

Status: repository rename implemented and validated. External ownership, repository transfer, and publishing remain separate release tasks. See [migration instructions](dovetail-migration.md).

## Naming contract

| Surface | Current | Target |
|---|---|---|
| Language | Domain | Dovetail |
| CLI executable | `domain` | `dovetail` |
| Cargo package | `domain` | `dovetail-lang` |
| Rust library name | `domain` | `dovetail` (explicit `[lib]` name) |
| Compiler directory | `domain/` | `dovetail/` |
| Source extension | `.domain` | `.dove` |
| Workspace manifest | `Domain.toml` | `Dovetail.toml` |
| Dependency lockfile | `Domain.lock` | `Dovetail.lock` |
| Generated files and dependency cache | `.domain/` | `.dovetail/` |
| Checkout completion marker | `.domain-checkout-complete` | `.dovetail-checkout-complete` |
| Compiler environment variables | `DOMAIN_*` | `DOVETAIL_*` |
| Editor directory | `vscode-domain/` | `vscode-dovetail/` |
| VS Code extension name | `domain-language` | `dovetail-language` |
| VS Code publisher | `domain-lang` | `dovetail-lang` (requires securing publisher separately) |
| Editor language ID / grammar scope | `domain` / `source.domain` | `dovetail` / `source.dovetail` |
| GitHub repository | existing repository | `somdoron/dovetail` |
| Website | — | `https://dovetaillang.org` |

Use explicit `[[bin]] name = "dovetail"` and `[lib] name = "dovetail"` so the published package name does not dictate the executable or Rust import name. Update Cargo workspace membership, path dependencies, package selectors, and lockfile metadata together. `cargo install dovetail-lang` should install `dovetail`; local development should continue using `cargo run --`.

## Migration policy

Recommend a coordinated breaking cutover while the project is at version 0.1.0. Do not maintain parallel source extensions, commands, or manifest formats by default. The authorized implementation uses this coordinated cutover; consumers must migrate together.

Preserve dependency revisions when renaming existing lockfiles. New caches can be rebuilt; do not silently resolve newer dependency versions. Audit cached path identities and test locked/offline behavior after migration. Give an actionable error for an old manifest instead of treating the project as absent. A workspace with both manifest names should fail clearly rather than silently selecting one.

Rename language branding, not every occurrence of the English word “domain.” Preserve business domains, domain-driven design, DNS/HTTP/TLS domains, mathematical domains, domain-model examples, and third-party protocol identifiers. Keep `standard.*` package namespaces. The local checkout directory `domain2` is independent of the compiler directory rename and can be renamed separately.

## Existing work to preserve

At implementation time the prelude move was already complete. The pre-existing edit to `docs/Backlog.md` was preserved while updating language references. The bundled prelude now lives in `dovetail/prelude/`.

Implementation renamed all 309 tracked `.domain` files to `.dove`, including the prelude, standard libraries, WASI code, consumer fixtures, and editor grammar fixtures.

## Implementation sequence

### 1. Repository and compiler identity

- Move `domain/` to `dovetail/`; update root Cargo configuration and `tools/p3-table-gen` dependency metadata.
- Rename Rust imports in the compiler binary, integration tests, and generator tools to `dovetail::`.
- Update CLI help, diagnostic branding, executable lookups, `CARGO_BIN_EXE_*` references if present, and temporary-path prefixes.
- Update paths in embedded prelude handling, `include_str!`/`include_dir!` references, WIT generator tests, and generator instructions.
- Set repository/homepage/description metadata to the agreed identity when ownership is confirmed; verify packaging includes the bundled prelude and WIT files.

### 2. Workspace and source formats

- Rename source files to `.dove`, including test and editor fixtures. Update filename literals, glob patterns, source discovery, prelude enumeration, macro-generated source paths, and generated WIT bindings.
- Rename manifests and lockfiles, including manifests synthesized in Rust tests and documentation examples.
- Update manifest discovery/loading, dependency fetch/update behavior, diagnostics, `init`, `add`, and generated `.gitignore` entries.
- Move generated-path conventions to `.dovetail/`, including dependencies, extracted prelude files, generated bindings, and completion markers.
- Ensure init/add produce `Dovetail.toml` and `main.dove`; ensure test discovery still finds source tests after the extension change.

Relevant implementation areas: `src/discovery.rs`, `src/manifest/`, `src/compiler/pipeline.rs`, `src/compiler/witgen/`, and integration-test helpers.

### 3. Runtime and generated identities

- Rename `DOMAIN_LSP_LOG`, `DOMAIN_DEBUG_RHAI`, `DOMAIN_DUMP_WIT_IMPORTS`, and `DOMAIN_DUMP_TEST_WASM` and their documented examples.
- Rename compiler-owned WIT test/package labels such as `domain:tests`, `domain:program`, `domain:plug*`, plus debug producer metadata.
- Rebuild the SQLite shim with the user-approved `dovetail:sqlite-raw` ABI namespace, updating its WIT and Rust exports together. Preserve the `sqlite.raw` language package. Other external WIT identities must still match their artifacts; never rewrite binary artifacts through text replacement.
- Regenerate compiler-owned outputs using their generators where appropriate, including checking the P3 table drift test.

### 4. Language server and VS Code

- Update server identity, log prefixes, diagnostic source, manifest/lockfile watchers, generated-file navigation, extracted prelude navigation, and extension filtering.
- Move the extension directory and grammar filename; update package identity, publisher, language ID, aliases, TextMate scopes, configuration keys, task types, command IDs, problem matchers, output channels, and default executable.
- Update server/client test command identifiers together so code lenses and test commands remain connected.
- Update build/install scripts, grammar test globs, snapshots if present, and ignore rules. Regenerate ignored npm lock metadata locally as needed without introducing it as a tracked file unintentionally.
- Document reinstalling the renamed extension and changing `domain.*` settings to `dovetail.*`. Check whether an existing published extension needs a migration notice.

### 5. Documentation and agent guidance

- Update `CLAUDE.md`, book chapters, specifications, design documents, README files, code examples, commands, links, and filenames where they refer to the language.
- Add a root `AGENTS.md` (none was found in this checkout). Keep the naming rule explicit in both files and reference shared guidance to avoid divergent policies. Carry over relevant build, style, compiler-bug, and staging policies rather than creating conflicting instructions.
- Use this naming rule in both files:

  > The language is Dovetail, formerly called Domain. The user may still say “Domain” or “the Domain language” out of habit; when referring to this language, compiler, or tooling, interpret that as Dovetail. Use Dovetail in new prose and code references, with `.dove` source files, `Dovetail.toml`, and the `dovetail` executable. Ordinary uses of “domain,” including domain-driven design and network domains, retain their meaning.

- Preserve the instruction to use the locally built compiler through `cargo run --` instead of a possibly stale installed binary.
- Add concise migration instructions covering source files, manifests, lockfiles, editor settings, and regenerated caches.

### 6. CI and external identity

- Update `.github/workflows/rust.yml`, particularly `cargo run --bin domain -- test`, and language-test step labels.
- Check helper scripts, consumer projects, generator paths, packaging commands, and repository links.
- External setup is a separate execution step: acquire the domain, create the GitHub organization, transfer/rename the repository while preserving history and issues, update remotes and repository settings, secure the VS Code publisher, and publish the Cargo package when release-ready. These resources have not been confirmed as acquired by this plan.
- Validate the Cargo package archive and perform a dry run before publishing; use an isolated local install to confirm the executable name and embedded resources.

## Validation and completion criteria

1. Capture the existing Rust and language-workspace test baseline before the cutover.
2. Run Rust formatting checks, workspace build/tests, and Clippy; compare failures with the baseline. Include generator drift checks.
3. Run `cargo run -- check`, `cargo run -- build`, and `cargo run -- test` on the real workspace. Preserve CI's `RUST_MIN_STACK=16777216` setting for language tests where needed.
4. Smoke-test a fresh project through init, check, build, run, and test. Verify add-project output and old/both-manifest diagnostics.
5. Exercise Git dependency fetch, locked/offline resolution, generated bindings, component composition, macros, and bundled prelude loading from an isolated installed compiler outside this checkout.
6. Compile the extension and run grammar tests. Manually check `.dove` highlighting, diagnostics, completion, go-to-definition into prelude/generated bindings, test code lenses, and manifest/lockfile change detection.
7. Audit remaining `Domain`, `domain`, `.domain`, and `DOMAIN_` references and paths. Classify legitimate conceptual/vendor/history uses instead of requiring zero textual matches. Require no unintended legacy executable, manifest, source-extension, or editor identifiers in active workflows.
8. Review the diff for accidental semantic edits, missing renamed sources, and preservation of pre-existing work. The intended behavior change is naming and project discovery only.

The repository implementation remains unstaged for review. Domain purchases, account creation, repository transfer, and registry publication have not been performed.

## Execution notes

- Renamed all 309 tracked source files and updated compiler, manifest, lockfile, cache, CLI, editor, CI, and documentation references. The compiler package is `dovetail-lang`, with library and executable named `dovetail`.
- Added shared naming guidance to `AGENTS.md` and `CLAUDE.md`, and migration errors for legacy or duplicate manifests/lockfiles. Legacy caches remain ignored while users migrate.
- Rebuilt the SQLite component from its WIT and Rust shim with the approved `dovetail:sqlite-raw/raw@0.1.0` ABI. `wasm-tools validate --features all` passed, and exported WIT confirms the new namespace. The language package remains `sqlite.raw`.
- Real workspace check and build passed; all **1,708 language tests passed**, including SQLite and its transitive consumer.
- Clippy passed with warnings. Formatting still reports the same **185 files** as the pre-rename baseline, with no newly affected files; unrelated formatting was preserved.
- The VS Code extension compiles and packages. All three grammar fixtures produce identical tokenization to the original grammar after normalizing the renamed scopes. LSP integration suites passed; interactive editor UI behavior was not manually exercised.
- Cargo packaging and publication dry run passed; no package was uploaded. A temporary `cargo install` produced the `dovetail` executable, which passed fresh-project init/check/build/run/test, add-project, locked/offline, embedded-prelude, and legacy-manifest smoke checks outside this checkout.
- Rust library tests, including both new migration regressions, passed. The first integration run caught the HTTP fixture's old Content-Length after the greeting rename; the assertion was updated to 19 bytes. The corrected HTTP and HTTPS interoperability tests passed. All 107 compiler integration suites, generator drift/variant tests, tool unit tests, and doctests passed across the initial and resumed runs (3279 Rust tests passed; 7 ignored).
- Validation uses `/tmp/dovetail-validation-target` with debug symbols disabled. The original target directory ran out of space, then stalled Rust directory scans; obsolete Domain binaries were removed and validation moved to the isolated target. The initial full baseline test run was stopped after successful compilation and partial test results.
- Domain/account acquisition, repository transfer, VS Code publisher registration, and registry publication remain external release tasks. Canonical links and the `standard-tag` repository now target `somdoron/dovetail`; that repository and matching migrated tags must exist before release.
