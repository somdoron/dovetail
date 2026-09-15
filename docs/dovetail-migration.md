# Migrating from Domain to Dovetail

Dovetail is the new name of the language. Source syntax and standard-library package names are unchanged.

1. Rename source files from `.domain` to `.dove`, including files under `test/` and any source paths embedded in scripts or macros.
2. Rename `Domain.toml` to `Dovetail.toml` and `Domain.lock` to `Dovetail.lock`. Preserve the lockfile contents and pinned Git revisions. Keep only the new filenames.
3. Use the `dovetail` executable. The Cargo package is `dovetail-lang`; building from this repository uses `cargo run --`. Install from source with `cargo install --path dovetail`.
4. Update `DOMAIN_*` compiler debugging variables to `DOVETAIL_*`.
5. Install the renamed VS Code extension from `vscode-dovetail/`, disable the old extension, and change `domain.*` settings and task/command references to `dovetail.*`. Source files use `.dove`; the editor language ID and TextMate scope are `dovetail` and `source.dovetail`.
6. Update source discovery globs, formatter/editor associations, CI commands, and any hardcoded compiler paths.

Generated files now live under `.dovetail/`. The old `.domain/` directory can be removed after migration once any needed offline dependency cache has been preserved. To avoid downloads, copy `.domain/deps/` to `.dovetail/deps/` and rename each `.domain-checkout-complete` marker there to `.dovetail-checkout-complete`; immutable cached dependency checkouts must themselves reference migrated releases. Regenerate extracted prelude files and generated bindings rather than copying them. Run dependency fetching online before relying on offline builds.

Dependencies whose source manifests still use `Domain.toml` and whose sources still use `.domain` must also migrate. Pin a migrated commit explicitly; do not delete the lockfile to force an implicit upgrade. `--locked` reports any required lockfile update.

The bundled SQLite component now exports `dovetail:sqlite-raw/raw@0.1.0`. Its Rust shim, WIT definition, and binary must migrate together. The Dovetail bindings package remains `sqlite.raw`. Business domains, domain-driven design terminology, network domains, and `standard.*` namespaces retain their existing meaning.

The intended repository is `somdoron/dovetail`, and the website is `dovetaillang.org`. Repository transfer, domain setup, VS Code publisher ownership, and registry publication are separate release tasks. Until the repository and matching tags exist there, `standard-tag` cannot fetch releases from the new location.
