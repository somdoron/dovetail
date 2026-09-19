mod formatting;
pub use formatting::formatting_directories;
mod dependencies;
mod git;
mod resolve;
pub use dependencies::validate_closure as validate_dependency_closure;
pub use dependencies::{ResolveOptions, UpdateRequest, load_manifest_with_options};
mod toml_schema;
mod validate;

use std::fmt;
use std::path::{Path, PathBuf};

use crate::common::types::{Fqn, PackagePath};

/// A fully resolved workspace, ready for compilation orchestration.
#[derive(Debug)]
pub struct ResolvedWorkspace {
    /// Manifest-local names for graph edges, retained for dependency-path diagnostics.
    pub dependency_aliases: std::collections::BTreeMap<(String, String), String>,
    /// Projects in topological order (dependencies before dependents).
    pub projects: Vec<ResolvedProject>,
    /// The workspace root directory (where Dovetail.toml lives).
    pub workspace_root: PathBuf,
}

/// A newtype for project names (keeps them distinct from package names).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ProjectName(pub String);

impl fmt::Display for ProjectName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// A fully resolved project.
#[derive(Debug)]
pub struct ResolvedProject {
    /// Stable repository/commit/manifest/project identity; absent for local projects.
    pub resolved_identity: Option<String>,
    /// The project name (matches directory name).
    pub name: ProjectName,
    /// The root package FQN prefix.
    pub root_package: PackagePath,
    /// Names of projects this project depends on.
    pub depends: Vec<ProjectName>,
    /// Packages in compilation order, each with its resolved PackagePath
    /// and validated source directory.
    pub packages: Vec<ResolvedPackage>,
    /// The project directory: `<workspace_root>/<name>/`.
    pub project_dir: PathBuf,
    /// Explicit main function FQN from Dovetail.toml, or None for auto-detection.
    pub main_function: Option<Fqn>,
    /// Binary resources declared in `Dovetail.toml`. Each entry is the path
    /// exactly as declared (e.g. `"tzdata/tzdata.bin"`) — that string is
    /// the key user code passes to `Resource.bytes(...)`. The absolute
    /// path is derived by joining with `project_dir`.
    pub resources: Vec<String>,
    /// Derive macros declared by this project via `[[project.macro]]`
    /// entries. Each macro's script source is loaded from disk at the
    /// pipeline's macro-phase boundary, not at manifest-resolve time —
    /// the script path is validated here, the contents are read later.
    pub macros: Vec<ResolvedMacro>,
    /// WASM component dependencies declared via `[[project.component]]`.
    /// The component bytes are loaded from disk at build time;
    /// bindings are injected as a generated package
    /// and the component is composed into the final output.
    pub components: Vec<ResolvedComponent>,
}

/// A resolved `[[project.component]]` entry.
#[derive(Debug, Clone)]
pub struct ResolvedComponent {
    /// Identity of the declaring project, retained during transitive propagation.
    pub provider: Option<String>,
    /// Where the component binary comes from.
    pub source: ComponentSource,
    /// Dovetail package path for the generated bindings (e.g. `sqlite.raw`).
    pub dovetail_package: PackagePath,
    /// Exported interface to import (`None` = the component's sole export).
    pub interface: Option<String>,
    /// File stem used for diagnostics.
    pub display_name: String,
}

/// The source of a component dependency's binary.
#[derive(Debug, Clone)]
pub enum ComponentSource {
    /// A component file on disk (absolute path, validated at resolve time).
    Path(PathBuf),
}

/// A resolved `[[project.macro]]` entry.
#[derive(Debug, Clone)]
pub struct ResolvedMacro {
    /// The macro's fully-qualified name, e.g. `"json.JsonCodec"`. Built as
    /// `<package>.<name>` where `<package>` is the resolved PackagePath.
    pub fqn: String,
    /// Always `MacroKind::Derive` for now; reserved for future kinds.
    pub kind: MacroKind,
    /// Absolute path to the macro's script source on disk.
    pub script_path: PathBuf,
    /// Optional trait FQN the derive targets, kept for diagnostics.
    pub trait_fqn: Option<String>,
}

/// The kind of a macro declared in `Dovetail.toml`. The macro phase only
/// supports `Derive` today.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MacroKind {
    Derive,
}

/// A resolved package within a project.
#[derive(Debug)]
pub struct ResolvedPackage {
    /// The canonical FQN of this package.
    pub path: PackagePath,
    /// The source directory for this package.
    pub source_dir: PathBuf,
}

/// Errors that can occur when loading and validating Dovetail.toml.
#[derive(Debug)]
pub enum ManifestError {
    /// Could not read the Dovetail.toml file.
    IoError {
        path: PathBuf,
        source: std::io::Error,
    },
    /// TOML parsing/deserialization failed.
    TomlError { message: String },
    /// Duplicate project name in manifest.
    DuplicateProject { name: String },
    /// A project depends on an unknown project.
    UnknownDependency { project: String, dependency: String },
    /// Circular dependency detected among projects.
    CyclicDependency { cycle: Vec<String> },
    /// Project directory does not exist.
    ProjectDirNotFound { project: String, expected: PathBuf },
    /// Project's `src/` directory does not exist.
    SrcDirNotFound { project: String, expected: PathBuf },
    /// A package entry in the manifest could not be resolved.
    InvalidPackageEntry {
        project: String,
        entry: String,
        reason: String,
    },
    /// Package directory does not exist.
    PackageDirNotFound {
        project: String,
        package_path: String,
        expected: PathBuf,
    },
    /// Duplicate package path within a project.
    DuplicatePackage {
        project: String,
        package_path: String,
    },
    /// Invalid `main` function value in manifest.
    InvalidMainFunction {
        project: String,
        value: String,
        reason: String,
    },
    /// Dovetail.toml already exists (for `init`).
    ManifestAlreadyExists { path: PathBuf },
    /// Project directory already exists on disk.
    ProjectDirAlreadyExists { project: String, path: PathBuf },
    /// A declared resource file does not exist on disk.
    ResourceNotFound {
        project: String,
        resource: String,
        expected: PathBuf,
    },
    /// A declared resource path escapes the project directory.
    ResourcePathEscape { project: String, resource: String },
    /// A `[[project.macro]]` entry has an unsupported `kind` value.
    UnsupportedMacroKind {
        project: String,
        macro_name: String,
        kind: String,
    },
    /// A `[[project.macro]]` entry's `package` doesn't match any of the
    /// project's declared packages.
    MacroPackageMismatch {
        project: String,
        macro_name: String,
        package: String,
    },
    /// A `[[project.macro]]` entry's `script` file is missing on disk.
    MacroScriptNotFound {
        project: String,
        macro_name: String,
        expected: PathBuf,
    },
    /// A `[[project.macro]]` entry's `script` path escapes the project
    /// directory (contains `..` segments).
    MacroScriptPathEscape {
        project: String,
        macro_name: String,
        script: String,
    },
    /// Two `[[project.macro]]` entries within the same project share an FQN.
    DuplicateMacro { project: String, fqn: String },
    /// A `[[project.component]]` entry is invalid.
    InvalidComponent { project: String, reason: String },
}

impl fmt::Display for ManifestError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IoError { path, source } => {
                write!(f, "cannot read `{}`: {}", path.display(), source)
            }
            Self::TomlError { message } => {
                write!(f, "invalid Dovetail.toml: {message}")
            }
            Self::DuplicateProject { name } => {
                write!(f, "duplicate project name `{name}`")
            }
            Self::UnknownDependency {
                project,
                dependency,
            } => {
                write!(
                    f,
                    "project `{project}` depends on unknown project `{dependency}`"
                )
            }
            Self::CyclicDependency { cycle } => {
                write!(
                    f,
                    "circular dependency among projects: {}",
                    cycle.join(" -> ")
                )
            }
            Self::ProjectDirNotFound { project, expected } => {
                write!(
                    f,
                    "project `{project}` directory not found: {}",
                    expected.display()
                )
            }
            Self::SrcDirNotFound { project, expected } => {
                write!(
                    f,
                    "project `{project}` missing `src/` directory: {}",
                    expected.display()
                )
            }
            Self::InvalidPackageEntry {
                project,
                entry,
                reason,
            } => {
                write!(
                    f,
                    "project `{project}`: invalid package entry `{entry}`: {reason}"
                )
            }
            Self::PackageDirNotFound {
                project,
                package_path,
                expected,
            } => {
                write!(
                    f,
                    "project `{project}`: package `{package_path}` directory not found: {}",
                    expected.display()
                )
            }
            Self::DuplicatePackage {
                project,
                package_path,
            } => {
                write!(f, "project `{project}`: duplicate package `{package_path}`")
            }
            Self::InvalidMainFunction {
                project,
                value,
                reason,
            } => {
                write!(
                    f,
                    "project `{project}`: invalid main function `{value}`: {reason}"
                )
            }
            Self::ManifestAlreadyExists { path } => {
                write!(f, "Dovetail.toml already exists: {}", path.display())
            }
            Self::ProjectDirAlreadyExists { project, path } => {
                write!(
                    f,
                    "project `{project}` directory already exists: {}",
                    path.display()
                )
            }
            Self::ResourceNotFound {
                project,
                resource,
                expected,
            } => {
                write!(
                    f,
                    "project `{project}`: resource `{resource}` not found: {}",
                    expected.display()
                )
            }
            Self::ResourcePathEscape { project, resource } => {
                write!(
                    f,
                    "project `{project}`: resource `{resource}` path escapes project directory (`..` not allowed)"
                )
            }
            Self::UnsupportedMacroKind {
                project,
                macro_name,
                kind,
            } => {
                write!(
                    f,
                    "project `{project}`: macro `{macro_name}` has unsupported kind `{kind}` (only \"derive\" is supported)"
                )
            }
            Self::MacroPackageMismatch {
                project,
                macro_name,
                package,
            } => {
                write!(
                    f,
                    "project `{project}`: macro `{macro_name}` declares package `{package}`, but no such package is declared in this project"
                )
            }
            Self::MacroScriptNotFound {
                project,
                macro_name,
                expected,
            } => {
                write!(
                    f,
                    "project `{project}`: macro `{macro_name}` script not found: {}",
                    expected.display()
                )
            }
            Self::MacroScriptPathEscape {
                project,
                macro_name,
                script,
            } => {
                write!(
                    f,
                    "project `{project}`: macro `{macro_name}` script `{script}` escapes project directory (`..` not allowed)"
                )
            }
            Self::DuplicateMacro { project, fqn } => {
                write!(f, "project `{project}`: duplicate macro `{fqn}`")
            }
            Self::InvalidComponent { project, reason } => {
                write!(
                    f,
                    "project '{project}': invalid [[project.component]] entry: {reason}"
                )
            }
        }
    }
}

/// Load, parse, and validate `Dovetail.toml` from the given workspace root.
///
/// Returns a `ResolvedWorkspace` with projects in topological order and all
/// packages resolved to canonical `PackagePath` values, or a list of errors.
pub fn load_manifest(workspace_root: &Path) -> Result<ResolvedWorkspace, Vec<ManifestError>> {
    load_manifest_with_options(workspace_root, &ResolveOptions::default())
}

/// Reject legacy workspace files before creating files or resolving dependencies.
fn validate_workspace_filenames(root: &Path) -> Result<(), ManifestError> {
    for (old, new) in [
        ("Domain.toml", "Dovetail.toml"),
        ("Domain.lock", "Dovetail.lock"),
    ] {
        if !root.join(old).exists() {
            continue;
        }
        let message = if root.join(new).exists() {
            format!(
                "both {old} and {new} exist in {}; keep only {new} after migrating",
                root.display()
            )
        } else {
            format!(
                "legacy {old} found in {}; rename it to {new}, preserving its contents, and rename .domain source files to .dove",
                root.display()
            )
        };
        return Err(dependencies::error(message));
    }
    Ok(())
}

/// Initialize a new workspace with a single project.
///
/// Creates `Dovetail.toml`, `<project_name>/src/`, and a placeholder `main.dove`.
/// Fails if `Dovetail.toml` or the project directory already exists.
pub fn init_workspace(workspace_root: &Path, project_name: &str) -> Result<(), Vec<ManifestError>> {
    validate_workspace_filenames(workspace_root).map_err(|error| vec![error])?;
    let manifest_path = workspace_root.join("Dovetail.toml");
    if manifest_path.exists() {
        return Err(vec![ManifestError::ManifestAlreadyExists {
            path: manifest_path,
        }]);
    }

    let project_dir = workspace_root.join(project_name);
    if project_dir.exists() {
        return Err(vec![ManifestError::ProjectDirAlreadyExists {
            project: project_name.to_string(),
            path: project_dir,
        }]);
    }

    // Create directory structure.
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| {
        vec![ManifestError::IoError {
            path: src_dir.clone(),
            source: e,
        }]
    })?;

    // Write placeholder source file.
    let main_file = src_dir.join("main.dove");
    let source = format!("package {project_name}\n\nfunction main(): Unit = ()\n");
    std::fs::write(&main_file, source).map_err(|e| {
        vec![ManifestError::IoError {
            path: main_file,
            source: e,
        }]
    })?;

    // Build and write Dovetail.toml.
    let manifest = toml_schema::RawManifest {
        compiler_version: env!("CARGO_PKG_VERSION").to_string(),
        standard_tag: None,
        dependencies: vec![],
        project: vec![toml_schema::RawProject {
            image: None,
            name: project_name.to_string(),
            path: None,
            root_package: project_name.to_string(),
            depends: vec![],
            packages: vec![".".to_string()],
            main: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
        }],
    };
    let ignore_path = workspace_root.join(".gitignore");
    let mut ignore = std::fs::read_to_string(&ignore_path).unwrap_or_default();
    if !ignore
        .lines()
        .any(|line| line == ".dovetail/" || line == "/.dovetail/")
    {
        if !ignore.is_empty() && !ignore.ends_with('\n') {
            ignore.push('\n');
        }
        ignore.push_str(".dovetail/\n");
        std::fs::write(&ignore_path, ignore).map_err(|source| {
            vec![ManifestError::IoError {
                path: ignore_path,
                source,
            }]
        })?;
    }
    let content = toml_schema::serialize_manifest(&manifest).map_err(|e| vec![e])?;
    std::fs::write(&manifest_path, content).map_err(|e| {
        vec![ManifestError::IoError {
            path: manifest_path,
            source: e,
        }]
    })?;

    Ok(())
}

/// Add a new project to an existing workspace.
///
/// Reads `Dovetail.toml`, appends a new project entry, creates the project
/// directory with `src/` and a placeholder `main.dove`.
/// Fails if `Dovetail.toml` is missing, the project name is a duplicate,
/// or the project directory already exists.
pub fn add_project(workspace_root: &Path, project_name: &str) -> Result<(), Vec<ManifestError>> {
    validate_workspace_filenames(workspace_root).map_err(|error| vec![error])?;
    let manifest_path = workspace_root.join("Dovetail.toml");

    // Read and parse existing manifest.
    let content = std::fs::read_to_string(&manifest_path).map_err(|e| {
        vec![ManifestError::IoError {
            path: manifest_path.clone(),
            source: e,
        }]
    })?;
    let mut raw = toml_schema::parse_manifest(&content).map_err(|e| vec![e])?;
    dependencies::check_version(&raw, &manifest_path).map_err(|e| vec![e])?;

    // Check local names and explicit imported aliases before creating files.
    if raw.project.iter().any(|p| p.name == project_name)
        || raw
            .dependencies
            .iter()
            .flat_map(|d| &d.projects)
            .any(|p| p.names().1 == project_name)
    {
        return Err(vec![ManifestError::DuplicateProject {
            name: project_name.to_string(),
        }]);
    }

    // Check project directory doesn't already exist.
    let project_dir = workspace_root.join(project_name);
    if project_dir.exists() {
        return Err(vec![ManifestError::ProjectDirAlreadyExists {
            project: project_name.to_string(),
            path: project_dir,
        }]);
    }

    // Create directory structure.
    let src_dir = project_dir.join("src");
    std::fs::create_dir_all(&src_dir).map_err(|e| {
        vec![ManifestError::IoError {
            path: src_dir.clone(),
            source: e,
        }]
    })?;

    // Write placeholder source file.
    let main_file = src_dir.join("main.dove");
    let source = format!("package {project_name}\n\nfunction main(): Unit = ()\n");
    std::fs::write(&main_file, source).map_err(|e| {
        vec![ManifestError::IoError {
            path: main_file,
            source: e,
        }]
    })?;

    // Append new project and write back.
    raw.project.push(toml_schema::RawProject {
        image: None,
        name: project_name.to_string(),
        path: None,
        root_package: project_name.to_string(),
        depends: vec![],
        packages: vec![".".to_string()],
        main: None,
        resources: vec![],
        macros: vec![],
        components: vec![],
    });
    let new_content = toml_schema::serialize_manifest(&raw).map_err(|e| vec![e])?;
    std::fs::write(&manifest_path, new_content).map_err(|e| {
        vec![ManifestError::IoError {
            path: manifest_path,
            source: e,
        }]
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_manifest_end_to_end() {
        let tmp = tempfile::tempdir().unwrap();

        // Create directory structure.
        std::fs::create_dir_all(tmp.path().join("libraries/mylib/src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("myapp/src/utils")).unwrap();

        // Write Dovetail.toml.
        let manifest = r#"compiler-version = "0.1.0"
[[project]]
name = "mylib"
path = "libraries/mylib"
root_package = "com.example.mylib"
packages = ["."]

[[project]]
name = "myapp"
root_package = "com.example.myapp"
depends = ["mylib"]
packages = ["utils", "."]
"#;
        std::fs::write(tmp.path().join("Dovetail.toml"), manifest).unwrap();

        let workspace = load_manifest(tmp.path()).unwrap();

        // Projects should be in topological order: mylib before myapp.
        assert_eq!(workspace.projects.len(), 2);
        assert_eq!(workspace.projects[0].name.0, "mylib");
        assert_eq!(workspace.projects[1].name.0, "myapp");

        let library_dir = tmp.path().join("libraries/mylib").canonicalize().unwrap();
        assert_eq!(workspace.projects[0].project_dir, library_dir);
        assert_eq!(
            workspace.projects[0].packages[0].source_dir,
            library_dir.join("src")
        );

        // mylib has one package (root).
        assert_eq!(workspace.projects[0].packages.len(), 1);
        assert_eq!(
            workspace.projects[0].packages[0].path,
            PackagePath::from_dotted("com.example.mylib")
        );

        // myapp has two packages in order: utils, root.
        assert_eq!(workspace.projects[1].packages.len(), 2);
        assert_eq!(
            workspace.projects[1].packages[0].path,
            PackagePath::from_dotted("com.example.myapp.utils")
        );
        assert_eq!(
            workspace.projects[1].packages[1].path,
            PackagePath::from_dotted("com.example.myapp")
        );

        // myapp depends on mylib.
        assert_eq!(workspace.projects[1].depends.len(), 1);
        assert_eq!(
            workspace.projects[1].depends[0].0,
            workspace.projects[0].identity()
        );
    }

    #[test]
    fn load_manifest_cycle_error() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::create_dir_all(tmp.path().join("a/src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("b/src")).unwrap();

        let manifest = r#"compiler-version = "0.1.0"
[[project]]
name = "a"
root_package = "com.a"
depends = ["b"]
packages = ["."]

[[project]]
name = "b"
root_package = "com.b"
depends = ["a"]
packages = ["."]
"#;
        std::fs::write(tmp.path().join("Dovetail.toml"), manifest).unwrap();

        let errors = load_manifest(tmp.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ManifestError::CyclicDependency { .. }));
    }

    #[test]
    fn load_manifest_missing_file() {
        let tmp = tempfile::tempdir().unwrap();
        let errors = load_manifest(tmp.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ManifestError::IoError { .. }));
    }

    #[test]
    fn load_manifest_fqn_package_entry() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::create_dir_all(tmp.path().join("myapp/src/utils")).unwrap();

        let manifest = r#"compiler-version = "0.1.0"
[[project]]
name = "myapp"
root_package = "com.example.myapp"
packages = ["com.example.myapp.utils", "."]
"#;
        std::fs::write(tmp.path().join("Dovetail.toml"), manifest).unwrap();

        let workspace = load_manifest(tmp.path()).unwrap();
        assert_eq!(workspace.projects[0].packages.len(), 2);
        assert_eq!(
            workspace.projects[0].packages[0].path,
            PackagePath::from_dotted("com.example.myapp.utils")
        );
    }

    // ── init_workspace tests ─────────────────────────────────────

    #[test]
    fn test_init_workspace() {
        let tmp = tempfile::tempdir().unwrap();
        init_workspace(tmp.path(), "myapp").unwrap();

        // Dovetail.toml exists and is valid TOML.
        let content = std::fs::read_to_string(tmp.path().join("Dovetail.toml")).unwrap();
        assert!(content.contains("name = \"myapp\""));
        assert!(content.contains("root_package = \"myapp\""));
        assert!(content.contains("packages = [\".\"]"));
        // depends should be omitted (skip_serializing_if).
        assert!(!content.contains("depends"));

        // Directory structure exists.
        assert!(tmp.path().join("myapp/src").is_dir());

        // Placeholder source file exists with correct content.
        let source = std::fs::read_to_string(tmp.path().join("myapp/src/main.dove")).unwrap();
        assert!(source.contains("package myapp"));
        assert!(source.contains("function main(): Unit = ()"));
    }

    #[test]
    fn test_init_workspace_already_exists() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("Dovetail.toml"), "").unwrap();

        let errors = init_workspace(tmp.path(), "myapp").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0],
            ManifestError::ManifestAlreadyExists { .. }
        ));
    }

    #[test]
    fn test_init_workspace_project_dir_exists() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir(tmp.path().join("myapp")).unwrap();

        let errors = init_workspace(tmp.path(), "myapp").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(
            &errors[0],
            ManifestError::ProjectDirAlreadyExists { .. }
        ));
    }

    // ── add_project tests ────────────────────────────────────────

    #[test]
    fn test_add_project() {
        let tmp = tempfile::tempdir().unwrap();
        init_workspace(tmp.path(), "mylib").unwrap();
        add_project(tmp.path(), "myapp").unwrap();

        // Dovetail.toml has both projects.
        let content = std::fs::read_to_string(tmp.path().join("Dovetail.toml")).unwrap();
        assert!(content.contains("name = \"mylib\""));
        assert!(content.contains("name = \"myapp\""));

        // New project directory exists.
        assert!(tmp.path().join("myapp/src").is_dir());
        assert!(tmp.path().join("myapp/src/main.dove").is_file());
    }

    #[test]
    fn test_add_project_no_manifest() {
        let tmp = tempfile::tempdir().unwrap();
        let errors = add_project(tmp.path(), "myapp").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ManifestError::IoError { .. }));
    }

    #[test]
    fn test_add_project_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        init_workspace(tmp.path(), "myapp").unwrap();

        let errors = add_project(tmp.path(), "myapp").unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(matches!(&errors[0], ManifestError::DuplicateProject { .. }));
    }

    #[test]
    fn test_init_then_load() {
        let tmp = tempfile::tempdir().unwrap();
        init_workspace(tmp.path(), "myapp").unwrap();

        let workspace = load_manifest(tmp.path()).unwrap();
        assert_eq!(workspace.projects.len(), 1);
        assert_eq!(workspace.projects[0].name.0, "myapp");
        assert_eq!(
            workspace.projects[0].root_package,
            PackagePath::from_dotted("myapp")
        );
    }

    #[test]
    fn test_add_then_load() {
        let tmp = tempfile::tempdir().unwrap();
        init_workspace(tmp.path(), "mylib").unwrap();
        add_project(tmp.path(), "myapp").unwrap();

        let workspace = load_manifest(tmp.path()).unwrap();
        assert_eq!(workspace.projects.len(), 2);
        // Both projects present (order is topological — both independent, so manifest order).
        let names: Vec<&str> = workspace
            .projects
            .iter()
            .map(|p| p.name.0.as_str())
            .collect();
        assert!(names.contains(&"mylib"));
        assert!(names.contains(&"myapp"));
    }

    #[cfg(unix)]
    #[test]
    fn local_project_symlink_retains_workspace_ownership() {
        let temporary = tempfile::tempdir().unwrap();
        init_workspace(temporary.path(), "app").unwrap();
        std::fs::create_dir(temporary.path().join("sources")).unwrap();
        std::fs::rename(
            temporary.path().join("app"),
            temporary.path().join("sources/app"),
        )
        .unwrap();
        std::os::unix::fs::symlink("sources/app", temporary.path().join("app")).unwrap();

        let workspace = load_manifest(temporary.path()).unwrap();
        let app = workspace
            .project("app")
            .expect("root project remains selectable");
        assert!(workspace.is_local(app));
        assert_eq!(workspace.result_key(app), "app");
    }
}

impl ResolvedProject {
    pub(super) fn local_identity(directory: &Path, name: &str) -> String {
        format!("{}#{name}", directory.display())
    }

    pub(crate) fn generated_sources_dir(&self, root: &Path) -> PathBuf {
        root.join(".dovetail/generated").join(git::digest(&format!(
            "{}:{}",
            env!("CARGO_PKG_VERSION"),
            self.identity()
        )))
    }

    /// Resolver identity, independent of the source package FQNs and aliases.
    pub fn identity(&self) -> String {
        self.resolved_identity
            .clone()
            .unwrap_or_else(|| Self::local_identity(&self.project_dir, &self.name.0))
    }
}

impl ResolvedWorkspace {
    pub fn is_local(&self, project: &ResolvedProject) -> bool {
        project.resolved_identity.is_none()
    }

    pub fn project(&self, key: &str) -> Option<&ResolvedProject> {
        self.projects
            .iter()
            .find(|p| p.identity() == key)
            .or_else(|| {
                self.projects
                    .iter()
                    .find(|p| self.is_local(p) && p.name.0 == key)
            })
    }

    pub fn result_key(&self, project: &ResolvedProject) -> String {
        if self.is_local(project) {
            project.name.0.clone()
        } else {
            project.identity()
        }
    }
}

#[cfg(test)]
mod rename_tests {
    use super::*;

    #[test]
    fn legacy_manifest_blocks_loading_and_creation_without_mutation() {
        let root = tempfile::tempdir().unwrap();
        let old = root.path().join("Domain.toml");
        std::fs::write(&old, "legacy manifest").unwrap();
        for errors in [
            load_manifest(root.path()).unwrap_err(),
            init_workspace(root.path(), "app").unwrap_err(),
            add_project(root.path(), "app").unwrap_err(),
        ] {
            assert!(errors[0].to_string().contains("rename it to Dovetail.toml"));
        }
        assert_eq!(std::fs::read_to_string(old).unwrap(), "legacy manifest");
        assert!(!root.path().join("Dovetail.toml").exists());
        assert!(!root.path().join("app").exists());
        assert!(!root.path().join(".dovetail").exists());
    }

    #[test]
    fn duplicate_manifests_and_legacy_lockfiles_are_not_silently_selected() {
        let root = tempfile::tempdir().unwrap();
        init_workspace(root.path(), "app").unwrap();
        let old_manifest = root.path().join("Domain.toml");
        std::fs::write(&old_manifest, "legacy manifest").unwrap();
        let errors = load_manifest(root.path()).unwrap_err();
        assert!(
            errors[0]
                .to_string()
                .contains("both Domain.toml and Dovetail.toml")
        );
        std::fs::remove_file(old_manifest).unwrap();

        let old_lock = root.path().join("Domain.lock");
        std::fs::write(&old_lock, "pinned revisions").unwrap();
        let errors = load_manifest(root.path()).unwrap_err();
        assert!(errors[0].to_string().contains("rename it to Dovetail.lock"));
        assert_eq!(
            std::fs::read_to_string(old_lock).unwrap(),
            "pinned revisions"
        );
        assert!(!root.path().join("Dovetail.lock").exists());
        assert!(!root.path().join(".dovetail").exists());
    }
}

/// Read image settings without fetching dependencies or requiring source files.
pub fn image_projects(root: &Path) -> anyhow::Result<Vec<crate::image::config::ImageProject>> {
    validate_workspace_filenames(root).map_err(|e| anyhow::anyhow!("{e}"))?;
    let content = std::fs::read_to_string(root.join("Dovetail.toml"))?;
    let manifest = toml_schema::parse_manifest(&content).map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(manifest
        .project
        .into_iter()
        .filter_map(|project| {
            project
                .image
                .map(|config| crate::image::config::ImageProject {
                    directory: root.join(project.path.as_deref().unwrap_or(&project.name)),
                    name: project.name,
                    config,
                })
        })
        .collect())
}
