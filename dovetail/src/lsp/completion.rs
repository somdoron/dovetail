use std::collections::BTreeSet;

use tower_lsp::lsp_types::*;

use crate::common::types::{Fqn, PackagePath, SymbolName, Visibility};
use crate::parser::ast::SourceFile;
use crate::typechecker::imports::{ImportScope, ImportTarget};
use crate::typechecker::registry::{FunctionSignature, Registry, VariantPayload};
use crate::typechecker::types::Type;

use super::scope::VisibleLocals;

/// Generate completions for a bare identifier (not after a dot).
///
/// Priority order via `sort_text`:
/// - `"0"` — locals + params
/// - `"1"` — same-package functions/types/globals
/// - `"2"` — imported symbols
/// - `"3"` — prelude symbols
pub fn scope_completion(
    visible: &VisibleLocals,
    registry: &Registry,
    import_scope: &ImportScope,
    file_package: &PackagePath,
    prefix: &str,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();
    let prefix_lower = prefix.to_lowercase();

    // Locals and params (highest priority)
    for local in &visible.locals {
        let name = local.name.0.clone();
        if !matches_prefix(&name, &prefix_lower) {
            continue;
        }
        if seen.insert(name.clone()) {
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(format!("{}", local.ty)),
                sort_text: Some(format!("0{name}")),
                ..Default::default()
            });
        }
    }

    for param in &visible.params {
        let name = param.name.clone();
        if !matches_prefix(&name, &prefix_lower) {
            continue;
        }
        if seen.insert(name.clone()) {
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::VARIABLE),
                detail: Some(format!("{}", param.ty)),
                sort_text: Some(format!("0{name}")),
                ..Default::default()
            });
        }
    }

    // Same-package functions
    for (fqn, sigs) in registry.all_functions() {
        if fqn.package != *file_package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, &prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        let detail = sigs.first().map(format_function_detail);
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail,
            sort_text: Some(format!("1{name}")),
            insert_text: Some(make_function_snippet(name, sigs.first())),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
    }

    // Same-package globals
    for (fqn, sig) in registry.all_globals() {
        if fqn.package != *file_package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, &prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(format!("{}", sig.ty)),
            sort_text: Some(format!("1{name}")),
            ..Default::default()
        });
    }

    // Same-package types (records, enums, classes, traits, newtypes, type aliases)
    add_type_completions(
        registry,
        file_package,
        &prefix_lower,
        "1",
        &mut seen,
        &mut items,
    );

    // Imported symbols
    for import in &import_scope.imports {
        let name = &import.local_name;
        if !matches_prefix(name, &prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        let (kind, detail) = match &import.target {
            ImportTarget::Symbol(fqn) => {
                if let Some(sigs) = registry.get_functions(fqn) {
                    (
                        CompletionItemKind::FUNCTION,
                        sigs.first().map(format_function_detail),
                    )
                } else if registry.get_record_type(fqn).is_some() {
                    (CompletionItemKind::STRUCT, Some(format!("record {name}")))
                } else if registry.get_enum_type(fqn).is_some() {
                    (CompletionItemKind::ENUM, Some(format!("enum {name}")))
                } else if registry.get_class_type(fqn).is_some() {
                    (CompletionItemKind::CLASS, Some(format!("class {name}")))
                } else if let Some(trait_sig) = registry.get_trait(fqn) {
                    let keyword = if trait_sig.is_interface {
                        "interface"
                    } else {
                        "trait"
                    };
                    (
                        CompletionItemKind::INTERFACE,
                        Some(format!("{keyword} {name}")),
                    )
                } else {
                    (CompletionItemKind::VALUE, None)
                }
            }
            ImportTarget::Package(_) => (CompletionItemKind::MODULE, Some("package".to_string())),
            ImportTarget::Extension(_) => {
                (CompletionItemKind::MODULE, Some("extension".to_string()))
            }
        };
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(kind),
            detail,
            sort_text: Some(format!("2{name}")),
            ..Default::default()
        });
    }

    // Prelude symbols (standard.prelude package)
    let prelude_pkg = PackagePath(vec!["standard".into(), "prelude".into()]);
    if *file_package != prelude_pkg {
        for (fqn, sigs) in registry.all_functions() {
            if fqn.package != prelude_pkg {
                continue;
            }
            let name = &fqn.symbol.0;
            if !matches_prefix(name, &prefix_lower) || !seen.insert(name.clone()) {
                continue;
            }
            let detail = sigs.first().map(format_function_detail);
            items.push(CompletionItem {
                label: name.clone(),
                kind: Some(CompletionItemKind::FUNCTION),
                detail,
                sort_text: Some(format!("3{name}")),
                ..Default::default()
            });
        }

        add_type_completions(
            registry,
            &prelude_pkg,
            &prefix_lower,
            "3",
            &mut seen,
            &mut items,
        );
    }

    items
}

/// Generate dot-completion items based on the receiver type.
pub fn dot_completion(
    receiver_type: &Type,
    registry: &Registry,
    import_scope: &ImportScope,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let type_fqn = match receiver_type.try_to_fqn() {
        Some(fqn) => fqn,
        None => return items,
    };

    match receiver_type {
        Type::Record(fqn, _) | Type::GenericRecord { fqn, .. } => {
            // Fields
            if let Some(info) = registry.get_record_type(fqn) {
                for (field_name, field_ty) in &info.fields {
                    items.push(CompletionItem {
                        label: field_name.clone(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!("{field_ty}")),
                        sort_text: Some(format!("0{field_name}")),
                        ..Default::default()
                    });
                }
            }
        }
        Type::Class(fqn, _) | Type::GenericClass { fqn, .. } => {
            // Instance fields + methods (walk class hierarchy)
            add_class_members(fqn, registry, &mut items);
        }
        Type::Newtype(fqn, _) => {
            if let Some(sig) = registry.get_newtype_type(fqn)
                && !sig.inner_private
            {
                items.push(CompletionItem {
                    label: "value".to_string(),
                    kind: Some(CompletionItemKind::PROPERTY),
                    detail: Some(format!("{}", sig.inner_type)),
                    sort_text: Some("0value".to_string()),
                    ..Default::default()
                });
            }
        }
        Type::InterfaceObject { traits, .. } => {
            for component in traits {
                if let Some(trait_sig) = registry.get_trait(&component.trait_fqn) {
                    for method in &trait_sig.methods {
                        let detail =
                            format_trait_method_detail(&method.params, &method.return_type);
                        let snippet = make_method_snippet(&method.name, &method.params);
                        items.push(CompletionItem {
                            label: method.name.clone(),
                            kind: Some(CompletionItemKind::METHOD),
                            detail: Some(detail),
                            sort_text: Some(format!("0{}", method.name)),
                            insert_text: Some(snippet),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            ..Default::default()
                        });
                    }
                    for prop in &trait_sig.properties {
                        items.push(CompletionItem {
                            label: prop.name.clone(),
                            kind: Some(CompletionItemKind::PROPERTY),
                            detail: Some(format!("{}", prop.return_type)),
                            sort_text: Some(format!("0{}", prop.name)),
                            ..Default::default()
                        });
                    }
                }
            }
        }
        _ => {}
    }

    // Trait impl methods
    for block in registry.find_impl_blocks_for_type(&type_fqn) {
        for method in block.methods.iter().chain(block.properties.iter()) {
            let snippet = make_method_snippet(&method.name.0, &method.params);
            items.push(CompletionItem {
                label: method.name.0.clone(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format!(
                    "({}) -> {}",
                    method
                        .params
                        .iter()
                        .map(|(n, t)| format!("{}: {}", n, t))
                        .collect::<Vec<_>>()
                        .join(", "),
                    method.return_type
                )),
                sort_text: Some(format!("1{}", method.name.0)),
                insert_text: Some(snippet),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
        }
    }

    // Extension methods from imports
    for ext_block in &import_scope.extension_blocks {
        if ext_block.for_type.to_fqn() != type_fqn {
            continue;
        }
        for method in ext_block.methods.iter().chain(ext_block.properties.iter()) {
            let snippet = make_method_snippet(&method.name.0, &method.params);
            items.push(CompletionItem {
                label: method.name.0.clone(),
                kind: Some(CompletionItemKind::METHOD),
                detail: Some(format_trait_method_detail(
                    &method.params,
                    &method.return_type,
                )),
                sort_text: Some(format!("2{}", method.name.0)),
                insert_text: Some(snippet),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
        }
    }

    // Deduplicate by label
    let mut seen = BTreeSet::new();
    items.retain(|item| seen.insert(item.label.clone()));

    items
}

/// Generate dot-completion items for a qualified name (e.g., `Async.`, `Math.`).
///
/// Resolves the identifier before the dot as an enum type or module and returns
/// appropriate completions (enum variants, module functions/globals).
pub fn qualified_dot_completion(
    name: &str,
    registry: &Registry,
    import_scope: &ImportScope,
    file_package: &PackagePath,
) -> Vec<CompletionItem> {
    let prelude_pkg = PackagePath(vec!["standard".into(), "prelude".into()]);

    // Resolve the name to an FQN by mimicking the typechecker's resolve_fqn:
    // 1. Check import scope for a symbol import
    // 2. Same-package lookup
    // 3. Prelude fallback
    let candidate_fqns: Vec<Fqn> = {
        let mut fqns = Vec::new();

        // 1. Import scope
        if let Some(resolved) = import_scope.lookup(name) {
            match &resolved.target {
                ImportTarget::Symbol(fqn) => fqns.push(fqn.clone()),
                ImportTarget::Package(pkg) => {
                    // Package import — show all public symbols from that package
                    return package_dot_completion(pkg, registry);
                }
                ImportTarget::Extension(_) => {}
            }
        }

        // 2. Same-package
        fqns.push(Fqn {
            package: file_package.clone(),
            symbol: SymbolName(name.to_string()),
        });

        // 3. Prelude
        if *file_package != prelude_pkg {
            fqns.push(Fqn {
                package: prelude_pkg,
                symbol: SymbolName(name.to_string()),
            });
        }

        fqns
    };

    // Try each candidate FQN as an enum or module
    for fqn in &candidate_fqns {
        if let Some(enum_sig) = registry.get_enum_type(fqn) {
            return enum_variant_completions(enum_sig);
        }
        if let Some(module_info) = registry.lookup_module(fqn) {
            return module_member_completions(module_info);
        }
    }

    Vec::new()
}

/// Generate completions for enum variants.
fn enum_variant_completions(
    enum_sig: &crate::typechecker::registry::EnumTypeSignature,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    for (variant_name, payload) in &enum_sig.variants {
        let detail = match payload {
            VariantPayload::None => variant_name.clone(),
            VariantPayload::Tuple(types) => {
                let type_strs: Vec<String> = types.iter().map(|t| format!("{t}")).collect();
                format!("{variant_name}({})", type_strs.join(", "))
            }
            VariantPayload::Record(fields) => {
                let field_strs: Vec<String> =
                    fields.iter().map(|(n, t)| format!("{n}: {t}")).collect();
                format!("{variant_name} {{ {} }}", field_strs.join(", "))
            }
        };
        let (insert_text, insert_text_format) = match payload {
            VariantPayload::None => (None, None),
            VariantPayload::Tuple(_) => (
                Some(format!("{variant_name}($1)")),
                Some(InsertTextFormat::SNIPPET),
            ),
            VariantPayload::Record(_) => (None, None),
        };
        items.push(CompletionItem {
            label: variant_name.clone(),
            kind: Some(CompletionItemKind::ENUM_MEMBER),
            detail: Some(detail),
            sort_text: Some(format!("0{variant_name}")),
            insert_text,
            insert_text_format,
            ..Default::default()
        });
    }
    items
}

/// Generate completions for module members (functions and globals).
fn module_member_completions(
    module_info: &crate::typechecker::registry::ModuleInfo,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();

    for (member_name, sigs) in &module_info.functions {
        if let Some(sig) = sigs.first() {
            if sig.visibility == Visibility::Private {
                continue;
            }
            let snippet = make_method_snippet(&member_name.0, &sig.params);
            items.push(CompletionItem {
                label: member_name.0.clone(),
                kind: Some(if sig.is_property {
                    CompletionItemKind::PROPERTY
                } else {
                    CompletionItemKind::FUNCTION
                }),
                detail: Some(format_function_detail(sig)),
                sort_text: Some(format!("0{}", member_name.0)),
                insert_text: Some(snippet),
                insert_text_format: Some(InsertTextFormat::SNIPPET),
                ..Default::default()
            });
        }
    }

    for (member_name, sig) in &module_info.globals {
        if sig.visibility == Visibility::Private {
            continue;
        }
        items.push(CompletionItem {
            label: member_name.0.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(format!("{}", sig.ty)),
            sort_text: Some(format!("0{}", member_name.0)),
            ..Default::default()
        });
    }

    items
}

/// Generate completions for all public symbols in a package (for package imports like `import a.utils as u`).
fn package_dot_completion(package: &PackagePath, registry: &Registry) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let mut seen = BTreeSet::new();

    for (fqn, sigs) in registry.all_functions() {
        if fqn.package != *package {
            continue;
        }
        if !sigs.iter().any(|s| s.visibility == Visibility::Public) {
            continue;
        }
        let name = &fqn.symbol.0;
        if !seen.insert(name.clone()) {
            continue;
        }
        let detail = sigs.first().map(format_function_detail);
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail,
            sort_text: Some(format!("0{name}")),
            insert_text: Some(make_function_snippet(name, sigs.first())),
            insert_text_format: Some(InsertTextFormat::SNIPPET),
            ..Default::default()
        });
    }

    add_type_completions(registry, package, "", "0", &mut seen, &mut items);

    items
}

/// Generate auto-import completions for symbols from other packages.
pub fn auto_import_completions(
    prefix: &str,
    registry: &Registry,
    visible_names: &BTreeSet<String>,
    import_insert_position: Position,
) -> Vec<CompletionItem> {
    let mut items = Vec::new();
    let prefix_lower = prefix.to_lowercase();

    if prefix.is_empty() {
        return items;
    }

    // Functions
    for (fqn, sigs) in registry.all_functions() {
        if !sigs.iter().any(|s| s.visibility == Visibility::Public) {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, &prefix_lower) || visible_names.contains(name) {
            continue;
        }
        let detail = sigs.first().map(format_function_detail);
        let import_text = format_import_fqn(fqn);
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::FUNCTION),
            detail,
            label_details: Some(CompletionItemLabelDetails {
                description: Some(format!("(import {})", import_text)),
                ..Default::default()
            }),
            sort_text: Some(format!("~{name}")),
            additional_text_edits: Some(vec![TextEdit {
                range: Range::new(import_insert_position, import_insert_position),
                new_text: format!("import {import_text}\n"),
            }]),
            ..Default::default()
        });
    }

    // Globals
    for (fqn, sig) in registry.all_globals() {
        if sig.visibility != Visibility::Public {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, &prefix_lower) || visible_names.contains(name) {
            continue;
        }
        let import_text = format_import_fqn(fqn);
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::VARIABLE),
            detail: Some(format!("{}", sig.ty)),
            label_details: Some(CompletionItemLabelDetails {
                description: Some(format!("(import {})", import_text)),
                ..Default::default()
            }),
            sort_text: Some(format!("~{name}")),
            additional_text_edits: Some(vec![TextEdit {
                range: Range::new(import_insert_position, import_insert_position),
                new_text: format!("import {import_text}\n"),
            }]),
            ..Default::default()
        });
    }

    // Types (records, enums, classes, traits, newtypes)
    add_auto_import_types(
        registry,
        &prefix_lower,
        visible_names,
        import_insert_position,
        &mut items,
    );

    // Limit results
    items.truncate(50);
    items
}

/// Compute the position where a new import statement should be inserted.
///
/// After the last import line, or after the package declaration + blank line if no imports.
pub fn compute_import_insert_position(source_file: &SourceFile) -> Position {
    // After last import
    if let Some(last_import) = source_file.imports.last() {
        return Position::new(last_import.span.end_line - 1 + 1, 0);
    }
    // After package declaration
    let pkg_end = source_file.package.span.end_line;
    Position::new(pkg_end, 0)
}

// ── helpers ──────────────────────────────────────────────────────────

fn matches_prefix(name: &str, prefix_lower: &str) -> bool {
    if prefix_lower.is_empty() {
        return true;
    }
    name.to_lowercase().starts_with(prefix_lower)
}

fn format_function_detail(sig: &FunctionSignature) -> String {
    let params: Vec<String> = sig
        .params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect();
    format!("({}) -> {}", params.join(", "), sig.return_type)
}

fn format_trait_method_detail(params: &[(String, Type)], return_type: &Type) -> String {
    let params: Vec<String> = params
        .iter()
        .map(|(name, ty)| format!("{name}: {ty}"))
        .collect();
    format!("({}) -> {}", params.join(", "), return_type)
}

fn make_function_snippet(name: &str, sig: Option<&FunctionSignature>) -> String {
    match sig {
        Some(s) if s.params.is_empty() => format!("{name}()"),
        Some(_) => format!("{name}($1)"),
        None => format!("{name}()"),
    }
}

fn make_method_snippet(name: &str, params: &[(String, Type)]) -> String {
    // Method params exclude `self` — the first param in trait impl sigs is the receiver,
    // but in our FunctionSignature the `self` param is not included for methods.
    if params.is_empty() {
        format!("{name}()")
    } else {
        format!("{name}($1)")
    }
}

fn format_import_fqn(fqn: &Fqn) -> String {
    let mut parts: Vec<&str> = fqn.package.0.iter().map(|s| s.as_str()).collect();
    parts.push(&fqn.symbol.0);
    parts.join(".")
}

fn add_type_completions(
    registry: &Registry,
    package: &PackagePath,
    prefix_lower: &str,
    sort_prefix: &str,
    seen: &mut BTreeSet<String>,
    items: &mut Vec<CompletionItem>,
) {
    for (fqn, _) in registry.all_record_types() {
        if fqn.package != *package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::STRUCT),
            sort_text: Some(format!("{sort_prefix}{name}")),
            ..Default::default()
        });
    }

    for (fqn, _) in registry.all_enum_types() {
        if fqn.package != *package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::ENUM),
            sort_text: Some(format!("{sort_prefix}{name}")),
            ..Default::default()
        });
    }

    for (fqn, _) in registry.all_class_types() {
        if fqn.package != *package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::CLASS),
            sort_text: Some(format!("{sort_prefix}{name}")),
            ..Default::default()
        });
    }

    for (fqn, _) in registry.all_traits() {
        if fqn.package != *package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::INTERFACE),
            sort_text: Some(format!("{sort_prefix}{name}")),
            ..Default::default()
        });
    }

    for (fqn, _) in registry.all_newtype_types() {
        if fqn.package != *package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::STRUCT),
            sort_text: Some(format!("{sort_prefix}{name}")),
            ..Default::default()
        });
    }

    for (fqn, _) in registry.all_modules() {
        if fqn.package != *package {
            continue;
        }
        let name = &fqn.symbol.0;
        if !matches_prefix(name, prefix_lower) || !seen.insert(name.clone()) {
            continue;
        }
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(CompletionItemKind::MODULE),
            sort_text: Some(format!("{sort_prefix}{name}")),
            ..Default::default()
        });
    }
}

fn add_auto_import_types(
    registry: &Registry,
    prefix_lower: &str,
    visible_names: &BTreeSet<String>,
    import_insert_position: Position,
    items: &mut Vec<CompletionItem>,
) {
    let type_iters: Vec<(&Fqn, CompletionItemKind)> = registry
        .all_record_types()
        .filter(|(_, sig)| sig.visibility == Visibility::Public)
        .map(|(fqn, _)| (fqn, CompletionItemKind::STRUCT))
        .chain(
            registry
                .all_enum_types()
                .filter(|(_, sig)| sig.visibility == Visibility::Public)
                .map(|(fqn, _)| (fqn, CompletionItemKind::ENUM)),
        )
        .chain(
            registry
                .all_class_types()
                .filter(|(_, sig)| sig.visibility == Visibility::Public)
                .map(|(fqn, _)| (fqn, CompletionItemKind::CLASS)),
        )
        .chain(
            registry
                .all_traits()
                .filter(|(_, sig)| sig.visibility == Visibility::Public)
                .map(|(fqn, _)| (fqn, CompletionItemKind::INTERFACE)),
        )
        .chain(
            registry
                .all_newtype_types()
                .filter(|(_, sig)| sig.visibility == Visibility::Public)
                .map(|(fqn, _)| (fqn, CompletionItemKind::STRUCT)),
        )
        .collect();

    for (fqn, kind) in type_iters {
        let name = &fqn.symbol.0;
        if !matches_prefix(name, prefix_lower) || visible_names.contains(name) {
            continue;
        }
        let import_text = format_import_fqn(fqn);
        items.push(CompletionItem {
            label: name.clone(),
            kind: Some(kind),
            label_details: Some(CompletionItemLabelDetails {
                description: Some(format!("(import {})", import_text)),
                ..Default::default()
            }),
            sort_text: Some(format!("~{name}")),
            additional_text_edits: Some(vec![TextEdit {
                range: Range::new(import_insert_position, import_insert_position),
                new_text: format!("import {import_text}\n"),
            }]),
            ..Default::default()
        });
    }
}

fn add_class_members(class_fqn: &Fqn, registry: &Registry, items: &mut Vec<CompletionItem>) {
    let mut current_fqn = Some(class_fqn.clone());
    let mut seen = BTreeSet::new();

    while let Some(fqn) = current_fqn {
        if let Some(class_sig) = registry.get_class_type(&fqn) {
            // Public/protected fields
            for field in &class_sig.fields {
                if field.visibility == Visibility::Private {
                    continue;
                }
                if seen.insert(field.name.clone()) {
                    items.push(CompletionItem {
                        label: field.name.clone(),
                        kind: Some(CompletionItemKind::FIELD),
                        detail: Some(format!("{}", field.ty)),
                        sort_text: Some(format!("0{}", field.name)),
                        ..Default::default()
                    });
                }
            }

            // Instance methods
            for (method_name, sigs) in &class_sig.instance_methods {
                if let Some(sig) = sigs.first() {
                    if sig.visibility == Visibility::Private {
                        continue;
                    }
                    if seen.insert(method_name.0.clone()) {
                        let snippet = make_method_snippet(&method_name.0, &sig.params);
                        items.push(CompletionItem {
                            label: method_name.0.clone(),
                            kind: Some(if sig.is_property {
                                CompletionItemKind::PROPERTY
                            } else {
                                CompletionItemKind::METHOD
                            }),
                            detail: Some(format_function_detail(sig)),
                            sort_text: Some(format!("0{}", method_name.0)),
                            insert_text: Some(snippet),
                            insert_text_format: Some(InsertTextFormat::SNIPPET),
                            ..Default::default()
                        });
                    }
                }
            }

            current_fqn = class_sig.parent_class.clone();
        } else {
            break;
        }
    }
}
