use std::io::IsTerminal;
use std::path::Path;

use anyhow::{Result, bail};
use dialoguer::{Confirm, MultiSelect};

use super::{Target, installer};

fn interactive() -> bool {
    std::io::stdin().is_terminal() && std::io::stderr().is_terminal()
}

pub(super) fn offer_install() -> Result<bool> {
    if !interactive() {
        return Ok(false);
    }
    Ok(Confirm::new()
        .with_prompt("Install Dovetail coding-agent support?")
        .default(true)
        .interact_opt()?
        .unwrap_or(false))
}

pub(super) fn select_targets(root: &Path, explicit: &[Target]) -> Result<Vec<Target>> {
    if !explicit.is_empty() {
        return Ok(explicit.to_vec());
    }
    let remembered = installer::remembered_targets(root)?;
    if !remembered.is_empty() {
        return Ok(remembered);
    }
    if !interactive() {
        bail!("choose installation targets with --agent generic,claude (or either target)");
    }
    let generic = root.join(".agents").is_dir();
    let claude = root.join(".claude").is_dir();
    let selected = MultiSelect::new()
        .with_prompt("Install support for (space to select, enter to continue)")
        .items(["Generic coding agents (including Codex)", "Claude"])
        .defaults(&[generic || !claude, claude])
        .interact_opt()?
        .unwrap_or_default();
    Ok(selected
        .into_iter()
        .map(|index| [Target::Generic, Target::Claude][index])
        .collect())
}
