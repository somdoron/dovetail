# Compiler releases

The [release workflow](../.github/workflows/release.yml) builds official binaries
and publishes the compiler package as `dovetail-lang` on crates.io. The installed
executable remains `dovetail`.

## One-time setup

Create a crates.io account and verify its email address. Create an API token with
permission to publish `dovetail-lang`, and store it in this GitHub repository's
Actions secrets as `CARGO_REGISTRY_TOKEN`. The initial release requires permission
to create the crate; later releases require ownership of it. Never commit the token.
See Cargo's [publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html).

## Publishing a version

1. Update `version` in `dovetail/Cargo.toml` and refresh `Cargo.lock` with Cargo.
2. Run `cargo publish --package dovetail-lang --locked --dry-run` to verify that
   the packaged sources build. This does not upload the crate.
3. Commit the release changes and publish a GitHub release with a matching tag,
   such as `v0.1.0` for package version `0.1.0`.

Publishing a GitHub release triggers all platform builds and smoke tests. The tag
must match the Cargo version. Once those builds and the crate dry run pass, the
workflow publishes to crates.io using the repository secret. A separate job
attaches the platform executables and checksums to the GitHub release.

Pull requests and manual workflow runs validate the package and build binaries
without publishing. Crates.io versions cannot be overwritten; if publishing
succeeds but another job fails, rerun only the failed jobs.

After publication, users can install with `cargo install dovetail-lang`, or add
`--locked` to use the release's recorded dependency versions.
