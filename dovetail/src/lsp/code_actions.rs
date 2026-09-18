use std::collections::HashMap;

use tower_lsp::lsp_types::*;

use crate::common::span::FilePath;
use crate::common::types::Visibility;
use crate::parser::ast::SourceFile;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind, TypedModule};

use super::completion::compute_import_insert_position;
use super::inlay_hints;

/// Generate auto-import code actions from diagnostics that mention unknown symbols.
///
/// Parses diagnostic messages for "unknown function", "unknown type", etc. patterns,
/// searches the registry for matching public symbols, and creates quick-fix code actions
/// to insert the appropriate import statement.
pub fn auto_import_code_actions(
    diagnostics: &[Diagnostic],
    registry: &Registry,
    source_file: &SourceFile,
    file_uri: &Url,
) -> Vec<CodeAction> {
    let mut actions = Vec::new();
    let import_pos = compute_import_insert_position(source_file);

    for diag in diagnostics {
        let Some(symbol_name) = extract_unknown_symbol(&diag.message) else {
            continue;
        };

        // Search for matching public functions
        for (fqn, sigs) in registry.all_functions() {
            if fqn.symbol.0 != symbol_name {
                continue;
            }
            if !sigs.iter().any(|s| s.visibility == Visibility::Public) {
                continue;
            }
            let import_text = format_import_fqn(fqn);
            let action = make_import_action(
                &format!("Import '{}'", import_text),
                &import_text,
                import_pos,
                file_uri,
                diag,
            );
            actions.push(action);
        }

        // Search for matching public types (records, enums, classes, traits, newtypes)
        for (fqn, sig) in registry.all_record_types() {
            if fqn.symbol.0 != symbol_name || sig.visibility != Visibility::Public {
                continue;
            }
            let import_text = format_import_fqn(fqn);
            actions.push(make_import_action(
                &format!("Import '{}'", import_text),
                &import_text,
                import_pos,
                file_uri,
                diag,
            ));
        }

        for (fqn, sig) in registry.all_enum_types() {
            if fqn.symbol.0 != symbol_name || sig.visibility != Visibility::Public {
                continue;
            }
            let import_text = format_import_fqn(fqn);
            actions.push(make_import_action(
                &format!("Import '{}'", import_text),
                &import_text,
                import_pos,
                file_uri,
                diag,
            ));
        }

        for (fqn, sig) in registry.all_class_types() {
            if fqn.symbol.0 != symbol_name || sig.visibility != Visibility::Public {
                continue;
            }
            let import_text = format_import_fqn(fqn);
            actions.push(make_import_action(
                &format!("Import '{}'", import_text),
                &import_text,
                import_pos,
                file_uri,
                diag,
            ));
        }

        for (fqn, sig) in registry.all_traits() {
            if fqn.symbol.0 != symbol_name || sig.visibility != Visibility::Public {
                continue;
            }
            let import_text = format_import_fqn(fqn);
            actions.push(make_import_action(
                &format!("Import '{}'", import_text),
                &import_text,
                import_pos,
                file_uri,
                diag,
            ));
        }

        // Search globals
        for (fqn, sig) in registry.all_globals() {
            if fqn.symbol.0 != symbol_name || sig.visibility != Visibility::Public {
                continue;
            }
            let import_text = format_import_fqn(fqn);
            actions.push(make_import_action(
                &format!("Import '{}'", import_text),
                &import_text,
                import_pos,
                file_uri,
                diag,
            ));
        }
    }

    actions
}

/// Extract an unknown symbol name from a diagnostic message.
///
/// Matches patterns like:
/// - "unknown function 'foo'"
/// - "unknown type 'Foo'"
/// - "cannot resolve 'foo'"
/// - "unknown identifier 'foo'"
fn extract_unknown_symbol(message: &str) -> Option<String> {
    // Look for 'name' pattern in various error message formats
    for prefix in [
        "unknown function '",
        "unknown type '",
        "unknown identifier '",
        "cannot resolve '",
        "undeclared function '",
        "undeclared variable '",
    ] {
        if let Some(rest) = message.strip_prefix(prefix)
            && let Some(end) = rest.find('\'')
        {
            return Some(rest[..end].to_string());
        }
    }

    // Also match "... 'name' ..." pattern anywhere in the message for
    // messages like "function 'foo' not found"
    if (message.contains("not found")
        || message.contains("unknown")
        || message.contains("undeclared"))
        && let Some(start) = message.find('\'')
    {
        let rest = &message[start + 1..];
        if let Some(end) = rest.find('\'') {
            let name = &rest[..end];
            // Only return if it looks like a valid identifier
            if !name.is_empty() && name.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Some(name.to_string());
            }
        }
    }

    None
}

fn format_import_fqn(fqn: &crate::common::types::Fqn) -> String {
    let mut parts: Vec<&str> = fqn.package.0.iter().map(|s| s.as_str()).collect();
    parts.push(&fqn.symbol.0);
    parts.join(".")
}

fn make_import_action(
    title: &str,
    import_text: &str,
    import_pos: Position,
    file_uri: &Url,
    diagnostic: &Diagnostic,
) -> CodeAction {
    let mut changes = HashMap::new();
    changes.insert(
        file_uri.clone(),
        vec![TextEdit {
            range: Range::new(import_pos, import_pos),
            new_text: format!("import {import_text}\n"),
        }],
    );

    CodeAction {
        title: title.to_string(),
        kind: Some(CodeActionKind::QUICKFIX),
        diagnostics: Some(vec![diagnostic.clone()]),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Generate "Add type annotation" code actions for let bindings without explicit types.
pub fn add_type_annotation_actions(
    typed_module: &TypedModule,
    file: &FilePath,
    range: &Range,
    document_content: &str,
    file_uri: &Url,
) -> Vec<CodeAction> {
    let mut actions = Vec::new();

    // Convert LSP range (0-indexed) to 1-indexed for span comparison
    let start_line = range.start.line + 1;
    let end_line = range.end.line + 1;

    for func in super::source_functions::values(typed_module) {
        if func.span.file != *file {
            continue;
        }
        collect_type_annotation_actions(
            &func.body,
            file,
            start_line,
            end_line,
            document_content,
            file_uri,
            &mut actions,
        );
    }

    for global in typed_module.globals.values() {
        if global.span.file != *file {
            continue;
        }
        collect_type_annotation_actions(
            &global.initializer,
            file,
            start_line,
            end_line,
            document_content,
            file_uri,
            &mut actions,
        );
    }

    for test in &typed_module.tests {
        if test.span.file != *file {
            continue;
        }
        collect_type_annotation_actions(
            &test.body,
            file,
            start_line,
            end_line,
            document_content,
            file_uri,
            &mut actions,
        );
    }

    actions
}

fn collect_type_annotation_actions(
    expr: &TypedExpr,
    file: &FilePath,
    start_line: u32,
    end_line: u32,
    document_content: &str,
    file_uri: &Url,
    actions: &mut Vec<CodeAction>,
) {
    if expr.span.file != *file {
        return;
    }
    if expr.span.end_line < start_line || expr.span.line > end_line {
        return;
    }

    if let TypedExprKind::Let {
        name,
        var_ty,
        value,
        ..
    } = &expr.kind
    {
        // Skip Error/Never types
        if !matches!(var_ty, Type::Error | Type::Never) {
            // Skip if already has explicit annotation
            if !inlay_hints::has_explicit_type_annotation(document_content, &expr.span, name) {
                // Skip obvious values
                if !is_obvious_type_value(&value.kind) {
                    let insert_pos =
                        inlay_hints::find_let_name_end(&expr.span, name, Some(document_content));
                    let annotation = format!(": {var_ty}");
                    let mut changes = HashMap::new();
                    changes.insert(
                        file_uri.clone(),
                        vec![TextEdit {
                            range: Range::new(insert_pos, insert_pos),
                            new_text: annotation.clone(),
                        }],
                    );
                    actions.push(CodeAction {
                        title: format!("Add type annotation{annotation}"),
                        kind: Some(CodeActionKind::REFACTOR),
                        edit: Some(WorkspaceEdit {
                            changes: Some(changes),
                            ..Default::default()
                        }),
                        ..Default::default()
                    });
                }
            }
        }
    }

    // Recurse into children
    walk_for_type_annotations(
        expr,
        file,
        start_line,
        end_line,
        document_content,
        file_uri,
        actions,
    );
}

fn is_obvious_type_value(kind: &TypedExprKind) -> bool {
    matches!(
        kind,
        TypedExprKind::BoolLiteral(_)
            | TypedExprKind::StringLiteral(_)
            | TypedExprKind::CharLiteral(_)
            | TypedExprKind::Int8Literal(_)
            | TypedExprKind::Int16Literal(_)
            | TypedExprKind::Int32Literal(_)
            | TypedExprKind::Int64Literal(_)
            | TypedExprKind::Uint8Literal(_)
            | TypedExprKind::Uint16Literal(_)
            | TypedExprKind::Uint32Literal(_)
            | TypedExprKind::Uint64Literal(_)
            | TypedExprKind::Uint128Literal(_)
            | TypedExprKind::Float32Literal(_)
            | TypedExprKind::Float64Literal(_)
            | TypedExprKind::RecordCreate { .. }
            | TypedExprKind::EnumCreate { .. }
            | TypedExprKind::ClassNew { .. }
    )
}

fn walk_for_type_annotations(
    expr: &TypedExpr,
    file: &FilePath,
    start_line: u32,
    end_line: u32,
    document_content: &str,
    file_uri: &Url,
    actions: &mut Vec<CodeAction>,
) {
    let recurse = |e: &TypedExpr, actions: &mut Vec<CodeAction>| {
        collect_type_annotation_actions(
            e,
            file,
            start_line,
            end_line,
            document_content,
            file_uri,
            actions,
        );
    };

    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            recurse(left, actions);
            recurse(right, actions);
        }
        TypedExprKind::UnaryOp { operand, .. } => recurse(operand, actions),
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                recurse(e, actions);
            }
        }
        TypedExprKind::NamedCall { call: message, .. } | TypedExprKind::Panic { message } => {
            recurse(message, actions)
        }
        TypedExprKind::Assert { condition, message } => {
            recurse(condition, actions);
            if let Some(msg) = message {
                recurse(msg, actions);
            }
        }
        TypedExprKind::Let { value, .. } => recurse(value, actions),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            recurse(value, actions);
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                recurse(arg, actions);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            recurse(condition, actions);
            recurse(then_branch, actions);
            if let Some(eb) = else_branch {
                recurse(eb, actions);
            }
        }
        TypedExprKind::While { condition, body } => {
            recurse(condition, actions);
            recurse(body, actions);
        }
        TypedExprKind::Match { subject, arms } => {
            recurse(subject, actions);
            for arm in arms {
                recurse(&arm.body, actions);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for element in elements {
                recurse(element, actions);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, field_expr) in fields {
                recurse(field_expr, actions);
            }
        }
        TypedExprKind::FieldAccess { object, .. } => recurse(object, actions),
        TypedExprKind::FieldAssign { object, value, .. } => {
            recurse(object, actions);
            recurse(value, actions);
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            recurse(object, actions);
            for (_, _, val) in overrides {
                recurse(val, actions);
            }
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                recurse(e, actions);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            recurse(object, actions);
            for arg in args {
                recurse(arg, actions);
            }
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            recurse(receiver, actions);
            for arg in args {
                recurse(arg, actions);
            }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            recurse(callee, actions);
            for arg in args {
                recurse(arg, actions);
            }
        }
        TypedExprKind::BoxToAny { inner }
        | TypedExprKind::TypeTest { value: inner, .. }
        | TypedExprKind::TypeCast { value: inner, .. }
        | TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner }
        | TypedExprKind::NewtypeCreate { value: inner }
        | TypedExprKind::NewtypeValue { value: inner } => recurse(inner, actions),
        TypedExprKind::LetDestructure { value, .. } => recurse(value, actions),
        TypedExprKind::Await { operand, .. }
        | TypedExprKind::Try { operand, .. }
        | TypedExprKind::Use { operand, .. } => {
            recurse(operand, actions);
        }
        TypedExprKind::Return { value, .. } => recurse(value, actions),
        TypedExprKind::ForLoop { iterable, body, .. } => {
            recurse(iterable, actions);
            recurse(body, actions);
        }
        TypedExprKind::AsyncBlock { body, .. } | TypedExprKind::Closure { body, .. } => {
            recurse(body, actions);
        }
        TypedExprKind::MethodRef { object, .. } => recurse(object, actions),
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for field in fields {
                recurse(field, actions);
            }
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                recurse(arg, actions);
            }
        }
        TypedExprKind::ImplFunctionRef { .. } | TypedExprKind::ExtFunctionRef { .. } => {}
        TypedExprKind::FunctionRef { .. }
        | TypedExprKind::VarRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::UnitLiteral
        | TypedExprKind::BoolLiteral(_)
        | TypedExprKind::StringLiteral(_)
        | TypedExprKind::CharLiteral(_)
        | TypedExprKind::Int8Literal(_)
        | TypedExprKind::Int16Literal(_)
        | TypedExprKind::Int32Literal(_)
        | TypedExprKind::Int64Literal(_)
        | TypedExprKind::Uint8Literal(_)
        | TypedExprKind::Uint16Literal(_)
        | TypedExprKind::Uint32Literal(_)
        | TypedExprKind::Uint64Literal(_)
        | TypedExprKind::Uint128Literal(_)
        | TypedExprKind::Float32Literal(_)
        | TypedExprKind::Float64Literal(_)
        | TypedExprKind::Break
        | TypedExprKind::Continue => {}
    }
}

/// Generate an "Organize Imports" code action for a source file.
///
/// Sorts imports alphabetically and groups them by top-level package,
/// inserting blank lines between groups.
pub fn organize_imports_action(source_file: &SourceFile, file_uri: &Url) -> Option<CodeAction> {
    let imports = &source_file.imports;
    if imports.is_empty() {
        return None;
    }

    // Collect import strings and their spans
    let mut import_strings: Vec<String> = Vec::new();
    for imp in imports {
        let path: Vec<&str> = imp.path.iter().map(|s| s.value.as_str()).collect();
        let path_str = path.join(".");
        if let Some(alias) = &imp.alias {
            import_strings.push(format!("import {} as {}", path_str, alias.value));
        } else {
            import_strings.push(format!("import {}", path_str));
        }
    }

    // Deduplicate
    import_strings.sort();
    import_strings.dedup();

    // Group by first path segment (top-level package)
    let mut groups: Vec<Vec<&str>> = Vec::new();
    let mut current_group: Vec<&str> = Vec::new();
    let mut current_prefix = String::new();

    for imp_str in &import_strings {
        // Extract first segment after "import "
        let after_import = imp_str.strip_prefix("import ").unwrap_or(imp_str);
        let first_seg = after_import.split('.').next().unwrap_or("");

        if current_prefix.is_empty() || first_seg == current_prefix {
            current_group.push(imp_str);
            current_prefix = first_seg.to_string();
        } else {
            groups.push(current_group);
            current_group = vec![imp_str];
            current_prefix = first_seg.to_string();
        }
    }
    if !current_group.is_empty() {
        groups.push(current_group);
    }

    // Build organized import text
    let mut organized = String::new();
    for (i, group) in groups.iter().enumerate() {
        if i > 0 {
            organized.push('\n');
        }
        for imp in group {
            organized.push_str(imp);
            organized.push('\n');
        }
    }

    // Compute the range covering all existing imports
    let first_span = &imports.first().unwrap().span;
    let last_span = &imports.last().unwrap().span;

    let start = Position::new(first_span.line.saturating_sub(1), 0);
    let end = Position::new(last_span.end_line.saturating_sub(1) + 1, 0);

    // Check if already organized
    // Build existing text for comparison
    let mut existing = String::new();
    for imp in imports {
        let path: Vec<&str> = imp.path.iter().map(|s| s.value.as_str()).collect();
        let path_str = path.join(".");
        if let Some(alias) = &imp.alias {
            existing.push_str(&format!("import {} as {}\n", path_str, alias.value));
        } else {
            existing.push_str(&format!("import {}\n", path_str));
        }
    }
    if organized == existing {
        return None;
    }

    let mut changes = HashMap::new();
    changes.insert(
        file_uri.clone(),
        vec![TextEdit {
            range: Range::new(start, end),
            new_text: organized,
        }],
    );

    Some(CodeAction {
        title: "Organize Imports".to_string(),
        kind: Some(CodeActionKind::SOURCE_ORGANIZE_IMPORTS),
        edit: Some(WorkspaceEdit {
            changes: Some(changes),
            ..Default::default()
        }),
        ..Default::default()
    })
}
