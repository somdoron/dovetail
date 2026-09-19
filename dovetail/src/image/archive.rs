use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Write,
    path::{Path, PathBuf},
};

pub const INDEX: &str = "application/vnd.oci.image.index.v1+json";
pub const MANIFEST: &str = "application/vnd.oci.image.manifest.v1+json";
pub const CONFIG: &str = "application/vnd.oci.image.config.v1+json";
pub const LAYER: &str = "application/vnd.oci.image.layer.v1.tar+gzip";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Descriptor {
    pub media_type: String,
    pub digest: String,
    pub size: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub platform: Option<Value>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub annotations: BTreeMap<String, String>,
}
pub fn digest(bytes: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(bytes))
}
pub fn blob_path(root: &Path, digest: &str) -> Result<PathBuf> {
    let hash = digest
        .strip_prefix("sha256:")
        .context("only SHA-256 image digests are supported")?;
    ensure!(
        hash.len() == 64
            && hash
                .bytes()
                .all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()),
        "invalid SHA-256 image digest"
    );
    Ok(root.join("blobs/sha256").join(hash))
}
pub fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    let parent = path.parent().context("output has no parent")?;
    std::fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary.write_all(bytes)?;
    temporary.persist(path)?;
    Ok(())
}
pub fn put(root: &Path, media_type: &str, bytes: &[u8]) -> Result<Descriptor> {
    let digest = digest(bytes);
    atomic_write(&blob_path(root, &digest)?, bytes)?;
    Ok(Descriptor {
        media_type: media_type.into(),
        digest,
        size: bytes.len() as u64,
        platform: None,
        annotations: BTreeMap::new(),
    })
}
pub fn read(root: &Path, descriptor: &Descriptor) -> Result<Vec<u8>> {
    let bytes = std::fs::read(blob_path(root, &descriptor.digest)?)?;
    ensure!(
        bytes.len() as u64 == descriptor.size && digest(&bytes) == descriptor.digest,
        "image blob {} is corrupt",
        descriptor.digest
    );
    Ok(bytes)
}
pub fn append<W: Write>(
    archive: &mut tar::Builder<W>,
    path: &str,
    bytes: &[u8],
    mode: u32,
) -> Result<()> {
    let mut header = tar::Header::new_gnu();
    header.set_size(bytes.len() as u64);
    if path.ends_with('/') {
        header.set_entry_type(tar::EntryType::Directory);
    }
    header.set_mode(mode);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_cksum();
    archive.append_data(&mut header, path, bytes)?;
    Ok(())
}
pub fn layer(root: &Path, files: &[(&str, &[u8], u32)]) -> Result<(Descriptor, String)> {
    let mut archive = tar::Builder::new(Vec::new());
    for (path, bytes, mode) in files {
        append(&mut archive, path, bytes, *mode)?;
    }
    let tar = archive.into_inner()?;
    let diff_id = digest(&tar);
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(&tar)?;
    Ok((put(root, LAYER, &encoder.finish()?)?, diff_id))
}

pub fn children(value: &Value) -> Result<Vec<Descriptor>> {
    if let Some(manifests) = value.get("manifests") {
        return Ok(serde_json::from_value(manifests.clone())?);
    }
    let mut children = vec![serde_json::from_value(
        value
            .get("config")
            .context("image manifest has no config")?
            .clone(),
    )?];
    children.extend(serde_json::from_value::<Vec<Descriptor>>(
        value
            .get("layers")
            .context("image manifest has no layers")?
            .clone(),
    )?);
    Ok(children)
}
pub fn is_manifest(media_type: &str) -> bool {
    [
        INDEX,
        MANIFEST,
        "application/vnd.docker.distribution.manifest.list.v2+json",
        "application/vnd.docker.distribution.manifest.v2+json",
    ]
    .contains(&media_type)
}
fn visit(
    root: &Path,
    descriptor: &Descriptor,
    visited: &mut BTreeMap<String, Descriptor>,
    depth: usize,
) -> Result<()> {
    ensure!(depth < 8, "image manifest nesting too deep");
    if let Some(previous) = visited.get(&descriptor.digest) {
        ensure!(
            previous.size == descriptor.size && previous.media_type == descriptor.media_type,
            "conflicting image descriptors"
        );
        return Ok(());
    }
    let bytes = read(root, descriptor)?;
    visited.insert(descriptor.digest.clone(), descriptor.clone());
    if is_manifest(&descriptor.media_type) {
        let value: Value = serde_json::from_slice(&bytes)?;
        ensure!(
            value["schemaVersion"] == 2,
            "unsupported image manifest version"
        );
        for child in children(&value)? {
            visit(root, &child, visited, depth + 1)?;
        }
    }
    Ok(())
}
pub fn validate(root: &Path, index: &Value) -> Result<BTreeMap<String, Descriptor>> {
    ensure!(
        index["schemaVersion"] == 2 && index["mediaType"] == INDEX,
        "archive must contain an OCI image index"
    );
    let manifests = children(index)?;
    ensure!(!manifests.is_empty(), "image index is empty");
    let mut visited = BTreeMap::new();
    for manifest in manifests {
        visit(root, &manifest, &mut visited, 0)?;
    }
    Ok(visited)
}
pub fn export(root: &Path, index: &Value, output: &Path) -> Result<()> {
    let mut image_index = put(root, INDEX, &serde_json::to_vec(index)?)?;
    image_index
        .annotations
        .insert("org.opencontainers.image.ref.name".into(), "latest".into());
    let layout_index = self::index(vec![image_index]);
    let index = &layout_index;
    let descriptors = validate(root, index)?;
    let parent = output.parent().context("archive has no parent")?;
    std::fs::create_dir_all(parent)?;
    let temporary = tempfile::NamedTempFile::new_in(parent)?;
    let mut tar = tar::Builder::new(temporary);
    append(
        &mut tar,
        "oci-layout",
        br#"{"imageLayoutVersion":"1.0.0"}"#,
        0o644,
    )?;
    append(&mut tar, "index.json", &serde_json::to_vec(index)?, 0o644)?;
    for descriptor in descriptors.values() {
        let name = format!(
            "blobs/sha256/{}",
            descriptor.digest.trim_start_matches("sha256:")
        );
        append(&mut tar, &name, &read(root, descriptor)?, 0o644)?;
    }
    tar.into_inner()?.persist(output)?;
    Ok(())
}

#[derive(Debug)]
pub struct Imported {
    pub directory: tempfile::TempDir,
    pub index: Value,
}
/// Imports only OCI metadata and digest-named regular blobs. Never extracts
/// arbitrary tar paths, links, ownership, or executable modes.
pub fn import(path: &Path) -> Result<Imported> {
    let directory = tempfile::tempdir()?;
    let file = std::fs::File::open(path).with_context(|| {
        format!(
            "cannot open {}; run dovetail image build first",
            path.display()
        )
    })?;
    let mut archive = tar::Archive::new(file);
    let mut seen = BTreeSet::new();
    for entry in archive.entries()? {
        let mut entry = entry?;
        let name = entry
            .path()?
            .to_str()
            .context("non-UTF8 archive path")?
            .to_string();
        ensure!(
            seen.insert(name.clone()),
            "duplicate archive entry '{name}'"
        );
        ensure!(
            entry.header().entry_type().is_file(),
            "OCI archive entries must be regular files"
        );
        let target = if name == "index.json" || name == "oci-layout" {
            directory.path().join(&name)
        } else {
            let hash = name
                .strip_prefix("blobs/sha256/")
                .context("unexpected OCI archive entry")?;
            blob_path(directory.path(), &format!("sha256:{hash}"))?
        };
        std::fs::create_dir_all(target.parent().unwrap())?;
        std::io::copy(&mut entry, &mut std::fs::File::create(target)?)?;
    }
    let layout: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("oci-layout"))?)?;
    ensure!(
        layout["imageLayoutVersion"] == "1.0.0",
        "unsupported OCI layout version"
    );
    let index: Value =
        serde_json::from_slice(&std::fs::read(directory.path().join("index.json"))?)?;
    validate(directory.path(), &index)?;
    let roots = children(&index)?;
    ensure!(
        roots.len() == 1 && roots[0].media_type == INDEX,
        "expected one multi-platform image in OCI archive"
    );
    let index = serde_json::from_slice(&read(directory.path(), &roots[0])?)?;
    Ok(Imported { directory, index })
}
pub fn index(manifests: Vec<Descriptor>) -> Value {
    json!({"schemaVersion": 2, "mediaType": INDEX, "manifests": manifests})
}
