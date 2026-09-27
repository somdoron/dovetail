# WASI runtime capabilities

Dovetail applications are Wasm components. `dovetail run` denies filesystem,
network, and inherited environment access by default; standard input/output are
available. Grant the capabilities the application actually needs.

Read [runtime permissions](https://dovetaillang.org/book/tool-commands.md#runtime-permissions)
for flags, and [production deployment](https://dovetaillang.org/book/production-deployment.md)
for image settings. Follow the [book access workflow](book.md).

Paths must exist and be appropriate to the current working directory. Do not
invent host:guest remapping syntax or granular network allowlists. Root access is
broad; prefer selected paths when sufficient. Diagnose denied capabilities separately
from missing files, connection failures, and application logic. Do not change code
just to hide a missing runtime grant.

For packaged images configure `[project.image.wasi]` with `allow-network`,
`inherit-env`, and `allow-path`. Paths refer to the container filesystem, not the
build host. A container mount, environment variable, or exposed port does not itself
grant guest access. Port publishing and actual mounts remain deployment concerns.
Image grants default to denied and are recorded at build time in
`/app/dovetail-image.json`. Change the manifest and rebuild to change them; image
startup has no WASI flag or environment-variable overrides. Normal container
arguments go to the application. Replacing the internal JSON is a manual trusted
configuration replacement, not a dedicated override interface. Image paths grant
read/write access subject to OS permissions and mount restrictions; image settings
do not provide per-path read-only grants, path remapping, or network allowlists.
