use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, path::Path};

fn main() {
    // Cargo packages carry their own lockfile and a normalized Cargo.toml.
    // Use the original manifest and the runtime dependency closure so installed
    // crates and source checkouts have the same identity, independent of tools
    // elsewhere in the workspace or the build host's architecture.
    let lock_path = if Path::new("Cargo.lock").is_file() {
        "Cargo.lock"
    } else {
        "../Cargo.lock"
    };
    println!("cargo:rerun-if-changed={lock_path}");
    let content = std::fs::read_to_string(lock_path).expect("runtime dependency lockfile");
    let lock: toml::Value = toml::from_str(&content).expect("valid Cargo.lock");
    let packages = lock["package"].as_array().expect("lockfile packages");
    let application = packages
        .iter()
        .find(|package| {
            package["name"].as_str() == Some("dovetail-lang")
                && package["version"].as_str() == Some(env!("CARGO_PKG_VERSION"))
        })
        .expect("compiler lockfile entry");
    let mut pending: Vec<_> = dependencies(application)
        .filter(|dependency| {
            ["wasmtime", "wasmtime-wasi"].contains(&dependency.split_whitespace().next().unwrap())
        })
        .collect();
    let mut visited = BTreeSet::new();
    let mut identities = Vec::new();
    while let Some(dependency) = pending.pop() {
        let index = resolve(packages, dependency);
        if !visited.insert(index) {
            continue;
        }
        let package = &packages[index];
        identities.push(format!(
            "{}|{}|{}|{}",
            field(package, "name"),
            field(package, "version"),
            field(package, "source"),
            field(package, "checksum")
        ));
        pending.extend(dependencies(package));
    }
    assert!(
        !identities.is_empty(),
        "runtime dependencies must be present"
    );
    identities.sort();
    let mut hash = Sha256::new();
    hash.update(identities.join("\n"));
    let manifest = if Path::new("Cargo.toml.orig").is_file() {
        "Cargo.toml.orig"
    } else {
        "Cargo.toml"
    };
    for path in [
        manifest,
        "src/p3.rs",
        "src/runner.rs",
        "src/main.rs",
        "src/image/config.rs",
        "src/image/runtime.rs",
    ] {
        println!("cargo:rerun-if-changed={path}");
        hash.update(std::fs::read(path).expect("runtime identity input"));
    }
    println!("cargo:rustc-env=DOVETAIL_RUNTIME_ID={:x}", hash.finalize());
}

fn field<'a>(package: &'a toml::Value, name: &str) -> &'a str {
    package
        .get(name)
        .and_then(toml::Value::as_str)
        .unwrap_or("")
}
fn dependencies(package: &toml::Value) -> impl Iterator<Item = &str> {
    package
        .get("dependencies")
        .and_then(toml::Value::as_array)
        .into_iter()
        .flatten()
        .map(|value| value.as_str().expect("dependency name"))
}
fn resolve(packages: &[toml::Value], dependency: &str) -> usize {
    let mut parts = dependency.split_whitespace();
    let name = parts.next().expect("dependency name");
    let version = parts.next();
    let source = parts.next().map(|s| s.trim_matches(['(', ')']));
    let mut matches = packages.iter().enumerate().filter(|(_, package)| {
        field(package, "name") == name
            && version.is_none_or(|version| field(package, "version") == version)
            && source.is_none_or(|source| field(package, "source") == source)
    });
    let index = matches.next().expect("resolved runtime dependency").0;
    assert!(matches.next().is_none(), "ambiguous runtime dependency");
    index
}
