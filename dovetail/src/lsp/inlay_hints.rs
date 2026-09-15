use tower_lsp::lsp_types::{InlayHint, InlayHintKind, InlayHintLabel, Position, Range};

use crate::common::span::{FilePath, Span};
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind, TypedModule};

/// Collect inlay hints for a file within the given range.
///
/// Two categories of hints:
/// - **Type hints** on `let` bindings without explicit type annotations
/// - **Parameter name hints** at call sites
pub fn inlay_hints_for_file(
    typed_module: &TypedModule,
    file: &FilePath,
    range: Range,
    document_content: Option<&str>,
) -> Vec<InlayHint> {
    let mut hints = Vec::new();

    // Convert LSP range (0-indexed) to 1-indexed for span comparison
    let start_line = range.start.line + 1;
    let end_line = range.end.line + 1;

    for func in super::source_functions::values(typed_module) {
        if func.span.file != *file {
            continue;
        }
        // Quick check: skip functions entirely outside the range
        if func.span.end_line < start_line || func.span.line > end_line {
            continue;
        }
        collect_hints(
            &func.body,
            file,
            start_line,
            end_line,
            typed_module,
            document_content,
            &mut hints,
        );
    }

    for global in typed_module.globals.values() {
        if global.span.file != *file {
            continue;
        }
        if global.span.end_line < start_line || global.span.line > end_line {
            continue;
        }
        collect_hints(
            &global.initializer,
            file,
            start_line,
            end_line,
            typed_module,
            document_content,
            &mut hints,
        );
    }

    for test in &typed_module.tests {
        if test.span.file != *file {
            continue;
        }
        if test.span.end_line < start_line || test.span.line > end_line {
            continue;
        }
        collect_hints(
            &test.body,
            file,
            start_line,
            end_line,
            typed_module,
            document_content,
            &mut hints,
        );
    }

    hints
}

/// Recursively collect inlay hints from an expression tree.
fn collect_hints(
    expr: &TypedExpr,
    file: &FilePath,
    start_line: u32,
    end_line: u32,
    typed_module: &TypedModule,
    document_content: Option<&str>,
    hints: &mut Vec<InlayHint>,
) {
    // Skip expressions outside the requested range
    if expr.span.file != *file {
        return;
    }
    if expr.span.end_line < start_line || expr.span.line > end_line {
        return;
    }

    match &expr.kind {
        TypedExprKind::Let {
            name,
            var_ty,
            value,
            ..
        } => {
            // Type hint: show inferred type on let bindings
            if should_show_type_hint(var_ty, value, name, &expr.span, document_content) {
                // Position: after the variable name
                // The Let span starts at "let", the name follows.
                // We use the span's start line and scan for the name position.
                let hint_pos = find_let_name_end(&expr.span, name, document_content);
                hints.push(InlayHint {
                    position: hint_pos,
                    label: InlayHintLabel::String(format!(": {var_ty}")),
                    kind: Some(InlayHintKind::TYPE),
                    text_edits: None,
                    tooltip: None,
                    padding_left: None,
                    padding_right: None,
                    data: None,
                });
            }
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::FunctionCall { name, args, type_params: _ } => {
            // Parameter name hints at call sites
            if let Some(func) = super::source_functions::get(typed_module, name) {
                add_param_hints(args, &func.params, hints);
            }
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::ClassNew { mangled_name, args, .. } => {
            // Parameter name hints for constructor calls
            if let Some(crate::typechecker::types::TypeDef::Class(class_def)) =
                typed_module.types.get(mangled_name)
            {
                add_constructor_param_hints(args, &class_def.constructor_params, hints);
            }
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        // Recurse into all other expression kinds
        _ => {
            walk_for_hints(expr, file, start_line, end_line, typed_module, document_content, hints);
        }
    }
}

/// Whether to show a type hint on a let binding.
fn should_show_type_hint(
    var_ty: &Type,
    value: &TypedExpr,
    name: &crate::common::types::VarName,
    let_span: &Span,
    document_content: Option<&str>,
) -> bool {
    // Never show for error/never types
    if matches!(var_ty, Type::Error | Type::Never) {
        return false;
    }

    // Skip when the value is a literal — type is obvious
    if is_obvious_value(&value.kind) {
        return false;
    }

    // Check if the user wrote an explicit type annotation by scanning the source line
    if let Some(content) = document_content {
        if has_explicit_type_annotation(content, let_span, name) {
            return false;
        }
    }

    true
}

/// Check if the value expression makes the type obvious (no hint needed).
fn is_obvious_value(kind: &TypedExprKind) -> bool {
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

/// Heuristic: scan the source line of the let binding to detect `: Type` between name and `=`.
pub(super) fn has_explicit_type_annotation(
    content: &str,
    let_span: &Span,
    name: &crate::common::types::VarName,
) -> bool {
    let lines: Vec<&str> = content.lines().collect();
    let line_idx = let_span.line.saturating_sub(1) as usize;
    if line_idx >= lines.len() {
        return false;
    }

    let line = lines[line_idx];
    let name_str = name.to_string();

    // Find the name as a whole word after "let " to avoid matching inside keywords
    // (e.g. name "e" matching the "e" in "let" or "mutable")
    let search_start = let_span.column.saturating_sub(1) as usize + 4;
    if search_start > line.len() {
        return false;
    }
    if let Some(name_abs) = find_whole_word(&line[search_start..], &name_str).map(|p| search_start + p) {
        let after_name = &line[name_abs + name_str.len()..];
        // Find `=` in the rest of the line
        if let Some(eq_pos) = after_name.find('=') {
            let between = &after_name[..eq_pos];
            // If there's a `:` between name and `=`, it's an explicit annotation
            return between.contains(':');
        }
    }

    false
}

/// Find the position right after the variable name in a let binding.
pub(super) fn find_let_name_end(
    let_span: &Span,
    name: &crate::common::types::VarName,
    document_content: Option<&str>,
) -> Position {
    // Try to find the exact position by scanning the source
    if let Some(content) = document_content {
        let lines: Vec<&str> = content.lines().collect();
        let line_idx = let_span.line.saturating_sub(1) as usize;
        if line_idx < lines.len() {
            let line = lines[line_idx];
            let name_str = name.to_string();
            // Search for name as a whole word after "let " to avoid matching
            // inside keywords (e.g. name "e" inside "let" or "mutable")
            let search_start = let_span.column.saturating_sub(1) as usize + 4;
            if search_start <= line.len() {
                if let Some(name_pos) = find_whole_word(&line[search_start..], &name_str) {
                    let absolute_pos = search_start + name_pos + name_str.len();
                    return Position {
                        line: let_span.line - 1, // Convert to 0-indexed
                        character: absolute_pos as u32,
                    };
                }
            }
        }
    }

    // Fallback: place after the let keyword + name (approximate)
    Position {
        line: let_span.line - 1,
        character: let_span.column + 3 + name.to_string().len() as u32,
    }
}

/// Find `needle` as a whole word within `haystack`, returning its start offset.
/// A "whole word" means the character before and after the match (if any) is not alphanumeric or `_`.
fn find_whole_word(haystack: &str, needle: &str) -> Option<usize> {
    let mut start = 0;
    while let Some(pos) = haystack[start..].find(needle) {
        let abs_pos = start + pos;
        let before_ok = abs_pos == 0
            || !haystack.as_bytes()[abs_pos - 1].is_ascii_alphanumeric()
                && haystack.as_bytes()[abs_pos - 1] != b'_';
        let end = abs_pos + needle.len();
        let after_ok = end >= haystack.len()
            || !haystack.as_bytes()[end].is_ascii_alphanumeric()
                && haystack.as_bytes()[end] != b'_';
        if before_ok && after_ok {
            return Some(abs_pos);
        }
        start = abs_pos + 1;
    }
    None
}

/// Add parameter name hints for function call arguments.
fn add_param_hints(
    args: &[TypedExpr],
    params: &[crate::typechecker::types::TypedParam],
    hints: &mut Vec<InlayHint>,
) {
    for (i, arg) in args.iter().enumerate() {
        if i >= params.len() {
            break;
        }
        let param = &params[i];

        // Skip self parameter
        if param.name == "self" {
            continue;
        }

        // Skip if it's a single-param function with a literal arg
        if params.len() == 1 && is_literal(&arg.kind) {
            continue;
        }

        // Skip if the argument is a VarRef whose name matches the parameter name
        if let TypedExprKind::VarRef { name, .. } = &arg.kind {
            if name.to_string() == param.name {
                continue;
            }
        }

        hints.push(InlayHint {
            position: Position {
                line: arg.span.line.saturating_sub(1),
                character: arg.span.column.saturating_sub(1),
            },
            label: InlayHintLabel::String(format!("{}: ", param.name)),
            kind: Some(InlayHintKind::PARAMETER),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: Some(false),
            data: None,
        });
    }
}

/// Add parameter name hints for constructor call arguments.
fn add_constructor_param_hints(
    args: &[TypedExpr],
    params: &[crate::typechecker::types::TypedParam],
    hints: &mut Vec<InlayHint>,
) {
    for (i, arg) in args.iter().enumerate() {
        if i >= params.len() {
            break;
        }
        let param = &params[i];

        // Skip if the argument is a VarRef whose name matches the parameter name
        if let TypedExprKind::VarRef { name, .. } = &arg.kind {
            if name.to_string() == param.name {
                continue;
            }
        }

        hints.push(InlayHint {
            position: Position {
                line: arg.span.line.saturating_sub(1),
                character: arg.span.column.saturating_sub(1),
            },
            label: InlayHintLabel::String(format!("{}: ", param.name)),
            kind: Some(InlayHintKind::PARAMETER),
            text_edits: None,
            tooltip: None,
            padding_left: None,
            padding_right: Some(false),
            data: None,
        });
    }
}

fn is_literal(kind: &TypedExprKind) -> bool {
    matches!(
        kind,
        TypedExprKind::UnitLiteral
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
    )
}

/// Walk into children of a typed expression to collect hints.
fn walk_for_hints(
    expr: &TypedExpr,
    file: &FilePath,
    start_line: u32,
    end_line: u32,
    typed_module: &TypedModule,
    document_content: Option<&str>,
    hints: &mut Vec<InlayHint>,
) {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_hints(left, file, start_line, end_line, typed_module, document_content, hints);
            collect_hints(right, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::UnaryOp { operand, .. } => {
            collect_hints(operand, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_hints(e, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::Panic { message } => {
            collect_hints(message, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::Assert { condition, message } => {
            collect_hints(condition, file, start_line, end_line, typed_module, document_content, hints);
            if let Some(msg) = message {
                collect_hints(msg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::Let { value, .. } => {
            // Already handled in collect_hints; just recurse into value
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::FunctionCall { args, .. } => {
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_hints(condition, file, start_line, end_line, typed_module, document_content, hints);
            collect_hints(then_branch, file, start_line, end_line, typed_module, document_content, hints);
            if let Some(eb) = else_branch {
                collect_hints(eb, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::While { condition, body } => {
            collect_hints(condition, file, start_line, end_line, typed_module, document_content, hints);
            collect_hints(body, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::Match { subject, arms } => {
            collect_hints(subject, file, start_line, end_line, typed_module, document_content, hints);
            for arm in arms {
                collect_hints(&arm.body, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for element in elements {
                collect_hints(element, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, field_expr) in fields {
                collect_hints(field_expr, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::FieldAccess { object, .. } => {
            collect_hints(object, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_hints(object, file, start_line, end_line, typed_module, document_content, hints);
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            collect_hints(object, file, start_line, end_line, typed_module, document_content, hints);
            for (_, _, val) in overrides {
                collect_hints(val, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                collect_hints(e, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::BoxToAny { inner } => {
            collect_hints(inner, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::TypeTest { value, .. } | TypedExprKind::TypeCast { value, .. } => {
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::LetDestructure { value, .. } => {
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::ClassNew { args, .. } => {
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            collect_hints(object, file, start_line, end_line, typed_module, document_content, hints);
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::ClassSuperCall { args, .. } => {
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::NewtypeCreate { value } | TypedExprKind::NewtypeValue { value } => {
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => {
            collect_hints(inner, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            collect_hints(receiver, file, start_line, end_line, typed_module, document_content, hints);
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::Await { operand, .. } => {
            collect_hints(operand, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::ForLoop {
            iterable, body, ..
        } => {
            collect_hints(iterable, file, start_line, end_line, typed_module, document_content, hints);
            collect_hints(body, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::AsyncBlock { body, .. } => {
            collect_hints(body, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::Try { operand, .. } | TypedExprKind::Use { operand, .. } => {
            collect_hints(operand, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::Return { value, .. } => {
            collect_hints(value, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::Closure { body, .. } => {
            collect_hints(body, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_hints(callee, file, start_line, end_line, typed_module, document_content, hints);
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::MethodRef { object, .. } => {
            collect_hints(object, file, start_line, end_line, typed_module, document_content, hints);
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for field in fields {
                collect_hints(field, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                collect_hints(arg, file, start_line, end_line, typed_module, document_content, hints);
            }
        }
        TypedExprKind::ImplFunctionRef { .. }
        | TypedExprKind::ExtFunctionRef { .. } => {}
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::Span;
    use crate::common::types::VarName;

    fn make_span(line: u32, column: u32) -> Span {
        Span::new("test.dove".into(), line, column, line, column)
    }

    #[test]
    fn find_let_name_end_single_char_name() {
        // "let e = error as E" — name "e" must not match the "e" in "let"
        let content = "        let e = error as E";
        let span = make_span(1, 9); // column of "let" (1-indexed)
        let name = VarName("e".to_string());
        let pos = find_let_name_end(&span, &name, Some(content));
        // "e" starts at column 12 (0-indexed), ends at 13
        assert_eq!(pos, Position { line: 0, character: 13 });
    }

    #[test]
    fn find_let_name_end_normal_name() {
        let content = "let result = foo()";
        let span = make_span(1, 1);
        let name = VarName("result".to_string());
        let pos = find_let_name_end(&span, &name, Some(content));
        assert_eq!(pos, Position { line: 0, character: 10 });
    }

    #[test]
    fn find_let_name_end_mutable_binding() {
        let content = "    let mutable e = 0";
        let span = make_span(1, 5); // column of "let"
        let name = VarName("e".to_string());
        let pos = find_let_name_end(&span, &name, Some(content));
        // "let mutable e" — "e" is at column 16 (0-indexed), ends at 17
        assert_eq!(pos, Position { line: 0, character: 17 });
    }

    #[test]
    fn has_explicit_annotation_single_char_name() {
        // No annotation — should return false
        let content = "        let e = error as E";
        let span = make_span(1, 9);
        let name = VarName("e".to_string());
        assert!(!has_explicit_type_annotation(content, &span, &name));
    }

    #[test]
    fn has_explicit_annotation_with_annotation() {
        let content = "        let e: E = error as E";
        let span = make_span(1, 9);
        let name = VarName("e".to_string());
        assert!(has_explicit_type_annotation(content, &span, &name));
    }

    #[test]
    fn has_explicit_annotation_normal_name() {
        let content = "let result = foo()";
        let span = make_span(1, 1);
        let name = VarName("result".to_string());
        assert!(!has_explicit_type_annotation(content, &span, &name));
    }

    #[test]
    fn has_explicit_annotation_normal_name_with_type() {
        let content = "let result: Int32 = foo()";
        let span = make_span(1, 1);
        let name = VarName("result".to_string());
        assert!(has_explicit_type_annotation(content, &span, &name));
    }
}
