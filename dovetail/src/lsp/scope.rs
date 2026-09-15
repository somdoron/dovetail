use crate::common::span::FilePath;
use crate::common::types::VarName;
use crate::typechecker::types::{
    Type, TypedExpr, TypedExprKind, TypedModule, TypedPattern,
};

use super::position::span_contains;

/// A local variable binding visible at the cursor.
pub struct LocalBinding {
    pub name: VarName,
    pub ty: Type,
    pub mutable: bool,
}

/// A function parameter visible at the cursor.
pub struct ParamBinding {
    pub name: String,
    pub ty: Type,
}

/// All locals and params visible at a given position.
pub struct VisibleLocals {
    pub params: Vec<ParamBinding>,
    pub locals: Vec<LocalBinding>,
}

/// Collect all variables (locals + params) visible at the given cursor position.
///
/// Walks the typed AST to find the enclosing function, then traverses its body
/// to collect let bindings that precede the cursor, as well as pattern bindings
/// from match arms, for loops, and closures whose bodies contain the cursor.
pub fn collect_visible_locals(
    typed_module: &TypedModule,
    file: &FilePath,
    line: u32,
    column: u32,
) -> VisibleLocals {
    let mut result = VisibleLocals {
        params: Vec::new(),
        locals: Vec::new(),
    };

    // Find enclosing function
    for func in super::source_functions::values(typed_module) {
        if func.span.file != *file || !span_contains(&func.span, line, column) {
            continue;
        }
        // Collect params
        for param in &func.params {
            result.params.push(ParamBinding {
                name: param.name.clone(),
                ty: param.ty.clone(),
            });
        }
        // Walk body for locals
        collect_locals_from_expr(&func.body, file, line, column, &mut result.locals);
        return result;
    }

    // Check test bodies
    for test in &typed_module.tests {
        if test.span.file != *file || !span_contains(&test.span, line, column) {
            continue;
        }
        collect_locals_from_expr(&test.body, file, line, column, &mut result.locals);
        return result;
    }

    result
}

/// Recursively collect let bindings visible at the cursor position.
fn collect_locals_from_expr(
    expr: &TypedExpr,
    file: &FilePath,
    line: u32,
    column: u32,
    locals: &mut Vec<LocalBinding>,
) {
    match &expr.kind {
        TypedExprKind::Block(exprs) => {
            for e in exprs {
                // If this sub-expression contains the cursor, recurse into it
                if e.span.file == *file && span_contains(&e.span, line, column) {
                    collect_locals_from_expr(e, file, line, column, locals);
                    return;
                }
                // Otherwise, if it's a Let before the cursor, add it
                collect_binding_from_expr(e, locals);
            }
        }
        TypedExprKind::Let { value, .. } => {
            // Recurse into the value expression
            if value.span.file == *file && span_contains(&value.span, line, column) {
                collect_locals_from_expr(value, file, line, column, locals);
            }
        }
        TypedExprKind::If { condition, then_branch, else_branch } => {
            if condition.span.file == *file && span_contains(&condition.span, line, column) {
                collect_locals_from_expr(condition, file, line, column, locals);
            } else if then_branch.span.file == *file
                && span_contains(&then_branch.span, line, column)
            {
                collect_locals_from_expr(then_branch, file, line, column, locals);
            } else if let Some(eb) = else_branch {
                if eb.span.file == *file && span_contains(&eb.span, line, column) {
                    collect_locals_from_expr(eb, file, line, column, locals);
                }
            }
        }
        TypedExprKind::While { condition, body } => {
            if condition.span.file == *file && span_contains(&condition.span, line, column) {
                collect_locals_from_expr(condition, file, line, column, locals);
            } else if body.span.file == *file && span_contains(&body.span, line, column) {
                collect_locals_from_expr(body, file, line, column, locals);
            }
        }
        TypedExprKind::Match { subject, arms } => {
            if subject.span.file == *file && span_contains(&subject.span, line, column) {
                collect_locals_from_expr(subject, file, line, column, locals);
                return;
            }
            for arm in arms {
                if arm.body.span.file == *file && span_contains(&arm.body.span, line, column) {
                    // Collect pattern bindings for this arm
                    collect_pattern_bindings(&arm.pattern, locals);
                    collect_locals_from_expr(&arm.body, file, line, column, locals);
                    return;
                }
            }
        }
        TypedExprKind::ForLoop { pattern, iterable, body, .. } => {
            if iterable.span.file == *file && span_contains(&iterable.span, line, column) {
                collect_locals_from_expr(iterable, file, line, column, locals);
            } else if body.span.file == *file && span_contains(&body.span, line, column) {
                collect_pattern_bindings(pattern, locals);
                collect_locals_from_expr(body, file, line, column, locals);
            }
        }
        TypedExprKind::Closure { params, body, .. } => {
            if body.span.file == *file && span_contains(&body.span, line, column) {
                for p in params {
                    locals.push(LocalBinding {
                        name: p.name.clone(),
                        ty: p.ty.clone(),
                        mutable: false,
                    });
                }
                collect_locals_from_expr(body, file, line, column, locals);
            }
        }
        TypedExprKind::LetDestructure { value, .. } => {
            if value.span.file == *file && span_contains(&value.span, line, column) {
                collect_locals_from_expr(value, file, line, column, locals);
            }
        }
        _ => {}
    }
}

/// Extract binding(s) from an expression if it's a Let or LetDestructure.
fn collect_binding_from_expr(expr: &TypedExpr, locals: &mut Vec<LocalBinding>) {
    match &expr.kind {
        TypedExprKind::Let { name, mutable, var_ty, .. } => {
            locals.push(LocalBinding {
                name: name.clone(),
                ty: var_ty.clone(),
                mutable: *mutable,
            });
        }
        TypedExprKind::LetDestructure { pattern, .. } => {
            collect_pattern_bindings(pattern, locals);
        }
        _ => {}
    }
}

/// Extract bindings from a typed pattern.
fn collect_pattern_bindings(pattern: &TypedPattern, locals: &mut Vec<LocalBinding>) {
    match pattern {
        TypedPattern::Variable(name, ty) => {
            locals.push(LocalBinding {
                name: name.clone(),
                ty: ty.clone(),
                mutable: false,
            });
        }
        TypedPattern::TypeAnnotated { binding, ty, .. } => {
            locals.push(LocalBinding {
                name: binding.clone(),
                ty: ty.clone(),
                mutable: false,
            });
        }
        TypedPattern::Record { fields, .. } => {
            for fp in fields {
                collect_pattern_bindings(&fp.pattern, locals);
            }
        }
        TypedPattern::EnumVariant { payload_patterns, .. } => {
            for p in payload_patterns {
                collect_pattern_bindings(p, locals);
            }
        }
        TypedPattern::EnumVariantRecord { field_patterns, .. } => {
            for fp in field_patterns {
                collect_pattern_bindings(&fp.pattern, locals);
            }
        }
        TypedPattern::Tuple { element_patterns, .. } => {
            for p in element_patterns {
                collect_pattern_bindings(p, locals);
            }
        }
        TypedPattern::Newtype { inner_pattern, .. } => {
            collect_pattern_bindings(inner_pattern, locals);
        }
        TypedPattern::Wildcard | TypedPattern::Literal(_) => {}
    }
}
