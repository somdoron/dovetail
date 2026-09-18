use std::path::Path;

use tower_lsp::lsp_types::Location;

use crate::common::span::{FilePath, Span};
use crate::common::types::{Fqn, MangledName, VarName};
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{
    Type, TypeDef, TypedExpr, TypedExprKind, TypedModule, TypedPattern,
};

use super::diagnostics::{file_path_to_uri, span_to_range};
use super::position::NodeAtPosition;

/// What kind of symbol we're looking for references to.
enum ReferenceTarget {
    /// A function (by mangled name)
    Function(MangledName),
    /// A global variable (by mangled name)
    Global(MangledName),
    /// A local variable scoped to a specific function
    Variable {
        name: VarName,
        function_name: MangledName,
    },
    /// A type (record, enum, class) by FQN
    Type(Fqn),
}

/// Find all references to the symbol at the cursor position.
pub fn find_references(
    node: &NodeAtPosition,
    typed_module: &TypedModule,
    registry: &Registry,
    file: &FilePath,
    workspace_root: &Path,
    include_declaration: bool,
) -> Vec<Location> {
    let target = match node {
        NodeAtPosition::FunctionCall { name, .. } | NodeAtPosition::FunctionRef { name, .. } => {
            ReferenceTarget::Function(name.clone())
        }
        NodeAtPosition::MethodRef { method_name, .. } => {
            ReferenceTarget::Function(method_name.clone())
        }
        NodeAtPosition::GlobalRef { name, .. } => ReferenceTarget::Global(name.clone()),
        NodeAtPosition::VarRef { name, .. } | NodeAtPosition::Let { name, .. } => {
            // Find the enclosing function for variable scoping
            let func_name = find_enclosing_function(typed_module, file, name);
            match func_name {
                Some(fn_name) => ReferenceTarget::Variable {
                    name: name.clone(),
                    function_name: fn_name,
                },
                None => return Vec::new(),
            }
        }
        NodeAtPosition::RecordCreate { fqn, .. } => ReferenceTarget::Type(fqn.clone()),
        NodeAtPosition::EnumCreate { fqn, .. } => ReferenceTarget::Type(fqn.clone()),
        NodeAtPosition::ClassNew { mangled_name, .. } => {
            // Extract FQN from the class type def
            if let Some(TypeDef::Class(class_def)) = typed_module.types.get(mangled_name) {
                ReferenceTarget::Type(class_def.fqn.clone())
            } else {
                return Vec::new();
            }
        }
        NodeAtPosition::TypeRef { ty, .. } => {
            if let Some(fqn) = type_to_fqn(ty) {
                ReferenceTarget::Type(fqn)
            } else {
                return Vec::new();
            }
        }
        NodeAtPosition::FieldAccess { .. } | NodeAtPosition::TypedExpr { .. } => {
            return Vec::new();
        }
    };

    let mut spans = Vec::new();

    // Optionally include the declaration site
    if include_declaration
        && let Some(decl_span) = find_declaration_span(&target, typed_module, registry)
    {
        spans.push(decl_span);
    }

    // Walk all bodies for references
    match &target {
        ReferenceTarget::Variable {
            name,
            function_name,
        } => {
            // Only search within the enclosing function
            if let Some(func) = super::source_functions::get(typed_module, function_name) {
                collect_variable_refs(&func.body, name, &mut spans);
            }
        }
        _ => {
            // Search all function bodies
            for func in super::source_functions::values(typed_module) {
                collect_references(&func.body, &target, &mut spans);
            }
            // Search all global initializers
            for global in typed_module.globals.values() {
                collect_references(&global.initializer, &target, &mut spans);
            }
            // Search all test bodies
            for test in &typed_module.tests {
                collect_references(&test.body, &target, &mut spans);
            }
            // Search class initializers
            for type_def in typed_module.types.values() {
                if let TypeDef::Class(class_def) = type_def {
                    for init_expr in &class_def.initializer {
                        collect_references(init_expr, &target, &mut spans);
                    }
                    if let Some(extends_args) = &class_def.extends_args {
                        for arg in extends_args {
                            collect_references(arg, &target, &mut spans);
                        }
                    }
                }
            }
        }
    }

    // Convert spans to locations
    spans
        .into_iter()
        .filter_map(|span| {
            let uri = file_path_to_uri(workspace_root, &span.file)?;
            Some(Location {
                uri,
                range: span_to_range(&span),
            })
        })
        .collect()
}

/// Find the enclosing function for a variable reference.
fn find_enclosing_function(
    typed_module: &TypedModule,
    file: &FilePath,
    var_name: &VarName,
) -> Option<MangledName> {
    for (name, func) in super::source_functions::iter(typed_module) {
        if func.span.file != *file {
            continue;
        }
        if expr_contains_var(&func.body, var_name) {
            return Some(name.clone());
        }
    }
    None
}

fn expr_contains_var(expr: &TypedExpr, var_name: &VarName) -> bool {
    match &expr.kind {
        TypedExprKind::VarRef { name, .. } | TypedExprKind::Let { name, .. } => {
            if name == var_name {
                return true;
            }
        }
        TypedExprKind::Assign { name, .. } if name == var_name => {
            return true;
        }
        _ => {}
    }
    expr_children_contain_var(expr, var_name)
}

fn expr_children_contain_var(expr: &TypedExpr, var_name: &VarName) -> bool {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            expr_contains_var(left, var_name) || expr_contains_var(right, var_name)
        }
        TypedExprKind::UnaryOp { operand, .. } => expr_contains_var(operand, var_name),
        TypedExprKind::Block(exprs) => exprs.iter().any(|e| expr_contains_var(e, var_name)),
        TypedExprKind::Let { value, .. } => expr_contains_var(value, var_name),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            expr_contains_var(value, var_name)
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            expr_contains_var(condition, var_name)
                || expr_contains_var(then_branch, var_name)
                || else_branch
                    .as_ref()
                    .is_some_and(|eb| expr_contains_var(eb, var_name))
        }
        TypedExprKind::While { condition, body } => {
            expr_contains_var(condition, var_name) || expr_contains_var(body, var_name)
        }
        TypedExprKind::Match { subject, arms } => {
            expr_contains_var(subject, var_name)
                || arms.iter().any(|a| expr_contains_var(&a.body, var_name))
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            args.iter().any(|a| expr_contains_var(a, var_name))
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            fields.iter().any(|f| expr_contains_var(f, var_name))
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            expr_contains_var(object, var_name)
                || args.iter().any(|a| expr_contains_var(a, var_name))
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            expr_contains_var(receiver, var_name)
                || args.iter().any(|a| expr_contains_var(a, var_name))
        }
        TypedExprKind::ClosureCall { callee, args } => {
            expr_contains_var(callee, var_name)
                || args.iter().any(|a| expr_contains_var(a, var_name))
        }
        TypedExprKind::Closure { body, .. } | TypedExprKind::AsyncBlock { body, .. } => {
            expr_contains_var(body, var_name)
        }
        TypedExprKind::Return { value, .. } => expr_contains_var(value, var_name),
        TypedExprKind::ForLoop { iterable, body, .. } => {
            expr_contains_var(iterable, var_name) || expr_contains_var(body, var_name)
        }
        TypedExprKind::Await { operand, .. }
        | TypedExprKind::Try { operand, .. }
        | TypedExprKind::Use { operand, .. } => expr_contains_var(operand, var_name),
        TypedExprKind::NamedCall { call: message, .. } | TypedExprKind::Panic { message } => {
            expr_contains_var(message, var_name)
        }
        TypedExprKind::Assert {
            condition, message, ..
        } => {
            expr_contains_var(condition, var_name)
                || message
                    .as_ref()
                    .is_some_and(|m| expr_contains_var(m, var_name))
        }
        TypedExprKind::TupleLiteral { elements } => {
            elements.iter().any(|e| expr_contains_var(e, var_name))
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            fields.iter().any(|(_, e)| expr_contains_var(e, var_name))
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            expr_contains_var(object, var_name)
                || overrides
                    .iter()
                    .any(|(_, _, e)| expr_contains_var(e, var_name))
        }
        TypedExprKind::FieldAccess { object, .. } | TypedExprKind::MethodRef { object, .. } => {
            expr_contains_var(object, var_name)
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            expr_contains_var(object, var_name) || expr_contains_var(value, var_name)
        }
        TypedExprKind::ArrayLiteral { elements } => {
            elements.iter().any(|e| expr_contains_var(e, var_name))
        }
        TypedExprKind::BoxToAny { inner }
        | TypedExprKind::TypeTest { value: inner, .. }
        | TypedExprKind::TypeCast { value: inner, .. }
        | TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner }
        | TypedExprKind::NewtypeCreate { value: inner }
        | TypedExprKind::NewtypeValue { value: inner } => expr_contains_var(inner, var_name),
        TypedExprKind::LetDestructure { value, .. } => expr_contains_var(value, var_name),
        _ => false,
    }
}

/// Find the declaration span for a reference target.
fn find_declaration_span(
    target: &ReferenceTarget,
    typed_module: &TypedModule,
    registry: &Registry,
) -> Option<Span> {
    match target {
        ReferenceTarget::Function(name) => {
            super::source_functions::get(typed_module, name).map(|f| f.span.clone())
        }
        ReferenceTarget::Global(name) => typed_module.globals.get(name).map(|g| g.span.clone()),
        ReferenceTarget::Variable {
            name,
            function_name,
        } => {
            let func = super::source_functions::get(typed_module, function_name)?;
            // Check params first
            for param in &func.params {
                if param.name == name.to_string() {
                    return Some(param.span.clone());
                }
            }
            // Then find the let binding
            find_let_span(&func.body, name)
        }
        ReferenceTarget::Type(fqn) => {
            if let Some(sig) = registry.get_record_type(fqn) {
                return Some(sig.span.clone());
            }
            if let Some(sig) = registry.get_enum_type(fqn) {
                return Some(sig.span.clone());
            }
            if let Some(sig) = registry.get_class_type(fqn) {
                return Some(sig.span.clone());
            }
            if let Some(sig) = registry.get_trait(fqn) {
                return Some(sig.span.clone());
            }
            None
        }
    }
}

fn find_let_span(expr: &TypedExpr, var_name: &VarName) -> Option<Span> {
    if let TypedExprKind::Let { name, .. } = &expr.kind
        && name == var_name
    {
        return Some(expr.span.clone());
    }
    find_let_span_in_children(expr, var_name)
}

fn find_let_span_in_children(expr: &TypedExpr, var_name: &VarName) -> Option<Span> {
    match &expr.kind {
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                if let Some(span) = find_let_span(e, var_name) {
                    return Some(span);
                }
            }
            None
        }
        TypedExprKind::Let { value, .. } => find_let_span(value, var_name),
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => find_let_span(condition, var_name)
            .or_else(|| find_let_span(then_branch, var_name))
            .or_else(|| {
                else_branch
                    .as_ref()
                    .and_then(|eb| find_let_span(eb, var_name))
            }),
        TypedExprKind::While { condition, body } => {
            find_let_span(condition, var_name).or_else(|| find_let_span(body, var_name))
        }
        TypedExprKind::Match { subject, arms } => find_let_span(subject, var_name)
            .or_else(|| arms.iter().find_map(|a| find_let_span(&a.body, var_name))),
        TypedExprKind::ForLoop { body, .. } => find_let_span(body, var_name),
        TypedExprKind::Closure { body, .. } | TypedExprKind::AsyncBlock { body, .. } => {
            find_let_span(body, var_name)
        }
        _ => None,
    }
}

/// Collect variable references (for local variable search within a single function).
fn collect_variable_refs(expr: &TypedExpr, var_name: &VarName, spans: &mut Vec<Span>) {
    match &expr.kind {
        TypedExprKind::VarRef { name, .. } => {
            if name == var_name {
                spans.push(expr.span.clone());
            }
        }
        TypedExprKind::Assign { name, value, .. } => {
            if name == var_name {
                spans.push(expr.span.clone());
            }
            collect_variable_refs(value, var_name, spans);
            return;
        }
        _ => {}
    }
    walk_children_for_refs(expr, var_name, spans);
}

fn walk_children_for_refs(expr: &TypedExpr, var_name: &VarName, spans: &mut Vec<Span>) {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_variable_refs(left, var_name, spans);
            collect_variable_refs(right, var_name, spans);
        }
        TypedExprKind::UnaryOp { operand, .. } => {
            collect_variable_refs(operand, var_name, spans);
        }
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_variable_refs(e, var_name, spans);
            }
        }
        TypedExprKind::NamedCall { call: message, .. } | TypedExprKind::Panic { message } => {
            collect_variable_refs(message, var_name, spans)
        }
        TypedExprKind::Assert {
            condition, message, ..
        } => {
            collect_variable_refs(condition, var_name, spans);
            if let Some(msg) = message {
                collect_variable_refs(msg, var_name, spans);
            }
        }
        TypedExprKind::Let { value, .. } => collect_variable_refs(value, var_name, spans),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            collect_variable_refs(value, var_name, spans);
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                collect_variable_refs(arg, var_name, spans);
            }
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for f in fields {
                collect_variable_refs(f, var_name, spans);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_variable_refs(condition, var_name, spans);
            collect_variable_refs(then_branch, var_name, spans);
            if let Some(eb) = else_branch {
                collect_variable_refs(eb, var_name, spans);
            }
        }
        TypedExprKind::While { condition, body } => {
            collect_variable_refs(condition, var_name, spans);
            collect_variable_refs(body, var_name, spans);
        }
        TypedExprKind::Match { subject, arms } => {
            collect_variable_refs(subject, var_name, spans);
            for arm in arms {
                collect_variable_refs(&arm.body, var_name, spans);
                collect_pattern_refs(&arm.pattern, var_name, spans);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for e in elements {
                collect_variable_refs(e, var_name, spans);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, e) in fields {
                collect_variable_refs(e, var_name, spans);
            }
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            collect_variable_refs(object, var_name, spans);
            for (_, _, val) in overrides {
                collect_variable_refs(val, var_name, spans);
            }
        }
        TypedExprKind::FieldAccess { object, .. } | TypedExprKind::MethodRef { object, .. } => {
            collect_variable_refs(object, var_name, spans);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_variable_refs(object, var_name, spans);
            collect_variable_refs(value, var_name, spans);
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                collect_variable_refs(e, var_name, spans);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            collect_variable_refs(object, var_name, spans);
            for arg in args {
                collect_variable_refs(arg, var_name, spans);
            }
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            collect_variable_refs(receiver, var_name, spans);
            for arg in args {
                collect_variable_refs(arg, var_name, spans);
            }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_variable_refs(callee, var_name, spans);
            for arg in args {
                collect_variable_refs(arg, var_name, spans);
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
            collect_variable_refs(inner, var_name, spans);
        }
        TypedExprKind::LetDestructure { value, .. } => {
            collect_variable_refs(value, var_name, spans);
        }
        TypedExprKind::Await { operand, .. }
        | TypedExprKind::Try { operand, .. }
        | TypedExprKind::Use { operand, .. } => {
            collect_variable_refs(operand, var_name, spans);
        }
        TypedExprKind::Return { value, .. } => collect_variable_refs(value, var_name, spans),
        TypedExprKind::ForLoop { iterable, body, .. } => {
            collect_variable_refs(iterable, var_name, spans);
            collect_variable_refs(body, var_name, spans);
        }
        TypedExprKind::Closure { body, .. } | TypedExprKind::AsyncBlock { body, .. } => {
            collect_variable_refs(body, var_name, spans);
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                collect_variable_refs(arg, var_name, spans);
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

fn collect_pattern_refs(pattern: &TypedPattern, var_name: &VarName, spans: &mut Vec<Span>) {
    match pattern {
        TypedPattern::Literal(expr) => collect_variable_refs(expr, var_name, spans),
        TypedPattern::Record { fields, .. } => {
            for fp in fields {
                collect_pattern_refs(&fp.pattern, var_name, spans);
            }
        }
        TypedPattern::EnumVariant {
            payload_patterns, ..
        } => {
            for p in payload_patterns {
                collect_pattern_refs(p, var_name, spans);
            }
        }
        TypedPattern::EnumVariantRecord { field_patterns, .. } => {
            for fp in field_patterns {
                collect_pattern_refs(&fp.pattern, var_name, spans);
            }
        }
        TypedPattern::Tuple {
            element_patterns, ..
        } => {
            for p in element_patterns {
                collect_pattern_refs(p, var_name, spans);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            collect_pattern_refs(inner_pattern, var_name, spans);
        }
        TypedPattern::Wildcard
        | TypedPattern::Variable(_, _)
        | TypedPattern::TypeAnnotated { .. } => {}
    }
}

/// Collect references to a function, global, or type target across all bodies.
fn collect_references(expr: &TypedExpr, target: &ReferenceTarget, spans: &mut Vec<Span>) {
    match (&expr.kind, target) {
        // Function references
        (TypedExprKind::FunctionCall { name, .. }, ReferenceTarget::Function(target_name))
        | (
            TypedExprKind::FunctionRef {
                name,
                type_params: _,
            },
            ReferenceTarget::Function(target_name),
        )
        | (
            TypedExprKind::ClassSuperCall {
                method_mangled: name,
                ..
            },
            ReferenceTarget::Function(target_name),
        ) => {
            if name == target_name {
                spans.push(expr.span.clone());
            }
        }
        (
            TypedExprKind::MethodRef {
                method_name: name, ..
            },
            ReferenceTarget::Function(target_name),
        ) => {
            if name == target_name {
                spans.push(expr.span.clone());
            }
        }

        // Global references
        (TypedExprKind::GlobalRef { name, .. }, ReferenceTarget::Global(target_name))
        | (TypedExprKind::GlobalAssign { name, .. }, ReferenceTarget::Global(target_name)) => {
            if name == target_name {
                spans.push(expr.span.clone());
            }
        }

        // Type references
        (TypedExprKind::RecordCreate { fqn, .. }, ReferenceTarget::Type(target_fqn))
        | (TypedExprKind::EnumCreate { fqn, .. }, ReferenceTarget::Type(target_fqn))
        | (TypedExprKind::EnumVariantRecordCreate { fqn, .. }, ReferenceTarget::Type(target_fqn))
        | (TypedExprKind::RecordWith { fqn, .. }, ReferenceTarget::Type(target_fqn)) => {
            if fqn == target_fqn {
                spans.push(expr.span.clone());
            }
        }
        (TypedExprKind::ClassNew { mangled_name, .. }, ReferenceTarget::Type(target_fqn)) => {
            let expected = MangledName::for_type(target_fqn);
            if *mangled_name == expected {
                spans.push(expr.span.clone());
            }
        }

        _ => {}
    }

    // Recurse into children
    walk_children_for_references(expr, target, spans);
}

fn walk_children_for_references(expr: &TypedExpr, target: &ReferenceTarget, spans: &mut Vec<Span>) {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            collect_references(left, target, spans);
            collect_references(right, target, spans);
        }
        TypedExprKind::UnaryOp { operand, .. } => collect_references(operand, target, spans),
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                collect_references(e, target, spans);
            }
        }
        TypedExprKind::NamedCall { call: message, .. } | TypedExprKind::Panic { message } => {
            collect_references(message, target, spans)
        }
        TypedExprKind::Assert {
            condition, message, ..
        } => {
            collect_references(condition, target, spans);
            if let Some(msg) = message {
                collect_references(msg, target, spans);
            }
        }
        TypedExprKind::Let { value, .. } => collect_references(value, target, spans),
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            collect_references(value, target, spans);
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                collect_references(arg, target, spans);
            }
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for f in fields {
                collect_references(f, target, spans);
            }
        }
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            collect_references(condition, target, spans);
            collect_references(then_branch, target, spans);
            if let Some(eb) = else_branch {
                collect_references(eb, target, spans);
            }
        }
        TypedExprKind::While { condition, body } => {
            collect_references(condition, target, spans);
            collect_references(body, target, spans);
        }
        TypedExprKind::Match { subject, arms } => {
            collect_references(subject, target, spans);
            for arm in arms {
                collect_references(&arm.body, target, spans);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for e in elements {
                collect_references(e, target, spans);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, e) in fields {
                collect_references(e, target, spans);
            }
        }
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => {
            collect_references(object, target, spans);
            for (_, _, val) in overrides {
                collect_references(val, target, spans);
            }
        }
        TypedExprKind::FieldAccess { object, .. } | TypedExprKind::MethodRef { object, .. } => {
            collect_references(object, target, spans);
        }
        TypedExprKind::FieldAssign { object, value, .. } => {
            collect_references(object, target, spans);
            collect_references(value, target, spans);
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                collect_references(e, target, spans);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            collect_references(object, target, spans);
            for arg in args {
                collect_references(arg, target, spans);
            }
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            collect_references(receiver, target, spans);
            for arg in args {
                collect_references(arg, target, spans);
            }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            collect_references(callee, target, spans);
            for arg in args {
                collect_references(arg, target, spans);
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
            collect_references(inner, target, spans);
        }
        TypedExprKind::LetDestructure { value, .. } => collect_references(value, target, spans),
        TypedExprKind::Await { operand, .. }
        | TypedExprKind::Try { operand, .. }
        | TypedExprKind::Use { operand, .. } => {
            collect_references(operand, target, spans);
        }
        TypedExprKind::Return { value, .. } => collect_references(value, target, spans),
        TypedExprKind::ForLoop { iterable, body, .. } => {
            collect_references(iterable, target, spans);
            collect_references(body, target, spans);
        }
        TypedExprKind::Closure { body, .. } | TypedExprKind::AsyncBlock { body, .. } => {
            collect_references(body, target, spans);
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                collect_references(arg, target, spans);
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

/// Extract FQN from a Type.
fn type_to_fqn(ty: &Type) -> Option<Fqn> {
    match ty {
        Type::Record(fqn, _)
        | Type::Enum(fqn, _)
        | Type::Class(fqn, _)
        | Type::GenericRecord { fqn, .. }
        | Type::GenericEnum { fqn, .. }
        | Type::GenericClass { fqn, .. }
        | Type::Newtype(fqn, _)
        | Type::GenericNewtype { fqn, .. } => Some(fqn.clone()),
        Type::InterfaceObject { traits, .. } => Some(traits[0].trait_fqn.clone()),
        _ => None,
    }
}
