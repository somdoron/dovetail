use crate::common::span::{FilePath, Span};
use crate::common::types::{Fqn, MangledName, VarName};
use crate::typechecker::types::{Type, TypeReference, TypedExpr, TypedExprKind, TypedModule, TypedPattern};

/// A classified node at a cursor position in the typed AST.
#[derive(Debug)]
pub enum NodeAtPosition {
    /// A function call: `add(1, 2)`
    FunctionCall { name: MangledName, span: Span },
    /// A first-class function reference: `let f = add`
    FunctionRef { name: MangledName, ty: Type, span: Span },
    /// A bound method reference: `obj.method`
    MethodRef { method_name: MangledName, span: Span },
    /// A field access: `record.field`
    FieldAccess { receiver_type: Type, field_name: String, span: Span },
    /// A local variable reference: `x`
    VarRef { name: VarName, ty: Type, span: Span },
    /// A global variable reference: `globalVar`
    GlobalRef { name: MangledName, ty: Type, span: Span },
    /// A record construction: `Point { x = 1, y = 2 }`
    RecordCreate { fqn: Fqn, span: Span },
    /// An enum variant construction: `Option.Some(1)`
    EnumCreate { fqn: Fqn, variant_name: String, span: Span },
    /// A class instantiation: `new MyClass(args)`
    ClassNew { mangled_name: MangledName, span: Span },
    /// A let binding: `let x = 1`
    Let { name: VarName, ty: Type, span: Span },
    /// A type reference: `e is T`, `e as T`, trait coercion, type annotation
    TypeRef { ty: Type, span: Span },
    /// Fallback: any typed expression with its type
    TypedExpr { ty: Type, span: Span },
}

/// Find the innermost typed node at the given cursor position in a file.
pub fn find_node_at_position(
    typed_module: &TypedModule,
    file: &FilePath,
    line: u32,
    column: u32,
) -> Option<NodeAtPosition> {
    let mut best: Option<(&TypedExpr, u64)> = None;

    // Walk all function bodies in this file
    for func in super::source_functions::values(typed_module) {
        if func.span.file != *file {
            continue;
        }
        walk_expr(&func.body, file, line, column, &mut best);
    }

    // Walk all global initializers in this file
    for global in typed_module.globals.values() {
        if global.span.file != *file {
            continue;
        }
        walk_expr(&global.initializer, file, line, column, &mut best);
    }

    // Walk all test bodies in this file
    for test in &typed_module.tests {
        if test.span.file != *file {
            continue;
        }
        walk_expr(&test.body, file, line, column, &mut best);
    }

    // Also search type references (from type annotations) for a more specific match
    let best_type_ref = find_best_type_reference(typed_module, file, line, column);

    match (best, best_type_ref) {
        (Some((expr, expr_area)), Some((tr, tr_area))) => {
            // Prefer the type reference if it's more specific (smaller span)
            if tr_area <= expr_area {
                Some(NodeAtPosition::TypeRef {
                    ty: tr.ty.clone(),
                    span: tr.span.clone(),
                })
            } else {
                Some(classify_expr(expr))
            }
        }
        (Some((expr, _)), None) => Some(classify_expr(expr)),
        (None, Some((tr, _))) => Some(NodeAtPosition::TypeRef {
            ty: tr.ty.clone(),
            span: tr.span.clone(),
        }),
        (None, None) => None,
    }
}

/// Find the best (smallest) type reference at the given position.
fn find_best_type_reference<'a>(
    typed_module: &'a TypedModule,
    file: &FilePath,
    line: u32,
    column: u32,
) -> Option<(&'a TypeReference, u64)> {
    let mut best: Option<(&TypeReference, u64)> = None;
    for tr in &typed_module.type_references {
        if tr.span.file != *file || !span_contains(&tr.span, line, column) {
            continue;
        }
        let area = span_area(&tr.span);
        let dominated = match best {
            Some((_, best_area)) => area < best_area,
            None => true,
        };
        if dominated {
            best = Some((tr, area));
        }
    }
    best
}

/// Check if a 1-indexed position is contained within a span.
pub(super) fn span_contains(span: &Span, line: u32, column: u32) -> bool {
    if span.line == 0 || span.end_line == 0 {
        return false;
    }
    if line < span.line || line > span.end_line {
        return false;
    }
    if line == span.line && column < span.column {
        return false;
    }
    if line == span.end_line && column > span.end_column {
        return false;
    }
    true
}

/// Compute span area (for finding the most specific/innermost node).
fn span_area(span: &Span) -> u64 {
    let lines = (span.end_line.saturating_sub(span.line)) as u64;
    let cols = (span.end_column.saturating_sub(span.column)) as u64;
    lines * 1000 + cols
}

/// Recursively walk the typed AST, tracking the smallest expression containing the cursor.
fn walk_expr<'a>(
    expr: &'a TypedExpr,
    file: &FilePath,
    line: u32,
    column: u32,
    best: &mut Option<(&'a TypedExpr, u64)>,
) {
    if expr.span.file != *file || !span_contains(&expr.span, line, column) {
        return;
    }

    let area = span_area(&expr.span);
    let dominated = match best {
        Some((_, best_area)) => area <= *best_area,
        None => true,
    };
    if dominated {
        *best = Some((expr, area));
    }

    // Recurse into children
    walk_children(expr, file, line, column, best);
}

/// Walk into children of a typed expression.
fn walk_children<'a>(
    expr: &'a TypedExpr,
    file: &FilePath,
    line: u32,
    column: u32,
    best: &mut Option<(&'a TypedExpr, u64)>,
) {
    match &expr.kind {
        TypedExprKind::BinaryOp { left, right, .. } => {
            walk_expr(left, file, line, column, best);
            walk_expr(right, file, line, column, best);
        }
        TypedExprKind::UnaryOp { operand, .. } => {
            walk_expr(operand, file, line, column, best);
        }
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                walk_expr(e, file, line, column, best);
            }
        }
        TypedExprKind::Panic { message } => {
            walk_expr(message, file, line, column, best);
        }
        TypedExprKind::Assert { condition, message } => {
            walk_expr(condition, file, line, column, best);
            if let Some(msg) = message {
                walk_expr(msg, file, line, column, best);
            }
        }
        TypedExprKind::Let { value, .. } => {
            walk_expr(value, file, line, column, best);
        }
        TypedExprKind::Assign { value, .. } | TypedExprKind::GlobalAssign { value, .. } => {
            walk_expr(value, file, line, column, best);
        }
        TypedExprKind::FunctionCall { args, .. } => {
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::If { condition, then_branch, else_branch } => {
            walk_expr(condition, file, line, column, best);
            walk_expr(then_branch, file, line, column, best);
            if let Some(eb) = else_branch {
                walk_expr(eb, file, line, column, best);
            }
        }
        TypedExprKind::While { condition, body } => {
            walk_expr(condition, file, line, column, best);
            walk_expr(body, file, line, column, best);
        }
        TypedExprKind::Match { subject, arms } => {
            walk_expr(subject, file, line, column, best);
            for arm in arms {
                walk_expr(&arm.body, file, line, column, best);
                walk_pattern(&arm.pattern, file, line, column, best);
            }
        }
        TypedExprKind::TupleLiteral { elements } => {
            for element in elements {
                walk_expr(element, file, line, column, best);
            }
        }
        TypedExprKind::RecordCreate { fields, .. } => {
            for (_, field_expr) in fields {
                walk_expr(field_expr, file, line, column, best);
            }
        }
        TypedExprKind::EnumCreate { args, .. } | TypedExprKind::EnumVariantRecordCreate { args, .. } => {
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::FieldAccess { object, .. } | TypedExprKind::FieldAssign { object, .. } => {
            walk_expr(object, file, line, column, best);
            if let TypedExprKind::FieldAssign { value, .. } = &expr.kind {
                walk_expr(value, file, line, column, best);
            }
        }
        TypedExprKind::RecordWith { object, overrides, .. } => {
            walk_expr(object, file, line, column, best);
            for (_, _, val) in overrides {
                walk_expr(val, file, line, column, best);
            }
        }
        TypedExprKind::ArrayLiteral { elements } => {
            for e in elements {
                walk_expr(e, file, line, column, best);
            }
        }
        TypedExprKind::IntrinsicCall { args, .. } => {
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::BoxToAny { inner } => {
            walk_expr(inner, file, line, column, best);
        }
        TypedExprKind::TypeTest { value, .. } | TypedExprKind::TypeCast { value, .. } => {
            walk_expr(value, file, line, column, best);
        }
        TypedExprKind::LetDestructure { value, .. } => {
            walk_expr(value, file, line, column, best);
        }
        TypedExprKind::ClassNew { args, .. } => {
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            walk_expr(object, file, line, column, best);
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::ClassSuperCall { args, .. } => {
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::NewtypeCreate { value } | TypedExprKind::NewtypeValue { value } => {
            walk_expr(value, file, line, column, best);
        }
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => {
            walk_expr(inner, file, line, column, best);
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            walk_expr(receiver, file, line, column, best);
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::Await { operand, .. } | TypedExprKind::Use { operand, .. } => {
            walk_expr(operand, file, line, column, best);
        }
        TypedExprKind::ForLoop { iterable, body, .. } => {
            walk_expr(iterable, file, line, column, best);
            walk_expr(body, file, line, column, best);
        }
        TypedExprKind::AsyncBlock { body, .. } => {
            walk_expr(body, file, line, column, best);
        }
        TypedExprKind::Try { operand, .. } => {
            walk_expr(operand, file, line, column, best);
        }
        TypedExprKind::Return { value, .. } => {
            walk_expr(value, file, line, column, best);
        }
        TypedExprKind::Closure { body, .. } => {
            walk_expr(body, file, line, column, best);
        }
        TypedExprKind::ClosureCall { callee, args } => {
            walk_expr(callee, file, line, column, best);
            for arg in args {
                walk_expr(arg, file, line, column, best);
            }
        }
        TypedExprKind::MethodRef { object, .. } => {
            walk_expr(object, file, line, column, best);
        }
        TypedExprKind::ClassStructCreate { fields, .. } => {
            for field in fields {
                walk_expr(field, file, line, column, best);
            }
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => {
            for arg in args {
                walk_expr(arg, file, line, column, best);
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

/// Walk into match patterns to find nested expressions (e.g. literal patterns).
fn walk_pattern<'a>(
    pattern: &'a TypedPattern,
    file: &FilePath,
    line: u32,
    column: u32,
    best: &mut Option<(&'a TypedExpr, u64)>,
) {
    match pattern {
        TypedPattern::Literal(expr) => walk_expr(expr, file, line, column, best),
        TypedPattern::Record { fields, .. } => {
            for fp in fields {
                walk_pattern(&fp.pattern, file, line, column, best);
            }
        }
        TypedPattern::EnumVariant { payload_patterns, .. } => {
            for p in payload_patterns {
                walk_pattern(p, file, line, column, best);
            }
        }
        TypedPattern::EnumVariantRecord { field_patterns, .. } => {
            for fp in field_patterns {
                walk_pattern(&fp.pattern, file, line, column, best);
            }
        }
        TypedPattern::Tuple { element_patterns, .. } => {
            for p in element_patterns {
                walk_pattern(p, file, line, column, best);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            walk_pattern(inner_pattern, file, line, column, best);
        }
        TypedPattern::Wildcard
        | TypedPattern::Variable(_, _)
        | TypedPattern::TypeAnnotated { .. } => {}
    }
}

/// Find the type of the expression at the given cursor position.
/// Used for dot-completion to determine the receiver type.
pub fn find_expression_type_at_position(
    typed_module: &TypedModule,
    file: &FilePath,
    line: u32,
    column: u32,
) -> Option<Type> {
    let mut best: Option<(&TypedExpr, u64)> = None;

    for func in super::source_functions::values(typed_module) {
        if func.span.file != *file {
            continue;
        }
        walk_expr(&func.body, file, line, column, &mut best);
    }

    for global in typed_module.globals.values() {
        if global.span.file != *file {
            continue;
        }
        walk_expr(&global.initializer, file, line, column, &mut best);
    }

    for test in &typed_module.tests {
        if test.span.file != *file {
            continue;
        }
        walk_expr(&test.body, file, line, column, &mut best);
    }

    best.map(|(expr, _)| expr.ty.clone())
}

/// Classify a TypedExpr into a NodeAtPosition variant.
fn classify_expr(expr: &TypedExpr) -> NodeAtPosition {
    match &expr.kind {
        TypedExprKind::FunctionCall { name, .. } => NodeAtPosition::FunctionCall {
            name: name.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::FunctionRef { name, type_params: _ } => NodeAtPosition::FunctionRef {
            name: name.clone(),
            ty: expr.ty.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::MethodRef { method_name, type_params: _, .. } => NodeAtPosition::MethodRef {
            method_name: method_name.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::FieldAccess { object, field_name, .. } => NodeAtPosition::FieldAccess {
            receiver_type: object.ty.clone(),
            field_name: field_name.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::VarRef { name, .. } => NodeAtPosition::VarRef {
            name: name.clone(),
            ty: expr.ty.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::GlobalRef { name, .. } => NodeAtPosition::GlobalRef {
            name: name.clone(),
            ty: expr.ty.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::RecordCreate { fqn, .. } => NodeAtPosition::RecordCreate {
            fqn: fqn.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::EnumCreate { fqn, variant_name, .. }
        | TypedExprKind::EnumVariantRecordCreate { fqn, variant_name, .. } => {
            NodeAtPosition::EnumCreate {
                fqn: fqn.clone(),
                variant_name: variant_name.clone(),
                span: expr.span.clone(),
            }
        }
        TypedExprKind::ClassNew { mangled_name, .. } => NodeAtPosition::ClassNew {
            mangled_name: mangled_name.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::Let { name, var_ty, .. } => NodeAtPosition::Let {
            name: name.clone(),
            ty: var_ty.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::ClassStructCreate { target_mangled_name, .. } => {
            NodeAtPosition::ClassNew {
                mangled_name: target_mangled_name.clone(),
                span: expr.span.clone(),
            }
        }
        TypedExprKind::RecordWith { fqn, .. } => NodeAtPosition::RecordCreate {
            fqn: fqn.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::TypeTest { target_type, .. } => NodeAtPosition::TypeRef {
            ty: target_type.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::TypeCast { target_type, .. } => NodeAtPosition::TypeRef {
            ty: target_type.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::InterfaceObjectCoerce { .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { .. }
        | TypedExprKind::InterfaceObjectUpcast { .. } => NodeAtPosition::TypeRef {
            ty: expr.ty.clone(),
            span: expr.span.clone(),
        },
        TypedExprKind::ClassSuperCall { method_mangled, .. } => {
            NodeAtPosition::FunctionCall {
                name: method_mangled.clone(),
                span: expr.span.clone(),
            }
        }
        _ => NodeAtPosition::TypedExpr {
            ty: expr.ty.clone(),
            span: expr.span.clone(),
        },
    }
}
