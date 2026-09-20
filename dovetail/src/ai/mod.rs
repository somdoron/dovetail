//! Workspace-local coding-agent guidance, bundled with the compiler.
mod installer;
mod prompt;

use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::ValueEnum;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Eq, Ord, PartialEq, PartialOrd, Serialize, Deserialize, ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum Target {
    Generic,
    Claude,
}

impl Target {
    pub(super) fn directory(self) -> &'static str {
        match self {
            Self::Generic => ".agents",
            Self::Claude => ".claude",
        }
    }

    pub(super) fn skill_path(self) -> PathBuf {
        Path::new(self.directory()).join("skills/dovetail")
    }
}

pub fn install_command(targets: &[Target], dry_run: bool) -> Result<()> {
    let root = workspace_root(&std::env::current_dir()?)?;
    let targets = prompt::select_targets(&root, targets)?;
    install(&root, &targets, dry_run)
}

pub fn install_after_init(targets: &[Target], no_ai: bool) -> Result<()> {
    if no_ai || (targets.is_empty() && !prompt::offer_install()?) {
        return Ok(());
    }
    install_command(targets, false)
}

/// Find the nearest containing workspace without loading or fetching dependencies.
pub fn workspace_root(start: &Path) -> Result<PathBuf> {
    let start = start
        .canonicalize()
        .context("cannot resolve working directory")?;
    for directory in start.ancestors() {
        if directory.join("Dovetail.toml").is_file() {
            return Ok(directory.to_owned());
        }
    }
    bail!("no Dovetail.toml found in this directory or its ancestors")
}

/// Install selected targets; unrelated targets and project instructions are untouched.
pub fn install(root: &Path, targets: &[Target], dry_run: bool) -> Result<()> {
    if targets.is_empty() {
        eprintln!("No AI installation targets selected; no files changed.");
        return Ok(());
    }
    let changes = installer::plan(root, targets)?;
    for change in &changes {
        eprintln!("{} {}", change.action(), change.path().display());
    }
    if changes.is_empty() {
        eprintln!("Dovetail AI guidance is up to date.");
    }
    if !dry_run {
        installer::apply(root, changes)?;
    }
    Ok(())
}
