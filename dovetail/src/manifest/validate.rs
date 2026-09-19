use std::collections::BTreeSet;
use std::path::Path;

use crate::common::types::{Fqn, PackagePath};

use super::toml_schema::{RawMacro, RawProject};
use super::{
    MacroKind, ManifestError, ProjectName, ResolvedComponent, ResolvedMacro, ResolvedPackage,
    ResolvedProject,
};

/// Resolve a single `packages` entry to a `ResolvedPackage`.
///
/// Rules:
/// - `"."` → root package at `src/`, FQN = `root_package`
/// - `"utils"` → folder-relative, FQN = `root_package.utils`, dir = `src/utils/`
/// - `"com.example.myapp.utils"` → FQN form (must start with `root_package.`),
///   dir derived from the suffix after root_package
/// - Dotted entries not starting with root_package → folder-relative
///   (e.g. `"utils.helpers"` → `src/utils/helpers/`)
pub(super) fn resolve_package_entry(
    entry: &str,
    root_package: &PackagePath,
    project_dir: &Path,
) -> Result<ResolvedPackage, String> {
    if entry == "." {
        return Ok(ResolvedPackage {
            path: root_package.clone(),
            source_dir: project_dir.join("src"),
        });
    }

    let root_str = root_package.to_string();

    // Check if entry is an FQN (starts with root_package followed by a dot).
    if let Some(suffix) = entry
        .strip_prefix(&root_str)
        .and_then(|rest| rest.strip_prefix('.'))
    {
        if suffix.is_empty() {
            return Err("FQN entry equals root_package; use \".\" instead".to_string());
        }
        let segments: Vec<&str> = suffix.split('.').collect();
        let relative_path: std::path::PathBuf = segments.iter().collect();
        let source_dir = project_dir.join("src").join(&relative_path);

        let mut full_segments = root_package.0.clone();
        full_segments.extend(segments.iter().map(|s| s.to_string()));

        return Ok(ResolvedPackage {
            path: PackagePath(full_segments),
            source_dir,
        });
    }

    // Check if entry exactly equals root_package (no trailing dot).
    if entry == root_str {
        return Err("FQN entry equals root_package; use \".\" instead".to_string());
    }

    // Folder-relative: split on dots, build path and PackagePath.
    let segments: Vec<&str> = entry.split('.').collect();
    let relative_path: std::path::PathBuf = segments.iter().collect();
    let source_dir = project_dir.join("src").join(&relative_path);

    let mut full_segments = root_package.0.clone();
    full_segments.extend(segments.iter().map(|s| s.to_string()));

    Ok(ResolvedPackage {
        path: PackagePath(full_segments),
        source_dir,
    })
}

/// Validate workspace directories and resolve all packages for each project.
///
/// `order` is the topological ordering (indices into `projects`).
pub(super) fn validate_workspace(
    projects: &[RawProject],
    order: &[usize],
    workspace_root: &Path,
) -> Result<Vec<ResolvedProject>, Vec<ManifestError>> {
    let mut errors = Vec::new();
    let mut resolved = Vec::with_capacity(order.len());

    for &idx in order {
        let raw = &projects[idx];
        let project_dir = workspace_root.join(raw.path.as_deref().unwrap_or(&raw.name));
        let src_dir = project_dir.join("src");

        // Validate project directory exists.
        if !project_dir.is_dir() {
            errors.push(ManifestError::ProjectDirNotFound {
                project: raw.name.clone(),
                expected: project_dir,
            });
            continue;
        }
        if !src_dir.is_dir() {
            errors.push(ManifestError::SrcDirNotFound {
                project: raw.name.clone(),
                expected: src_dir,
            });
            continue;
        }

        let root_package = PackagePath::from_dotted(&raw.root_package);

        // Resolve and validate each package entry.
        let mut packages = Vec::new();
        let mut seen_paths: BTreeSet<PackagePath> = BTreeSet::new();

        for entry in &raw.packages {
            match resolve_package_entry(entry, &root_package, &project_dir) {
                Ok(pkg) => {
                    if !seen_paths.insert(pkg.path.clone()) {
                        errors.push(ManifestError::DuplicatePackage {
                            project: raw.name.clone(),
                            package_path: pkg.path.to_string(),
                        });
                        continue;
                    }
                    // Root package source_dir is src/ which we already validated.
                    if entry != "." && !pkg.source_dir.is_dir() {
                        errors.push(ManifestError::PackageDirNotFound {
                            project: raw.name.clone(),
                            package_path: pkg.path.to_string(),
                            expected: pkg.source_dir,
                        });
                        continue;
                    }
                    packages.push(pkg);
                }
                Err(reason) => {
                    errors.push(ManifestError::InvalidPackageEntry {
                        project: raw.name.clone(),
                        entry: entry.clone(),
                        reason,
                    });
                }
            }
        }

        // Resolve optional main function.
        let main_function = match &raw.main {
            Some(s) => match Fqn::from_dotted(s) {
                Some(fqn) => {
                    if fqn.package.starts_with(&root_package) {
                        Some(fqn)
                    } else {
                        errors.push(ManifestError::InvalidMainFunction {
                            project: raw.name.clone(),
                            value: s.clone(),
                            reason: format!(
                                "must be within the project's root package '{}' subtree",
                                root_package
                            ),
                        });
                        None
                    }
                }
                None => {
                    errors.push(ManifestError::InvalidMainFunction {
                        project: raw.name.clone(),
                        value: s.clone(),
                        reason: "must be a fully-qualified name with at least a package and function (e.g. 'a.main')".to_string(),
                    });
                    None
                }
            },
            None => None,
        };

        // Validate resource paths exist and stay within the project directory.
        let mut resources = Vec::with_capacity(raw.resources.len());
        for entry in &raw.resources {
            // Reject any `..` segment to keep resource paths sandboxed inside
            // the project directory.
            let has_parent_segment = std::path::Path::new(entry)
                .components()
                .any(|c| matches!(c, std::path::Component::ParentDir));
            if has_parent_segment {
                errors.push(ManifestError::ResourcePathEscape {
                    project: raw.name.clone(),
                    resource: entry.clone(),
                });
                continue;
            }
            let abs = project_dir.join(entry);
            if !abs.is_file() {
                errors.push(ManifestError::ResourceNotFound {
                    project: raw.name.clone(),
                    resource: entry.clone(),
                    expected: abs,
                });
                continue;
            }
            resources.push(entry.clone());
        }

        // Resolve `[[project.macro]]` entries. Macros are validated against
        // the package list we just computed (they must live inside one of
        // the project's declared packages) and against the file system
        // (the script file must exist within the project directory).
        let macros = match resolve_macros(raw, &packages, &project_dir) {
            Ok(ms) => ms,
            Err(macro_errors) => {
                errors.extend(macro_errors);
                vec![]
            }
        };

        // Resolve `[[project.component]]` entries.
        let components = match resolve_components(raw, &project_dir) {
            Ok(cs) => cs,
            Err(component_errors) => {
                errors.extend(component_errors);
                vec![]
            }
        };

        resolved.push(ResolvedProject {
            resolved_identity: None,
            name: ProjectName(raw.name.clone()),
            root_package,
            depends: raw.depends.iter().map(|d| ProjectName(d.clone())).collect(),
            packages,
            project_dir,
            main_function,
            resources,
            macros,
            components,
        });
    }

    if errors.is_empty() {
        propagate_transitive_components(&mut resolved);
        Ok(resolved)
    } else {
        Err(errors)
    }
}

/// Propagate component dependencies transitively: a project inherits the
/// components declared by every project it (transitively) depends on, so a
/// downstream app gets a dependency's component (its generated bindings +
/// composed binary) without re-declaring `[[project.component]]` itself.
///
/// `resolved` is in topological order (dependencies before dependents), so a
/// single forward pass suffices: by the time a project is visited, each of its
/// dependencies already carries its own fully-aggregated component set.
/// Components are deduped by bindings package — a diamond dependency must not
/// compose the same component twice.
pub(super) fn propagate_transitive_components(resolved: &mut [ResolvedProject]) {
    use std::collections::HashMap;
    let index_of: HashMap<ProjectName, usize> = resolved
        .iter()
        .enumerate()
        .flat_map(|(i, p)| [(p.name.clone(), i), (ProjectName(p.identity()), i)])
        .collect();
    for i in 0..resolved.len() {
        // A valid workspace is a DAG in topological order, so every dependency
        // index is < i and already aggregated. Collect first (immutable
        // borrows), then extend (mutable borrow) to satisfy the borrow checker.
        let dep_indices: Vec<usize> = resolved[i]
            .depends
            .iter()
            .filter_map(|d| index_of.get(d).copied())
            .collect();
        let mut seen: BTreeSet<PackagePath> = resolved[i]
            .components
            .iter()
            .map(|c| c.dovetail_package.clone())
            .collect();
        let mut inherited: Vec<ResolvedComponent> = Vec::new();
        for di in dep_indices {
            for component in &resolved[di].components {
                if seen.insert(component.dovetail_package.clone()) {
                    inherited.push(component.clone());
                }
            }
        }
        resolved[i].components.extend(inherited);
    }
}

/// Resolve `[[project.macro]]` entries for a single project.
///
/// Each entry is validated for: a supported `kind`, a `package` value that
/// matches one of the project's declared packages (or its `root_package`),
/// a `script` path inside the project directory pointing at an existing
/// file, and uniqueness of the resulting FQN within the project.
fn resolve_macros(
    raw: &RawProject,
    packages: &[ResolvedPackage],
    project_dir: &Path,
) -> Result<Vec<ResolvedMacro>, Vec<ManifestError>> {
    let mut errors = Vec::new();
    let mut resolved = Vec::with_capacity(raw.macros.len());
    let mut seen_fqns: BTreeSet<String> = BTreeSet::new();

    let declared_packages: BTreeSet<String> = packages.iter().map(|p| p.path.to_string()).collect();

    for m in &raw.macros {
        match validate_macro(m, &declared_packages, project_dir, &raw.name) {
            Ok(resolved_macro) => {
                if !seen_fqns.insert(resolved_macro.fqn.clone()) {
                    errors.push(ManifestError::DuplicateMacro {
                        project: raw.name.clone(),
                        fqn: resolved_macro.fqn.clone(),
                    });
                    continue;
                }
                resolved.push(resolved_macro);
            }
            Err(e) => errors.push(e),
        }
    }

    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

fn validate_macro(
    m: &RawMacro,
    declared_packages: &BTreeSet<String>,
    project_dir: &Path,
    project_name: &str,
) -> Result<ResolvedMacro, ManifestError> {
    // 1. Kind.
    let kind = match m.kind.as_str() {
        "derive" => MacroKind::Derive,
        other => {
            return Err(ManifestError::UnsupportedMacroKind {
                project: project_name.to_string(),
                macro_name: m.name.clone(),
                kind: other.to_string(),
            });
        }
    };

    // 2. Package — must be one of the project's declared packages.
    if !declared_packages.contains(&m.package) {
        return Err(ManifestError::MacroPackageMismatch {
            project: project_name.to_string(),
            macro_name: m.name.clone(),
            package: m.package.clone(),
        });
    }

    // 3. Script path — reject `..` segments, ensure file exists.
    let has_parent_segment = Path::new(&m.script)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir));
    if has_parent_segment {
        return Err(ManifestError::MacroScriptPathEscape {
            project: project_name.to_string(),
            macro_name: m.name.clone(),
            script: m.script.clone(),
        });
    }
    let script_path = project_dir.join(&m.script);
    if !script_path.is_file() {
        return Err(ManifestError::MacroScriptNotFound {
            project: project_name.to_string(),
            macro_name: m.name.clone(),
            expected: script_path,
        });
    }

    let fqn = format!("{}.{}", m.package, m.name);

    Ok(ResolvedMacro {
        fqn,
        kind,
        script_path,
        trait_fqn: m.trait_.clone(),
    })
}

/// Resolve component file paths inside a project and their bindings packages.
fn resolve_components(
    raw: &RawProject,
    project_dir: &std::path::Path,
) -> Result<Vec<crate::manifest::ResolvedComponent>, Vec<ManifestError>> {
    use crate::manifest::{ComponentSource, ResolvedComponent};

    let mut errors = Vec::new();
    let mut resolved = Vec::new();
    let mut seen_packages = std::collections::BTreeSet::new();

    for entry in &raw.components {
        let package_ok = !entry.package.is_empty()
            && entry
                .package
                .split('.')
                .all(|seg| !seg.is_empty() && seg.chars().all(|c| c.is_ascii_alphanumeric()));
        if !package_ok {
            errors.push(ManifestError::InvalidComponent {
                project: raw.name.clone(),
                reason: format!(
                    "component package `{}` is not a valid dotted package path",
                    entry.package
                ),
            });
            continue;
        }
        if !seen_packages.insert(entry.package.clone()) {
            errors.push(ManifestError::InvalidComponent {
                project: raw.name.clone(),
                reason: format!(
                    "two components map to the same bindings package `{}`",
                    entry.package
                ),
            });
            continue;
        }
        let dovetail_package =
            PackagePath(entry.package.split('.').map(|s| s.to_string()).collect());

        let path = &entry.path;
        let has_parent_segment = std::path::Path::new(path)
            .components()
            .any(|c| matches!(c, std::path::Component::ParentDir));
        if has_parent_segment {
            errors.push(ManifestError::InvalidComponent {
                project: raw.name.clone(),
                reason: format!("component path `{path}` escapes the project directory"),
            });
            continue;
        }
        let abs = project_dir.join(path);
        if !abs.is_file() {
            errors.push(ManifestError::InvalidComponent {
                project: raw.name.clone(),
                reason: format!(
                    "component file `{path}` not found (expected {})",
                    abs.display()
                ),
            });
            continue;
        }
        let display_name = std::path::Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.clone());
        let source = ComponentSource::Path(abs);

        resolved.push(ResolvedComponent {
            provider: None,
            source,
            dovetail_package,
            interface: entry.interface.clone(),
            display_name,
        });
    }

    if errors.is_empty() {
        Ok(resolved)
    } else {
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw_project(name: &str, root_package: &str, packages: &[&str]) -> RawProject {
        RawProject {
            image: None,
            name: name.to_string(),
            path: None,
            root_package: root_package.to_string(),
            depends: vec![],
            packages: packages.iter().map(|s| s.to_string()).collect(),
            main: None,
            resources: vec![],
            macros: vec![],
            components: vec![],
        }
    }

    /// Build a minimal `ResolvedProject` for testing component propagation.
    /// `component_pkgs` are bindings-package names, one `ResolvedComponent` each.
    fn project_with(name: &str, depends: &[&str], component_pkgs: &[&str]) -> ResolvedProject {
        use crate::manifest::ComponentSource;
        ResolvedProject {
            resolved_identity: None,
            name: ProjectName(name.to_string()),
            root_package: PackagePath::from_dotted(name),
            depends: depends.iter().map(|d| ProjectName(d.to_string())).collect(),
            packages: vec![],
            project_dir: std::path::PathBuf::from("/fake").join(name),
            main_function: None,
            resources: vec![],
            macros: vec![],
            components: component_pkgs
                .iter()
                .map(|pkg| ResolvedComponent {
                    provider: None,
                    source: ComponentSource::Path(
                        std::path::PathBuf::from("/fake").join(format!("{pkg}.wasm")),
                    ),
                    dovetail_package: PackagePath::from_dotted(pkg),
                    interface: None,
                    display_name: pkg.to_string(),
                })
                .collect(),
        }
    }

    fn component_pkgs(project: &ResolvedProject) -> Vec<String> {
        let mut v: Vec<String> = project
            .components
            .iter()
            .map(|c| c.dovetail_package.to_string())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn transitive_components_single_level() {
        // app -> lib (lib declares component `sqlite.raw`). In topo order deps
        // come first: [lib, app].
        let mut resolved = vec![
            project_with("lib", &[], &["sqlite.raw"]),
            project_with("app", &["lib"], &[]),
        ];
        propagate_transitive_components(&mut resolved);
        assert_eq!(component_pkgs(&resolved[0]), vec!["sqlite.raw"]); // lib unchanged
        assert_eq!(component_pkgs(&resolved[1]), vec!["sqlite.raw"]); // app inherited
    }

    #[test]
    fn transitive_components_multi_level() {
        // app -> mid -> base (base declares the component). Order: [base, mid, app].
        let mut resolved = vec![
            project_with("base", &[], &["sqlite.raw"]),
            project_with("mid", &["base"], &[]),
            project_with("app", &["mid"], &[]),
        ];
        propagate_transitive_components(&mut resolved);
        assert_eq!(component_pkgs(&resolved[1]), vec!["sqlite.raw"]); // mid inherited
        assert_eq!(component_pkgs(&resolved[2]), vec!["sqlite.raw"]); // app inherited transitively
    }

    #[test]
    fn transitive_components_diamond_dedup() {
        // app -> {left, right}, both -> base (base declares the component). The
        // component must reach app exactly once. Order: [base, left, right, app].
        let mut resolved = vec![
            project_with("base", &[], &["sqlite.raw"]),
            project_with("left", &["base"], &[]),
            project_with("right", &["base"], &[]),
            project_with("app", &["left", "right"], &[]),
        ];
        propagate_transitive_components(&mut resolved);
        assert_eq!(component_pkgs(&resolved[3]), vec!["sqlite.raw"]);
        assert_eq!(
            resolved[3].components.len(),
            1,
            "diamond must not duplicate"
        );
    }

    #[test]
    fn transitive_components_keep_own_and_merge() {
        // app declares its own `other.iface` AND depends on lib (has `sqlite.raw`).
        let mut resolved = vec![
            project_with("lib", &[], &["sqlite.raw"]),
            project_with("app", &["lib"], &["other.iface"]),
        ];
        propagate_transitive_components(&mut resolved);
        assert_eq!(
            component_pkgs(&resolved[1]),
            vec!["other.iface", "sqlite.raw"]
        );
    }

    #[test]
    fn resolve_dot_entry() {
        let root = PackagePath::from_dotted("com.example.myapp");
        let dir = Path::new("/fake/myapp");
        let pkg = resolve_package_entry(".", &root, dir).unwrap();
        assert_eq!(pkg.path, PackagePath::from_dotted("com.example.myapp"));
        assert_eq!(pkg.source_dir, dir.join("src"));
    }

    #[test]
    fn resolve_folder_relative_entry() {
        let root = PackagePath::from_dotted("com.example.myapp");
        let dir = Path::new("/fake/myapp");
        let pkg = resolve_package_entry("utils", &root, dir).unwrap();
        assert_eq!(
            pkg.path,
            PackagePath::from_dotted("com.example.myapp.utils")
        );
        assert_eq!(pkg.source_dir, dir.join("src").join("utils"));
    }

    #[test]
    fn resolve_fqn_entry() {
        let root = PackagePath::from_dotted("com.example.myapp");
        let dir = Path::new("/fake/myapp");
        let pkg = resolve_package_entry("com.example.myapp.utils", &root, dir).unwrap();
        assert_eq!(
            pkg.path,
            PackagePath::from_dotted("com.example.myapp.utils")
        );
        assert_eq!(pkg.source_dir, dir.join("src").join("utils"));
    }

    #[test]
    fn resolve_nested_folder_relative() {
        let root = PackagePath::from_dotted("com.example.myapp");
        let dir = Path::new("/fake/myapp");
        let pkg = resolve_package_entry("utils.helpers", &root, dir).unwrap();
        assert_eq!(
            pkg.path,
            PackagePath::from_dotted("com.example.myapp.utils.helpers")
        );
        assert_eq!(
            pkg.source_dir,
            dir.join("src").join("utils").join("helpers")
        );
    }

    #[test]
    fn resolve_fqn_nested() {
        let root = PackagePath::from_dotted("com.example.myapp");
        let dir = Path::new("/fake/myapp");
        let pkg = resolve_package_entry("com.example.myapp.utils.helpers", &root, dir).unwrap();
        assert_eq!(
            pkg.path,
            PackagePath::from_dotted("com.example.myapp.utils.helpers")
        );
        assert_eq!(
            pkg.source_dir,
            dir.join("src").join("utils").join("helpers")
        );
    }

    #[test]
    fn resolve_fqn_equals_root_is_error() {
        let root = PackagePath::from_dotted("com.example.myapp");
        let dir = Path::new("/fake/myapp");
        let err = resolve_package_entry("com.example.myapp", &root, dir).unwrap_err();
        assert!(err.contains("use \".\" instead"));
    }

    #[test]
    fn resolve_non_root_dotted_treated_as_relative() {
        let root = PackagePath::from_dotted("com.example.myapp");
        let dir = Path::new("/fake/myapp");
        // "other.pkg" doesn't start with "com.example.myapp" so it's folder-relative.
        let pkg = resolve_package_entry("other.pkg", &root, dir).unwrap();
        assert_eq!(
            pkg.path,
            PackagePath::from_dotted("com.example.myapp.other.pkg")
        );
        assert_eq!(pkg.source_dir, dir.join("src").join("other").join("pkg"));
    }

    #[test]
    fn validate_project_dir_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        let projects = vec![raw_project("myapp", "com.example.myapp", &["."])];
        // Don't create the project directory.
        let errors = validate_workspace(&projects, &[0], tmp.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::ProjectDirNotFound { project, .. } if project == "myapp")
        );
    }

    #[test]
    fn validate_src_dir_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        // Create project dir but not src/.
        std::fs::create_dir(tmp.path().join("myapp")).unwrap();
        let projects = vec![raw_project("myapp", "com.example.myapp", &["."])];
        let errors = validate_workspace(&projects, &[0], tmp.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::SrcDirNotFound { project, .. } if project == "myapp")
        );
    }

    #[test]
    fn validate_package_dir_not_found() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("myapp/src")).unwrap();
        // List "utils" but don't create src/utils/.
        let projects = vec![raw_project("myapp", "com.example.myapp", &["utils"])];
        let errors = validate_workspace(&projects, &[0], tmp.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::PackageDirNotFound { project, .. } if project == "myapp")
        );
    }

    #[test]
    fn validate_duplicate_package() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("myapp/src/utils")).unwrap();
        // List "utils" twice.
        let projects = vec![raw_project(
            "myapp",
            "com.example.myapp",
            &["utils", "utils"],
        )];
        let errors = validate_workspace(&projects, &[0], tmp.path()).unwrap_err();
        assert_eq!(errors.len(), 1);
        assert!(
            matches!(&errors[0], ManifestError::DuplicatePackage { project, .. } if project == "myapp")
        );
    }

    #[test]
    fn validate_success() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("myapp/src/utils")).unwrap();
        let projects = vec![raw_project("myapp", "com.example.myapp", &["utils", "."])];
        let resolved = validate_workspace(&projects, &[0], tmp.path()).unwrap();
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0].packages.len(), 2);
        assert_eq!(
            resolved[0].packages[0].path,
            PackagePath::from_dotted("com.example.myapp.utils")
        );
        assert_eq!(
            resolved[0].packages[1].path,
            PackagePath::from_dotted("com.example.myapp")
        );
        assert!(resolved[0].main_function.is_none());
    }

    #[test]
    fn validate_explicit_main_function() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("myapp/src")).unwrap();
        let mut project = raw_project("myapp", "com.example.myapp", &["."]);
        project.main = Some("com.example.myapp.entry".to_string());
        let resolved = validate_workspace(&[project], &[0], tmp.path()).unwrap();
        let main = resolved[0].main_function.as_ref().unwrap();
        assert_eq!(main.package, PackagePath::from_dotted("com.example.myapp"));
        assert_eq!(main.symbol.0, "entry");
    }

    #[test]
    fn validate_invalid_main_function() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("myapp/src")).unwrap();
        let mut project = raw_project("myapp", "com.example.myapp", &["."]);
        project.main = Some("main".to_string()); // single segment, invalid
        let errors = validate_workspace(&[project], &[0], tmp.path()).unwrap_err();
        assert!(
            matches!(&errors[0], ManifestError::InvalidMainFunction { project, .. } if project == "myapp")
        );
    }

    fn raw_macro(name: &str, package: &str, script: &str) -> RawMacro {
        RawMacro {
            name: name.to_string(),
            package: package.to_string(),
            kind: "derive".to_string(),
            trait_: None,
            script: script.to_string(),
        }
    }

    #[test]
    fn validate_macro_success() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("mylib/src")).unwrap();
        std::fs::create_dir_all(tmp.path().join("mylib/macros")).unwrap();
        std::fs::write(tmp.path().join("mylib/macros/Foo.rhai"), "// stub").unwrap();

        let mut project = raw_project("mylib", "mylib", &["."]);
        project
            .macros
            .push(raw_macro("Foo", "mylib", "macros/Foo.rhai"));

        let resolved = validate_workspace(&[project], &[0], tmp.path()).unwrap();
        assert_eq!(resolved[0].macros.len(), 1);
        let m = &resolved[0].macros[0];
        assert_eq!(m.fqn, "mylib.Foo");
        assert_eq!(m.kind, MacroKind::Derive);
        assert_eq!(
            m.script_path,
            tmp.path().join("mylib").join("macros").join("Foo.rhai")
        );
    }

    #[test]
    fn validate_macro_unsupported_kind() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("mylib/src")).unwrap();
        std::fs::write(tmp.path().join("mylib/macros.rhai"), "").unwrap();

        let mut project = raw_project("mylib", "mylib", &["."]);
        let mut m = raw_macro("Bar", "mylib", "macros.rhai");
        m.kind = "function".to_string();
        project.macros.push(m);

        let errors = validate_workspace(&[project], &[0], tmp.path()).unwrap_err();
        assert!(
            matches!(&errors[0], ManifestError::UnsupportedMacroKind { macro_name, kind, .. }
                if macro_name == "Bar" && kind == "function")
        );
    }

    #[test]
    fn validate_macro_package_mismatch() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("mylib/src")).unwrap();
        std::fs::write(tmp.path().join("mylib/macros.rhai"), "").unwrap();

        let mut project = raw_project("mylib", "mylib", &["."]);
        // `package` doesn't match `root_package` "mylib" or any subpackage.
        project
            .macros
            .push(raw_macro("Bar", "other.lib", "macros.rhai"));

        let errors = validate_workspace(&[project], &[0], tmp.path()).unwrap_err();
        assert!(matches!(
            &errors[0],
            ManifestError::MacroPackageMismatch { .. }
        ));
    }

    #[test]
    fn validate_macro_script_missing() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("mylib/src")).unwrap();

        let mut project = raw_project("mylib", "mylib", &["."]);
        project
            .macros
            .push(raw_macro("Bar", "mylib", "macros/missing.rhai"));

        let errors = validate_workspace(&[project], &[0], tmp.path()).unwrap_err();
        assert!(
            matches!(&errors[0], ManifestError::MacroScriptNotFound { macro_name, .. } if macro_name == "Bar")
        );
    }

    #[test]
    fn validate_macro_path_escape() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("mylib/src")).unwrap();

        let mut project = raw_project("mylib", "mylib", &["."]);
        project
            .macros
            .push(raw_macro("Bar", "mylib", "../outside.rhai"));

        let errors = validate_workspace(&[project], &[0], tmp.path()).unwrap_err();
        assert!(matches!(
            &errors[0],
            ManifestError::MacroScriptPathEscape { .. }
        ));
    }

    #[test]
    fn validate_macro_duplicate() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("mylib/src")).unwrap();
        std::fs::write(tmp.path().join("mylib/m.rhai"), "").unwrap();

        let mut project = raw_project("mylib", "mylib", &["."]);
        project.macros.push(raw_macro("Bar", "mylib", "m.rhai"));
        project.macros.push(raw_macro("Bar", "mylib", "m.rhai"));

        let errors = validate_workspace(&[project], &[0], tmp.path()).unwrap_err();
        assert!(
            matches!(&errors[0], ManifestError::DuplicateMacro { fqn, .. } if fqn == "mylib.Bar")
        );
    }

    #[test]
    fn validate_main_outside_root_package() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("myapp/src")).unwrap();
        let mut project = raw_project("myapp", "com.example.myapp", &["."]);
        project.main = Some("other.pkg.entry".to_string()); // outside root_package
        let errors = validate_workspace(&[project], &[0], tmp.path()).unwrap_err();
        assert!(
            matches!(&errors[0], ManifestError::InvalidMainFunction { project, reason, .. }
                if project == "myapp" && reason.contains("root package"))
        );
    }
}
