//! Compiler-backed declaration discovery, independent of the LSP transport.
mod declarations;
mod output;
mod types;

use crate::common::{diagnostics::Diagnostics, span::Span, types::Visibility};
use crate::manifest::{ResolveOptions, ResolvedWorkspace};
use clap::{Args, Subcommand};
pub use declarations::DeclarationView;
pub(crate) use declarations::{add_generated, capture, enrich};
use std::collections::{BTreeSet, HashMap};
use std::path::Path;

#[derive(Debug, Args)]
pub struct QueryArgs {
    /// Select a local project's dependency context.
    #[arg(long, global = true)]
    pub project: Option<String>,
    /// Include nonpublic dependency declarations (inspection only).
    #[arg(long, global = true)]
    pub all: bool,
    #[command(subcommand)]
    pub command: QueryCommand,
}

#[derive(Debug, Subcommand)]
pub enum QueryCommand {
    /// Find declarations and members by name or qualified name.
    Search {
        text: String,
        /// Search only this exact package, including its members.
        #[arg(long)]
        package: Option<String>,
        #[arg(long, default_value_t = 50, value_parser = clap::value_parser!(u32).range(1..))]
        limit: u32,
        #[arg(long, default_value_t = 0)]
        offset: u32,
    },
    /// List available packages, or immediate declarations in one package.
    Package { name: Option<String> },
    /// Show body-free declarations for a fully qualified name.
    Definition { name: String },
}

pub struct QueryResult {
    pub output: String,
    pub diagnostics: Diagnostics,
    pub success: bool,
}

struct ProjectDeclarations {
    name: String,
    identity: String,
    local: bool,
    declarations: Vec<DeclarationView>,
}

pub(crate) fn parse_declarations(
    source: &str,
    file: &str,
    diagnostics: &mut Diagnostics,
) -> Vec<DeclarationView> {
    let (source, errors) = crate::discovery::parse_source(source, file.into());
    diagnostics.extend_from(&errors);
    declarations::capture_file(&source)
}

/// Recover source declarations from files the normal checker deliberately skips
/// after parse errors. Never feed the recovered AST back into type checking.
pub(crate) fn recover_unparsed(
    package: &crate::parser::ast::PackageAst,
    source_dir: &Path,
    root: &Path,
    diagnostics: &Diagnostics,
) -> Vec<DeclarationView> {
    let parsed: BTreeSet<_> = package
        .files
        .iter()
        .map(|f| f.package.span.file.as_ref())
        .collect();
    let failed: BTreeSet<_> = diagnostics
        .iter()
        .filter(|d| !parsed.contains(d.span.file.as_ref()))
        .map(|d| d.span.file.as_ref())
        .collect();
    let mut views = Vec::new();
    for file in failed {
        let path = root.join(file);
        if path.parent() != Some(source_dir) || path.extension().is_none_or(|e| e != "dove") {
            continue;
        }
        if let Ok(source) = std::fs::read_to_string(&path) {
            views.extend(parse_declarations(&source, file, &mut Diagnostics::new()));
        }
    }
    views
}

fn prelude(diagnostics: &mut Diagnostics) -> ProjectDeclarations {
    let files = crate::prelude_sources()
        .map(|(file, source)| {
            let (file, errors) =
                crate::discovery::parse_source(source, format!("<prelude>/{file}").into());
            diagnostics.extend_from(&errors);
            file
        })
        .collect();
    let mut package = crate::parser::ast::PackageAst {
        package_path: crate::common::types::PackagePath(vec!["standard".into(), "prelude".into()]),
        files,
    };
    let mut declarations = capture(&package);
    crate::macros::expand_package(
        &mut package,
        &crate::macros::builtin_registry(),
        diagnostics,
    );
    add_generated(&mut declarations, &package);
    let (_, registry, module) = &*crate::compiler::PRELUDE_OUTPUT;
    enrich(&mut declarations, registry, module, true);
    for name in [
        "Unit", "Bool", "String", "Char", "Int8", "Int16", "Int32", "Int64", "Uint8", "Uint16",
        "Uint32", "Uint64", "Uint128", "Float32", "Float64", "Never", "Any", "Array", "Tuple",
    ] {
        if declarations
            .iter()
            .any(|d| d.name == name && d.kind != "module")
        {
            continue;
        }
        declarations.push(DeclarationView {
            name: name.to_owned(),
            kind: "builtin",
            header: format!(
                "// Compiler-built-in {}{name}{}",
                if name == "Tuple" {
                    "structural constraint "
                } else {
                    "type "
                },
                if name == "Array" { "<T>" } else { "" }
            ),
            docs: None,
            span: Span::point("<compiler>".into(), 1, 1),
            visibility: Visibility::Public,
            children: Vec::new(),
            block: false,
            target: None,
            target_spelling: None,
            generated: false,
            package: "standard.prelude".to_owned(),
            imports: Vec::new(),
        });
    }
    ProjectDeclarations {
        name: "prelude (embedded)".to_owned(),
        identity: "<prelude>".to_owned(),
        local: false,
        declarations,
    }
}

fn requested_package(command: &QueryCommand) -> Option<&str> {
    match command {
        QueryCommand::Package { name } => name.as_deref(),
        QueryCommand::Definition { name } => Some(name),
        QueryCommand::Search { package, .. } => package.as_deref(),
    }
}

fn prelude_only(command: &QueryCommand) -> bool {
    requested_package(command)
        .is_some_and(|p| p == "standard.prelude" || p.starts_with("standard.prelude."))
}

fn load_workspace(
    root: &Path,
    args: &QueryArgs,
    options: &ResolveOptions,
) -> anyhow::Result<Option<ResolvedWorkspace>> {
    if prelude_only(&args.command) {
        return Ok(None);
    }
    let root = root
        .ancestors()
        .find(|p| p.join("Dovetail.toml").exists() || p.join("Domain.toml").exists());
    let Some(root) = root else {
        anyhow::ensure!(
            args.project.is_none(),
            "--project requires a Dovetail workspace"
        );
        return Ok(None);
    };
    let options = ResolveOptions {
        target: args.project.clone(),
        ..options.clone()
    };
    crate::manifest::load_manifest_with_options(root, &options)
        .map(Some)
        .map_err(|errors| {
            anyhow::anyhow!(
                errors
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        })
}

/// A qualified target can be checked in its owner's closure, without checking
/// consumer bodies. Preserve every owner when different contexts provide it.
fn owner_filters(workspace: &ResolvedWorkspace, command: &QueryCommand) -> Vec<String> {
    let Some(name) = requested_package(command) else {
        return Vec::new();
    };
    let mut candidates = Vec::new();
    for project in &workspace.projects {
        let packages = project
            .packages
            .iter()
            .map(|p| &p.path)
            .chain(project.components.iter().map(|c| &c.dovetail_package));
        for package in packages {
            let package = package.to_string();
            let matches = match command {
                QueryCommand::Definition { .. } => name.starts_with(&format!("{package}.")),
                QueryCommand::Package { .. } => {
                    name == package || package.starts_with(&format!("{name}."))
                }
                QueryCommand::Search { .. } => name == package,
            };
            if matches {
                candidates.push((package.len(), workspace.result_key(project)));
            }
        }
    }
    if matches!(command, QueryCommand::Definition { .. }) {
        let longest = candidates.iter().map(|(length, _)| *length).max();
        candidates.retain(|(length, _)| Some(*length) == longest);
    }
    candidates
        .into_iter()
        .map(|(_, key)| key)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

fn analyze(
    workspace: &ResolvedWorkspace,
    args: &QueryArgs,
    diagnostics: &mut Diagnostics,
) -> Vec<ProjectDeclarations> {
    let mut owners = owner_filters(workspace, &args.command);
    if owners.is_empty() && args.project.is_none() {
        let local: Vec<_> = workspace
            .projects
            .iter()
            .filter(|p| workspace.is_local(p))
            .collect();
        if local
            .iter()
            .any(|p| crate::manifest::validate_dependency_closure(workspace, &p.name.0).is_err())
        {
            owners = local.iter().map(|p| p.name.0.clone()).collect();
        }
    }
    let filters: Vec<Option<&str>> = if owners.is_empty() {
        vec![args.project.as_deref()]
    } else {
        owners.iter().map(|s| Some(s.as_str())).collect()
    };
    let mut seen = BTreeSet::new();
    let mut projects = Vec::new();
    for filter in filters {
        if let Some(target) = filter
            && let Err(error) = crate::manifest::validate_dependency_closure(workspace, target)
        {
            diagnostics.error(Span::point("Dovetail.toml".into(), 1, 1), error.to_string());
            continue;
        }
        let result = crate::build_workspace(
            workspace,
            filter,
            crate::BuildMode::Query,
            &HashMap::new(),
            true,
            None,
        );
        diagnostics.extend_from(&result.diagnostics);
        for (key, result) in result.project_results {
            let Some(project) = workspace.project(&key) else {
                continue;
            };
            if seen.insert(project.identity()) {
                projects.push(ProjectDeclarations {
                    name: project.name.0.clone(),
                    identity: project.identity(),
                    local: workspace.is_local(project),
                    declarations: result.declarations,
                });
            }
        }
    }
    projects
}

pub fn execute(
    root: &Path,
    args: &QueryArgs,
    options: &ResolveOptions,
) -> anyhow::Result<QueryResult> {
    let workspace = load_workspace(root, args, options)?;
    if matches!(args.command, QueryCommand::Package { name: None }) {
        return Ok(QueryResult {
            output: output::packages(workspace.as_ref()),
            diagnostics: Diagnostics::new(),
            success: true,
        });
    }
    let mut diagnostics = Diagnostics::new();
    let mut projects = vec![prelude(&mut diagnostics)];
    if let Some(workspace) = &workspace {
        let mut analyzed = analyze(workspace, args, &mut diagnostics);
        if let Some(source_prelude) = analyzed.iter_mut().find(|p| {
            p.declarations
                .iter()
                .any(|d| d.package == "standard.prelude")
        }) {
            source_prelude.declarations.extend(
                projects
                    .remove(0)
                    .declarations
                    .into_iter()
                    .filter(|d| d.kind == "builtin"),
            );
        }
        projects.extend(analyzed);
    }
    let (mut output, found) = output::query(&projects, args, workspace.as_ref())?;
    if diagnostics.has_errors() {
        output.insert_str(
            0,
            "// INCOMPLETE: analysis reported errors; inferred types may be unavailable.\n\n",
        );
    }
    Ok(QueryResult {
        output,
        success: found && !diagnostics.has_errors(),
        diagnostics,
    })
}

#[cfg(test)]
mod tests;
