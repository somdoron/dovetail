//! Deferred arguments cannot suspend their implicit synchronous closures.
use super::Inference;
use crate::compiler::coerce_byname::needs_byname_coercion;
use crate::compiler::monomorphize::visit_expr_children;
use crate::compiler::named_calls::explicit_arguments;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

impl Inference<'_> {
    pub(super) fn validate_named_deferred_arguments(
        &mut self,
        call: &TypedExpr,
        parameter_types: &[Type],
    ) {
        let Some(arguments) = explicit_arguments(call, parameter_types.len()) else {
            return;
        };
        for (argument, expected) in arguments.into_iter().zip(parameter_types) {
            if needs_byname_coercion(&argument.ty, expected) && contains_direct_await(argument) {
                self.diagnostics.error(
                    argument.span.clone(),
                    "await is not allowed in a deferred ByName argument; await the value before passing it, or defer an async value",
                );
            }
        }
    }
}

fn contains_direct_await(expression: &TypedExpr) -> bool {
    match expression.kind {
        TypedExprKind::Await { .. } => true,
        TypedExprKind::Closure { .. } | TypedExprKind::AsyncBlock { .. } => false,
        _ => {
            let mut found = false;
            visit_expr_children(expression, |child| found |= contains_direct_await(child));
            found
        }
    }
}
