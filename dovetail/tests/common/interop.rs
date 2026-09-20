use std::path::PathBuf;
use std::sync::OnceLock;

/// Build only the interop project and its dependencies, once per test binary.
pub fn interop_wasm() -> &'static [u8] {
    static WASM: OnceLock<Vec<u8>> = OnceLock::new();
    WASM.get_or_init(build_interop_wasm)
}

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR points at the `dovetail/` crate root; the workspace
    // root is one level up.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("dovetail crate must live in a parent dir")
        .to_path_buf()
}

fn build_interop_wasm() -> Vec<u8> {
    let project = "standard-io-http-interop";
    let root = repo_root();
    let workspace = dovetail::manifest::load_manifest(&root)
        .unwrap_or_else(|errs| panic!("manifest load errors: {errs:?}"));

    let result = dovetail::build_workspace(
        &workspace,
        Some(project),
        dovetail::BuildMode::Build,
        &std::collections::HashMap::new(),
        false,
        None,
    );

    if result.diagnostics.has_errors() {
        let errors: Vec<String> = result
            .diagnostics
            .iter()
            .map(|d| format!("{}: {}", d.span.file, d.message))
            .collect();
        panic!("build_workspace failed:\n{}", errors.join("\n"));
    }

    let (_, project_result) = result
        .project_results
        .into_iter()
        .find(|(name, _)| name == project)
        .unwrap_or_else(|| panic!("project '{project}' not in build results"));

    project_result
        .wasm
        .unwrap_or_else(|| panic!("project '{project}' has no WASM output"))
}
