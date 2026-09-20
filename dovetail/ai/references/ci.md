# GitHub Actions

Start with the bundled [consumer checks workflow](../assets/dovetail-checks.yml).
It installs a version-matched Linux compiler, verifies its release checksum, and
runs locked dependency resolution, formatting, checking, and testing. The example
assumes a root workspace and committed Dovetail.lock. The selected compiler version
must have published release assets; unpublished compiler checkouts require a source
build instead. Select a matching standard-library revision in the manifest.

The template uses Ubuntu x64, explicit version selection, and contents:read.
Dependency caches are optional; if added, key them on the compiler version and
lockfile. Caches do not replace locked resolution. Do not rely on an installed
binary from an unrelated runner image or invent a Dovetail setup action.

For source-based validation, check out the compiler at the intended revision and
run `cargo run --manifest-path /path/to/compiler/Cargo.toml -- <command>` with the
consumer workspace as the working directory. Build dependencies must be available.

Image publishing belongs in a separate trusted push/tag workflow, not untrusted PR
execution. Start from [image publishing](../assets/dovetail-publish.yml), replace
`api` and the manifest registry repository, and keep compiler/library versions aligned.
The workflow first builds an archive, logs into GHCR, then pushes it with the commit
SHA tag. Grant packages:write only to the publishing job. Login and push must share
Docker configuration and helper availability. Never embed registry credentials in
Dovetail.toml or image environment. Public release assets can lag release publication;
wait for matching runtime binaries rather than substituting another version.

Run a normal initial image build to create Dovetail.images.lock and commit it before
using `--locked` in publishing. A build can succeed without Docker installed; running
the resulting image locally is a separate operation. The templates configure CI;
reading this guidance does not authorize publishing a workflow, release, or image.

Validate the exact release revision before publishing: run the same locked fetch,
format check, typecheck, and tests before `image build`, or depend on a check job
for that revision. The publishing template alone does not run those checks.
Smoke-test the published image with its intended mounts, environment, network, and
nonroot user before production promotion. Record its registry digest and promote
that artifact across environments; retain the previous digest for rollback.
Database migrations must remain compatible with the intended rollback.

Deployment supplies port mappings, persistent storage, secrets, resource limits,
restart policy, and health probes. WASI grants must already be present in the
image; deployment flags cannot add them. Provision writable directories for the
configured UID/GID, keep credentials out of manifest image environment and files,
and configure probes externally when the base lacks a shell or probe executable.
An image build is not a deployment, and the stop signal alone does not establish
application-level graceful shutdown.
