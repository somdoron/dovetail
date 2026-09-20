# OCI images

Building creates Linux AMD64/ARM64 precompiled OCI archives. Pushing uploads an
existing archive and never rebuilds. Neither operation needs a Docker daemon,
Dockerfile, or QEMU. Running an image still requires an appropriate Linux runtime.

Place settings after the application's project entry:

```toml
[project.image]
name = "ghcr.io/acme/api"
tag = "latest"
base = "gcr.io/distroless/cc-debian13:nonroot"
platforms = ["linux/amd64", "linux/arm64"]
user = "65532:65532"
workdir = "/app"
expose = ["8080/tcp"]
args = []
stop-signal = "SIGTERM"

[project.image.env]
PORT = "8080"

[project.image.wasi]
allow-network = true
allow-path = ["/data"]

[[project.image.files]]
source = "public"
destination = "/app/public"
```

Customize values for the application; `/data` must exist when the image starts.
WASI permissions default to denied and are baked into `/app/dovetail-image.json`
at image build time. There are no image startup flags or environment variables
that override them. Change `[project.image.wasi]` and rebuild to change grants;
arguments after the image name are application arguments, not runtime flags.
Replacing the internal JSON through a mount is a manual replacement of trusted
runtime configuration, not a dedicated WASI override interface.
Enable `inherit-env` only if guest environment access is intended. `expose` is
metadata, not a port mapping. Copied files are relative to the project; paths outside
it, symlinks, OCI whiteouts, and overriding generated runtime files are rejected.
Manifest `resources` are embedded in the component and need no extra copy.
Labels and annotations use `project.image.labels`/`annotations` tables.
Allowed paths grant read/write WASI access subject to container filesystem
permissions; there are no per-path read-only settings or host:guest mappings.
Use deployment mounts to map host storage into those container paths and to impose
read-only access where needed. Network access is one TCP/UDP/DNS toggle, without
destination allowlists. Environment inheritance exposes all container environment
entries to the guest; keep secrets out of image settings and copied files.

```sh
dovetail image build -p api
dovetail image build -p api --platform linux/amd64
dovetail image push -p api --tag 1.2.3
```

Without `-p`, select all image-configured local applications. The default archive
is `build/images/<project>.oci.tar`; `output` overrides it relative to the workspace.
`name` contains a repository only, not a tag/digest. Push publishes all platforms
in the archive; projects cannot target the same tag. Login/publish requires the
user's intended destination and authorization, not merely loading this reference.

## Runtime matching and reproducibility

Images contain the matching Linux Dovetail executable and architecture-specific
precompiled component. A host macOS executable cannot serve as the Linux runtime.
The builder obtains checksum-verified release assets for its compiler version;
matching runtime build identity also matters. For development builds, set:

```toml
[project.image.runtime]
"linux/amd64" = "dist/dovetail-x86_64-unknown-linux-gnu"
"linux/arm64" = "dist/dovetail-aarch64-unknown-linux-gnu"
```

These paths are workspace-relative; build trusted runtimes from the same checkout
and lockfile on the relevant platforms. Default runtimes need compatible GNU/Linux
loader/libraries; Alpine/scratch are not drop-in bases. A self-contained custom
runtime is required for scratch. Precompiled native artifacts are trusted code;
checksums and identity checks are not a sandbox for malicious artifacts.

Commit `Dovetail.images.lock` alongside `Dovetail.lock`. First build resolves base
image pins; `--locked` requires existing pins. Updating base pins is deliberate.
Offline builds require cached blobs/runtimes or explicit paths. Push cannot run
offline. Credentials come from `$DOCKER_CONFIG/config.json` or Docker's default
configuration, including configured helpers; credentials are not embedded in archives.
