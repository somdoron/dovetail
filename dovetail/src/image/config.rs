use anyhow::{Result, bail, ensure};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, clap::ValueEnum)]
pub enum Platform {
    #[serde(rename = "linux/amd64")]
    #[value(name = "linux/amd64")]
    Amd64,
    #[serde(rename = "linux/arm64")]
    #[value(name = "linux/arm64")]
    Arm64,
}
impl Platform {
    pub fn architecture(self) -> &'static str {
        match self {
            Self::Amd64 => "amd64",
            Self::Arm64 => "arm64",
        }
    }
    pub fn target(self) -> &'static str {
        match self {
            Self::Amd64 => "x86_64-unknown-linux-gnu",
            Self::Arm64 => "aarch64-unknown-linux-gnu",
        }
    }
    pub fn name(self) -> String {
        format!("linux/{}", self.architecture())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ImageConfig {
    pub name: String,
    pub tag: String,
    pub base: String,
    pub platforms: Vec<Platform>,
    pub output: Option<PathBuf>,
    pub user: String,
    pub workdir: String,
    pub env: BTreeMap<String, String>,
    pub labels: BTreeMap<String, String>,
    pub annotations: BTreeMap<String, String>,
    pub files: Vec<ImageFile>,
    pub expose: Vec<String>,
    pub args: Vec<String>,
    pub stop_signal: String,
    pub wasi: WasiConfig,
    /// Linux runtime executable paths relative to Dovetail.toml.
    pub runtime: BTreeMap<String, PathBuf>,
}
impl Default for ImageConfig {
    fn default() -> Self {
        Self {
            name: String::new(),
            tag: "latest".into(),
            base: "gcr.io/distroless/cc-debian13:nonroot".into(),
            platforms: vec![Platform::Amd64, Platform::Arm64],
            output: None,
            user: "65532:65532".into(),
            workdir: "/app".into(),
            env: BTreeMap::new(),
            labels: BTreeMap::new(),
            annotations: BTreeMap::new(),
            files: vec![],
            expose: vec![],
            args: vec![],
            stop_signal: "SIGTERM".into(),
            wasi: WasiConfig::default(),
            runtime: BTreeMap::new(),
        }
    }
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct WasiConfig {
    pub allow_network: bool,
    pub inherit_env: bool,
    pub allow_path: Vec<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImageFile {
    pub source: PathBuf,
    pub destination: String,
}

#[derive(Debug)]
pub struct ImageProject {
    pub directory: PathBuf,
    pub name: String,
    pub config: ImageConfig,
}
impl ImageProject {
    pub fn archive(&self, root: &Path) -> PathBuf {
        root.join(
            self.config
                .output
                .clone()
                .unwrap_or_else(|| PathBuf::from(format!("build/images/{}.oci.tar", self.name))),
        )
    }
    pub fn reference(&self, tag: Option<&str>) -> Result<oci_client::Reference> {
        let tag = tag.unwrap_or(&self.config.tag);
        ensure!(
            !tag.is_empty()
                && tag.len() <= 128
                && tag
                    .bytes()
                    .enumerate()
                    .all(|(i, c)| c.is_ascii_alphanumeric()
                        || c == b'_'
                        || (i > 0 && (c == b'.' || c == b'-'))),
            "invalid image tag '{tag}'"
        );
        let reference: oci_client::Reference = format!("{}:{tag}", self.config.name).parse()?;
        ensure!(
            reference.digest().is_none(),
            "image.name must be a repository without a digest"
        );
        Ok(reference)
    }
    pub fn validate(&self) -> Result<()> {
        ensure!(
            !self.name.is_empty()
                && self.name != "."
                && self.name != ".."
                && !self.name.contains(['/', '\\']),
            "invalid image project name '{}'",
            self.name
        );
        ensure!(
            !self.config.name.is_empty(),
            "project '{}': image.name is required",
            self.name
        );
        // A colon in the final component is a tag; a registry port is allowed.
        ensure!(
            !self
                .config
                .name
                .rsplit('/')
                .next()
                .unwrap_or("")
                .contains(':')
                && !self.config.name.contains('@'),
            "image.name must not include a tag or digest"
        );
        self.reference(None)?;
        if let Some(output) = &self.config.output {
            ensure!(
                output.to_string_lossy().ends_with(".oci.tar"),
                "image.output must end in .oci.tar"
            );
        }
        ensure!(
            !self.config.stop_signal.is_empty() && !self.config.stop_signal.contains('\0'),
            "invalid image.stop-signal"
        );
        ensure!(
            !self.config.args.iter().any(|arg| arg.contains('\0')),
            "image arguments cannot contain NUL"
        );
        ensure!(
            !self.config.env.values().any(|value| value.contains('\0')),
            "image environment values cannot contain NUL"
        );
        ensure!(
            !self.config.platforms.is_empty(),
            "image.platforms cannot be empty"
        );
        let mut platforms = vec![];
        for platform in &self.config.platforms {
            ensure!(
                !platforms.contains(platform),
                "duplicate image platform {}",
                platform.name()
            );
            platforms.push(*platform);
        }
        for key in self.config.runtime.keys() {
            ensure!(
                key == "linux/amd64" || key == "linux/arm64",
                "unsupported runtime platform '{key}'"
            );
        }
        ensure!(
            self.config.workdir.starts_with('/')
                && !self.config.workdir.contains('\0')
                && !Path::new(&self.config.workdir)
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir)),
            "image.workdir must be absolute"
        );
        for reserved in [
            "/app/application.cwasm",
            "/app/dovetail-image.json",
            "/usr/local/bin/dovetail",
        ] {
            let workdir: PathBuf = Path::new(&self.config.workdir).components().collect();
            ensure!(
                !workdir.starts_with(reserved),
                "image.workdir overlaps a generated runtime file"
            );
        }
        ensure!(!self.config.user.is_empty(), "image.user cannot be empty");
        for key in self.config.env.keys() {
            ensure!(
                !key.is_empty() && !key.contains(['=', '\0']),
                "invalid environment variable name"
            );
        }
        for path in &self.config.wasi.allow_path {
            ensure!(path.is_absolute(), "WASI allow-path must be absolute");
        }
        for port in &self.config.expose {
            let (number, protocol) = port.split_once('/').unwrap_or((port, "tcp"));
            ensure!(
                number.parse::<u16>().is_ok_and(|p| p > 0)
                    && ["tcp", "udp", "sctp"].contains(&protocol),
                "invalid exposed port '{port}'"
            );
        }
        for file in &self.config.files {
            super::files::validate(file)?;
        }
        if self.config.base != "scratch" {
            let _: oci_client::Reference = self.config.base.parse()?;
        }
        Ok(())
    }
}

pub fn select(projects: Vec<ImageProject>, filter: Option<&str>) -> Result<Vec<ImageProject>> {
    let selected: Vec<_> = projects
        .into_iter()
        .filter(|p| filter.is_none_or(|name| p.name == name))
        .collect();
    if selected.is_empty() {
        bail!(
            "no image-configured project found{}",
            filter.map(|p| format!(" named '{p}'")).unwrap_or_default()
        );
    }
    let mut names = std::collections::BTreeSet::new();
    for project in &selected {
        ensure!(
            names.insert(&project.name),
            "duplicate image project '{}'",
            project.name
        );
        project.validate()?;
    }
    Ok(selected)
}
