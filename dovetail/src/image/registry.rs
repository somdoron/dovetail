use super::{
    archive::{self, Descriptor},
    config::Platform,
};
use anyhow::{Context, Result, ensure};
use oci_client::{Client, Reference, secrets::RegistryAuth};
use serde_json::Value;
use std::{collections::BTreeMap, path::Path};

const ACCEPT: &[&str] = &[
    archive::INDEX,
    archive::MANIFEST,
    "application/vnd.docker.distribution.manifest.list.v2+json",
    "application/vnd.docker.distribution.manifest.v2+json",
];
pub type BasePins = BTreeMap<String, Descriptor>;

pub fn load_pins(root: &Path) -> Result<BasePins> {
    match std::fs::read(root.join("Dovetail.images.lock")) {
        Ok(bytes) => Ok(serde_json::from_slice(&bytes).context("invalid Dovetail.images.lock")?),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(BTreeMap::new()),
        Err(error) => Err(error.into()),
    }
}
pub fn save_pins(root: &Path, pins: &BasePins) -> Result<()> {
    archive::atomic_write(
        &root.join("Dovetail.images.lock"),
        &serde_json::to_vec_pretty(pins)?,
    )
}
async fn manifest(
    client: &Client,
    reference: &Reference,
    authentication: &RegistryAuth,
    cache: &Path,
) -> Result<(Descriptor, Value)> {
    let (bytes, expected) = client
        .pull_manifest_raw(reference, authentication, ACCEPT)
        .await?;
    ensure!(
        archive::digest(&bytes) == expected,
        "base manifest digest mismatch"
    );
    if let Some(pinned) = reference.digest() {
        ensure!(
            pinned == expected,
            "base manifest does not match requested digest"
        );
    }
    let value: Value = serde_json::from_slice(&bytes)?;
    let media_type = value["mediaType"]
        .as_str()
        .context("base manifest has no mediaType")?;
    let descriptor = archive::put(cache, media_type, &bytes)?;
    Ok((descriptor, value))
}
async fn download_blob(
    client: &Client,
    reference: &Reference,
    cache: &Path,
    descriptor: &Descriptor,
) -> Result<()> {
    if archive::read(cache, descriptor).is_ok() {
        return Ok(());
    }
    let mut bytes = Vec::new();
    client
        .pull_blob(reference, descriptor.digest.as_str(), &mut bytes)
        .await?;
    ensure!(
        bytes.len() as u64 == descriptor.size,
        "base blob size mismatch"
    );
    let stored = archive::put(cache, &descriptor.media_type, &bytes)?;
    ensure!(
        stored.digest == descriptor.digest,
        "base blob digest mismatch"
    );
    Ok(())
}

pub async fn base(
    cache: &Path,
    source: &str,
    platform: Platform,
    pins: &mut BasePins,
    locked: bool,
    offline: bool,
) -> Result<(Value, Vec<Descriptor>)> {
    if source == "scratch" {
        return Ok((
            serde_json::json!({"architecture": platform.architecture(), "os": "linux", "config": {}, "rootfs": {"type": "layers", "diff_ids": []}}),
            vec![],
        ));
    }
    let key = format!("{source}|{}", platform.name());
    let reference: Reference = source.parse()?;
    let client = Client::default();
    let authentication = if offline {
        RegistryAuth::Anonymous
    } else {
        super::auth::resolve(&reference, false).await?
    };
    let (descriptor, value) = if let Some(pin) = pins.get(&key) {
        match archive::read(cache, pin) {
            Ok(bytes) => (pin.clone(), serde_json::from_slice(&bytes)?),
            Err(_) => {
                ensure!(!offline, "base {} is not cached", platform.name());
                let pinned: Reference = format!(
                    "{}/{}@{}",
                    reference.registry(),
                    reference.repository(),
                    pin.digest
                )
                .parse()?;
                manifest(&client, &pinned, &authentication, cache).await?
            }
        }
    } else {
        ensure!(
            !locked,
            "base '{source}' for {} is not in Dovetail.images.lock; build once without --locked",
            platform.name()
        );
        ensure!(
            !offline,
            "base '{source}' is not cached; build once without --offline"
        );
        let (descriptor, value) = manifest(&client, &reference, &authentication, cache).await?;
        if value.get("manifests").is_some() {
            let candidates = archive::children(&value)?;
            let selected = candidates
                .iter()
                .find(|d| {
                    d.platform.as_ref().is_some_and(|p| {
                        p["os"] == "linux"
                            && p["architecture"] == platform.architecture()
                            && p.get("variant").is_none_or(|v| {
                                v == "" || (platform == Platform::Arm64 && v == "v8")
                            })
                    })
                })
                .context("base image has no matching platform")?;
            let pinned: Reference = format!(
                "{}/{}@{}",
                reference.registry(),
                reference.repository(),
                selected.digest
            )
            .parse()?;
            manifest(&client, &pinned, &authentication, cache).await?
        } else {
            (descriptor, value)
        }
    };
    ensure!(
        value.get("layers").is_some(),
        "base must resolve to an image manifest"
    );
    if !offline {
        client
            .store_auth_if_needed(reference.resolve_registry(), &authentication)
            .await;
    }
    let children = archive::children(&value)?;
    for child in &children {
        if offline {
            archive::read(cache, child).context("base blob is not cached")?;
        } else {
            download_blob(&client, &reference, cache, child).await?;
        }
    }
    let config: Value = serde_json::from_slice(&archive::read(cache, &children[0])?)?;
    ensure!(
        config["os"] == "linux" && config["architecture"] == platform.architecture(),
        "base configuration architecture does not match {}",
        platform.name()
    );
    pins.insert(key, descriptor);
    Ok((config, children.into_iter().skip(1).collect()))
}

async fn upload_manifest(
    client: &Client,
    reference: &Reference,
    root: &Path,
    descriptor: &Descriptor,
    uploaded: &mut std::collections::BTreeSet<String>,
) -> Result<()> {
    // Our archive format has one index of platform image manifests.
    ensure!(
        descriptor.media_type == archive::MANIFEST,
        "expected an OCI platform image manifest"
    );
    let bytes = archive::read(root, descriptor)?;
    let value: Value = serde_json::from_slice(&bytes)?;
    for child in archive::children(&value)? {
        if !uploaded.insert(child.digest.clone()) {
            continue;
        }
        if !client.blob_exists(reference, &child.digest).await? {
            client
                .push_blob(reference, archive::read(root, &child)?, &child.digest)
                .await?;
        }
    }
    let destination: Reference = format!(
        "{}/{}@{}",
        reference.registry(),
        reference.repository(),
        descriptor.digest
    )
    .parse()?;
    client
        .push_manifest_raw(&destination, bytes, descriptor.media_type.parse()?)
        .await?;
    Ok(())
}
pub async fn push(
    imported: &archive::Imported,
    reference: &Reference,
    authentication: RegistryAuth,
) -> Result<String> {
    push_with_client(&Client::default(), imported, reference, authentication).await
}

pub(super) async fn push_with_client(
    client: &Client,
    imported: &archive::Imported,
    reference: &Reference,
    authentication: RegistryAuth,
) -> Result<String> {
    client
        .auth(
            reference,
            &authentication,
            oci_client::RegistryOperation::Push,
        )
        .await?;
    let mut uploaded = std::collections::BTreeSet::new();
    for descriptor in archive::children(&imported.index)? {
        upload_manifest(
            client,
            reference,
            imported.directory.path(),
            &descriptor,
            &mut uploaded,
        )
        .await?;
    }
    let bytes = serde_json::to_vec(&imported.index)?;
    let digest = archive::digest(&bytes);
    client
        .push_manifest_raw(reference, bytes, archive::INDEX.parse()?)
        .await?;
    Ok(digest)
}
