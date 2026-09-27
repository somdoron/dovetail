# Part 27: Production Deployment and CI

A Dovetail release can be packaged as an OCI container image containing the Linux
runtime and a precompiled application. CI validates the source and publishes the
image; your container platform runs it with the required configuration and storage.
This chapter follows an application project named `api`. Replace that name and
the example registry repository with your own.

## 27.1 Prepare a Reproducible Release

Use a compiler matching `compiler-version` in `Dovetail.toml` and a compatible
standard-library revision. Commit the manifest and `Dovetail.lock`. The selected
compiler release must have matching Linux runtime assets available before image
builds can download them. For compiler development, use the explicit runtime paths
described in the [image guide](../../../docs/container-images.md#runtime-releases-and-development-builds).

Add these settings after the application's `[[project]]` entry:

```toml
[project.image]
name = "ghcr.io/acme/api"
platforms = ["linux/amd64", "linux/arm64"]
user = "65532:65532"
expose = ["8080/tcp"]
stop-signal = "SIGTERM"

[project.image.env]
PORT = "8080"

[project.image.wasi]
allow-network = true
inherit-env = true
allow-path = ["/data"]
```

This example assumes the application reads `PORT`, listens on a container-reachable
address such as `0.0.0.0`, and uses `/data` for persistent files. Image configuration
does not implement those application behaviors. Remove grants your application
does not need. Keep passwords, tokens, and private configuration out of image
environment settings and copied files.

Run the initial build to resolve base-image digests:

```sh
dovetail image build -p api
```

Review and commit the resulting `Dovetail.images.lock`. Subsequent release builds
use `--locked` to preserve dependency and base-image selections. It can still
download pinned inputs; `--offline` additionally requires those inputs to be cached.
Update base pins deliberately, then rebuild and validate the new image.

## 27.2 Image Permissions and Deployment Configuration

There are two sets of settings to coordinate:

| Capability | Image setting | Deployment setting |
| --- | --- | --- |
| TCP, UDP, DNS | `allow-network = true` | Container networking, routing, and network policy |
| Incoming connections | Network grant and application listener | Published port or service routing |
| Environment | `inherit-env = true` | Environment values and injected secrets |
| Filesystem | `allow-path = ["/data"]` | Storage mounted at `/data` with appropriate ownership |
| Arguments | Optional image `args` | Container command arguments replace those defaults |

WASI grants default to denied. The builder writes them into
`/app/dovetail-image.json`; there are no startup flags or environment variables
that override them. Change `[project.image.wasi]` and rebuild to change permissions.
Passing `--allow-network` after the image name only passes that string to your
application. An operator could mount a replacement internal JSON file, but this is
a manual replacement of trusted runtime configuration, not a dedicated override
interface.

Allowed filesystem paths refer to the container, not the build or deployment host.
They must exist at startup and are preopened at the same guest path with read/write
permissions. Container OS permissions and read-only mounts still restrict access.
There is no per-path read-only or host:guest mapping syntax in image WASI settings.
Map host storage through the container platform and provision ownership for the
configured user, `65532:65532` in this example.

Network permission enables TCP, UDP, and DNS together; there are no destination
allowlists in image settings. Use your deployment's network controls for narrower
access. Environment inheritance exposes all container environment entries to the
guest. Supplying a volume, environment variable, or port mapping alone cannot grant
guest access.

## 27.3 Configure CI Checks

Copy the bundled [checks workflow](../../../dovetail/ai/assets/dovetail-checks.yml) into
your application's `.github/workflows/dovetail-checks.yml`. Replace
`{{DOVETAIL_VERSION}}` with the exact compiler version from your manifest; this
placeholder is rendered automatically when installed through the AI bundle.
The template downloads the matching Linux executable, verifies its release
checksum, and runs on pull requests and pushes to `main` with `contents: read`.

Its validation sequence runs from the application workspace root:

```sh
dovetail deps fetch --locked
dovetail fmt --check
dovetail check --locked
dovetail test --locked
```

The lockfile must already be committed. If you cache dependency downloads, key
the cache on the compiler version and lockfile, and keep locked checks on cache
hits. Private dependencies also need read credentials available to the job.

These commands use an installed compiler. To validate with a local compiler
checkout, run `cargo run --manifest-path /path/to/compiler/Cargo.toml -- <command>`
from the application workspace instead.

## 27.4 Build and Publish in CI

Copy the bundled [publishing workflow](../../../dovetail/ai/assets/dovetail-publish.yml)
into `.github/workflows/dovetail-publish.yml`. Set the same compiler version,
replace `api`, and configure your repository in `[project.image]`. The template
runs on `v*` tags, grants `packages: write` to the publishing job, and logs into
GHCR with the workflow token. Ensure that token has access to the destination
package. Keep publishing credentials out of untrusted pull-request jobs.

The publishing template builds and pushes; it does not run the checks above.
Insert them before the image build so the tagged revision is validated, or make
publishing depend on a successful check job for that exact revision. Passing checks
on an earlier commit does not validate a later release tag.

The image steps are:

```sh
dovetail image build -p api --locked
# Registry login occurs between these steps in the workflow.
dovetail image push -p api --tag "$GITHUB_SHA"
```

Build creates `build/images/api.oci.tar` for the configured platforms. It requires
no Docker daemon, Dockerfile, or QEMU. Push uploads the existing archive and never
rebuilds it. Login and push must share Docker credential configuration and any
required credential helpers. A separate publishing job must receive the archive
and manifest and perform its own registry login.

Use a commit SHA tag to associate an image with its source, then record the
published registry digest for deployment. Tags can be moved; promote the same
tested digest through staging and production. Retain the archive or published
image and the release's source and lockfiles for diagnosis and rollback.

## 27.5 Run and Verify the Image

After publishing, a Linux container host can run the image. Set `API_IMAGE` to
your published reference, such as `ghcr.io/acme/api@sha256:<actual-digest>`.
Provision `/srv/api-data` on that host with ownership and permissions appropriate
for UID/GID `65532:65532` before starting this example:

```sh
docker run --detach --name api \
  --publish 8080:8080 \
  --mount type=bind,source=/srv/api-data,target=/data \
  --env PORT=8080 \
  "$API_IMAGE"
docker logs api
```

The host path maps into the container path already granted by WASI. The environment
value is visible because the image enabled inheritance. Docker port publishing
and mounts are deployment settings; see the [Docker run reference](https://docs.docker.com/engine/containers/run/)
for their options and image digest syntax. macOS requires a Linux VM or Docker
to execute these Linux images, with emulation when needed for the architecture.

Test the published image with the intended user, environment, storage, and network
before promoting it. Exercise a real application request and a read/write operation
on persistent storage where applicable. Distinguish a missing WASI grant from a
missing directory, incorrect ownership, a read-only mount, or a connection failure.

## 27.6 Operate and Roll Back

Configure resource limits, restart behavior, service routing, and health probes in
your deployment platform. Supply secrets at deployment time, using environment
inheritance or files under an allowed path as appropriate for the application.
Store persistent data outside the container's writable layer and test restoration
from backups.

Expose application health endpoints if your platform needs them. The default
distroless base does not provide a shell for probe scripts; use external probes
or deliberately supply the required executable in a compatible custom base.
Dovetail removes inherited image healthchecks, so configure health monitoring
explicitly. Collect application stdout/stderr through your platform's logging.

`stop-signal` sets container metadata; it does not implement graceful shutdown in
the application. Verify termination behavior and in-flight request handling before
choosing the deployment's shutdown timeout.

Retain the previous successful image digest and deployment configuration. If a
release fails verification, restore those together. Database and file-format
migrations need a compatibility plan: reverting an image does not revert data.
Smoke-test the restored release and verify access to its persistent state.
