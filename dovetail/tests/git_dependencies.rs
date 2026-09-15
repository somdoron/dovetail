use dovetail::manifest::{
    ResolveOptions, UpdateRequest, load_manifest, load_manifest_with_options,
};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

fn git(root: &Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout).unwrap().trim().into()
}
fn project(root: &Path, name: &str, package: &str, source: &str) {
    std::fs::create_dir_all(root.join(name).join("src")).unwrap();
    std::fs::write(
        root.join(name).join("src/main.dove"),
        format!("package {package}\n{source}\n"),
    )
    .unwrap();
}
fn manifest(root: &Path, text: &str) {
    std::fs::write(
        root.join("Dovetail.toml"),
        format!(
            "compiler-version = \"{}\"\n{text}",
            env!("CARGO_PKG_VERSION")
        ),
    )
    .unwrap();
}
fn entry(name: &str, package: &str, deps: &[&str]) -> String {
    format!(
        "\n[[project]]\nname = {name:?}\nroot_package = {package:?}\npackages = [\".\"]\ndepends = {deps:?}\n"
    )
}
fn dependency(repo: &Path, selector: &str, selections: &str) -> String {
    format!(
        "\n[[dependencies]]\ngit = {:?}\n{selector}\nprojects = [{selections}]\n",
        repo.to_str().unwrap()
    )
}
fn repo() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    git(dir.path(), &["init", "-b", "main"]);
    git(
        dir.path(),
        &["config", "user.email", "test@example.invalid"],
    );
    git(dir.path(), &["config", "user.name", "Dovetail test"]);
    dir
}
fn commit(root: &Path) -> String {
    git(root, &["add", "."]);
    git(root, &["commit", "-m", "fixture"]);
    git(root, &["rev-parse", "HEAD"])
}
fn check(workspace: &dovetail::manifest::ResolvedWorkspace, target: Option<&str>) {
    let result = dovetail::build_workspace(
        workspace,
        target,
        dovetail::BuildMode::Check,
        &HashMap::new(),
        false,
        None,
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.iter().collect::<Vec<_>>()
    );
}

#[test]
fn transitive_repositories_are_locked_and_compile_offline() {
    let encoding = repo();
    project(
        encoding.path(),
        "encoding",
        "encoding",
        "public function value(): Int32 = 42",
    );
    manifest(encoding.path(), &entry("encoding", "encoding", &[]));
    commit(encoding.path());
    let protocol = repo();
    project(
        protocol.path(),
        "protocol",
        "protocol",
        "import encoding.value\npublic function answer(): Int32 = value()",
    );
    manifest(
        protocol.path(),
        &(dependency(encoding.path(), "branch = \"main\"", "\"encoding\"")
            + &*entry("protocol", "protocol", &["encoding"])),
    );
    commit(protocol.path());
    let postgres = repo();
    project(
        postgres.path(),
        "postgres",
        "postgres",
        "import protocol.answer\npublic function query(): Int32 = answer()",
    );
    manifest(
        postgres.path(),
        &(dependency(protocol.path(), "", "\"protocol\"")
            + &*entry("postgres", "postgres", &["protocol"])),
    );
    commit(postgres.path());
    let app = tempfile::tempdir().unwrap();
    project(
        app.path(),
        "app",
        "app",
        "import postgres.query\nfunction main(): Int32 = query()",
    );
    manifest(
        app.path(),
        &(dependency(postgres.path(), "", "\"postgres\"") + &*entry("app", "app", &["postgres"])),
    );
    let workspace = load_manifest(app.path()).unwrap();
    assert_eq!(workspace.projects.len(), 4);
    check(&workspace, Some("app"));
    let lock = std::fs::read_to_string(app.path().join("Dovetail.lock")).unwrap();
    assert!(lock.contains("encoding"));
    let offline = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            locked: true,
            offline: true,
            ..Default::default()
        },
    )
    .unwrap();
    check(&offline, Some("app"));
    assert_eq!(
        lock,
        std::fs::read_to_string(app.path().join("Dovetail.lock")).unwrap()
    );
}

#[test]
fn branch_is_pinned_until_explicit_update_and_aliases_share_a_checkout() {
    let remote = repo();
    for name in ["one", "two"] {
        project(
            remote.path(),
            name,
            name,
            "public function value(): Int32 = 1",
        );
    }
    manifest(
        remote.path(),
        &(entry("one", "one", &[]) + &*entry("two", "two", &[])),
    );
    let first = commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "function main(): Unit = ()");
    manifest(
        app.path(),
        &(dependency(
            remote.path(),
            "branch = \"main\"",
            "\"one\", { project = \"two\", alias = \"second\" }",
        ) + &*entry("app", "app", &["one", "second"])),
    );
    let initial = load_manifest(app.path()).unwrap();
    assert_eq!(
        initial.projects[0].project_dir.parent(),
        initial.projects[1].project_dir.parent()
    );
    project(
        remote.path(),
        "one",
        "one",
        "public function value(): Int32 = 2",
    );
    let second = commit(remote.path());
    let pinned = load_manifest(app.path()).unwrap();
    assert!(pinned.projects[0].identity().contains(&first));
    let updated = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            update: UpdateRequest::Alias("second".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(updated.projects[0].identity().contains(&second));
    assert!(updated.projects[1].identity().contains(&second));
}

#[test]
fn separate_versions_build_but_combined_closure_fails() {
    let remote = repo();
    project(
        remote.path(),
        "postgres",
        "postgres",
        "public function value(): Int32 = 1",
    );
    manifest(remote.path(), &entry("postgres", "postgres", &[]));
    commit(remote.path());
    git(remote.path(), &["tag", "v1"]);
    project(
        remote.path(),
        "postgres",
        "postgres",
        "public function value(): Int32 = 2",
    );
    commit(remote.path());
    git(remote.path(), &["tag", "v2"]);
    let app = tempfile::tempdir().unwrap();
    for name in ["old", "new", "combined"] {
        project(app.path(), name, name, "function main(): Unit = ()");
    }
    let base = dependency(
        remote.path(),
        "tag = \"v1\"",
        "{ project = \"postgres\", alias = \"pg1\" }",
    ) + &*dependency(
        remote.path(),
        "tag = \"v2\"",
        "{ project = \"postgres\", alias = \"pg2\" }",
    ) + &*entry("old", "old", &["pg1"])
        + &*entry("new", "new", &["pg2"]);
    manifest(app.path(), &base);
    let workspace = load_manifest(app.path()).unwrap();
    check(&workspace, None);
    manifest(
        app.path(),
        &(base + &*entry("combined", "combined", &["old", "new"])),
    );
    let errors = load_manifest(app.path()).unwrap_err();
    let text = errors[0].to_string();
    assert!(
        text.contains("multiple providers") && text.contains("old") && text.contains("new"),
        "{text}"
    );
    let selected = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            target: Some("old".into()),
            ..Default::default()
        },
    )
    .unwrap();
    check(&selected, Some("old"));
}

#[test]
fn compiler_mismatch_precedes_network_and_cache_creation() {
    let app = tempfile::tempdir().unwrap();
    std::fs::write(app.path().join("Dovetail.toml"), "compiler-version = \"999.0.0\"\nproject = []\n[[dependencies]]\ngit = \"https://invalid.example/repo\"\nprojects = [\"x\"]\n").unwrap();
    assert!(
        load_manifest(app.path()).unwrap_err()[0]
            .to_string()
            .contains("running binary")
    );
    assert!(!app.path().join(".dovetail").exists());
    std::fs::write(app.path().join("Dovetail.toml"), "project = []").unwrap();
    assert!(
        load_manifest(app.path()).unwrap_err()[0]
            .to_string()
            .contains("compiler-version")
    );
}

#[test]
fn transitive_version_mismatch_reports_path() {
    let remote = repo();
    project(remote.path(), "driver", "driver", "");
    std::fs::write(
        remote.path().join("Dovetail.toml"),
        format!(
            "compiler-version = \"999.0.0\"\n{}",
            entry("driver", "driver", &[])
        ),
    )
    .unwrap();
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"driver\"") + &*entry("app", "app", &["driver"])),
    );
    let text = load_manifest(app.path()).unwrap_err()[0].to_string();
    assert!(text.contains("app") && text.contains("999.0.0"), "{text}");
    assert!(!app.path().join("Dovetail.lock").exists());
}

#[test]
fn diamonds_deduplicate_and_nested_manifests_resolve() {
    let remote = repo();
    let nested = remote.path().join("libraries");
    for name in ["base", "left", "right"] {
        project(&nested, name, name, "");
    }
    manifest(
        &nested,
        &(entry("base", "base", &[])
            + &*entry("left", "left", &["base"])
            + &*entry("right", "right", &["base"])),
    );
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &(dependency(
            remote.path(),
            "manifest = \"libraries/Dovetail.toml\"",
            "\"left\", \"right\"",
        ) + &*entry("app", "app", &["left", "right"])),
    );
    let workspace = load_manifest(app.path()).unwrap();
    assert_eq!(workspace.projects.len(), 4);
    assert_eq!(
        workspace
            .projects
            .iter()
            .filter(|p| p.name.0 == "base")
            .count(),
        1
    );
}

#[test]
fn concurrent_resolution_publishes_one_complete_checkout() {
    let remote = repo();
    project(remote.path(), "lib", "lib", "");
    manifest(remote.path(), &entry("lib", "lib", &[]));
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"lib\"") + &*entry("app", "app", &["lib"])),
    );
    std::thread::scope(|scope| {
        let first = scope.spawn(|| load_manifest(app.path()).unwrap());
        let second = scope.spawn(|| load_manifest(app.path()).unwrap());
        assert_eq!(
            first.join().unwrap().projects[0].identity(),
            second.join().unwrap().projects[0].identity()
        );
    });
    load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            locked: true,
            offline: true,
            ..Default::default()
        },
    )
    .unwrap();
}

#[cfg(unix)]
fn git_wrapper(root: &Path, script: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("fake-git");
    std::fs::write(&path, format!("#!/bin/sh\n{script}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[cfg(unix)]
#[test]
fn standard_shorthand_uses_canonical_repository_and_bundled_prelude() {
    let remote = repo();
    project(
        remote.path(),
        "prelude",
        "standard.prelude",
        "invalid prelude must not be compiled",
    );
    project(
        remote.path(),
        "standard-json",
        "standard.json",
        "public function value(): Int32 = 1",
    );
    project(remote.path(), "fixture", "interop.fixture", "");
    manifest(
        remote.path(),
        &(entry("prelude", "standard.prelude", &[])
            + &*entry("standard-json", "standard.json", &["prelude"])
            + &*entry("fixture", "interop.fixture", &[])),
    );
    commit(remote.path());
    git(remote.path(), &["tag", "1.1"]);
    let app = tempfile::tempdir().unwrap();
    project(
        app.path(),
        "app",
        "app",
        "import standard.json.value\nfunction main(): Int32 = value()",
    );
    manifest(
        app.path(),
        &("standard-tag = \"1.1\"\n".to_string() + &*entry("app", "app", &["standard-json"])),
    );
    let executable = git_wrapper(
        app.path(),
        &format!(
            "exec git -c 'url.{}.insteadOf=https://github.com/somdoron/dovetail.git' \"$@\"",
            remote.path().display()
        ),
    );
    let workspace = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            git_executable: Some(executable),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(workspace.projects.len(), 2);
    check(&workspace, Some("app"));
    load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            locked: true,
            offline: true,
            ..Default::default()
        },
    )
    .unwrap();
}

#[cfg(unix)]
#[test]
fn failed_private_fetch_is_redacted_and_cancelled_fetch_leaves_no_lockfile() {
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &("[[dependencies]]\ngit = \"ssh://git@github.com/private/lib.git\"\nprojects = [\"lib\"]\n"
            .to_string() + &*entry("app", "app", &["lib"])),
    );
    let executable = git_wrapper(
        app.path(),
        "if [ \"$1\" = fetch ]; then echo 'secret-token' >&2; exit 128; fi\nexec git \"$@\"",
    );
    let errors = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            git_executable: Some(executable),
            ..Default::default()
        },
    )
    .unwrap_err();
    let text = errors[0].to_string();
    assert!(
        text.contains("credentials") && text.contains("app") && text.contains("lib"),
        "{text}"
    );
    assert!(!text.contains("secret-token"));
    assert!(!app.path().join("Dovetail.lock").exists());
    let executable = git_wrapper(
        app.path(),
        "if [ \"$1\" = fetch ]; then exec sleep 30; fi\nexec git \"$@\"",
    );
    let cancelled = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    std::thread::scope(|scope| {
        let flag = cancelled.clone();
        scope.spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(200));
            flag.store(true, std::sync::atomic::Ordering::SeqCst);
        });
        let errors = load_manifest_with_options(
            app.path(),
            &ResolveOptions {
                git_executable: Some(executable),
                cancelled: Some(cancelled),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert!(errors[0].to_string().contains("cancelled"), "{}", errors[0]);
    });
    assert!(!app.path().join("Dovetail.lock").exists());
}

#[test]
fn fetched_component_and_resources_propagate_to_consumer() {
    let remote = repo();
    let source_root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let source = load_manifest_with_options(
        source_root,
        &ResolveOptions {
            target: Some("standard-sqlite".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let names: Vec<_> = source
        .projects
        .iter()
        .filter(|p| p.name.0 != "prelude")
        .map(|p| p.name.0.clone())
        .collect();
    for project in source.projects.iter().filter(|p| p.name.0 != "prelude") {
        copy_directory(&project.project_dir, &remote.path().join(&project.name.0));
    }
    let mut raw: toml::Value = std::fs::read_to_string(source_root.join("Dovetail.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let entries = raw.get_mut("project").unwrap().as_array_mut().unwrap();
    entries.retain(|p| names.contains(&p["name"].as_str().unwrap().to_string()));
    for project in entries.iter_mut() {
        if let Some(deps) = project.get_mut("depends").and_then(|d| d.as_array_mut()) {
            deps.retain(|d| d.as_str() != Some("prelude"));
        }
    }
    std::fs::write(
        remote.path().join("Dovetail.toml"),
        toml::to_string(&raw).unwrap(),
    )
    .unwrap();
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "function main(): Unit = ()");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"standard-sqlite\"")
            + &*entry("app", "app", &["standard-sqlite"])),
    );
    let workspace = load_manifest(app.path()).unwrap();
    assert_eq!(workspace.projects.last().unwrap().components.len(), 1);
    check(&workspace, Some("app"));
    let build = dovetail::build_workspace(
        &workspace,
        Some("app"),
        dovetail::BuildMode::Build,
        &HashMap::new(),
        false,
        None,
    );
    assert!(
        !build.diagnostics.has_errors(),
        "{:?}",
        build.diagnostics.iter().collect::<Vec<_>>()
    );
    assert_eq!(
        build
            .project_results
            .iter()
            .filter(|(_, r)| r.wasm.is_some())
            .count(),
        1
    );
    let generated = app.path().join(".dovetail/generated");
    assert!(
        std::fs::read_dir(generated).unwrap().any(|entry| entry
            .unwrap()
            .path()
            .join("sqlite.raw.dove")
            .exists())
    );
    let locked = std::fs::read(app.path().join("Dovetail.lock")).unwrap();
    let sqlite = raw["project"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|project| project["name"].as_str() == Some("standard-sqlite"))
        .unwrap();
    sqlite["component"][0]["package"] = "standard.prelude".into();
    std::fs::write(
        remote.path().join("Dovetail.toml"),
        toml::to_string(&raw).unwrap(),
    )
    .unwrap();
    commit(remote.path());
    let errors = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            update: UpdateRequest::Alias("standard-sqlite".into()),
            ..Default::default()
        },
    )
    .unwrap_err();
    assert!(
        errors[0]
            .to_string()
            .contains("overlaps the bundled prelude")
    );
    assert_eq!(
        std::fs::read(app.path().join("Dovetail.lock")).unwrap(),
        locked
    );
}

#[cfg(unix)]
#[test]
fn escaped_source_and_lfs_artifacts_are_rejected() {
    use std::os::unix::fs::symlink;
    let remote = repo();
    project(remote.path(), "lib", "lib", "");
    std::fs::write(
        remote.path().join("lib/data.bin"),
        "version https://git-lfs.github.com/spec/v1\noid sha256:123\nsize 42\n",
    )
    .unwrap();
    manifest(
        remote.path(),
        &(entry("lib", "lib", &[]) + "resources = [\"data.bin\"]\n"),
    );
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"lib\"") + &*entry("app", "app", &["lib"])),
    );
    assert!(
        load_manifest(app.path()).unwrap_err()[0]
            .to_string()
            .contains("LFS")
    );
    std::fs::write(remote.path().join("lib/data.bin"), b"normal").unwrap();
    std::fs::remove_file(remote.path().join("lib/src/main.dove")).unwrap();
    symlink(
        app.path().join("app/src/main.dove"),
        remote.path().join("lib/src/main.dove"),
    )
    .unwrap();
    commit(remote.path());
    assert!(
        load_manifest(app.path()).unwrap_err()[0]
            .to_string()
            .contains("escapes")
    );
}

#[test]
fn local_targets_produce_outputs_and_remote_tests_are_not_run() {
    let remote = repo();
    project(
        remote.path(),
        "lib",
        "lib",
        "test \"remote failure\" = assert false",
    );
    manifest(remote.path(), &entry("lib", "lib", &[]));
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    for name in ["one", "two"] {
        project(app.path(), name, name, "function main(): Unit = ()");
    }
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"lib\"")
            + &*entry("one", "one", &["lib"])
            + &*entry("two", "two", &[])),
    );
    let workspace = load_manifest(app.path()).unwrap();
    for mode in [dovetail::BuildMode::Build, dovetail::BuildMode::Test] {
        let result =
            dovetail::build_workspace(&workspace, None, mode, &HashMap::new(), false, None);
        assert!(
            !result.diagnostics.has_errors(),
            "{:?}",
            result.diagnostics.iter().collect::<Vec<_>>()
        );
        assert_eq!(
            result
                .project_results
                .iter()
                .filter(|(_, r)| r.wasm.is_some())
                .count(),
            2
        );
        assert!(
            result
                .project_results
                .iter()
                .all(|(_, r)| r.test_exports.is_empty())
        );
    }
}

#[test]
fn lsp_snapshots_navigate_to_correct_revision_and_clear_on_version_error() {
    use tower_lsp::lsp_types::Url;
    let remote = repo();
    project(
        remote.path(),
        "lib",
        "lib",
        "public function value(): Int32 = 1",
    );
    manifest(remote.path(), &entry("lib", "lib", &[]));
    let old_commit = commit(remote.path());
    git(remote.path(), &["tag", "v1"]);
    project(
        remote.path(),
        "lib",
        "lib",
        "public function value(): Int32 = 2",
    );
    let new_commit = commit(remote.path());
    git(remote.path(), &["tag", "v2"]);
    let app = tempfile::tempdir().unwrap();
    for name in ["old", "new"] {
        project(
            app.path(),
            name,
            name,
            "import lib.value\nfunction main(): Int32 = value()",
        );
    }
    manifest(
        app.path(),
        &(dependency(
            remote.path(),
            "tag = \"v1\"",
            "{ project = \"lib\", alias = \"lib1\" }",
        ) + &*dependency(
            remote.path(),
            "tag = \"v2\"",
            "{ project = \"lib\", alias = \"lib2\" }",
        ) + &*entry("old", "old", &["lib1"])
            + &*entry("new", "new", &["lib2"])),
    );
    let state = dovetail::lsp::state::WorldState::new();
    let root = app.path().canonicalize().unwrap();
    *state.workspace_root.write().unwrap() = Some(root.clone());
    let diagnostics = state.load_and_check_workspace(None).unwrap();
    assert!(diagnostics.values().all(Vec::is_empty), "{diagnostics:?}");
    for (name, expected) in [("old", &old_commit), ("new", &new_commit)] {
        let uri = Url::from_file_path(root.join(name).join("src/main.dove")).unwrap();
        let context = state.context_for(&uri).unwrap();
        let file: dovetail::common::span::FilePath = format!("{name}/src/main.dove").into();
        let node =
            dovetail::lsp::position::find_node_at_position(&context.module, &file, 3, 28).unwrap();
        let definition = dovetail::lsp::navigation::goto_definition(
            &node,
            &context.module,
            &context.registry,
            &root,
        )
        .unwrap();
        assert!(definition.uri.as_str().contains(expected), "{definition:?}");
        assert!(definition.uri.to_file_path().unwrap().is_file());
        assert!(state.context_for(&definition.uri).is_some());
    }
    // Refresh from an externally updated lockfile without changing the manifest.
    git(remote.path(), &["tag", "-f", "v1"]);
    load_manifest_with_options(
        &root,
        &ResolveOptions {
            update: UpdateRequest::Alias("lib1".into()),
            ..Default::default()
        },
    )
    .unwrap();
    let diagnostics = state.load_and_check_workspace(None).unwrap();
    assert!(diagnostics.values().all(Vec::is_empty), "{diagnostics:?}");
    let uri = Url::from_file_path(root.join("old/src/main.dove")).unwrap();
    let context = state.context_for(&uri).unwrap();
    let file: dovetail::common::span::FilePath = "old/src/main.dove".into();
    let node =
        dovetail::lsp::position::find_node_at_position(&context.module, &file, 3, 28).unwrap();
    let definition = dovetail::lsp::navigation::goto_definition(
        &node,
        &context.module,
        &context.registry,
        &root,
    )
    .unwrap();
    assert!(
        definition.uri.as_str().contains(&new_commit),
        "{definition:?}"
    );
    let path = root.join("Dovetail.toml");
    let source = std::fs::read_to_string(&path).unwrap();
    std::fs::write(path, source.replace(env!("CARGO_PKG_VERSION"), "999.0.0")).unwrap();
    let diagnostics = state.load_and_check_workspace(None).unwrap();
    assert!(
        diagnostics
            .values()
            .flatten()
            .any(|d| d.message.contains("compiler version"))
    );
    assert!(state.contexts.read().unwrap().is_empty());
}

fn copy_directory(source: &Path, target: &Path) {
    std::fs::create_dir_all(target).unwrap();
    for entry in std::fs::read_dir(source).unwrap() {
        let entry = entry.unwrap();
        let destination = target.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_directory(&entry.path(), &destination);
        } else {
            std::fs::copy(entry.path(), destination).unwrap();
        }
    }
}

#[test]
fn binary_reports_version_and_rejects_incompatible_workspace() {
    let binary = env!("CARGO_BIN_EXE_dovetail");
    let output = Command::new(binary).arg("--version").output().unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains(env!("CARGO_PKG_VERSION")));
    let root = tempfile::tempdir().unwrap();
    std::fs::write(
        root.path().join("Dovetail.toml"),
        "compiler-version = \"999.0.0\"\nproject = []\n",
    )
    .unwrap();
    for command in ["build", "check", "run", "test"] {
        let output = Command::new(binary)
            .current_dir(root.path())
            .args([command, "--locked", "--offline"])
            .output()
            .unwrap();
        assert!(!output.status.success(), "{command}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("compiler version mismatch"));
    }
    assert!(!root.path().join(".dovetail").exists());
}

#[test]
fn duplicate_aliases_and_dependency_cycles_are_errors() {
    let remote = repo();
    project(remote.path(), "a", "a", "");
    project(remote.path(), "b", "b", "");
    manifest(
        remote.path(),
        &(entry("a", "a", &["b"]) + &*entry("b", "b", &["a"])),
    );
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"a\"") + &*entry("app", "app", &["a"])),
    );
    let text = load_manifest(app.path()).unwrap_err()[0].to_string();
    assert!(text.contains("circular dependency"), "{text}");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "{ project = \"a\", alias = \"app\" }")
            + &*entry("app", "app", &[])),
    );
    assert!(
        load_manifest(app.path()).unwrap_err()[0]
            .to_string()
            .contains("alias")
    );
    assert!(!app.path().join("Dovetail.lock").exists());
}

#[test]
fn manually_assembled_local_project_outside_workspace_still_builds() {
    use dovetail::common::types::PackagePath;
    use dovetail::manifest::{ProjectName, ResolvedPackage, ResolvedProject, ResolvedWorkspace};
    let root = tempfile::tempdir().unwrap();
    let external = tempfile::tempdir().unwrap();
    project(external.path(), "app", "app", "function main(): Unit = ()");
    let workspace = ResolvedWorkspace {
        workspace_root: root.path().into(),
        dependency_aliases: Default::default(),
        projects: vec![ResolvedProject {
            resolved_identity: None,
            name: ProjectName("app".into()),
            root_package: PackagePath::from_dotted("app"),
            project_dir: external.path().join("app"),
            packages: vec![ResolvedPackage {
                path: PackagePath::from_dotted("app"),
                source_dir: external.path().join("app/src"),
            }],
            depends: vec![],
            main_function: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
        }],
    };
    let result = dovetail::build_workspace(
        &workspace,
        Some("app"),
        dovetail::BuildMode::Build,
        &HashMap::new(),
        false,
        None,
    );
    assert!(
        !result.diagnostics.has_errors(),
        "{:?}",
        result.diagnostics.iter().collect::<Vec<_>>()
    );
    assert_eq!(result.project_results.len(), 1);
    assert!(result.project_results[0].1.wasm.is_some());
}

#[cfg(unix)]
#[test]
fn repository_cannot_overwrite_files_through_checkout_marker() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    let protected = outside.path().join("keep.txt");
    std::fs::write(&protected, "keep this content").unwrap();
    let remote = repo();
    project(remote.path(), "lib", "lib", "");
    manifest(remote.path(), &entry("lib", "lib", &[]));
    symlink(
        &protected,
        remote.path().join(".dovetail-checkout-complete"),
    )
    .unwrap();
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"lib\"") + &*entry("app", "app", &["lib"])),
    );
    let _ = load_manifest(app.path());
    assert_eq!(
        std::fs::read_to_string(protected).unwrap(),
        "keep this content"
    );
}

#[test]
fn fetching_unused_selection_validates_its_complete_closure() {
    let remote = repo();
    for name in ["one", "two"] {
        project(remote.path(), name, "shared", "");
    }
    project(remote.path(), "combined", "combined", "");
    manifest(
        remote.path(),
        &(entry("one", "shared", &[])
            + &*entry("two", "shared", &[])
            + &*entry("combined", "combined", &["one", "two"])),
    );
    commit(remote.path());
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &(dependency(remote.path(), "", "\"combined\"") + &*entry("app", "app", &[])),
    );
    let errors = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            fetch_all: true,
            ..Default::default()
        },
    )
    .unwrap_err();
    let text = errors[0].to_string();
    assert!(
        text.contains("multiple providers") && text.contains("one") && text.contains("two"),
        "{text}"
    );
    assert!(!app.path().join("Dovetail.lock").exists());
}

#[test]
fn targeted_resolution_ignores_unreachable_local_cycles() {
    let app = tempfile::tempdir().unwrap();
    for name in ["good", "a", "b"] {
        project(app.path(), name, name, "");
    }
    manifest(
        app.path(),
        &(entry("good", "good", &[]) + &*entry("a", "a", &["b"]) + &*entry("b", "b", &["a"])),
    );
    let workspace = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            target: Some("good".into()),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(workspace.projects.len(), 1);
    check(&workspace, Some("good"));
    assert!(load_manifest(app.path()).is_err());
}

#[cfg(unix)]
#[test]
fn standard_manifest_symlink_is_rejected_before_parsing_outside_file() {
    use std::os::unix::fs::symlink;
    let outside = tempfile::tempdir().unwrap();
    let external_manifest = outside.path().join("outside.toml");
    std::fs::write(&external_manifest, "this is not a Dovetail manifest").unwrap();
    let remote = repo();
    symlink(external_manifest, remote.path().join("Dovetail.toml")).unwrap();
    commit(remote.path());
    git(remote.path(), &["tag", "1.1"]);
    let app = tempfile::tempdir().unwrap();
    project(app.path(), "app", "app", "");
    manifest(
        app.path(),
        &("standard-tag = \"1.1\"\n".to_string() + &*entry("app", "app", &[])),
    );
    let executable = git_wrapper(
        app.path(),
        &format!(
            "exec git -c 'url.{}.insteadOf=https://github.com/somdoron/dovetail.git' \"$@\"",
            remote.path().display()
        ),
    );
    let errors = load_manifest_with_options(
        app.path(),
        &ResolveOptions {
            git_executable: Some(executable),
            ..Default::default()
        },
    )
    .unwrap_err();
    let text = errors[0].to_string();
    assert!(text.contains("escapes"), "{text}");
    assert!(!app.path().join("Dovetail.lock").exists());
}
