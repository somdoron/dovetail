//! Componentize the hand-written async-stackful sqlite core module.
//!
//! Usage: componentize-sqlite <wit-dir> <core.wasm> <adapter.wasm> <out.wasm>
//!
//! Mirrors how the Dovetail compiler wraps its own async-stackful exports:
//! `embed_component_metadata` (carries the `async func` world) + a
//! `ComponentEncoder` that lifts the module's `[async-lift-stackful]` exports
//! async-without-callback, wiring the p1 libc imports through the reactor
//! adapter.

use std::path::Path;

use wit_component::{ComponentEncoder, StringEncoding, embed_component_metadata};
use wit_parser::Resolve;

fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() != 5 {
        anyhow::bail!("usage: componentize-sqlite <wit-dir> <core.wasm> <adapter.wasm> <out.wasm>");
    }
    let (wit_dir, core_path, adapter_path, out_path) = (&args[1], &args[2], &args[3], &args[4]);

    let mut resolve = Resolve::default();
    let (pkg, _sources) = resolve.push_path(Path::new(wit_dir))?;
    let world = resolve.select_world(&[pkg], Some("sqlite"))?;

    let mut core = std::fs::read(core_path)?;
    embed_component_metadata(&mut core, &resolve, world, StringEncoding::UTF8)?;

    let adapter = std::fs::read(adapter_path)?;
    let component = ComponentEncoder::default()
        .module(&core)?
        .adapter("wasi_snapshot_preview1", &adapter)?
        .encode()?;
    std::fs::write(out_path, &component)?;
    eprintln!("wrote {} ({} bytes)", out_path, component.len());
    Ok(())
}
