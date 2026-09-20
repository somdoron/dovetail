use std::fs;
use std::path::Path;
use std::process::{Command, Output, Stdio};

use dovetail::ai::{Target, install, workspace_root};
use serde_json::Value;
use sha2::{Digest, Sha256};

fn run(root: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dovetail"))
        .args(arguments)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .unwrap()
}

fn skill(target: Target) -> &'static str {
    match target {
        Target::Generic => ".agents/skills/dovetail",
        Target::Claude => ".claude/skills/dovetail",
    }
}

fn receipt(root: &Path, target: Target) -> Value {
    serde_json::from_slice(
        &fs::read(root.join(skill(target)).join(".dovetail-install.json")).unwrap(),
    )
    .unwrap()
}

fn write_receipt(root: &Path, target: Target, value: &Value) {
    fs::write(
        root.join(skill(target)).join(".dovetail-install.json"),
        serde_json::to_vec_pretty(value).unwrap(),
    )
    .unwrap();
}

#[test]
fn installs_both_targets_and_remembers_them_without_touching_instructions() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::write(root.join("AGENTS.md"), "user instructions").unwrap();
    fs::write(root.join("CLAUDE.md"), "claude instructions").unwrap();
    let output = run(root, &["init", "app", "--ai", "generic,claude"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    for target in [Target::Generic, Target::Claude] {
        let text = fs::read_to_string(root.join(skill(target)).join("SKILL.md")).unwrap();
        assert!(text.contains(env!("CARGO_PKG_VERSION")));
        assert!(!text.contains("{{DOVETAIL_VERSION}}"));
        assert!(
            receipt(root, target)["files"]
                .as_object()
                .unwrap()
                .keys()
                .all(|path| !path.contains('\\'))
        );
        assert_eq!(
            receipt(root, target)["compiler_version"],
            env!("CARGO_PKG_VERSION")
        );
    }
    assert!(root.join(".claude/agents/dovetail-reviewer.md").is_file());
    assert!(
        root.join(".claude/agents/dovetail-ddd-reviewer.md")
            .is_file()
    );
    assert!(!root.join(".codex").exists());
    assert_eq!(
        fs::read_to_string(root.join("AGENTS.md")).unwrap(),
        "user instructions"
    );
    assert_eq!(
        fs::read_to_string(root.join("CLAUDE.md")).unwrap(),
        "claude instructions"
    );
    let nested = root.join("app/src");
    assert_eq!(
        workspace_root(&nested).unwrap(),
        root.canonicalize().unwrap()
    );
    let output = run(&nested, &["ai", "install"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("up to date"));
}

#[test]
fn noninteractive_init_skips_ai_and_explicit_targets_are_required_initially() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert!(run(root, &["init", "app"]).status.success());
    assert!(!root.join(".agents").exists());
    let output = run(root, &["ai", "install"]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("--agent"));
    assert!(
        !run(root, &["ai", "install", "--agent", "codex"])
            .status
            .success()
    );
    assert!(
        !run(root, &["init", "other", "--ai", "generic", "--no-ai"])
            .status
            .success()
    );
    assert!(!root.join("other").exists());
}

#[test]
fn dry_run_and_repeated_targets_do_not_create_extra_state() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert!(run(root, &["init", "app", "--no-ai"]).status.success());
    let output = run(
        root,
        &["ai", "install", "--agent", "generic,claude", "--dry-run"],
    );
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("add .agents/skills"));
    assert!(!root.join(".agents").exists());
    assert!(!root.join(".claude").exists());
    assert!(
        run(
            root,
            &["ai", "install", "--agent", "generic", "--agent", "generic"]
        )
        .status
        .success()
    );
    assert!(!root.join(".claude").exists());
    assert!(
        run(root, &["ai", "install", "--agent", "claude"])
            .status
            .success()
    );
    assert!(run(root, &["ai", "install"]).status.success());
}

#[test]
fn conflicts_preflight_all_targets_and_preserve_user_changes() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    install(root, &[Target::Generic], false).unwrap();
    let path = root
        .join(skill(Target::Generic))
        .join("references/syntax.md");
    fs::write(&path, "user edit").unwrap();
    let unrelated = root.join(skill(Target::Generic)).join("personal.md");
    fs::write(&unrelated, "personal notes").unwrap();
    let error = install(root, &[Target::Generic, Target::Claude], false).unwrap_err();
    assert!(error.to_string().contains("references/syntax.md"));
    assert_eq!(fs::read_to_string(path).unwrap(), "user edit");
    assert_eq!(fs::read_to_string(unrelated).unwrap(), "personal notes");
    assert!(!root.join(".claude").exists());
    assert!(install(root, &[Target::Generic], true).is_err());
}

#[test]
fn updates_old_managed_content_and_removes_only_unchanged_obsolete_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    install(root, &[Target::Generic], false).unwrap();
    let prefix = skill(Target::Generic);
    let syntax = format!("{prefix}/references/syntax.md");
    let obsolete = format!("{prefix}/references/obsolete.md");
    let unrelated = format!("{prefix}/references/personal.md");
    for name in [&syntax, &obsolete, &unrelated] {
        fs::write(root.join(name), "old content").unwrap();
    }
    let mut state = receipt(root, Target::Generic);
    state["compiler_version"] = "0.0.0".into();
    let hash = format!("{:x}", Sha256::digest(b"old content"));
    state["files"][&syntax] = hash.clone().into();
    state["files"][&obsolete] = hash.into();
    write_receipt(root, Target::Generic, &state);
    install(root, &[Target::Generic], true).unwrap();
    assert!(root.join(&obsolete).exists());
    install(root, &[Target::Generic], false).unwrap();
    assert!(!root.join(&obsolete).exists());
    assert_eq!(
        fs::read_to_string(root.join(unrelated)).unwrap(),
        "old content"
    );
    assert!(
        fs::read_to_string(root.join(syntax))
            .unwrap()
            .contains("# Syntax")
    );
    assert_eq!(
        receipt(root, Target::Generic)["compiler_version"],
        env!("CARGO_PKG_VERSION")
    );
}

#[test]
fn rejects_receipts_claiming_unmanaged_paths() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    install(root, &[Target::Generic], false).unwrap();
    let mut state = receipt(root, Target::Generic);
    state["files"]["AGENTS.md"] = "hash".into();
    write_receipt(root, Target::Generic, &state);
    assert!(
        install(root, &[Target::Generic], false)
            .unwrap_err()
            .to_string()
            .contains("unmanaged")
    );
}

#[test]
fn rejects_preexisting_conflicting_files_without_receipt() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join(skill(Target::Claude));
    fs::create_dir_all(&path).unwrap();
    fs::write(path.join("SKILL.md"), "custom skill").unwrap();
    assert!(install(directory.path(), &[Target::Claude], false).is_err());
    assert!(!path.join(".dovetail-install.json").exists());
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_destination_ancestors() {
    let directory = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), directory.path().join(".agents")).unwrap();
    assert!(
        install(directory.path(), &[Target::Generic], false)
            .unwrap_err()
            .to_string()
            .contains("symlink")
    );
    assert_eq!(fs::read_dir(outside.path()).unwrap().count(), 0);
}

#[test]
fn init_reports_workspace_survives_optional_install_failure() {
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join(".agents"), "not a directory").unwrap();
    let output = run(directory.path(), &["init", "app", "--ai", "generic"]);
    assert!(!output.status.success());
    assert!(directory.path().join("Dovetail.toml").is_file());
    let message = String::from_utf8_lossy(&output.stderr);
    assert!(message.contains("workspace was created"));
    assert!(message.contains("dovetail ai install"));
}

#[test]
fn edited_obsolete_files_are_preserved_and_reported() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    install(root, &[Target::Generic], false).unwrap();
    let obsolete = format!("{}/references/old.md", skill(Target::Generic));
    fs::write(root.join(&obsolete), "user changed this").unwrap();
    let mut state = receipt(root, Target::Generic);
    state["files"][&obsolete] = format!("{:x}", Sha256::digest(b"previous content")).into();
    write_receipt(root, Target::Generic, &state);
    assert!(install(root, &[Target::Generic], false).is_err());
    assert_eq!(
        fs::read_to_string(root.join(obsolete)).unwrap(),
        "user changed this"
    );
}

#[test]
fn claude_wrapper_conflicts_do_not_install_generic_files() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    fs::create_dir_all(root.join(".claude/agents")).unwrap();
    fs::write(
        root.join(".claude/agents/dovetail-reviewer.md"),
        "custom agent",
    )
    .unwrap();
    assert!(install(root, &[Target::Generic, Target::Claude], false).is_err());
    assert!(!root.join(".agents").exists());
    assert!(!root.join(".claude/skills").exists());
}

#[test]
fn install_requires_a_workspace_but_not_resolved_dependencies() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    assert!(
        !run(root, &["ai", "install", "--agent", "generic"])
            .status
            .success()
    );
    fs::write(root.join("Dovetail.toml"), "not a resolved manifest").unwrap();
    assert!(
        run(root, &["ai", "install", "--agent", "generic", "--offline"])
            .status
            .success()
    );
}

#[cfg(unix)]
#[test]
fn rejects_symlinked_managed_file_and_preserves_update_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    install(root, &[Target::Generic], false).unwrap();
    let relative = format!("{}/references/syntax.md", skill(Target::Generic));
    let path = root.join(&relative);
    fs::write(&path, "old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640)).unwrap();
    let mut state = receipt(root, Target::Generic);
    state["files"][&relative] = format!("{:x}", Sha256::digest(b"old")).into();
    write_receipt(root, Target::Generic, &state);
    install(root, &[Target::Generic], false).unwrap();
    assert_eq!(
        fs::metadata(&path).unwrap().permissions().mode() & 0o777,
        0o640
    );
    fs::remove_file(&path).unwrap();
    let outside = tempfile::NamedTempFile::new().unwrap();
    std::os::unix::fs::symlink(outside.path(), &path).unwrap();
    assert!(install(root, &[Target::Generic], false).is_err());
}
