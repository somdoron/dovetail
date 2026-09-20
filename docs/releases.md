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

1. Update `version` in `dovetail/Cargo.toml` and `vscode-dovetail/package.json` to
   the same version. Refresh `Cargo.lock` with Cargo and the extension lockfile
   with `npm install --package-lock-only` in `vscode-dovetail`.
2. Run `cargo publish --package dovetail-lang --locked --dry-run` to verify that
   the packaged sources build. This does not upload the crate.
3. Commit the release changes and publish a GitHub release with a matching tag,
   such as `v0.1.0` for package version `0.1.0`.

Publishing a GitHub release triggers all platform builds and smoke tests. The tag
must match the Cargo version. Once those builds and the crate dry run pass, the
workflow publishes to crates.io using the repository secret. A separate job
attaches the platform executables, VS Code extension (`.vsix`), and checksums to
the GitHub release after the extension's TypeScript compilation and packaging pass. The
extension version must also match the release tag.

Pull requests and manual workflow runs validate the package and build binaries
without publishing. Crates.io versions cannot be overwritten; if publishing
succeeds but another job fails, rerun only the failed jobs.

After publication, users can install with `cargo install dovetail-lang`, or add
`--locked` to use the release's recorded dependency versions.

## VS Code Marketplace

GitHub release assets let users install the extension with **Extensions: Install
from VSIX...**. Marketplace publication is separate and is not automated by this
workflow yet.

1. Create a publisher in the
   [Visual Studio Marketplace management portal](https://marketplace.visualstudio.com/manage).
   The publisher ID must match `publisher` in `vscode-dovetail/package.json`
   (currently `dovetail-lang`). Confirm ownership or availability before the first
   release; changing this ID changes the extension's identity.
2. Download the release's `dovetail-language-<version>.vsix`.
3. Upload it through the publisher portal as a new Visual Studio Code extension,
   or upload it as an update to the existing extension. Each update needs a new
   version. Users who install from the Marketplace receive updates through VS Code.

For CI automation, publish the already-built release VSIX using
`vsce publish --packagePath <release.vsix> --azure-credential`. Configure Microsoft
Entra ID workload identity federation and grant that identity access to the
Marketplace publisher first. This keeps the store package identical to the
GitHub release artifact without storing a long-lived publishing token.

Microsoft recommends identity-based publishing and documents retirement of global
Azure DevOps personal access tokens on December 1, 2026. See the official
[extension publishing guide](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)
for publisher registration and authentication setup.

The extension requires a separately installed `dovetail` executable; the VSIX does
not bundle platform-specific compiler binaries. Keep its combined `LICENSE` in sync
with the repository's `LICENSE-MIT` and `LICENSE-APACHE`.
