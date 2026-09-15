use crate::common::diagnostics::Diagnostics;
use crate::common::span::Span;
use crate::typechecker::rules::visitor::{self, TypedExprVisitor};
use crate::typechecker::types::{TypedExpr, TypedExprKind};

use super::Inference;

impl Inference<'_> {
    /// Await lowering moves a suspending loop's body into callbacks. Loop
    /// control cannot cross those callback boundaries until lowering supports it.
    pub(super) fn check_async_loop(&mut self, condition: Option<&TypedExpr>, body: &TypedExpr) {
        let mut scan = LoopSuspension::default();
        if let Some(condition) = condition {
            scan.visit_expr(condition, self.diagnostics);
            if let Some(span) = scan.await_span {
                self.diagnostics
                    .error(span, "await in a loop condition is not supported");
            }
            if let Some(span) = scan.use_span {
                self.diagnostics
                    .error(span, "use in a loop condition is not supported");
            }
        }
        let mut scan = LoopSuspension::default();
        scan.visit_expr(body, self.diagnostics);
        if scan.await_span.is_some()
            || (self.async_return_type.is_some() && scan.use_span.is_some())
        {
            for (span, keyword) in scan.controls {
                self.diagnostics.error(
                    span,
                    format!("{keyword} in a loop containing await or use is not supported"),
                );
            }
        }
    }
}

#[derive(Default)]
struct LoopSuspension {
    await_span: Option<Span>,
    use_span: Option<Span>,
    nested_loops: usize,
    controls: Vec<(Span, &'static str)>,
}

impl TypedExprVisitor for LoopSuspension {
    fn visit_expr(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        match &expr.kind {
            TypedExprKind::Closure { .. } | TypedExprKind::AsyncBlock { .. } => return,
            TypedExprKind::Await { .. } => self.await_span = Some(expr.span.clone()),
            TypedExprKind::Use { .. } => self.use_span = Some(expr.span.clone()),
            TypedExprKind::Break if self.nested_loops == 0 => {
                self.controls.push((expr.span.clone(), "break"));
            }
            TypedExprKind::Continue if self.nested_loops == 0 => {
                self.controls.push((expr.span.clone(), "continue"));
            }
            TypedExprKind::While { .. } | TypedExprKind::ForLoop { .. } => {
                // Inner awaits also force the outer loop into callbacks, but
                // the inner loop owns its own break and continue expressions.
                self.nested_loops += 1;
                visitor::walk_expr(self, expr, diagnostics);
                self.nested_loops -= 1;
                return;
            }
            _ => {}
        }
        visitor::walk_expr(self, expr, diagnostics);
    }
}
