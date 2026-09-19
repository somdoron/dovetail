pub mod archive;
mod auth;
pub mod config;
mod files;
mod registry;
pub mod runtime;
#[cfg(test)]
mod tests;

use anyhow::{Context, Result, ensure};
use config::{ImageProject, Platform};
use serde_json::{Value, json};
use std::path::Path;

pub struct BuildOptions {
    pub platforms: Vec<Platform>,
    pub locked: bool,
    pub offline: bool,
}

pub async fn build(
    root: &Path,
    project: &ImageProject,
    wasm: &[u8],
    options: &BuildOptions,
) -> Result<()> {
    project.validate()?;
    let cache = root.join(".dovetail/images");
    let mut pins = registry::load_pins(root)?;
    let platforms = if options.platforms.is_empty() {
        &project.config.platforms
    } else {
        &options.platforms
    };
    let mut manifests = vec![];
    let mut seen = vec![];
    for &platform in platforms {
        ensure!(
            !seen.contains(&platform),
            "duplicate platform {}",
            platform.name()
        );
        seen.push(platform);
        eprintln!("building image {} ({})", project.name, platform.name());
        let binary = runtime::binary(root, &project.config, platform, options.offline).await?;
        let compiled = runtime::precompile(wasm, platform)?;
        let (base, layers) = registry::base(
            &cache,
            &project.config.base,
            platform,
            &mut pins,
            options.locked,
            options.offline,
        )
        .await?;
        manifests.push(assemble(
            &cache, project, platform, &binary, &compiled, base, layers,
        )?);
    }
    let index = archive::index(manifests);
    archive::export(&cache, &index, &project.archive(root))?;
    if !options.locked {
        registry::save_pins(root, &pins)?;
    }
    eprintln!(
        "built {} -> {}",
        project.name,
        project.archive(root).display()
    );
    Ok(())
}

fn assemble(
    cache: &Path,
    project: &ImageProject,
    platform: Platform,
    binary: &[u8],
    compiled: &[u8],
    mut config: Value,
    mut layers: Vec<archive::Descriptor>,
) -> Result<archive::Descriptor> {
    let execution = runtime::ExecutionConfig {
        runtime_id: runtime::RUNTIME_ID.into(),
        component: "/app/application.cwasm".into(),
        digest: archive::digest(compiled),
        wasi: project.config.wasi.clone(),
    };
    let execution = serde_json::to_vec(&execution)?;
    let (runtime, runtime_diff) =
        archive::layer(cache, &[("usr/local/bin/dovetail", binary, 0o755)])?;
    let workdir = format!("{}/", project.config.workdir.trim_matches('/'));
    let mut files = vec![
        ("app/application.cwasm", compiled, 0o644),
        ("app/dovetail-image.json", execution.as_slice(), 0o644),
    ];
    if !workdir.is_empty() && workdir != "/" {
        files.push((&workdir, &[], 0o755));
    }
    let (application, application_diff) = archive::layer(cache, &files)?;
    let mut added_layers = vec![(runtime, runtime_diff), (application, application_diff)];
    if let Some(layer) = files::layer(cache, project)? {
        added_layers.push(layer);
    }
    let added_count = added_layers.len();
    let diff_ids = config["rootfs"]["diff_ids"]
        .as_array_mut()
        .context("base rootfs has no diff_ids")?;
    ensure!(
        diff_ids.len() == layers.len(),
        "base layers and diff_ids do not match"
    );
    for (layer, diff_id) in added_layers {
        layers.push(layer);
        diff_ids.push(json!(diff_id));
    }
    configure_process(&mut config, project)?;
    if let Some(history) = config.get_mut("history") {
        let history = history.as_array_mut().context("invalid base history")?;
        history.extend((0..added_count).map(|_| json!({"created_by": "dovetail image build"})));
    }
    let config = archive::put(cache, archive::CONFIG, &serde_json::to_vec(&config)?)?;
    let manifest = json!({"schemaVersion": 2, "mediaType": archive::MANIFEST, "config": config, "layers": layers, "annotations": project.config.annotations});
    let mut descriptor = archive::put(cache, archive::MANIFEST, &serde_json::to_vec(&manifest)?)?;
    descriptor.platform = Some(json!({"os": "linux", "architecture": platform.architecture()}));
    Ok(descriptor)
}
fn configure_process(config: &mut Value, project: &ImageProject) -> Result<()> {
    if !config["config"].is_object() {
        config["config"] = json!({});
    }
    let settings = &mut config["config"];
    let mut environment = std::collections::BTreeMap::new();
    if let Some(existing) = settings["Env"].as_array() {
        for variable in existing {
            let (key, value) = variable
                .as_str()
                .and_then(|s| s.split_once('='))
                .context("invalid base environment")?;
            environment.insert(key.to_owned(), value.to_owned());
        }
    }
    environment.extend(project.config.env.clone());
    settings["Env"] = json!(
        environment
            .into_iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
    );
    settings["Entrypoint"] = json!([
        "/usr/local/bin/dovetail",
        "image",
        "run",
        "--config",
        "/app/dovetail-image.json",
        "--"
    ]);
    settings["Cmd"] = json!(project.config.args);
    settings["WorkingDir"] = json!(project.config.workdir);
    settings["User"] = json!(project.config.user);
    settings["StopSignal"] = json!(project.config.stop_signal);
    settings.as_object_mut().unwrap().remove("Healthcheck");
    settings["ExposedPorts"] = json!({});
    for port in &project.config.expose {
        let port = if port.contains('/') {
            port.clone()
        } else {
            format!("{port}/tcp")
        };
        settings["ExposedPorts"][port] = json!({});
    }
    if !settings["Labels"].is_object() {
        settings["Labels"] = json!({});
    }
    for (key, value) in &project.config.labels {
        settings["Labels"][key] = json!(value);
    }
    settings["Labels"]["org.dovetail.runtime"] = json!(runtime::RUNTIME_ID);
    settings["Labels"]["org.dovetail.project"] = json!(project.name);
    Ok(())
}

pub async fn push(root: &Path, projects: &[ImageProject], tag: Option<&str>) -> Result<()> {
    // Validate every archive and destination before even looking up credentials.
    let mut pending = vec![];
    let mut destinations = std::collections::BTreeSet::new();
    for project in projects {
        let reference = project.reference(tag)?;
        ensure!(
            destinations.insert(reference.to_string()),
            "multiple projects would push to {reference}"
        );
        let imported = archive::import(&project.archive(root))?;
        for descriptor in archive::children(&imported.index)? {
            ensure!(
                descriptor.media_type == archive::MANIFEST,
                "archive must contain platform image manifests"
            );
            let manifest: Value =
                serde_json::from_slice(&archive::read(imported.directory.path(), &descriptor)?)?;
            let config_descriptor: archive::Descriptor =
                serde_json::from_value(manifest["config"].clone())?;
            let config: Value = serde_json::from_slice(&archive::read(
                imported.directory.path(),
                &config_descriptor,
            )?)?;
            ensure!(
                config["config"]["Labels"]["org.dovetail.project"] == project.name,
                "archive does not belong to project '{}'",
                project.name
            );
        }
        pending.push((reference, imported));
    }
    let mut authenticated = vec![];
    for (reference, imported) in pending {
        let auth = auth::resolve(&reference, true).await?;
        authenticated.push((reference, imported, auth));
    }
    for (reference, imported, auth) in authenticated {
        eprintln!("pushing {reference}");
        let digest = registry::push(&imported, &reference, auth).await?;
        println!("pushed {reference}@{digest}");
    }
    Ok(())
}
