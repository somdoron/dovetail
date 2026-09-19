use super::{
    archive,
    config::{ImageFile, ImageProject},
};
use anyhow::{Context, Result, ensure};
use std::{
    collections::BTreeMap,
    path::{Component, Path},
};

pub fn validate(file: &ImageFile) -> Result<()> {
    ensure!(
        !file.source.is_absolute()
            && !file
                .source
                .components()
                .any(|c| matches!(c, Component::ParentDir)),
        "image file source must stay inside the project directory"
    );
    ensure!(
        file.destination.starts_with('/')
            && !file.destination.contains('\0')
            && !Path::new(&file.destination)
                .components()
                .any(|c| matches!(c, Component::ParentDir)),
        "image file destination must be an absolute container path without '..'"
    );
    let normalized = format!(
        "/{}",
        Path::new(&file.destination)
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/")
    );
    let destination = normalized.trim_end_matches('/');
    ensure!(
        !destination.split('/').any(|name| name.starts_with(".wh.")),
        "image filenames cannot be OCI whiteouts"
    );
    for reserved in [
        "/app/application.cwasm",
        "/app/dovetail-image.json",
        "/usr/local/bin/dovetail",
    ] {
        ensure!(
            destination != reserved
                && !destination.starts_with(&format!("{reserved}/"))
                && !reserved.starts_with(&format!("{destination}/")),
            "image file destination overlaps a generated runtime file"
        );
    }
    Ok(())
}

fn collect(
    source: &Path,
    destination: &str,
    files: &mut BTreeMap<String, (Vec<u8>, u32)>,
) -> Result<()> {
    ensure!(
        !destination.split('/').any(|name| name.starts_with(".wh.")),
        "image filenames cannot be OCI whiteouts"
    );
    let metadata = std::fs::symlink_metadata(source)
        .with_context(|| format!("reading image file {}", source.display()))?;
    ensure!(
        !metadata.file_type().is_symlink(),
        "image files cannot include symlinks: {}",
        source.display()
    );
    ensure!(
        metadata.is_file() || metadata.is_dir(),
        "image files must be regular files or directories"
    );
    if metadata.is_file() {
        ensure!(
            files
                .insert(destination.to_owned(), (std::fs::read(source)?, 0o644))
                .is_none(),
            "duplicate image file destination '{destination}'"
        );
    } else {
        ensure!(
            files
                .insert(format!("{destination}/"), (vec![], 0o755))
                .is_none(),
            "duplicate image directory destination '{destination}'"
        );
        let mut children = std::fs::read_dir(source)?.collect::<std::io::Result<Vec<_>>>()?;
        children.sort_by_key(|entry| entry.file_name());
        for child in children {
            let name = child
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("image filenames must be UTF-8"))?;
            collect(&child.path(), &format!("{destination}/{name}"), files)?;
        }
    }
    Ok(())
}

pub fn layer(
    cache: &Path,
    project: &ImageProject,
) -> Result<Option<(archive::Descriptor, String)>> {
    if project.config.files.is_empty() {
        return Ok(None);
    }
    let directory = project.directory.canonicalize()?;
    let mut files = BTreeMap::new();
    for file in &project.config.files {
        validate(file)?;
        let source = directory.join(&file.source);
        ensure!(
            source.canonicalize()?.starts_with(&directory),
            "image source escapes the project directory"
        );
        let destination = Path::new(&file.destination)
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => name.to_str(),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("/");
        collect(&source, &destination, &mut files)?;
    }
    for name in files.keys() {
        let mut parent = Path::new(name).parent();
        while let Some(path) = parent {
            if let Some(path) = path.to_str() {
                ensure!(
                    !files.contains_key(path),
                    "image file conflicts with directory '{path}'"
                );
            }
            parent = path.parent();
        }
    }
    let entries: Vec<_> = files
        .iter()
        .map(|(path, (bytes, mode))| (path.as_str(), bytes.as_slice(), *mode))
        .collect();
    Ok(Some(archive::layer(cache, &entries)?))
}
