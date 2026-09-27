use std::path::Path;
use std::process::{Command, Output};

const UNFORMATTED: &str = r#"package a
function add( x:Int32,y:Int32 ):Int32 =
  x+y
"#;
const FORMATTED: &str = r#"package a

function add(x: Int32, y: Int32): Int32 = x + y
"#;

fn cli(directory: &Path, arguments: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_dovetail"))
        .current_dir(directory)
        .args(arguments)
        .output()
        .unwrap()
}

#[test]
fn explicit_files_work_without_manifest_and_check_never_writes() {
    let directory = tempfile::tempdir().unwrap();
    let file = directory.path().join("example.dove");
    std::fs::write(&file, UNFORMATTED).unwrap();
    let output = cli(directory.path(), &["fmt", "--check", "example.dove"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stdout)
            .unwrap()
            .contains("example.dove")
    );
    assert_eq!(std::fs::read_to_string(&file).unwrap(), UNFORMATTED);
    let output = cli(directory.path(), &["fmt", "example.dove", "./example.dove"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 1);
    assert_eq!(std::fs::read_to_string(&file).unwrap(), FORMATTED);
    assert!(
        cli(directory.path(), &["fmt", "--check", "example.dove"])
            .status
            .success()
    );
}

#[test]
fn invalid_input_prevents_all_writes() {
    let directory = tempfile::tempdir().unwrap();
    let valid = directory.path().join("valid.dove");
    let invalid = directory.path().join("invalid.dove");
    std::fs::write(&valid, UNFORMATTED).unwrap();
    std::fs::write(
        &invalid,
        r#"package a
function broken( =
"#,
    )
    .unwrap();
    assert_eq!(
        cli(directory.path(), &["fmt", "valid.dove", "invalid.dove"])
            .status
            .code(),
        Some(2)
    );
    assert_eq!(std::fs::read_to_string(valid).unwrap(), UNFORMATTED);
    assert_eq!(cli(directory.path(), &["fmt", "."]).status.code(), Some(2));
    assert_eq!(
        cli(directory.path(), &["fmt", "--stdin"]).status.code(),
        Some(2)
    );
}

#[test]
fn workspace_discovery_is_local_and_includes_test_trees() {
    let directory = tempfile::tempdir().unwrap();
    let root = directory.path();
    std::fs::write(
        root.join("Dovetail.toml"),
        r#"compiler-version = "0.1.1"
[[dependencies]]
git = "https://invalid.example/unavailable.git"
rev = "0000000000000000000000000000000000000000"
projects = ["dependency"]
[[project]]
name = "app"
path = "custom"
root_package = "a"
packages = [".", "a.helpers"]
depends = ["dependency"]
"#,
    )
    .unwrap();
    for path in [
        "custom/src/main.dove",
        "custom/src/helpers/helper.dove",
        "custom/test/nested/exampleTest.dove",
    ] {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, UNFORMATTED).unwrap();
    }
    let excluded = root.join("build/generated.dove");
    std::fs::create_dir_all(excluded.parent().unwrap()).unwrap();
    std::fs::write(&excluded, "deliberately invalid generated content").unwrap();
    let output = cli(&root.join("custom/src"), &["fmt"]);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8(output.stdout).unwrap().lines().count(), 3);
    assert!(!root.join("Dovetail.lock").exists());
    assert_eq!(
        std::fs::read_to_string(excluded).unwrap(),
        "deliberately invalid generated content"
    );
    assert!(cli(root, &["fmt", "--check"]).status.success());
}
