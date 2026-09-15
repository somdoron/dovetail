//! Local source discovery deliberately does not resolve dependency graphs.
use super::toml_schema::RawManifest;
use std::path::{Path, PathBuf};

pub fn formatting_directories(root: &Path) -> Result<Vec<PathBuf>, String> {
    let path = root.join("Dovetail.toml");
    let source =
        std::fs::read_to_string(&path).map_err(|error| format!("{}: {error}", path.display()))?;
    let manifest: RawManifest =
        toml::from_str(&source).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut directories = Vec::new();
    for project in manifest.project {
        let directory = root.join(project.path.as_deref().unwrap_or(&project.name));
        for package in project.packages {
            let root_package =
                crate::common::types::PackagePath::from_dotted(&project.root_package);
            directories.push(
                super::validate::resolve_package_entry(&package, &root_package, &directory)?
                    .source_dir,
            );
        }
        let tests = directory.join("test");
        if tests.is_dir() {
            collect_test_directories(&tests, &mut directories)?;
        }
    }
    Ok(directories)
}

fn collect_test_directories(directory: &Path, output: &mut Vec<PathBuf>) -> Result<(), String> {
    output.push(directory.to_path_buf());
    for entry in std::fs::read_dir(directory).map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        if entry
            .file_type()
            .map_err(|error| error.to_string())?
            .is_dir()
        {
            collect_test_directories(&entry.path(), output)?;
        }
    }
    Ok(())
}
