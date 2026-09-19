use super::config::{ImageConfig, Platform, WasiConfig};
use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use wasmtime::{Config, Engine};

pub const RUNTIME_ID: &str = concat!("dovetail-runtime:", env!("DOVETAIL_RUNTIME_ID"));

pub fn engine(platform: Platform) -> Result<Engine> {
    let mut config = Config::new();
    crate::p3::configure_p3_engine(&mut config);
    // An explicit target disables host CPU feature inference, including when
    // cross-compiling on macOS. Both architectures use the baseline ISA.
    config.target(platform.target())?;
    Ok(Engine::new(&config)?)
}

pub fn precompile(wasm: &[u8], platform: Platform) -> Result<Vec<u8>> {
    engine(platform)?
        .precompile_component(wasm)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("precompiling application component")
}

pub fn validate_binary(bytes: &[u8], platform: Platform) -> Result<()> {
    ensure!(
        bytes.len() >= 20 && &bytes[..4] == b"\x7fELF" && bytes[4] == 2 && bytes[5] == 1,
        "runtime must be a 64-bit little-endian Linux ELF executable"
    );
    let machine = u16::from_le_bytes([bytes[18], bytes[19]]);
    ensure!(
        machine
            == match platform {
                Platform::Amd64 => 62,
                Platform::Arm64 => 183,
            },
        "runtime architecture does not match {}",
        platform.name()
    );
    ensure!(
        bytes
            .windows(RUNTIME_ID.len())
            .any(|part| part == RUNTIME_ID.as_bytes()),
        "runtime was built with a different Dovetail runtime or Cargo.lock; build both from the same revision using --locked"
    );
    Ok(())
}

pub async fn binary(
    root: &Path,
    config: &ImageConfig,
    platform: Platform,
    offline: bool,
) -> Result<Vec<u8>> {
    if let Some(path) = config.runtime.get(&platform.name()) {
        let path = root.join(path);
        let bytes =
            std::fs::read(&path).with_context(|| format!("reading runtime {}", path.display()))?;
        validate_binary(&bytes, platform)?;
        return Ok(bytes);
    }
    let cache = root
        .join(".dovetail/images/runtimes")
        .join(env!("DOVETAIL_RUNTIME_ID"))
        .join(platform.architecture());
    if cache.is_file() {
        let bytes = std::fs::read(&cache)?;
        let checksum = std::fs::read_to_string(cache.with_extension("sha256"))
            .context("cached runtime has no checksum; remove it and rebuild")?;
        ensure!(
            super::archive::digest(&bytes) == checksum.trim(),
            "cached runtime checksum mismatch: {}",
            cache.display()
        );
        validate_binary(&bytes, platform)?;
        return Ok(bytes);
    }
    if cfg!(target_os = "linux") {
        let bytes = std::fs::read(std::env::current_exe()?)?;
        if validate_binary(&bytes, platform).is_ok() {
            return Ok(bytes);
        }
    }
    ensure!(
        !offline,
        "runtime {} is not cached; configure project.image.runtime or fetch it without --offline",
        platform.name()
    );
    let url = format!(
        "https://github.com/somdoron/dovetail/releases/download/v{}/dovetail-{}",
        env!("CARGO_PKG_VERSION"),
        platform.target()
    );
    let client = reqwest::Client::new();
    let bytes = client.get(&url).send().await?.error_for_status().with_context(|| format!("runtime release unavailable at {url}; configure [project.image.runtime] for a development build"))?.bytes().await?;
    let checksum = client
        .get(format!("{url}.sha256"))
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?;
    ensure!(
        checksum.split_whitespace().next()
            == Some(super::archive::digest(&bytes).trim_start_matches("sha256:")),
        "runtime checksum mismatch"
    );
    validate_binary(&bytes, platform)?;
    super::archive::atomic_write(&cache, &bytes)?;
    super::archive::atomic_write(
        &cache.with_extension("sha256"),
        super::archive::digest(&bytes).as_bytes(),
    )?;
    Ok(bytes.to_vec())
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionConfig {
    pub runtime_id: String,
    pub component: PathBuf,
    pub digest: String,
    pub wasi: WasiConfig,
}

/// Executes native code produced by the image builder. This command is an
/// explicit trust boundary, like executing a native binary: the config and
/// component must both come from a trusted image, never untrusted Wasm input.
pub fn execute(config_path: &Path, args: Vec<String>) -> Result<()> {
    let config: ExecutionConfig = serde_json::from_slice(&std::fs::read(config_path)?)?;
    ensure!(
        config.runtime_id == RUNTIME_ID,
        "image runtime identity mismatch"
    );
    let bytes = std::fs::read(&config.component)?;
    ensure!(
        super::archive::digest(&bytes) == config.digest,
        "precompiled component digest mismatch"
    );
    let engine = crate::p3::p3_engine().map_err(anyhow::Error::msg)?;
    // SAFETY: the explicit image execution command accepts trusted native code.
    // Digest checks detect corruption, not authenticity of an untrusted config.
    let component = unsafe { wasmtime::component::Component::deserialize(&engine, &bytes) }?;
    let filesystem = crate::runner::FsPermissions {
        allow_paths: config.wasi.allow_path,
        ..Default::default()
    };
    let environment = crate::runner::EnvPermissions::from_flags(config.wasi.inherit_env, &[], args)
        .map_err(anyhow::Error::msg)?;
    let network = crate::runner::NetPermissions {
        allow_network: config.wasi.allow_network,
    };
    crate::runner::run_loaded_component(&engine, &component, &filesystem, &environment, &network)
        .map_err(|e| anyhow::anyhow!("{}", e.message))
}
