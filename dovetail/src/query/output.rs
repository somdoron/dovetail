use super::{DeclarationView, ProjectDeclarations, QueryArgs, QueryCommand};
use crate::manifest::ResolvedWorkspace;
use std::collections::{BTreeMap, BTreeSet};

struct Symbol<'a> {
    fqn: String,
    view: &'a DeclarationView,
    ancestors: Vec<&'a DeclarationView>,
    owner: &'a ProjectDeclarations,
}

fn symbols<'a>(projects: &'a [ProjectDeclarations], all: bool) -> Vec<Symbol<'a>> {
    let mut symbols = Vec::new();
    for project in projects {
        for view in &project.declarations {
            collect(
                view,
                &view.package,
                project,
                all || project.local,
                &[],
                &mut symbols,
            );
        }
    }
    symbols.sort_by(|a, b| {
        (
            &a.fqn,
            &a.owner.identity,
            a.view.kind,
            &a.view.span.file,
            a.view.span.line,
            a.view.span.column,
        )
            .cmp(&(
                &b.fqn,
                &b.owner.identity,
                b.view.kind,
                &b.view.span.file,
                b.view.span.line,
                b.view.span.column,
            ))
    });
    symbols
}

fn collect<'a>(
    view: &'a DeclarationView,
    parent: &str,
    owner: &'a ProjectDeclarations,
    all: bool,
    ancestors: &[&'a DeclarationView],
    output: &mut Vec<Symbol<'a>>,
) {
    if !view.visible(all) {
        return;
    }
    let fqn = format!("{parent}.{}", view.name);
    output.push(Symbol {
        fqn: fqn.clone(),
        view,
        ancestors: ancestors.to_vec(),
        owner,
    });
    let mut parents = ancestors.to_vec();
    parents.push(view);
    let member_parent = if view.kind == "implementation" {
        view.target.as_deref().unwrap_or(&fqn)
    } else {
        &fqn
    };
    for child in &view.children {
        collect(child, member_parent, owner, all, &parents, output);
    }
}

pub(super) fn packages(workspace: Option<&ResolvedWorkspace>) -> String {
    let mut text = "prelude (embedded)\n    standard.prelude\n".to_owned();
    if let Some(workspace) = workspace {
        for project in &workspace.projects {
            text.push_str(&format!(
                "{} ({})\n",
                project.name,
                if workspace.is_local(project) {
                    "local"
                } else {
                    "dependency"
                }
            ));
            for package in &project.packages {
                text.push_str(&format!("    {}\n", package.path));
            }
            for component in &project.components {
                text.push_str(&format!("    {} (generated)\n", component.dovetail_package));
            }
        }
    }
    text
}

pub(super) fn query(
    projects: &[ProjectDeclarations],
    args: &QueryArgs,
    workspace: Option<&ResolvedWorkspace>,
) -> anyhow::Result<(String, bool)> {
    let symbols = symbols(projects, args.all);
    match &args.command {
        QueryCommand::Search {
            text,
            package,
            limit,
            offset,
        } => Ok((
            search(
                &symbols,
                text,
                package.as_deref(),
                *limit as usize,
                *offset as usize,
            ),
            true,
        )),
        QueryCommand::Package { name: Some(name) } => package(&symbols, name, workspace),
        QueryCommand::Definition { name } => definition(&symbols, name, args.all),
        QueryCommand::Package { name: None } => unreachable!("metadata-only package listing"),
    }
}

fn summary(symbols: &[&Symbol<'_>]) -> String {
    let first = symbols[0];
    if first.view.kind == "implementation" {
        return format!(
            "{}  [implementation]  {}  {}:{}\n",
            first.view.header.lines().next().unwrap_or(&first.view.name),
            first.owner.name,
            first.view.span.file,
            first.view.span.line
        );
    }
    let kinds: BTreeSet<_> = symbols.iter().map(|s| s.view.kind).collect();
    let locations: BTreeSet<_> = symbols
        .iter()
        .map(|s| format!("{}:{}", s.view.span.file, s.view.span.line))
        .collect();
    format!(
        "{}  [{}]  {}  {}\n",
        first.fqn,
        kinds.into_iter().collect::<Vec<_>>().join(" + "),
        first.owner.name,
        locations.into_iter().collect::<Vec<_>>().join(", ")
    )
}

fn search(
    symbols: &[Symbol<'_>],
    text: &str,
    package: Option<&str>,
    limit: usize,
    offset: usize,
) -> String {
    let text = text.to_lowercase();
    let mut groups: BTreeMap<(usize, &str, &str), Vec<&Symbol<'_>>> = BTreeMap::new();
    for symbol in symbols {
        if symbol.view.kind == "implementation" {
            continue;
        }
        if package.is_some_and(|p| p != symbol.view.package) {
            continue;
        }
        let fqn = symbol.fqn.to_lowercase();
        let name = symbol.view.name.to_lowercase();
        if !fqn.contains(&text) {
            continue;
        }
        let rank = if name == text || fqn == text {
            0
        } else if name.starts_with(&text) || fqn.starts_with(&text) {
            1
        } else {
            2
        };
        groups
            .entry((rank, &symbol.fqn, &symbol.owner.identity))
            .or_default()
            .push(symbol);
    }
    let total = groups.len();
    let mut result = String::new();
    for group in groups.values().skip(offset).take(limit) {
        result.push_str(&summary(group));
    }
    result.push_str(&format!(
        "// {} matches; showing {}..{}{}\n",
        total,
        offset.min(total),
        offset.saturating_add(limit).min(total),
        if offset.saturating_add(limit) < total {
            "; more results available with --offset"
        } else {
            ""
        }
    ));
    result
}

fn package(
    symbols: &[Symbol<'_>],
    name: &str,
    workspace: Option<&ResolvedWorkspace>,
) -> anyhow::Result<(String, bool)> {
    let prefix = format!("{name}.");
    let mut children = BTreeSet::new();
    let mut exists = name == "standard.prelude";
    let package_names = workspace
        .into_iter()
        .flat_map(|w| &w.projects)
        .flat_map(|p| &p.packages)
        .map(|p| p.path.to_string())
        .chain(symbols.iter().map(|s| s.view.package.clone()));
    for package in package_names {
        exists |= package == name;
        if let Some(suffix) = package.strip_prefix(&prefix) {
            children.insert(format!("{name}.{}", suffix.split('.').next().unwrap()));
        }
    }
    let matching: Vec<_> = symbols
        .iter()
        .filter(|s| s.view.package == name && s.ancestors.is_empty())
        .collect();
    let owners: BTreeSet<_> = matching.iter().map(|s| &s.owner.identity).collect();
    ensure_unambiguous(&matching, owners.len(), name)?;
    let mut result = format!("package {name}\n");
    for child in &children {
        result.push_str(&format!("{child}  [package]\n"));
    }
    let mut groups: BTreeMap<(&str, &str), Vec<&Symbol<'_>>> = BTreeMap::new();
    for symbol in matching {
        groups
            .entry((&symbol.fqn, &symbol.owner.identity))
            .or_default()
            .push(symbol);
    }
    for group in groups.values() {
        result.push_str(&summary(group));
    }
    if !exists && children.is_empty() {
        result.push_str("// Package not found in this dependency context.\n");
    }
    Ok((result, exists || !children.is_empty()))
}

fn ensure_unambiguous(symbols: &[&Symbol<'_>], owners: usize, name: &str) -> anyhow::Result<()> {
    if owners <= 1 {
        return Ok(());
    }
    let candidates: BTreeSet<_> = symbols
        .iter()
        .map(|s| format!("{} ({})", s.owner.name, s.owner.identity))
        .collect();
    anyhow::bail!(
        "'{name}' has multiple owners; select a consumer context with --project:\n{}",
        candidates.into_iter().collect::<Vec<_>>().join("\n")
    );
}

fn definition(symbols: &[Symbol<'_>], name: &str, all: bool) -> anyhow::Result<(String, bool)> {
    let mut matching: Vec<_> = symbols.iter().filter(|s| s.fqn == name).collect();
    matching.sort_by_key(|s| s.view.kind == "module");
    let implementation_targets: BTreeSet<_> = matching
        .iter()
        .flat_map(|s| &s.ancestors)
        .filter(|v| v.kind == "implementation")
        .filter_map(|v| v.target.as_deref())
        .collect();
    let owner_symbols: Vec<_> = if implementation_targets.is_empty() {
        matching.clone()
    } else {
        symbols
            .iter()
            .filter(|s| implementation_targets.contains(s.fqn.as_str()) && s.ancestors.is_empty())
            .collect()
    };
    let owners: BTreeSet<_> = owner_symbols.iter().map(|s| &s.owner.identity).collect();
    ensure_unambiguous(&owner_symbols, owners.len(), name)?;
    if matching.is_empty() {
        return Ok((format!("// Definition not found: {name}\n"), false));
    }
    let mut output =
        "// Declaration view: executable bodies and initializers omitted.\n".to_owned();
    for symbol in &matching {
        render_symbol(symbol, all, &mut output);
    }
    let is_type = matching.iter().any(|s| {
        matches!(
            s.view.kind,
            "record" | "enum" | "class" | "newtype" | "builtin"
        )
    });
    if is_type {
        for related in symbols
            .iter()
            .filter(|s| s.ancestors.is_empty() && s.view.target.as_deref() == Some(name))
        {
            match related.view.kind {
                "implementation" => render_symbol(related, all, &mut output),
                "extension" => output.push_str(&format!("\n// Named extension: {}\n// import {}\n// dovetail query definition {}\n// {}\n", related.fqn, related.fqn, related.fqn, related.view.header)),
                _ => {}
            }
        }
    }
    Ok((output, true))
}

fn render_symbol(symbol: &Symbol<'_>, all: bool, output: &mut String) {
    output.push_str(&format!(
        "\n// {} — {}:{}\npackage {}\n",
        symbol.owner.name, symbol.view.span.file, symbol.view.span.line, symbol.view.package
    ));
    for import in &symbol.view.imports {
        output.push_str(import);
        output.push('\n');
    }
    output.push('\n');
    for (depth, ancestor) in symbol.ancestors.iter().enumerate() {
        for line in ancestor.header.lines() {
            output.push_str(&format!("{}{line}\n", "    ".repeat(depth)));
        }
    }
    symbol
        .view
        .render(all || symbol.owner.local, symbol.ancestors.len(), output);
    for (depth, ancestor) in symbol.ancestors.iter().enumerate().rev() {
        if ancestor.kind == "case" && !ancestor.children.is_empty() {
            output.push_str(&format!("{}}}\n", "    ".repeat(depth)));
        }
    }
}
