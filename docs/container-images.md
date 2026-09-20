# Container images

`dovetail image build` compiles configured applications, precompiles each Wasm
component for Linux AMD64 and ARM64, and creates an OCI archive per project.
`dovetail image push` publishes the existing archives using Docker credentials.
Neither command requires a Docker daemon, Dockerfile, or QEMU.

Add image settings after the application's `[[project]]` entry in `Dovetail.toml`:

```toml
[project.image]
name = "ghcr.io/acme/api"
tag = "latest"
```

```sh
dovetail image build -p api
docker login ghcr.io
dovetail image push -p api
```

Omit `-p` to build or push all local projects with image configuration. Imported
projects and libraries without image configuration are not selected. Configured
image projects must have an application entry point.

The default archive is `build/images/<project>.oci.tar`, relative to the workspace
root. Both commands use the same manifest setting when `output` overrides that
path. Push validates all selected archives and destinations before uploading;
it never rebuilds. An archive can be published even if the project's sources or
Git dependencies are unavailable. Run commands from the workspace root.

```sh
dovetail image build -p api --platform linux/amd64
dovetail image push -p api --tag 1.2.3
```

`--tag` replaces only the tag, retaining each project's configured repository.
Push uploads every platform present in the archive, regardless of the current
build platform settings. Two selected projects cannot publish to the same tag.

## Configuration

```toml
[project.image]
name = "ghcr.io/acme/api" # repository only, without tag or digest
tag = "latest"
base = "gcr.io/distroless/cc-debian13:nonroot"
platforms = ["linux/amd64", "linux/arm64"]
output = "build/images/api.oci.tar"
user = "65532:65532"
workdir = "/app"
expose = ["8080/tcp"]
args = []
stop-signal = "SIGTERM"

[project.image.env]
PORT = "8080"

[project.image.labels]
"org.opencontainers.image.title" = "API"

[project.image.annotations]
"org.opencontainers.image.description" = "Application image"

[[project.image.files]]
source = "public"
destination = "/app/public"

[project.image.wasi]
allow-network = true
inherit-env = true
allow-path = ["/data"]
```

Precompilation is mandatory: there is no setting to disable it and no fallback
to startup compilation. Each platform image contains its matching Linux Dovetail
executable and architecture-specific `/app/application.cwasm`.

WASI permissions default to denied, matching `dovetail run`. Setting a container
environment variable, publishing a port, or mounting a directory does not itself
grant the Wasm guest access. Enable network/environment access and list container
paths as needed. Listed paths must exist when the container starts. `expose` is
image metadata; the deployment still supplies port mappings and volume mounts.
Application arguments override image `args` through the container runtime's normal
command arguments. No shell is involved in the generated entrypoint.

These WASI grants are baked into `/app/dovetail-image.json` during image build.
There are no startup flags or environment variables for overriding them: change
`[project.image.wasi]` and rebuild the image. Passing `--allow-network` after the
image name passes an application argument; it does not enable network access.
An operator can manually mount a replacement internal JSON configuration, but
that replaces trusted runtime configuration rather than using a dedicated WASI
override interface.

`allow-network` is one toggle for TCP, UDP, and DNS, without destination allowlists.
`inherit-env` exposes all container environment entries to the guest. `allow-path`
grants read/write access at the same container and guest path, subject to OS
permissions and mount restrictions. There are no per-path read-only grants or
host:guest mappings in these settings; configure mappings and read-only mounts in
the deployment. See [Production Deployment and CI](../book/27-production-deployment.md)
for a release workflow and deployment configuration example.

Base layers and their environment and labels are retained; explicitly configured
values override them. Dovetail replaces the process entrypoint, arguments, user,
working directory, ports, and stop signal, and removes an inherited healthcheck.
Additional files are copied from paths relative to the project directory, with
regular files readable as mode 0644 and directories as 0755. Symlinks, paths
outside the project, OCI whiteout names, and overrides of generated runtime files
are rejected. Application resources declared with `resources` remain embedded in
the component and need no extra copy setting.

Additional operating-system packages belong in a custom, already-built base.
A custom base must supply the runtime's compatible GNU/Linux loader and libraries;
Alpine and `scratch` are not suitable for the default dynamically linked binary.
`scratch` is supported only with a self-contained runtime supplied by the user.

## Runtime releases and development builds

The builder downloads matching release executables from the Dovetail GitHub
release `v<compiler-version>`, with assets named:

- `dovetail-x86_64-unknown-linux-gnu` and its `.sha256` file
- `dovetail-aarch64-unknown-linux-gnu` and its `.sha256` file

Downloads are checksum-verified and cached under `.dovetail/images/runtimes`.
Executables must also contain the matching runtime build identity and have the
correct ELF architecture. The runtime identity covers pinned Wasmtime/Cranelift dependency versions,
crate configuration, runtime configuration schema, CLI, and runtime implementation. The image runner checks this
identity and the component digest before deserializing the native artifact.
These checks detect mismatches and corruption; they do not make untrusted native
code safe to execute. The internal `image run` command is only for trusted images.

The `Release binaries` workflow runs when a GitHub release is published (including
prereleases). Its release tag must equal `v<version>` from `dovetail/Cargo.toml`.
It builds and smoke-tests official executables for Linux AMD64/ARM64, macOS
Intel/Apple Silicon, and Windows x64, then attaches each binary and its SHA-256
checksum to that existing release after every platform succeeds. Linux jobs also
execute the packaged container. Pull requests and manual runs validate builds
without uploading to a release.

Additional official assets are named `dovetail-x86_64-apple-darwin`,
`dovetail-aarch64-apple-darwin`, and `dovetail-x86_64-pc-windows-msvc.exe`, each
with a `.sha256` companion. Unix downloads need executable permission
(`chmod +x`) and can be renamed to `dovetail`; rename the Windows binary to
`dovetail.exe`. The Linux executables are also the image runtimes.

The release becomes visible before the build finishes; binaries are available
once the workflow completes successfully. **Until matching Linux assets have
been attached**, development builds need explicit Linux runtime paths:

```toml
[project.image.runtime]
"linux/amd64" = "dist/dovetail-x86_64-unknown-linux-gnu"
"linux/arm64" = "dist/dovetail-aarch64-unknown-linux-gnu"
```

Paths are relative to `Dovetail.toml` (absolute paths also work). Build both Linux
executables and the host compiler from the same checkout and Cargo.lock using
`cargo build --release --locked --bin dovetail`, on the appropriate native hosts
or with a suitable cross toolchain. A Linux host can reuse its running Dovetail
executable for its own architecture. A macOS executable cannot be packaged as a
Linux runtime. Overridden executables remain the caller's trusted input.

### Cross-compiling local runtimes from macOS

With Zig installed, install the Cargo wrapper and Linux Rust targets, then build
both runtimes from the compiler checkout:

```sh
cargo install cargo-zigbuild --locked
rustup target add x86_64-unknown-linux-gnu aarch64-unknown-linux-gnu
cargo zigbuild --release --locked --bin dovetail \
  --target x86_64-unknown-linux-gnu \
  --target aarch64-unknown-linux-gnu
```

Set the runtime overrides to the resulting
`target/x86_64-unknown-linux-gnu/release/dovetail` and
`target/aarch64-unknown-linux-gnu/release/dovetail` executables. Use absolute
paths when the application lives outside the compiler checkout. Building the
image archive requires no Docker; executing its Linux images on macOS requires
a Linux VM or Docker, with emulation for the non-native CPU architecture.

## Reproducibility and offline builds

The first build records platform-specific base-image digests in
`Dovetail.images.lock`; commit that file alongside `Dovetail.lock`. Subsequent
builds reuse those digests even if a base tag moves. To update bases, remove the
relevant entries from `Dovetail.images.lock` and rebuild without `--locked`.

`--locked` requires existing base-image pins and leaves the image lockfile
unchanged. `--offline` requires cached base blobs and runtime binaries (or explicit
runtime paths). Both also retain their normal source-dependency behavior.
`image push --offline` is an error.

Generated layers use stable ordering, modes, ownership, and timestamps. Archives
are written atomically so an unsuccessful build does not truncate an existing
archive. Matching source inputs, runtime, base pins, and image settings produce
repeatable image metadata and layers.

## Registry credentials and GitHub Actions

Dovetail reads `$DOCKER_CONFIG/config.json`, or `~/.docker/config.json` by default.
It respects registry-specific credential helpers, the default credential store,
and inline credentials, including Docker Hub's credential key. Helper programs
must be available on `PATH`. Identity tokens are exchanged for scoped access
tokens. Dovetail neither stores credentials nor includes them in the archive.
Registry traffic uses HTTPS.

After checkout and installing a matching Dovetail release:

```yaml
permissions:
  contents: read
  packages: write

# Inside a job's steps:
steps:
  - run: dovetail image build -p api

  - uses: docker/login-action@v4
    with:
      registry: ghcr.io
      username: ${{ github.actor }}
      password: ${{ secrets.GITHUB_TOKEN }}

  - run: dovetail image push -p api --tag "$GITHUB_SHA"
```

The login and push steps must share the Docker configuration and access to any
configured credential helper. A separate publishing job can download the archive
and `Dovetail.toml`, log in, and run push without compiling the application again.

Archives are standard OCI layouts wrapped in tar, with a `latest` local reference
to the multi-platform index. External tools can publish them too:

```sh
skopeo copy --all oci-archive:build/images/api.oci.tar docker://ghcr.io/acme/api:1.2.3
```
