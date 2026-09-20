use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, bail};
use include_dir::{Dir, include_dir};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Target;

static BUNDLE: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/ai");
const VERSION: &str = env!("CARGO_PKG_VERSION");
const RECEIPT: &str = ".dovetail-install.json";
const REVIEWERS: [&str; 2] = ["dovetail-reviewer", "dovetail-ddd-reviewer"];

#[derive(Serialize, Deserialize)]
struct Receipt {
    schema: u32,
    target: Target,
    compiler_version: String,
    files: BTreeMap<String, String>,
}

pub(super) struct Change {
    path: PathBuf,
    content: Option<Vec<u8>>,
    existed: bool,
}

impl Change {
    pub(super) fn path(&self) -> &Path {
        &self.path
    }

    pub(super) fn action(&self) -> &'static str {
        match (&self.content, self.existed) {
            (None, _) => "remove",
            (Some(_), true) => "update",
            (Some(_), false) => "add",
        }
    }
}

pub(super) fn plan(root: &Path, targets: &[Target]) -> Result<Vec<Change>> {
    let mut changes = Vec::new();
    let mut receipts = Vec::new();
    let mut conflicts = Vec::new();
    for target in targets.iter().copied().collect::<BTreeSet<_>>() {
        plan_target(root, target, &mut changes, &mut receipts, &mut conflicts)?;
    }
    if !conflicts.is_empty() {
        bail!(
            "AI installation conflicts; no files written. Preserve or resolve these files and rerun:\n{}",
            conflicts.join("\n")
        );
    }
    changes.extend(receipts);
    Ok(changes)
}

pub(super) fn apply(root: &Path, changes: Vec<Change>) -> Result<()> {
    for change in changes {
        check_path(root, &change.path)?;
        let path = root.join(&change.path);
        if let Some(bytes) = change.content {
            let parent = path.parent().context("installation file has no parent")?;
            std::fs::create_dir_all(parent)?;
            let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
            temporary.write_all(&bytes)?;
            if let Ok(metadata) = std::fs::metadata(&path) {
                temporary
                    .as_file()
                    .set_permissions(metadata.permissions())?;
            }
            temporary
                .persist(&path)
                .with_context(|| format!("cannot replace {}", path.display()))?;
        } else {
            std::fs::remove_file(&path)
                .with_context(|| format!("cannot remove {}", path.display()))?;
        }
    }
    Ok(())
}

pub(super) fn remembered_targets(root: &Path) -> Result<Vec<Target>> {
    let mut targets = Vec::new();
    for target in [Target::Generic, Target::Claude] {
        if read_receipt(root, target)?.is_some() {
            targets.push(target);
        }
    }
    Ok(targets)
}

fn receipt_path(target: Target) -> PathBuf {
    target.skill_path().join(RECEIPT)
}

fn check_path(root: &Path, relative: &Path) -> Result<()> {
    let mut path = root.to_owned();
    for component in relative.components() {
        let Component::Normal(name) = component else {
            bail!("invalid AI installation path: {}", relative.display());
        };
        path.push(name);
        match std::fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                bail!(
                    "AI installation cannot traverse symlink: {}",
                    path.display()
                );
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error).with_context(|| path.display().to_string()),
        }
    }
    Ok(())
}

fn owned_path(target: Target, path: &Path) -> bool {
    (path.starts_with(target.skill_path()) && path != receipt_path(target))
        || (target == Target::Claude
            && REVIEWERS
                .iter()
                .any(|name| path == Path::new(".claude/agents").join(format!("{name}.md"))))
}

fn read_receipt(root: &Path, target: Target) -> Result<Option<Receipt>> {
    let path = receipt_path(target);
    check_path(root, &path)?;
    let Some(bytes) = read_file(&root.join(&path))? else {
        return Ok(None);
    };
    let receipt: Receipt = serde_json::from_slice(&bytes)
        .with_context(|| format!("invalid AI installation metadata: {}", path.display()))?;
    if receipt.schema != 1 || receipt.target != target {
        bail!("unsupported AI installation metadata: {}", path.display());
    }
    for name in receipt.files.keys() {
        let file = Path::new(name);
        if !owned_path(target, file) {
            bail!("metadata claims an unmanaged file: {}", file.display());
        }
        check_path(root, file)?;
    }
    Ok(Some(receipt))
}

fn read_file(path: &Path) -> Result<Option<Vec<u8>>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("cannot read {}", path.display())),
    }
}

fn portable_path(path: &Path) -> String {
    path.iter()
        .map(|part| part.to_str().expect("bundled AI paths are UTF-8"))
        .collect::<Vec<_>>()
        .join("/")
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn collect_bundle(directory: &Dir<'_>, prefix: &Path, files: &mut BTreeMap<PathBuf, Vec<u8>>) {
    for file in directory.files() {
        let content = file.contents_utf8().expect("AI bundle contains UTF-8 text");
        files.insert(
            prefix.join(file.path()),
            content
                .replace("{{DOVETAIL_VERSION}}", VERSION)
                .into_bytes(),
        );
    }
    for child in directory.dirs() {
        collect_bundle(child, prefix, files);
    }
}

fn rendered_files(target: Target) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    collect_bundle(&BUNDLE, &target.skill_path(), &mut files);
    if target == Target::Claude {
        for name in REVIEWERS {
            let description = if name == "dovetail-reviewer" {
                "Review Dovetail correctness, tests, and idioms only when requested."
            } else {
                "Review Dovetail domain modeling and architecture only when requested."
            };
            let text = format!(
                "---\nname: {name}\ndescription: {description}\ntools: Read, Grep, Glob\n---\n\n\
                 Read `.claude/skills/dovetail/SKILL.md` and \
                 `.claude/skills/dovetail/reviews/{name}.md` relative to the workspace root.\n\
                 Load only relevant references linked there. Review the requested scope.\n\
                 Do not edit files or delegate further. If execution evidence is needed,\n\
                 request it from the parent agent and disclose what was not verified.\n"
            );
            files.insert(
                Path::new(".claude/agents").join(format!("{name}.md")),
                text.into_bytes(),
            );
        }
    }
    files
}

fn plan_target(
    root: &Path,
    target: Target,
    changes: &mut Vec<Change>,
    receipts: &mut Vec<Change>,
    conflicts: &mut Vec<String>,
) -> Result<()> {
    let old = read_receipt(root, target)?;
    let files = rendered_files(target);
    let hashes = files
        .iter()
        .map(|(path, bytes)| (portable_path(path), digest(bytes)))
        .collect();
    for (path, content) in &files {
        plan_file(root, path, Some(content), old.as_ref(), changes, conflicts)?;
    }
    if let Some(old) = &old {
        for path in old
            .files
            .keys()
            .map(Path::new)
            .filter(|path| !files.contains_key(*path))
        {
            plan_file(root, path, None, Some(old), changes, conflicts)?;
        }
    }
    let receipt = Receipt {
        schema: 1,
        target,
        compiler_version: VERSION.to_string(),
        files: hashes,
    };
    let bytes = serde_json::to_vec_pretty(&receipt)?;
    let path = receipt_path(target);
    let current = read_file(&root.join(&path))?;
    if current.as_deref() != Some(&bytes) {
        receipts.push(Change {
            path,
            content: Some(bytes),
            existed: current.is_some(),
        });
    }
    Ok(())
}

fn plan_file(
    root: &Path,
    path: &Path,
    desired: Option<&Vec<u8>>,
    receipt: Option<&Receipt>,
    changes: &mut Vec<Change>,
    conflicts: &mut Vec<String>,
) -> Result<()> {
    check_path(root, path)?;
    let current = read_file(&root.join(path))?;
    if current.as_ref() == desired {
        return Ok(());
    }
    if let Some(bytes) = &current {
        let previous_hash = receipt.and_then(|receipt| receipt.files.get(&portable_path(path)));
        if previous_hash != Some(&digest(bytes)) {
            conflicts.push(path.display().to_string());
            return Ok(());
        }
    }
    changes.push(Change {
        path: path.to_owned(),
        content: desired.cloned(),
        existed: current.is_some(),
    });
    Ok(())
}
