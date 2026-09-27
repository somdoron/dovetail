# Image build and release checks

Read [Production deployment](https://dovetaillang.org/book/production-deployment.md), [Tool commands](https://dovetaillang.org/book/tool-commands.md) for syntax and examples.
Follow the [book access workflow](book.md) for version matching and offline fallback.

Use [the image configuration reference](https://dovetaillang.org/guides/container-images.md)
for manifest fields and command examples. Build creates an archive; push uploads
that archive without rebuilding. Neither requires Docker, but running the image
requires an appropriate Linux runtime.

WASI grants are baked into `/app/dovetail-image.json`; deployment arguments cannot
add permissions. Change the manifest and rebuild. Container paths must exist with
appropriate ownership. Mounts, port publishing, and guest grants are separate.
Keep credentials out of copied files and image environment settings.

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
