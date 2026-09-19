use std::path::PathBuf;

use wasmtime::Store;
use wasmtime::component::{Component, Linker};
use wasmtime_wasi::{DirPerms, FilePerms, WasiCtxBuilder};

use crate::p3::State;

pub struct RunError {
    pub message: String,
}

/// Filesystem capabilities granted to a `dovetail run` invocation. Each maps to
/// one or more WASI preopens. Default-deny: with all fields empty/false the
/// component sees an empty preopen list and any path-relative operation will
/// fail with `NotFound`.
#[derive(Default)]
pub struct FsPermissions {
    /// Preopen the current working directory at `.` inside the component.
    pub allow_cwd: bool,
    /// Preopen the host filesystem root at `/` inside the component.
    pub allow_root: bool,
    /// Additional host paths to preopen. Each is mounted at the same absolute
    /// path inside the component.
    pub allow_paths: Vec<PathBuf>,
}

/// Network capabilities granted to a `dovetail run` invocation. Default-deny:
/// without `allow_network`, no `inherit_network` / `allow_tcp` / `allow_udp`
/// / `allow_ip_name_lookup` calls happen on the WASI ctx and any network
/// operation in the component fails with `NetError.AccessDenied`.
#[derive(Default)]
pub struct NetPermissions {
    pub allow_network: bool,
}

/// Environment-var and argument capabilities granted to a `dovetail run`
/// invocation. Default-deny: empty env, empty argv.
#[derive(Default)]
pub struct EnvPermissions {
    /// Resolved env vars, in order. Duplicates already removed by `from_flags`.
    pub variables: Vec<(String, String)>,
    /// argv — user-controlled, only the trailing `--` args.
    pub args: Vec<String>,
}

impl EnvPermissions {
    /// Build from CLI flags. Applies inherit-then-override semantics: if
    /// `inherit_env`, seed with `std::env::vars()`; then for each
    /// `KEY=VALUE` in `env_flags`, upsert (replace existing key, append
    /// if new). Returns Err with a clear message on a malformed flag.
    pub fn from_flags(
        inherit_env: bool,
        env_flags: &[String],
        args: Vec<String>,
    ) -> Result<Self, String> {
        let mut variables: Vec<(String, String)> = if inherit_env {
            std::env::vars().collect()
        } else {
            Vec::new()
        };
        for raw in env_flags {
            let (key, value) = raw
                .split_once('=')
                .ok_or_else(|| format!("--env: expected KEY=VALUE, got '{raw}'"))?;
            if key.is_empty() {
                return Err(format!("--env: empty key in '{raw}'"));
            }
            if let Some(existing) = variables
                .iter_mut()
                .find(|(existing_key, _)| existing_key == key)
            {
                existing.1 = value.to_owned();
            } else {
                variables.push((key.to_owned(), value.to_owned()));
            }
        }
        Ok(Self { variables, args })
    }
}

/// Run a WASI CLI component from raw WASM bytes.
///
/// Inherits stdin/stdout/stderr. Filesystem preopens, env vars, args, and
/// network access are each opt-in via their permissions struct. By default
/// the component sees no fs preopens, empty environment, empty argv, and no
/// network access.
pub fn run_component(
    wasm_bytes: &[u8],
    fs_permissions: &FsPermissions,
    env_permissions: &EnvPermissions,
    net_permissions: &NetPermissions,
) -> Result<(), RunError> {
    let engine = crate::p3::p3_engine().map_err(|message| RunError { message })?;

    let component = Component::new(&engine, wasm_bytes).map_err(|e| RunError {
        message: format!("failed to load component: {e}"),
    })?;

    run_loaded_component(
        &engine,
        &component,
        fs_permissions,
        env_permissions,
        net_permissions,
    )
}

pub(crate) fn run_loaded_component(
    engine: &wasmtime::Engine,
    component: &Component,
    fs_permissions: &FsPermissions,
    env_permissions: &EnvPermissions,
    net_permissions: &NetPermissions,
) -> Result<(), RunError> {
    let mut linker = Linker::<State>::new(engine);
    wasmtime_wasi::p3::add_to_linker(&mut linker).map_err(|e| RunError {
        message: format!("failed to add WASI to linker: {e}"),
    })?;
    // A composed component may embed a p2-based dependency (e.g. the sqlite
    // shim does its disk I/O through synchronous wasi:filesystem/wasi:io), so
    // satisfy the p2 imports too. Harmless when no such dependency is present.
    wasmtime_wasi::p2::add_to_linker_async(&mut linker).map_err(|e| RunError {
        message: format!("failed to add WASI p2 to linker: {e}"),
    })?;

    let mut builder = WasiCtxBuilder::new();
    builder.inherit_stdio();

    if net_permissions.allow_network {
        builder
            .inherit_network()
            .allow_tcp(true)
            .allow_udp(true)
            .allow_ip_name_lookup(true);
    }

    if !env_permissions.variables.is_empty() {
        builder.envs(&env_permissions.variables);
    }
    if !env_permissions.args.is_empty() {
        builder.args(&env_permissions.args);
    }

    if fs_permissions.allow_cwd {
        let cwd = std::env::current_dir().map_err(|e| RunError {
            message: format!("--allow-cwd: cannot read current directory: {e}"),
        })?;
        builder
            .preopened_dir(&cwd, ".", DirPerms::all(), FilePerms::all())
            .map_err(|e| RunError {
                message: format!("--allow-cwd: failed to preopen {}: {e}", cwd.display()),
            })?;
    }

    if fs_permissions.allow_root {
        builder
            .preopened_dir("/", "/", DirPerms::all(), FilePerms::all())
            .map_err(|e| RunError {
                message: format!("--allow-root: failed to preopen /: {e}"),
            })?;
    }

    for host_path in &fs_permissions.allow_paths {
        let guest_mount = host_path.to_string_lossy().into_owned();
        builder
            .preopened_dir(host_path, &guest_mount, DirPerms::all(), FilePerms::all())
            .map_err(|e| RunError {
                message: format!(
                    "--allow-path {}: failed to preopen: {e}",
                    host_path.display()
                ),
            })?;
    }

    let mut store = Store::new(engine, State::new(builder.build()));

    crate::p3::run_cli_component(component, &linker, &mut store)
        .map_err(|message| RunError { message })
}
