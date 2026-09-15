use std::path::Path;

use tower_lsp::lsp_types::{DocumentSymbol, Location, SymbolKind};

use crate::parser::ast::*;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::TypedModule;

use super::diagnostics::{clamp_range, file_path_to_uri, span_to_range};

/// Convert a parsed source file into a list of document symbols for the outline view.
#[allow(deprecated)]
pub fn source_file_to_document_symbols(source_file: &SourceFile) -> Vec<DocumentSymbol> {
    source_file
        .declarations
        .iter()
        .map(declaration_to_symbol)
        .collect()
}

#[allow(deprecated)]
fn declaration_to_symbol(decl: &Declaration) -> DocumentSymbol {
    match decl {
        Declaration::Function(f) => function_symbol(f),
        Declaration::GlobalVar(g) => global_var_symbol(g),
        Declaration::Record(r) => record_symbol(r),
        Declaration::Enum(e) => enum_symbol(e),
        Declaration::Trait(t) => trait_symbol(t),
        Declaration::Class(c) => class_symbol(c),
        Declaration::Module(m) => module_symbol(m),
        Declaration::Extension(e) => extension_symbol(e),
        Declaration::Implement(i) => implement_symbol(i),
        Declaration::Newtype(n) => newtype_symbol(n),
        Declaration::TypeAlias(t) => type_alias_symbol(t),
        Declaration::Test(t) => test_symbol(t),
    }
}

#[allow(deprecated)]
fn function_symbol(f: &FunctionDecl) -> DocumentSymbol {
    let detail = f.return_type.as_ref().map(format_type_expr);
    let range = span_to_range(&f.span);
    let selection_range = clamp_range(span_to_range(&f.name.span), range);
    DocumentSymbol {
        name: f.name.value.clone(),
        detail,
        kind: SymbolKind::FUNCTION,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

#[allow(deprecated)]
fn global_var_symbol(g: &GlobalVarDecl) -> DocumentSymbol {
    let kind = if g.mutable {
        SymbolKind::VARIABLE
    } else {
        SymbolKind::CONSTANT
    };
    let detail = g.type_annotation.as_ref().map(format_type_expr);
    let range = span_to_range(&g.span);
    let selection_range = clamp_range(span_to_range(&g.name.span), range);
    DocumentSymbol {
        name: g.name.value.clone(),
        detail,
        kind,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

#[allow(deprecated)]
fn record_symbol(r: &RecordDecl) -> DocumentSymbol {
    let children: Vec<DocumentSymbol> = r
        .fields
        .iter()
        .map(|field| {
            let range = span_to_range(&field.span);
            let selection_range = clamp_range(span_to_range(&field.name.span), range);
            DocumentSymbol {
                name: field.name.value.clone(),
                detail: Some(format_type_expr(&field.type_annotation)),
                kind: SymbolKind::FIELD,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: None,
            }
        })
        .collect();
    let range = span_to_range(&r.span);
    let selection_range = clamp_range(span_to_range(&r.name.span), range);
    DocumentSymbol {
        name: r.name.value.clone(),
        detail: None,
        kind: SymbolKind::STRUCT,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn enum_symbol(e: &EnumDecl) -> DocumentSymbol {
    let children: Vec<DocumentSymbol> = e
        .variants
        .iter()
        .map(|variant| {
            let range = span_to_range(&variant.span);
            let selection_range = clamp_range(span_to_range(&variant.name.span), range);
            DocumentSymbol {
                name: variant.name.value.clone(),
                detail: None,
                kind: SymbolKind::ENUM_MEMBER,
                tags: None,
                deprecated: None,
                range,
                selection_range,
                children: None,
            }
        })
        .collect();
    let range = span_to_range(&e.span);
    let selection_range = clamp_range(span_to_range(&e.name.span), range);
    DocumentSymbol {
        name: e.name.value.clone(),
        detail: None,
        kind: SymbolKind::ENUM,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn trait_symbol(t: &TraitDecl) -> DocumentSymbol {
    let mut children = Vec::new();
    for method in &t.methods {
        let range = span_to_range(&method.span);
        let selection_range = clamp_range(span_to_range(&method.name.span), range);
        children.push(DocumentSymbol {
            name: method.name.value.clone(),
            detail: method.return_type.as_ref().map(format_type_expr),
            kind: SymbolKind::METHOD,
            tags: None,
            deprecated: None,
            range,
            selection_range,
            children: None,
        });
    }
    for prop in &t.properties {
        let range = span_to_range(&prop.span);
        let selection_range = clamp_range(span_to_range(&prop.name.span), range);
        children.push(DocumentSymbol {
            name: prop.name.value.clone(),
            detail: Some(format_type_expr(&prop.return_type)),
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range,
            selection_range,
            children: None,
        });
    }
    let range = span_to_range(&t.span);
    let selection_range = clamp_range(span_to_range(&t.name.span), range);
    DocumentSymbol {
        name: t.name.value.clone(),
        detail: None,
        kind: SymbolKind::INTERFACE,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn class_symbol(c: &ClassDecl) -> DocumentSymbol {
    let mut children = Vec::new();

    // Constructor params as properties
    for param in &c.params {
        let range = span_to_range(&param.span);
        let selection_range = clamp_range(span_to_range(&param.name.span), range);
        children.push(DocumentSymbol {
            name: param.name.value.clone(),
            detail: Some(format_type_expr(&param.type_annotation)),
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range,
            selection_range,
            children: None,
        });
    }

    // Body members
    for member in &c.body {
        match member {
            ClassMember::Method(f) => children.push(function_symbol(f)),
            ClassMember::Property(p) => {
                let range = span_to_range(&p.span);
                let selection_range = clamp_range(span_to_range(&p.name.span), range);
                children.push(DocumentSymbol {
                    name: p.name.value.clone(),
                    detail: Some(format_type_expr(&p.return_type)),
                    kind: SymbolKind::PROPERTY,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range,
                    children: None,
                });
            }
            ClassMember::LetBinding(l) => {
                let kind = if l.mutable {
                    SymbolKind::VARIABLE
                } else {
                    SymbolKind::CONSTANT
                };
                let range = span_to_range(&l.span);
                let selection_range = clamp_range(span_to_range(&l.name.span), range);
                children.push(DocumentSymbol {
                    name: l.name.value.clone(),
                    detail: l.type_annotation.as_ref().map(format_type_expr),
                    kind,
                    tags: None,
                    deprecated: None,
                    range,
                    selection_range,
                    children: None,
                });
            }
            ClassMember::Expression(_) => {}
        }
    }

    let range = span_to_range(&c.span);
    let selection_range = clamp_range(span_to_range(&c.name.span), range);
    DocumentSymbol {
        name: c.name.value.clone(),
        detail: None,
        kind: SymbolKind::CLASS,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn module_symbol(m: &ModuleDecl) -> DocumentSymbol {
    let mut children = Vec::new();
    for f in &m.functions {
        children.push(function_symbol(f));
    }
    for p in &m.properties {
        let range = span_to_range(&p.span);
        let selection_range = clamp_range(span_to_range(&p.name.span), range);
        children.push(DocumentSymbol {
            name: p.name.value.clone(),
            detail: Some(format_type_expr(&p.return_type)),
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range,
            selection_range,
            children: None,
        });
    }
    for g in &m.globals {
        children.push(global_var_symbol(g));
    }
    for t in &m.tests {
        children.push(test_symbol(t));
    }
    let range = span_to_range(&m.span);
    let selection_range = clamp_range(span_to_range(&m.name.span), range);
    DocumentSymbol {
        name: m.name.value.clone(),
        detail: None,
        kind: SymbolKind::MODULE,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn extension_symbol(e: &ExtensionDecl) -> DocumentSymbol {
    let mut children = Vec::new();
    for m in &e.methods {
        children.push(function_symbol(m));
    }
    for p in &e.properties {
        let range = span_to_range(&p.span);
        let selection_range = clamp_range(span_to_range(&p.name.span), range);
        children.push(DocumentSymbol {
            name: p.name.value.clone(),
            detail: Some(format_type_expr(&p.return_type)),
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range,
            selection_range,
            children: None,
        });
    }
    let detail = Some(format!("for {}", format_type_expr(&e.for_type)));
    let range = span_to_range(&e.span);
    let selection_range = clamp_range(span_to_range(&e.name.span), range);
    DocumentSymbol {
        name: e.name.value.clone(),
        detail,
        kind: SymbolKind::NAMESPACE,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn implement_symbol(i: &ImplementDecl) -> DocumentSymbol {
    let mut children = Vec::new();
    for m in &i.methods {
        children.push(function_symbol(m));
    }
    for p in &i.properties {
        let range = span_to_range(&p.span);
        let selection_range = clamp_range(span_to_range(&p.name.span), range);
        children.push(DocumentSymbol {
            name: p.name.value.clone(),
            detail: Some(format_type_expr(&p.return_type)),
            kind: SymbolKind::PROPERTY,
            tags: None,
            deprecated: None,
            range,
            selection_range,
            children: None,
        });
    }
    let detail = Some(format!("for {}", format_type_expr(&i.for_type)));
    let range = span_to_range(&i.span);
    let selection_range = clamp_range(span_to_range(&i.trait_name.span), range);
    DocumentSymbol {
        name: i.trait_name.value.clone(),
        detail,
        kind: SymbolKind::NAMESPACE,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: if children.is_empty() {
            None
        } else {
            Some(children)
        },
    }
}

#[allow(deprecated)]
fn newtype_symbol(n: &NewtypeDecl) -> DocumentSymbol {
    let detail = Some(format_type_expr(&n.inner_type));
    let range = span_to_range(&n.span);
    let selection_range = clamp_range(span_to_range(&n.name.span), range);
    DocumentSymbol {
        name: n.name.value.clone(),
        detail,
        kind: SymbolKind::STRUCT,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

#[allow(deprecated)]
fn type_alias_symbol(t: &TypeAliasDecl) -> DocumentSymbol {
    let detail = Some(format_type_expr(&t.type_expr));
    let range = span_to_range(&t.span);
    let selection_range = clamp_range(span_to_range(&t.name.span), range);
    DocumentSymbol {
        name: t.name.value.clone(),
        detail,
        kind: SymbolKind::TYPE_PARAMETER,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

#[allow(deprecated)]
fn test_symbol(t: &TestDecl) -> DocumentSymbol {
    let range = span_to_range(&t.span);
    let selection_range = clamp_range(span_to_range(&t.name.span), range);
    DocumentSymbol {
        name: t.name.value.clone(),
        detail: None,
        kind: SymbolKind::EVENT,
        tags: None,
        deprecated: None,
        range,
        selection_range,
        children: None,
    }
}

/// Format a TypeExpr as a human-readable string for the detail field.
fn format_type_expr(type_expr: &TypeExpr) -> String {
    match type_expr {
        TypeExpr::Named(named) => {
            let mut s = named.name.value.clone();
            if !named.type_args.is_empty() {
                s.push('<');
                for (i, arg) in named.type_args.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&format_type_expr(arg));
                }
                s.push('>');
            }
            s
        }
        TypeExpr::TupleExtend(left, right, _) => {
            let operand = |ty: &TypeExpr| {
                let text = format_type_expr(ty);
                if matches!(ty, TypeExpr::Function(..)) {
                    format!("({text})")
                } else {
                    text
                }
            };
            format!("({} ~ {})", operand(left), operand(right))
        }
        TypeExpr::Tuple(elements, _) => {
            let mut s = String::from("(");
            for (i, elem) in elements.iter().enumerate() {
                if i > 0 {
                    s.push_str(", ");
                }
                s.push_str(&format_type_expr(elem));
            }
            s.push(')');
            s
        }
        TypeExpr::Intersection(types) => {
            let parts: Vec<String> = types
                .iter()
                .map(|t| {
                    let mut s = t.name.value.clone();
                    if !t.type_args.is_empty() {
                        s.push('<');
                        for (i, arg) in t.type_args.iter().enumerate() {
                            if i > 0 {
                                s.push_str(", ");
                            }
                            s.push_str(&format_type_expr(arg));
                        }
                        s.push('>');
                    }
                    s
                })
                .collect();
            parts.join(" and ")
        }
        TypeExpr::Function(params, ret, _) => {
            let mut s = String::new();
            if params.len() == 1 {
                s.push_str(&format_type_expr(&params[0]));
            } else {
                s.push('(');
                for (i, p) in params.iter().enumerate() {
                    if i > 0 {
                        s.push_str(", ");
                    }
                    s.push_str(&format_type_expr(p));
                }
                s.push(')');
            }
            s.push_str(" => ");
            s.push_str(&format_type_expr(ret));
            s
        }
    }
}

/// Return workspace-wide symbols matching a query string.
/// Iterates over all functions, globals, and types in the cached TypedModule + Registry.
#[allow(deprecated)]
pub fn workspace_symbols(
    query: &str,
    typed_module: &TypedModule,
    registry: &Registry,
    workspace_root: &Path,
) -> Vec<WorkspaceSymbolResponse> {
    let query_lower = query.to_lowercase();
    let mut results = Vec::new();

    // Functions from typed module
    for func in typed_module.functions.values() {
        if func.span.file.starts_with('<') {
            continue;
        }
        let display_name = &func.display_name;
        let short_name = func.name.0.split('$').next().unwrap_or(&func.name.0);
        let name = short_name.rsplit('.').next().unwrap_or(short_name);
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &func.span.file) {
            results.push(WorkspaceSymbolResponse {
                name: display_name.clone(),
                kind: SymbolKind::FUNCTION,
                location: Location {
                    uri,
                    range: span_to_range(&func.span),
                },
            });
        }
    }

    // Globals from typed module
    for global in typed_module.globals.values() {
        if global.span.file.starts_with('<') {
            continue;
        }
        let name_str = global.name.0.rsplit('.').next().unwrap_or(&global.name.0);
        if !query_lower.is_empty() && !name_str.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &global.span.file) {
            let kind = if global.mutable {
                SymbolKind::VARIABLE
            } else {
                SymbolKind::CONSTANT
            };
            results.push(WorkspaceSymbolResponse {
                name: name_str.to_string(),
                kind,
                location: Location {
                    uri,
                    range: span_to_range(&global.span),
                },
            });
        }
    }

    // Record types
    for (fqn, sig) in registry.all_record_types() {
        if sig.source_file.starts_with('<') {
            continue;
        }
        let name = &fqn.symbol.0;
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &sig.source_file) {
            results.push(WorkspaceSymbolResponse {
                name: name.clone(),
                kind: SymbolKind::STRUCT,
                location: Location {
                    uri,
                    range: span_to_range(&sig.span),
                },
            });
        }
    }

    // Enum types
    for (fqn, sig) in registry.all_enum_types() {
        if sig.source_file.starts_with('<') {
            continue;
        }
        let name = &fqn.symbol.0;
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &sig.source_file) {
            results.push(WorkspaceSymbolResponse {
                name: name.clone(),
                kind: SymbolKind::ENUM,
                location: Location {
                    uri,
                    range: span_to_range(&sig.span),
                },
            });
        }
    }

    // Class types
    for (fqn, sig) in registry.all_class_types() {
        if sig.source_file.starts_with('<') {
            continue;
        }
        let name = &fqn.symbol.0;
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &sig.source_file) {
            results.push(WorkspaceSymbolResponse {
                name: name.clone(),
                kind: SymbolKind::CLASS,
                location: Location {
                    uri,
                    range: span_to_range(&sig.span),
                },
            });
        }
    }

    // Traits
    for (fqn, sig) in registry.all_traits() {
        if sig.source_file.starts_with('<') {
            continue;
        }
        let name = &fqn.symbol.0;
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &sig.source_file) {
            results.push(WorkspaceSymbolResponse {
                name: name.clone(),
                kind: SymbolKind::INTERFACE,
                location: Location {
                    uri,
                    range: span_to_range(&sig.span),
                },
            });
        }
    }

    // Modules
    for (fqn, sig) in registry.all_modules() {
        if sig.source_file.starts_with('<') {
            continue;
        }
        let name = &fqn.symbol.0;
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &sig.source_file) {
            results.push(WorkspaceSymbolResponse {
                name: name.clone(),
                kind: SymbolKind::MODULE,
                location: Location {
                    uri,
                    range: Default::default(), // ModuleInfo doesn't have a span
                },
            });
        }
    }

    // Newtype types
    for (fqn, sig) in registry.all_newtype_types() {
        if sig.source_file.starts_with('<') {
            continue;
        }
        let name = &fqn.symbol.0;
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &sig.source_file) {
            results.push(WorkspaceSymbolResponse {
                name: name.clone(),
                kind: SymbolKind::STRUCT,
                location: Location {
                    uri,
                    range: Default::default(), // Newtype doesn't have a span field
                },
            });
        }
    }

    // Type aliases
    for (fqn, sig) in registry.all_type_aliases() {
        if sig.source_file.starts_with('<') {
            continue;
        }
        let name = &fqn.symbol.0;
        if !query_lower.is_empty() && !name.to_lowercase().contains(&query_lower) {
            continue;
        }
        if let Some(uri) = file_path_to_uri(workspace_root, &sig.source_file) {
            results.push(WorkspaceSymbolResponse {
                name: name.clone(),
                kind: SymbolKind::TYPE_PARAMETER,
                location: Location {
                    uri,
                    range: Default::default(), // TypeAlias doesn't have a span field
                },
            });
        }
    }

    results
}

/// Intermediate workspace symbol result (converted to SymbolInformation by the handler).
pub struct WorkspaceSymbolResponse {
    pub name: String,
    pub kind: SymbolKind,
    pub location: Location,
}
