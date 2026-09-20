# WASI runtime capabilities

Dovetail applications are Wasm components. `dovetail run` denies filesystem,
network, and inherited environment access by default; standard input/output are
available. Grant the capabilities the application actually needs.

| Runtime option | Capability |
|---|---|
| `--allow-cwd` | Preopen current working directory at `.` |
| `--allow-path PATH` (repeatable) | Preopen the host path at that same guest path |
| `--allow-root` | Preopen the host filesystem root at `/` |
| `--allow-network` | Enable TCP, UDP, and DNS |
| `--env KEY=VALUE` (repeatable) | Pass explicit environment values; later values win |
| `--inherit-env` | Inherit host environment; explicit env flags override it |
| `-- arguments...` | Pass guest command-line arguments |

```sh
dovetail run worker --allow-path /srv/data --env MODE=development -- input.json
dovetail run server --allow-network
```

Paths must exist and be appropriate to the current working directory. Do not
invent host:guest remapping syntax or granular network allowlists. Root access is
broad; prefer selected paths when sufficient. Diagnose denied capabilities separately
from missing files, connection failures, and application logic. Do not change code
just to hide a missing runtime grant.

For packaged images configure `[project.image.wasi]` with `allow-network`,
`inherit-env`, and `allow-path`. Paths refer to the container filesystem, not the
build host. A container mount, environment variable, or exposed port does not itself
grant guest access. Port publishing and actual mounts remain deployment concerns.
