//! Scoped dependency graph resolution shared by the CLI and language server.
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, atomic::AtomicBool};

use serde::{Deserialize, Serialize};

use super::git;
use super::toml_schema::{self, ProjectSelection, RawDependency, RawManifest};
use super::{ComponentSource, ManifestError, ProjectName, ResolvedProject, ResolvedWorkspace};

const STANDARD_REPOSITORY: &str = "https://github.com/somdoron/dovetail.git";

#[derive(Clone, Debug, Default)]
pub enum UpdateRequest {
    #[default]
    None,
    All,
    Alias(String),
}

pub type DependencyProgress = dyn Fn(&str) + Send + Sync;

#[derive(Clone, Default)]
pub struct ResolveOptions {
    pub target: Option<String>,
    /// Optional Git executable override for embedding and deterministic transport tests.
    pub git_executable: Option<PathBuf>,
    pub locked: bool,
    pub offline: bool,
    pub update: UpdateRequest,
    pub cancelled: Option<Arc<AtomicBool>>,
    pub progress: Option<Arc<DependencyProgress>>,
    /// Materialize all directly declared selections, including unused projects.
    pub fetch_all: bool,
}

pub(super) fn error(message: impl Into<String>) -> ManifestError {
    ManifestError::TomlError {
        message: message.into(),
    }
}

pub(super) fn check_version(raw: &RawManifest, path: &Path) -> Result<(), ManifestError> {
    if raw.compiler_version != env!("CARGO_PKG_VERSION") {
        return Err(error(format!(
            "compiler version mismatch in {}: manifest requires Dovetail {}, running binary is Dovetail {}",
            path.display(),
            raw.compiler_version,
            env!("CARGO_PKG_VERSION")
        )));
    }
    Ok(())
}

#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
struct Lockfile {
    version: u32,
    compiler_version: String,
    manifest_digest: String,
    #[serde(default)]
    sources: BTreeMap<String, LockedSource>,
    #[serde(default)]
    projects: BTreeMap<String, LockedProject>,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LockedSource {
    repository: String,
    selector: String,
    commit: String,
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
struct LockedProject {
    manifest: String,
    project: String,
    dependencies: BTreeMap<String, String>,
}

#[derive(Clone)]
struct Scope {
    raw: RawManifest,
    directory: PathBuf,
    boundary: PathBuf,
    remote: bool,
    aliases: BTreeMap<String, (RawDependency, String)>,
}

struct Resolver<'a> {
    root: PathBuf,
    store: PathBuf,
    options: &'a ResolveOptions,
    scopes: BTreeMap<PathBuf, Scope>,
    projects: Vec<ResolvedProject>,
    done: BTreeSet<String>,
    active: Vec<String>,
    lock: Lockfile,
    updating: BTreeSet<String>,
    fetched: BTreeMap<String, PathBuf>,
    aliases: BTreeMap<(String, String), String>,
}

fn read_manifest(path: &Path) -> Result<RawManifest, ManifestError> {
    if path.file_name().is_some_and(|name| name == "Dovetail.toml") {
        super::validate_workspace_filenames(path.parent().unwrap())?;
    }
    let content = std::fs::read_to_string(path).map_err(|source| ManifestError::IoError {
        path: path.into(),
        source,
    })?;
    let raw = toml_schema::parse_manifest(&content)
        .map_err(|e| error(format!("{}: {e}", path.display())))?;
    check_version(&raw, path)?;
    Ok(raw)
}

fn selector(dep: &RawDependency) -> Result<String, ManifestError> {
    if [&dep.branch, &dep.tag, &dep.rev]
        .iter()
        .filter(|s| s.is_some())
        .count()
        > 1
    {
        return Err(error("dependency accepts at most one of branch, tag, rev"));
    }
    let value = if let Some(branch) = &dep.branch {
        format!("refs/heads/{branch}")
    } else if let Some(tag) = &dep.tag {
        format!("refs/tags/{tag}")
    } else {
        dep.rev.clone().unwrap_or_else(|| "HEAD".into())
    };
    if value.starts_with('-') || value.contains(['\n', '\r', '\0']) || value.is_empty() {
        return Err(error("invalid dependency revision"));
    }
    Ok(value)
}

fn source_key(dep: &RawDependency) -> Result<String, ManifestError> {
    Ok(git::digest(&format!(
        "{}\n{}",
        git::repository(&dep.git)?,
        selector(dep)?
    )))
}

fn lock_path(root: &Path, path: &str) -> String {
    let path = Path::new(path);
    path.strip_prefix(root)
        .unwrap_or(path)
        .components()
        .map(|part| part.as_os_str().to_string_lossy())
        .collect::<Vec<_>>()
        .join("/")
}

fn insert_alias(
    scope: &mut Scope,
    dep: &RawDependency,
    project: &str,
    alias: &str,
) -> Result<(), ManifestError> {
    if alias.is_empty()
        || scope.raw.project.iter().any(|p| p.name == alias)
        || scope.aliases.contains_key(alias)
    {
        return Err(error(format!(
            "duplicate or invalid dependency alias `{alias}` in {}",
            scope.directory.display()
        )));
    }
    scope
        .aliases
        .insert(alias.into(), (dep.clone(), project.into()));
    Ok(())
}

impl Resolver<'_> {
    fn fetch(&mut self, dep: &RawDependency) -> Result<PathBuf, ManifestError> {
        let key = source_key(dep)?;
        if let Some(path) = self.fetched.get(&key) {
            return Ok(path.clone());
        }
        let requested = selector(dep)?;
        let update =
            matches!(self.options.update, UpdateRequest::All) || self.updating.contains(&key);
        let pinned = if update {
            None
        } else {
            self.lock.sources.get(&key).map(|s| s.commit.clone())
        };
        if self.options.locked && pinned.is_none() {
            return Err(error(format!(
                "dependency {} requires a lockfile update",
                dep.git
            )));
        }
        let (commit, checkout) = git::checkout(
            &self.store,
            &dep.git,
            &requested,
            pinned.as_deref(),
            self.options,
        )?;
        self.lock.sources.insert(
            key.clone(),
            LockedSource {
                repository: git::repository(&dep.git)?,
                selector: requested,
                commit,
            },
        );
        // An update request advances each source only once in this resolution.
        self.updating.remove(&key);
        self.fetched.insert(key, checkout.clone());
        Ok(checkout)
    }

    fn add_scope(
        &mut self,
        directory: &Path,
        boundary: &Path,
        remote: bool,
    ) -> Result<(), ManifestError> {
        let path = directory.join("Dovetail.toml");
        if self.scopes.contains_key(&path) {
            return Ok(());
        }
        // Validate symlink containment, while retaining the workspace's logical
        // manifest location as the scope and project-directory base.
        git::contained(boundary, &path)?;
        self.add_manifest(&path, boundary, remote)
    }

    fn add_manifest(
        &mut self,
        path: &Path,
        boundary: &Path,
        remote: bool,
    ) -> Result<(), ManifestError> {
        let directory = path.parent().unwrap().to_path_buf();
        // Scope key is the exact manifest path (supports differently named manifests).
        if self.scopes.contains_key(path) {
            return Ok(());
        }
        let raw = read_manifest(path)?;
        let mut names = BTreeSet::new();
        for project in &raw.project {
            if project.name.is_empty()
                || project.name == "."
                || project.name == ".."
                || project.name.contains(['/', '\\', '#'])
            {
                return Err(error(format!(
                    "invalid project directory name `{}`",
                    project.name
                )));
            }
            if !names.insert(&project.name) {
                return Err(ManifestError::DuplicateProject {
                    name: project.name.clone(),
                });
            }
        }
        let mut scope = Scope {
            raw,
            directory,
            boundary: boundary.into(),
            remote,
            aliases: BTreeMap::new(),
        };
        for dep in scope.raw.dependencies.clone() {
            selector(&dep)?;
            git::repository(&dep.git)?;
            if dep.projects.is_empty() {
                return Err(error("dependency projects must not be empty"));
            }
            for selection in &dep.projects {
                let (project, alias) = selection.names();
                insert_alias(&mut scope, &dep, project, alias)?;
            }
        }
        if let Some(tag) = scope.raw.standard_tag.clone() {
            let mut dep = RawDependency {
                git: STANDARD_REPOSITORY.into(),
                tag: Some(tag),
                branch: None,
                rev: None,
                manifest: None,
                projects: vec![],
            };
            // Publish the scope before fetching to keep initialization non-recursive.
            let checkout = self.fetch(&dep)?;
            let standard_manifest = git::contained(&checkout, &checkout.join("Dovetail.toml"))?;
            let standard = read_manifest(&standard_manifest)?;
            dep.projects = standard
                .project
                .iter()
                .filter(|p| {
                    p.root_package != "standard.prelude"
                        && (p.root_package == "standard" || p.root_package.starts_with("standard."))
                })
                .map(|p| ProjectSelection::Name(p.name.clone()))
                .collect();
            for selection in &dep.projects {
                let (project, alias) = selection.names();
                insert_alias(&mut scope, &dep, project, alias)?;
            }
        }
        self.scopes.insert(path.into(), scope);
        Ok(())
    }

    fn visit(&mut self, manifest: &Path, name: &str) -> Result<Option<String>, ManifestError> {
        let nested = !self.active.is_empty() || self.scopes.get(manifest).is_some_and(|s| s.remote);
        self.visit_project(manifest, name).map_err(|failure| {
            let message = match failure {
                ManifestError::TomlError { message } => message,
                failure if nested => failure.to_string(),
                failure => return failure,
            };
            if message.starts_with(&format!("{name} -> ")) {
                error(message)
            } else {
                error(format!("{name} -> {message}"))
            }
        })
    }

    fn visit_project(
        &mut self,
        manifest: &Path,
        name: &str,
    ) -> Result<Option<String>, ManifestError> {
        git::cancelled(self.options)?;
        let scope = self
            .scopes
            .get(manifest)
            .cloned()
            .ok_or_else(|| error("missing resolver scope"))?;
        if let Some((dep, project)) = scope.aliases.get(name) {
            let checkout = self.fetch(dep)?;
            let target = git::contained(
                &checkout,
                &checkout.join(dep.manifest.as_deref().unwrap_or("Dovetail.toml")),
            )?;
            self.add_manifest(&target, &checkout, true)?;
            if !self.scopes[&target]
                .raw
                .project
                .iter()
                .any(|p| &p.name == project)
            {
                return Err(error(format!(
                    "remote project `{project}` not found in {}",
                    target.display()
                )));
            }
            return self.visit(&target, project);
        }
        let raw = scope
            .raw
            .project
            .iter()
            .find(|p| p.name == name)
            .cloned()
            .ok_or_else(|| {
                error(format!(
                    "unknown dependency/project `{name}` in {}",
                    manifest.display()
                ))
            })?;
        if scope.remote && raw.root_package == "standard.prelude" {
            let source = self.lock.sources.values().any(|s| {
                s.repository == STANDARD_REPOSITORY.trim_end_matches(".git")
                    && scope.boundary == self.store.join(git::digest(&s.repository)).join(&s.commit)
            });
            if source {
                return Ok(None);
            }
            return Err(error(format!(
                "dependency {name} attempts to replace the bundled prelude"
            )));
        }
        let directory = git::contained(
            &scope.boundary,
            &scope.directory.join(raw.path.as_deref().unwrap_or(name)),
        )?;
        let id = if scope.remote {
            format!("{}#{}", manifest.display(), name)
        } else {
            ResolvedProject::local_identity(&directory, name)
        };
        if self.active.contains(&id) {
            let mut cycle = self.active.clone();
            cycle.push(id);
            return Err(ManifestError::CyclicDependency { cycle });
        }
        if self.done.contains(&id) {
            return Ok(Some(id));
        }
        self.active.push(id.clone());
        let mut edges = BTreeMap::new();
        for dependency in &raw.depends {
            if let Some(target) = self.visit(manifest, dependency)? {
                edges.insert(dependency.clone(), target);
            }
        }
        let mut project = super::validate::validate_workspace(&[raw], &[0], &scope.directory)
            .map_err(|mut errors| errors.remove(0))?
            .remove(0);
        project.project_dir = directory;
        for package in &mut project.packages {
            package.source_dir = git::contained(&project.project_dir, &package.source_dir)?;
        }
        project.resolved_identity = scope.remote.then(|| id.clone());
        for component in &mut project.components {
            component.provider = Some(id.clone());
        }
        project.depends = edges.values().cloned().map(ProjectName).collect();
        for (alias, dependency) in &edges {
            self.aliases
                .insert((id.clone(), dependency.clone()), alias.clone());
        }
        validate_assets(&project, &scope.boundary)?;
        if scope.remote
            && project
                .packages
                .iter()
                .map(|p| &p.path)
                .chain(project.components.iter().map(|c| &c.dovetail_package))
                .any(|package| {
                    package.to_string() == "standard.prelude"
                        || package.to_string().starts_with("standard.prelude.")
                })
        {
            return Err(error(format!(
                "dependency {name} overlaps the bundled prelude"
            )));
        }
        // Lock identities are portable paths relative to the consuming workspace.
        let portable = |path: &str| lock_path(&self.root, path);
        self.lock.projects.insert(
            portable(&id),
            LockedProject {
                manifest: portable(&manifest.to_string_lossy()),
                project: name.into(),
                dependencies: edges
                    .iter()
                    .map(|(alias, id)| (alias.clone(), portable(id)))
                    .collect(),
            },
        );
        self.active.pop();
        self.done.insert(id.clone());
        self.projects.push(project);
        Ok(Some(id))
    }
}

fn validate_assets(project: &ResolvedProject, boundary: &Path) -> Result<(), ManifestError> {
    for package in &project.packages {
        git::contained(&project.project_dir, &package.source_dir)?;
        for entry in std::fs::read_dir(&package.source_dir).map_err(|e| error(e.to_string()))? {
            let path = entry.map_err(|e| error(e.to_string()))?.path();
            if path.extension().is_some_and(|ext| ext == "dove") {
                git::contained(boundary, &path)?;
            }
        }
    }
    let paths = project
        .resources
        .iter()
        .map(|p| project.project_dir.join(p))
        .chain(project.macros.iter().map(|m| m.script_path.clone()))
        .chain(project.components.iter().map(|c| match &c.source {
            ComponentSource::Path(p) => p.clone(),
        }));
    for path in paths {
        git::contained(&project.project_dir, &path)?;
        use std::io::Read;
        let mut prefix = [0; 42];
        let count = std::fs::File::open(&path)
            .and_then(|mut f| f.read(&mut prefix))
            .map_err(|e| error(e.to_string()))?;
        if prefix[..count].starts_with(b"version https://git-lfs.github.com/spec/") {
            return Err(error(format!(
                "{} is an unhydrated Git LFS pointer; commit the actual artifact (LFS hydration is not supported)",
                path.display()
            )));
        }
    }
    Ok(())
}

/// Resolve the complete reachable graph, then atomically publish its lockfile.
pub fn load_manifest_with_options(
    root: &Path,
    options: &ResolveOptions,
) -> Result<ResolvedWorkspace, Vec<ManifestError>> {
    resolve(root, options).map_err(|e| vec![e])
}

fn resolve(root: &Path, options: &ResolveOptions) -> Result<ResolvedWorkspace, ManifestError> {
    let root = root.canonicalize().map_err(|e| error(e.to_string()))?;
    let manifest = root.join("Dovetail.toml");
    // Must precede all cache and network operations.
    let raw = read_manifest(&manifest)?;
    if options.locked && !matches!(options.update, UpdateRequest::None) {
        return Err(error("dependency updates cannot be combined with --locked"));
    }
    let targets: Vec<String> = if let Some(target) = &options.target {
        if !raw.project.iter().any(|p| &p.name == target) {
            return Err(error(format!("unknown local project `{target}`")));
        }
        vec![target.clone()]
    } else {
        raw.project.iter().map(|p| p.name.clone()).collect()
    };
    let store = root.join(".dovetail/deps");
    let uses_lock = !raw.dependencies.is_empty()
        || raw.standard_tag.is_some()
        || root.join("Dovetail.lock").exists();
    let _guard = if uses_lock {
        Some(git::lock(&store, options)?)
    } else {
        None
    };
    let old = match std::fs::read_to_string(root.join("Dovetail.lock")) {
        Ok(text) => Some(
            toml::from_str::<Lockfile>(&text)
                .map_err(|e| error(format!("invalid Dovetail.lock: {e}")))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(error(format!("cannot read Dovetail.lock: {e}"))),
    };
    if old.as_ref().is_some_and(|l| l.version != 1) {
        return Err(error("unsupported Dovetail.lock format version"));
    }
    let mut lock = old.clone().unwrap_or_default();
    lock.version = 1;
    lock.compiler_version = raw.compiler_version.clone();
    lock.manifest_digest = git::digest(&toml::to_string(&raw).map_err(|e| error(e.to_string()))?);
    let mut resolver = Resolver {
        root: root.clone(),
        store,
        options,
        scopes: BTreeMap::new(),
        projects: vec![],
        done: BTreeSet::new(),
        active: vec![],
        lock,
        updating: BTreeSet::new(),
        fetched: BTreeMap::new(),
        aliases: BTreeMap::new(),
    };
    if let UpdateRequest::Alias(alias) = &options.update {
        if let Some(dep) = raw
            .dependencies
            .iter()
            .find(|d| d.projects.iter().any(|s| s.names().1 == alias))
        {
            resolver.updating.insert(source_key(dep)?);
        } else if let Some(tag) = &raw.standard_tag {
            let standard = RawDependency {
                git: STANDARD_REPOSITORY.into(),
                tag: Some(tag.clone()),
                branch: None,
                rev: None,
                manifest: None,
                projects: vec![],
            };
            resolver.updating.insert(source_key(&standard)?);
        } else {
            return Err(error(format!("unknown root dependency alias `{alias}`")));
        }
    }
    if options.target.is_none() && raw.dependencies.is_empty() && raw.standard_tag.is_none() {
        super::resolve::topological_sort(&raw.project).map_err(|mut errors| errors.remove(0))?;
    }
    resolver.add_scope(&root, &root, false)?;
    if let UpdateRequest::Alias(alias) = &options.update {
        let dep = resolver.scopes[&manifest]
            .aliases
            .get(alias)
            .ok_or_else(|| error(format!("unknown root dependency alias `{alias}`")))?
            .0
            .clone();
        resolver.updating.insert(source_key(&dep)?);
    }
    for target in &targets {
        resolver.visit(&manifest, target)?;
    }
    // Fetch/update also materializes directly declared selections unused by targets.
    let mut selected_roots = BTreeSet::new();
    if options.fetch_all || !matches!(options.update, UpdateRequest::None) {
        let aliases: Vec<_> = resolver.scopes[&manifest].aliases.keys().cloned().collect();
        for alias in aliases {
            if let Some(identity) = resolver.visit(&manifest, &alias)? {
                selected_roots.insert(identity);
            }
        }
    }
    let mut workspace = ResolvedWorkspace {
        dependency_aliases: resolver.aliases,
        workspace_root: root,
        projects: resolver.projects,
    };
    for target in &targets {
        validate_closure(&workspace, target)?;
    }
    for selected in selected_roots {
        validate_closure(&workspace, &selected)?;
    }
    super::validate::propagate_transitive_components(&mut workspace.projects);
    git::cancelled(options)?;
    if uses_lock && old.as_ref() != Some(&resolver.lock) {
        if options.locked {
            return Err(error(
                "Dovetail.lock needs updating; run dovetail deps fetch without --locked",
            ));
        }
        let text = toml::to_string_pretty(&resolver.lock).map_err(|e| error(e.to_string()))?;
        use std::io::Write;
        let mut temporary = tempfile::NamedTempFile::new_in(&workspace.workspace_root)
            .map_err(|e| error(e.to_string()))?;
        temporary
            .write_all(text.as_bytes())
            .map_err(|e| error(e.to_string()))?;
        temporary
            .persist(workspace.workspace_root.join("Dovetail.lock"))
            .map_err(|e| error(e.to_string()))?;
    }
    Ok(workspace)
}

/// Check providers before any registry merge; identical graph nodes are visited once.
pub fn validate_closure(workspace: &ResolvedWorkspace, target: &str) -> Result<(), ManifestError> {
    let mut pending = vec![(target.to_string(), target.to_string())];
    let mut seen = BTreeSet::new();
    let mut packages: BTreeMap<String, (String, String)> = BTreeMap::new();
    while let Some((key, chain)) = pending.pop() {
        let project = workspace
            .project(&key)
            .ok_or_else(|| error(format!("unknown project in dependency path {chain}")))?;
        let id = project.identity();
        if !seen.insert(id.clone()) {
            continue;
        }
        let providers = project
            .packages
            .iter()
            .map(|p| (p.path.to_string(), id.clone()))
            .chain(project.components.iter().map(|c| {
                (
                    c.dovetail_package.to_string(),
                    format!(
                        "component:{:?}:{:?}:{:?}",
                        c.provider, c.source, c.interface
                    ),
                )
            }));
        for (package, provider) in providers {
            if let Some((owner, previous)) = packages.get(&package) {
                if owner != &provider {
                    return Err(error(format!(
                        "package `{package}` has multiple providers in target `{target}`:\n  {previous} ({owner})\n  {chain} ({id})"
                    )));
                }
            } else {
                packages.insert(package, (provider, chain.clone()));
            }
        }
        for dep in &project.depends {
            let label = workspace
                .dependency_aliases
                .get(&(id.clone(), dep.0.clone()))
                .map(String::as_str)
                .or_else(|| workspace.project(&dep.0).map(|p| p.name.0.as_str()))
                .unwrap_or(&dep.0);
            pending.push((dep.0.clone(), format!("{chain} -> {label}")));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lock_identities_use_portable_path_separators() {
        let root = Path::new("workspace");
        let project = root
            .join(".dovetail")
            .join("deps")
            .join("repository")
            .join("commit")
            .join("Dovetail.toml#library");
        assert_eq!(
            lock_path(root, &project.to_string_lossy()),
            ".dovetail/deps/repository/commit/Dovetail.toml#library",
        );
    }

    #[cfg(unix)]
    #[test]
    fn root_manifest_symlink_keeps_workspace_project_base() {
        let temporary = tempfile::tempdir().unwrap();
        super::super::init_workspace(temporary.path(), "app").unwrap();
        std::fs::create_dir(temporary.path().join("config")).unwrap();
        std::fs::rename(
            temporary.path().join("Dovetail.toml"),
            temporary.path().join("config/workspace.toml"),
        )
        .unwrap();
        std::os::unix::fs::symlink(
            "config/workspace.toml",
            temporary.path().join("Dovetail.toml"),
        )
        .unwrap();

        let workspace =
            load_manifest_with_options(temporary.path(), &ResolveOptions::default()).unwrap();
        let app = workspace.project("app").unwrap();
        assert_eq!(
            app.project_dir,
            temporary.path().join("app").canonicalize().unwrap()
        );
        assert!(workspace.is_local(app));
    }

    #[cfg(unix)]
    #[test]
    fn local_projects_sharing_source_directory_keep_distinct_identities() {
        let temporary = tempfile::tempdir().unwrap();
        super::super::init_workspace(temporary.path(), "first").unwrap();
        std::os::unix::fs::symlink("first", temporary.path().join("second")).unwrap();
        let manifest = temporary.path().join("Dovetail.toml");
        let mut contents = std::fs::read_to_string(&manifest).unwrap();
        contents.push_str(
            "\n[[project]]\nname = \"second\"\nroot_package = \"first\"\npackages = [\".\"]\n",
        );
        std::fs::write(&manifest, contents).unwrap();

        let workspace =
            load_manifest_with_options(temporary.path(), &ResolveOptions::default()).unwrap();
        let first = workspace.project("first").unwrap();
        let second = workspace.project("second").unwrap();
        assert_eq!(first.project_dir, second.project_dir);
        assert_ne!(first.identity(), second.identity());
        assert_eq!(first.packages[0].source_dir, second.packages[0].source_dir);
        assert_eq!(
            second.packages[0].source_dir,
            second.project_dir.join("src")
        );
        assert!(workspace.is_local(first) && workspace.is_local(second));
    }
}
