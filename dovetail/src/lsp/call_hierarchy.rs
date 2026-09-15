use std::path::Path;

use tower_lsp::lsp_types::{
    CallHierarchyIncomingCall, CallHierarchyItem, CallHierarchyOutgoingCall, Range, SymbolKind,
    Url,
};

use crate::common::span::{FilePath, Span};
use crate::common::types::MangledName;
use crate::typechecker::types::{TypedExpr, TypedExprKind, TypedModule, TypedPattern};

use super::diagnostics::{file_path_to_uri, span_to_range};
use super::position;

/// Prepare call hierarchy at the given position.
///
/// Returns a `CallHierarchyItem` for the function at the cursor, or `None` if
/// the cursor is not on a function-like symbol.
pub fn prepare_call_hierarchy(
    typed_module: &TypedModule,
    file: &FilePath,
    line: u32,
    column: u32,
    workspace_root: &Path,
) -> Option<Vec<CallHierarchyItem>> {
    let node = position::find_node_at_position(typed_module, file, line, column)?;

    let mangled_name = match &node {
        position::NodeAtPosition::FunctionCall { name, .. }
        | position::NodeAtPosition::FunctionRef { name, .. } => name.clone(),
        position::NodeAtPosition::MethodRef { method_name, .. } => method_name.clone(),
        _ => {
            // Check if the cursor is inside a function definition
            // Find the enclosing function
            return find_enclosing_function_item(typed_module, file, line, column, workspace_root);
        }
    };

    let func = super::source_functions::get(typed_module, &mangled_name)?;
    let uri = file_path_to_uri(workspace_root, &func.span.file)?;

    Some(vec![build_call_hierarchy_item(
        &func.display_name,
        &mangled_name,
        uri,
        span_to_range(&func.span),
        span_to_range(&func.span),
    )])
}

/// Match a symbol's declaration location before searching another dependency context.
pub fn owns_item(module: &TypedModule, item: &CallHierarchyItem, root: &Path) -> bool {
    extract_mangled_name(item)
        .and_then(|name| super::source_functions::get(module, &name))
        .is_some_and(|function| {
            file_path_to_uri(root, &function.span.file).as_ref() == Some(&item.uri)
                && span_to_range(&function.span) == item.range
        })
}

/// Find all callers of the given function (incoming calls).
pub fn incoming_calls(
    typed_module: &TypedModule,
    item: &CallHierarchyItem,
    workspace_root: &Path,
) -> Vec<CallHierarchyIncomingCall> {
    let target_mangled = match extract_mangled_name(item) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let mut results: Vec<CallHierarchyIncomingCall> = Vec::new();

    // Walk all function bodies
    for (caller_name, func) in super::source_functions::iter(typed_module) {
        let mut call_sites = Vec::new();
        collect_calls_to(&func.body, &target_mangled, &mut call_sites);

        if !call_sites.is_empty() {
            let uri = match file_path_to_uri(workspace_root, &func.span.file) {
                Some(u) => u,
                None => continue,
            };

            let from = build_call_hierarchy_item(
                &func.display_name,
                caller_name,
                uri,
                span_to_range(&func.span),
                span_to_range(&func.span),
            );

            let from_ranges: Vec<Range> =
                call_sites.iter().map(span_to_range).collect();

            results.push(CallHierarchyIncomingCall {
                from,
                from_ranges,
            });
        }
    }

    // Walk global initializers
    for (global_name, global) in &typed_module.globals {
        let mut call_sites = Vec::new();
        collect_calls_to(&global.initializer, &target_mangled, &mut call_sites);

        if !call_sites.is_empty() {
            let uri = match file_path_to_uri(workspace_root, &global.span.file) {
                Some(u) => u,
                None => continue,
            };

            let from = build_call_hierarchy_item(
                &global_name.0,
                global_name,
                uri,
                span_to_range(&global.span),
                span_to_range(&global.span),
            );

            let from_ranges: Vec<Range> =
                call_sites.iter().map(span_to_range).collect();

            results.push(CallHierarchyIncomingCall {
                from,
                from_ranges,
            });
        }
    }

    // Walk test bodies
    for test in &typed_module.tests {
        let mut call_sites = Vec::new();
        collect_calls_to(&test.body, &target_mangled, &mut call_sites);

        if !call_sites.is_empty() {
            let uri = match file_path_to_uri(workspace_root, &test.span.file) {
                Some(u) => u,
                None => continue,
            };

            let from = build_call_hierarchy_item(
                &test.name,
                &test.mangled_name,
                uri,
                span_to_range(&test.span),
                span_to_range(&test.span),
            );

            let from_ranges: Vec<Range> =
                call_sites.iter().map(span_to_range).collect();

            results.push(CallHierarchyIncomingCall {
                from,
                from_ranges,
            });
        }
    }

    results
}

/// Find all functions called by the given function (outgoing calls).
pub fn outgoing_calls(
    typed_module: &TypedModule,
    item: &CallHierarchyItem,
    workspace_root: &Path,
) -> Vec<CallHierarchyOutgoingCall> {
    let source_mangled = match extract_mangled_name(item) {
        Some(m) => m,
        None => return Vec::new(),
    };

    let func = match super::source_functions::get(typed_module, &source_mangled) {
        Some(f) => f,
        None => return Vec::new(),
    };

    // Collect all calls from this function
    let mut calls: Vec<(MangledName, Span)> = Vec::new();
    collect_all_calls(&func.body, &mut calls);

    // Group by target function
    let mut grouped: std::collections::BTreeMap<MangledName, Vec<Span>> =
        std::collections::BTreeMap::new();
    for (name, span) in calls {
        grouped.entry(name).or_default().push(span);
    }

    let mut results = Vec::new();
    for (target_name, spans) in grouped {
        let target_func = match super::source_functions::get(typed_module, &target_name) {
            Some(f) => f,
            None => continue,
        };

        let uri = match file_path_to_uri(workspace_root, &target_func.span.file) {
            Some(u) => u,
            None => continue,
        };

        let to = build_call_hierarchy_item(
            &target_func.display_name,
            &target_name,
            uri,
            span_to_range(&target_func.span),
            span_to_range(&target_func.span),
        );

        let from_ranges: Vec<Range> = spans.iter().map(span_to_range).collect();

        results.push(CallHierarchyOutgoingCall { to, from_ranges });
    }

    results
}

/// Build a `CallHierarchyItem` for a function.
fn build_call_hierarchy_item(
    display_name: &str,
    mangled_name: &MangledName,
    uri: Url,
    range: Range,
    selection_range: Range,
) -> CallHierarchyItem {
    CallHierarchyItem {
        name: display_name.to_string(),
        kind: SymbolKind::FUNCTION,
        tags: None,
        detail: None,
        uri,
        range,
        selection_range,
        data: Some(serde_json::Value::String(mangled_name.0.clone())),
    }
}

/// Extract the MangledName stored in the `data` field of a CallHierarchyItem.
fn extract_mangled_name(item: &CallHierarchyItem) -> Option<MangledName> {
    item.data
        .as_ref()
        .and_then(|v| v.as_str())
        .map(|s| MangledName(s.to_string()))
}

/// Find the enclosing function at a cursor position for prepare.
fn find_enclosing_function_item(
    typed_module: &TypedModule,
    file: &FilePath,
    line: u32,
    column: u32,
    workspace_root: &Path,
) -> Option<Vec<CallHierarchyItem>> {
    for (name, func) in super::source_functions::iter(typed_module) {
        if func.span.file != *file {
            continue;
        }
        if position::span_contains(&func.span, line, column) {
            let uri = file_path_to_uri(workspace_root, &func.span.file)?;
            return Some(vec![build_call_hierarchy_item(
                &func.display_name,
                name,
                uri,
                span_to_range(&func.span),
                span_to_range(&func.span),
            )]);
        }
    }
    None
}

/// Collect all call sites to a specific target function within an expression tree.
fn collect_calls_to(expr: &TypedExpr, target: &MangledName, sites: &mut Vec<Span>) {
    match &expr.kind {
        TypedExprKind::FunctionCall { name, .. } if name == target => {
            sites.push(expr.span.clone());
        }
        TypedExprKind::FunctionRef { name, type_params: _ } if name == target => {
            sites.push(expr.span.clone());
        }
        TypedExprKind::ClassSuperCall {
            method_mangled, ..
        } if method_mangled == target => {
            sites.push(expr.span.clone());
        }
        TypedExprKind::MethodRef { method_name, .. } if method_name == target => {
            sites.push(expr.span.clone());
        }
        _ => {}
    }

    walk_children_for_calls_to(expr, target, sites);
}

fn walk_children_for_calls_to(expr: &TypedExpr, target: &MangledName, sites: &mut Vec<Span>) {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_calls_to(left, target, sites);
            collect_calls_to(right, target, sites);
        }
        TypedExprKind::UnaryOp { operand, .. } => collect_calls_to(operand, target, sites),
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_calls_to(e, target, sites);
            }
        }
        TypedExprKind::Panic { message } => collect_calls_to(message, target, sites),
        TypedExprKind::Assert {
            condition, message, ..
        } => {
            collect_calls_to(condition, target, sites);
            if let Some(msg) = message {
                collect_calls_to(msg, target, sites);
            }
        }
        TypedExprKind::Let { value, .. } => collect_calls_to(value, target, sites),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            collect_calls_to(value, target, sites);
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                collect_calls_to(arg, target, sites);
            }
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for f in fields {
                collect_calls_to(f, target, sites);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_calls_to(condition, target, sites);
            collect_calls_to(then_branch, target, sites);
            if let Some(eb) = else_branch {
                collect_calls_to(eb, target, sites);
            }
        }
        TypedExprKind::While { condition, body } => {
            collect_calls_to(condition, target, sites);
            collect_calls_to(body, target, sites);
        }
        TypedExprKind::Match { subject, arms } => {
            collect_calls_to(subject, target, sites);
            for arm in arms {
                collect_calls_to(&arm.body, target, sites);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for e in elements {
                collect_calls_to(e, target, sites);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, e) in fields {
                collect_calls_to(e, target, sites);
            }
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            collect_calls_to(object, target, sites);
            for (_, _, val) in overrides {
                collect_calls_to(val, target, sites);
            }
        }
        TypedExprKind::FieldAccess { object, .. } | TypedExprKind::MethodRef { object, .. } => {
            collect_calls_to(object, target, sites);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_calls_to(object, target, sites);
            collect_calls_to(value, target, sites);
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                collect_calls_to(e, target, sites);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            collect_calls_to(object, target, sites);
            for arg in args {
                collect_calls_to(arg, target, sites);
            }
        }
        TypedExprKind::InterfaceObjectMethodCall {
            receiver, args, ..
        } => {
            collect_calls_to(receiver, target, sites);
            for arg in args {
                collect_calls_to(arg, target, sites);
            }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_calls_to(callee, target, sites);
            for arg in args {
                collect_calls_to(arg, target, sites);
            }
        }
        TypedExprKind::BoxToAny { inner }
        | TypedExprKind::TypeTest { value: inner, .. }
        | TypedExprKind::TypeCast { value: inner, .. }
        | TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner }
        | TypedExprKind::NewtypeCreate { value: inner }
        | TypedExprKind::NewtypeValue { value: inner } => {
            collect_calls_to(inner, target, sites);
        }
        TypedExprKind::LetDestructure { value, .. } => collect_calls_to(value, target, sites),
        TypedExprKind::Await { operand, .. }
        | TypedExprKind::Try { operand, .. }
        | TypedExprKind::Use { operand, .. } => {
            collect_calls_to(operand, target, sites);
        }
        TypedExprKind::Return { value, .. } => collect_calls_to(value, target, sites),
        TypedExprKind::ForLoop {
            iterable, body, ..
        } => {
            collect_calls_to(iterable, target, sites);
            collect_calls_to(body, target, sites);
        }
        TypedExprKind::Closure { body, .. } | TypedExprKind::AsyncBlock { body, .. } => {
            collect_calls_to(body, target, sites);
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                collect_calls_to(arg, target, sites);
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

/// Collect all outgoing calls from an expression tree.
fn collect_all_calls(expr: &TypedExpr, calls: &mut Vec<(MangledName, Span)>) {
    match &expr.kind {
        TypedExprKind::FunctionCall { name, .. } => {
            calls.push((name.clone(), expr.span.clone()));
        }
        TypedExprKind::FunctionRef { name, type_params: _ } => {
            calls.push((name.clone(), expr.span.clone()));
        }
        TypedExprKind::ClassSuperCall {
            method_mangled, ..
        } => {
            calls.push((method_mangled.clone(), expr.span.clone()));
        }
        TypedExprKind::MethodRef { method_name, .. } => {
            calls.push((method_name.clone(), expr.span.clone()));
        }
        TypedExprKind::ClassNew { mangled_name, .. } => {
            calls.push((mangled_name.clone(), expr.span.clone()));
        }
        _ => {}
    }

    walk_children_for_all_calls(expr, calls);
}

fn walk_children_for_all_calls(expr: &TypedExpr, calls: &mut Vec<(MangledName, Span)>) {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_all_calls(left, calls);
            collect_all_calls(right, calls);
        }
        TypedExprKind::UnaryOp { operand, .. } => collect_all_calls(operand, calls),
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_all_calls(e, calls);
            }
        }
        TypedExprKind::Panic { message } => collect_all_calls(message, calls),
        TypedExprKind::Assert {
            condition, message, ..
        } => {
            collect_all_calls(condition, calls);
            if let Some(msg) = message {
                collect_all_calls(msg, calls);
            }
        }
        TypedExprKind::Let { value, .. } => collect_all_calls(value, calls),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            collect_all_calls(value, calls);
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                collect_all_calls(arg, calls);
            }
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for f in fields {
                collect_all_calls(f, calls);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_all_calls(condition, calls);
            collect_all_calls(then_branch, calls);
            if let Some(eb) = else_branch {
                collect_all_calls(eb, calls);
            }
        }
        TypedExprKind::While { condition, body } => {
            collect_all_calls(condition, calls);
            collect_all_calls(body, calls);
        }
        TypedExprKind::Match { subject, arms } => {
            collect_all_calls(subject, calls);
            for arm in arms {
                collect_all_calls(&arm.body, calls);
                walk_pattern_for_calls(&arm.pattern, calls);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for e in elements {
                collect_all_calls(e, calls);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, e) in fields {
                collect_all_calls(e, calls);
            }
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            collect_all_calls(object, calls);
            for (_, _, val) in overrides {
                collect_all_calls(val, calls);
            }
        }
        TypedExprKind::FieldAccess { object, .. } | TypedExprKind::MethodRef { object, .. } => {
            collect_all_calls(object, calls);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_all_calls(object, calls);
            collect_all_calls(value, calls);
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                collect_all_calls(e, calls);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            collect_all_calls(object, calls);
            for arg in args {
                collect_all_calls(arg, calls);
            }
        }
        TypedExprKind::InterfaceObjectMethodCall {
            receiver, args, ..
        } => {
            collect_all_calls(receiver, calls);
            for arg in args {
                collect_all_calls(arg, calls);
            }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_all_calls(callee, calls);
            for arg in args {
                collect_all_calls(arg, calls);
            }
        }
        TypedExprKind::BoxToAny { inner }
        | TypedExprKind::TypeTest { value: inner, .. }
        | TypedExprKind::TypeCast { value: inner, .. }
        | TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner }
        | TypedExprKind::NewtypeCreate { value: inner }
        | TypedExprKind::NewtypeValue { value: inner } => {
            collect_all_calls(inner, calls);
        }
        TypedExprKind::LetDestructure { value, .. } => collect_all_calls(value, calls),
        TypedExprKind::Await { operand, .. }
        | TypedExprKind::Try { operand, .. }
        | TypedExprKind::Use { operand, .. } => {
            collect_all_calls(operand, calls);
        }
        TypedExprKind::Return { value, .. } => collect_all_calls(value, calls),
        TypedExprKind::ForLoop {
            iterable, body, ..
        } => {
            collect_all_calls(iterable, calls);
            collect_all_calls(body, calls);
        }
        TypedExprKind::Closure { body, .. } | TypedExprKind::AsyncBlock { body, .. } => {
            collect_all_calls(body, calls);
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                collect_all_calls(arg, calls);
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

fn walk_pattern_for_calls(pattern: &TypedPattern, calls: &mut Vec<(MangledName, Span)>) {
    match pattern {
        TypedPattern::Literal(expr) => collect_all_calls(expr, calls),
        TypedPattern::Record { fields, .. } => {
            for fp in fields {
                walk_pattern_for_calls(&fp.pattern, calls);
            }
        }
        TypedPattern::EnumVariant {
            payload_patterns, ..
        } => {
            for p in payload_patterns {
                walk_pattern_for_calls(p, calls);
            }
        }
        TypedPattern::EnumVariantRecord {
            field_patterns, ..
        } => {
            for fp in field_patterns {
                walk_pattern_for_calls(&fp.pattern, calls);
            }
        }
        TypedPattern::Tuple {
            element_patterns, ..
        } => {
            for p in element_patterns {
                walk_pattern_for_calls(p, calls);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            walk_pattern_for_calls(inner_pattern, calls);
        }
        TypedPattern::Wildcard
        | TypedPattern::Variable(_, _)
        | TypedPattern::TypeAnnotated { .. } => {}
    }
}
