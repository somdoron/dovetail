# Compiler releases

The [release workflow](../.github/workflows/release.yml) builds official binaries,
publishes the compiler package as `dovetail-lang` on crates.io, and publishes the
VS Code extension to the Visual Studio Marketplace and Open VSX. The installed
executable remains `dovetail`.

Release notes: [0.1.4](releases/0.1.4.md), [0.1.3](releases/0.1.3.md), [0.1.2](releases/0.1.2.md).

## One-time setup

Create a crates.io account and verify its email address. Create an API token with
permission to publish `dovetail-lang`, and store it in this GitHub repository's
Actions secrets as `CARGO_REGISTRY_TOKEN`. The initial release requires permission
to create the crate; later releases require ownership of it. Never commit the token.
See Cargo's [publishing guide](https://doc.rust-lang.org/cargo/reference/publishing.html).

Store a Marketplace publishing token in the repository's Actions secrets as
`VSCE_PAT`. Its account must have publishing access to the `dovetail-lang`
publisher. See the [VS Code automated publishing guide](https://code.visualstudio.com/api/working-with-extensions/continuous-integration#github-actions-automated-publishing).

For Open VSX, complete the publisher setup below and store its access token as
the `OVSX_PAT` repository Actions secret.

## Publishing a version

1. Update `version` in `dovetail/Cargo.toml` and `vscode-dovetail/package.json` to
   the same version. Refresh `Cargo.lock` with Cargo and the extension lockfile
   with `npm install --package-lock-only` in `vscode-dovetail`.
   Update `compiler-version` in workspace and fixture manifests, test manifests,
   and current book examples. Update the extension installation script's VSIX filename.
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
The Visual Studio Marketplace and Open VSX publishing jobs wait for the platform
builds and extension packaging, verify the VSIX checksum, and publish that same
artifact using `VSCE_PAT` and `OVSX_PAT`, respectively. The jobs run independently
so a failure in one registry can be retried without republishing to the other.

Pull requests and manual workflow runs validate the package and build binaries
without publishing. Published versions cannot be overwritten; if publishing
succeeds but another job fails, rerun only the failed jobs.

After assets are uploaded, the workflow promotes the newest published release
(including prereleases) to the `render` branch. Configure Render once to build
this branch with no build filters; see [website hosting](../website/README.md).
Production documentation and the default installer version then match that release.
A failed build/upload does not advance the branch. Rerunning an older release does
not roll the website back. The workflow needs permission to update `render`,
including non-fast-forward updates if release tags are on different branches.

Users install with `curl -fsSL https://dovetaillang.org/install.sh | sh` or, in
Windows PowerShell, `irm https://dovetaillang.org/install.ps1 | iex`. Cargo remains
available with `cargo install dovetail-lang --locked`.

The website build generates `/install.sh` and `/install.ps1` from
`website/installers/`, pinning their default version to `dovetail/Cargo.toml`.
The scripts download binaries and checksums from that GitHub release; the scripts
themselves are served only by the website.

Only bootstrap the `render` branch once the selected release contains the website
installer-generation code. Older tags cannot serve these new endpoints.

## VS Code Marketplace

The release workflow publishes Marketplace updates automatically. GitHub release
assets also let users install the extension with **Extensions: Install from
VSIX...**. For initial publisher setup or manual publication:

1. Create a publisher in the
   [Visual Studio Marketplace management portal](https://marketplace.visualstudio.com/manage).
   The publisher ID must match `publisher` in `vscode-dovetail/package.json`
   (currently `dovetail-lang`). Confirm ownership or availability before the first
   release; changing this ID changes the extension's identity.
2. Download the release's `dovetail-language-<version>.vsix`.
3. Upload it through the publisher portal as a new Visual Studio Code extension,
   or upload it as an update to the existing extension. Each update needs a new
   version. Users who install from the Marketplace receive updates through VS Code.

For a future migration to identity-based CI authentication, publish the already-built release VSIX using
`vsce publish --packagePath <release.vsix> --azure-credential`. Configure Microsoft
Entra ID workload identity federation and grant that identity access to the
Marketplace publisher first. This keeps the store package identical to the
GitHub release artifact without storing a long-lived publishing token.

Microsoft recommends identity-based publishing and documents retirement of global
Azure DevOps personal access tokens on December 1, 2026. See the official
[extension publishing guide](https://code.visualstudio.com/api/working-with-extensions/publishing-extension)
for publisher registration and authentication setup.

## Open VSX

Create an Open VSX account using GitHub, link your Eclipse account, and accept
the Eclipse Publisher Agreement. Generate an access token in your Open VSX
settings and save it as the `OVSX_PAT` GitHub Actions secret.

Before the first automated release, create the `dovetail-lang` namespace. With
`OVSX_PAT` set in your local environment, run from `vscode-dovetail`:

```sh
npm ci
npx --no-install ovsx create-namespace dovetail-lang
```

If the namespace already exists, ensure your account has publishing access.
Namespace ownership verification is a separate step; see the official
[Open VSX publishing guide](https://github.com/eclipse-openvsx/openvsx/wiki/Publishing-Extensions).
The release workflow publishes the existing VSIX with the extension ID
`dovetail-lang.dovetail-language`. Cursor uses Open VSX through its marketplace
proxy, so availability in Cursor may lag publication to the registry.

The extension requires a separately installed `dovetail` executable; the VSIX does
not bundle platform-specific compiler binaries. Keep its combined `LICENSE` in sync
with the repository's `LICENSE-MIT` and `LICENSE-APACHE`.
