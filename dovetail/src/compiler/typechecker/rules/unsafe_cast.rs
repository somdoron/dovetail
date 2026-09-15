use crate::common::diagnostics::Diagnostics;
use crate::common::types::VarName;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind, TypedModule};

use super::Rule;
use super::visitor::{self, TypedExprVisitor};

/// Warns when `as` is used on an `Any` value without a preceding `is` guard.
///
/// Recognises the pattern `if x is T then ... x as T ...` and suppresses the
/// warning inside the then-branch. All other uses of `as` on `Any` emit a
/// warning suggesting the safe `if`/`is` pattern or a match with type pattern.
pub struct UnsafeCastRule {
    /// Active (variable, type) guards from enclosing `if x is T` conditions.
    /// Vec because Type doesn't implement Ord; typically 0-1 entries.
    guards: Vec<(VarName, Type)>,
}

impl UnsafeCastRule {
    pub fn new() -> Self {
        Self { guards: Vec::new() }
    }
}

impl Rule for UnsafeCastRule {
    fn check(&mut self, typed_module: &TypedModule, diagnostics: &mut Diagnostics) {
        for func in typed_module.functions.values() {
            self.guards.clear();
            self.visit_expr(&func.body, diagnostics);
        }
        // Trait default bodies live outside `functions` and are materialized
        // only after rules run — check them here.
        for func in typed_module.default_templates.values() {
            self.guards.clear();
            self.visit_expr(&func.body, diagnostics);
        }
    }
}

impl TypedExprVisitor for UnsafeCastRule {
    fn visit_expr(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        if let TypedExprKind::TypeCast { value, target_type } = &expr.kind
            && value.ty.is_any()
        {
            if let TypedExprKind::VarRef { name, .. } = &value.kind {
                if !self
                    .guards
                    .iter()
                    .any(|(v, t)| v == name && t == target_type)
                {
                    diagnostics.warning(
                        expr.span.clone(),
                        format!(
                            "'as {t}' cast may panic at runtime; \
                                 use 'if {v} is {t} then ... {v} as {t} ...' \
                                 or a match with 'case {v}: {t} =>'",
                            v = name,
                            t = target_type,
                        ),
                    );
                }
            } else {
                diagnostics.warning(
                    expr.span.clone(),
                    format!(
                        "'as {}' cast may panic at runtime; \
                             use a match with type pattern instead",
                        target_type,
                    ),
                );
            }
        }
        visitor::walk_expr(self, expr, diagnostics);
    }

    fn visit_if(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        if let TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } = &expr.kind
        {
            // Visit condition normally (no guards from it yet).
            self.visit_expr(condition, diagnostics);

            // Extract guard if condition is `x is T` where x is a VarRef.
            let guard = extract_is_guard(condition);

            if let Some(ref g) = guard {
                self.guards.push(g.clone());
            }

            self.visit_expr(then_branch, diagnostics);

            if guard.is_some() {
                self.guards.pop();
            }

            // Visit else-branch without the guard.
            if let Some(else_br) = else_branch {
                self.visit_expr(else_br, diagnostics);
            }
        }
    }
}

fn extract_is_guard(condition: &TypedExpr) -> Option<(VarName, Type)> {
    if let TypedExprKind::TypeTest { value, target_type } = &condition.kind
        && let TypedExprKind::VarRef { name, .. } = &value.kind
    {
        return Some((name.clone(), target_type.clone()));
    }
    None
}
