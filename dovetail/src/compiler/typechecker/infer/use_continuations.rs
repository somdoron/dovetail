use crate::common::diagnostics::Diagnostics;
use crate::common::types::Fqn;
use crate::typechecker::rules::visitor::{TypedExprVisitor, walk_expr};
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

use super::Inference;

struct ContinuationCheck<'a> {
    result: &'a Type,
    usable: Fqn,
}

impl TypedExprVisitor for ContinuationCheck<'_> {
    fn visit_expr(&mut self, expression: &TypedExpr, diagnostics: &mut Diagnostics) {
        // Nested blocks and closures check their own continuation result during
        // inference. Other expressions share this enclosing continuation.
        if matches!(
            expression.kind,
            TypedExprKind::Block(_) | TypedExprKind::Closure { .. }
        ) {
            return;
        }
        if let TypedExprKind::Use {
            operand,
            target_error,
            ..
        } = &expression.kind
            && matches!(operand.ty, Type::TypeVariable(..) | Type::GenericParam(..))
            && !self.matches_wrapper(&operand.ty, target_error)
        {
            diagnostics.error(
                expression.span.clone(),
                format!(
                    "generic use requires a continuation returning '{}.Wrapped<U, {}>', found '{}'",
                    operand.ty, target_error, self.result,
                ),
            );
        }
        walk_expr(self, expression, diagnostics);
    }
}

impl ContinuationCheck<'_> {
    fn matches_wrapper(&self, receiver: &Type, error: &Type) -> bool {
        let Type::AssociatedProjection(projection) = self.result else {
            return false;
        };
        projection.trait_fqn == self.usable
            && projection.member == "Wrapped"
            && projection.parameters.len() == 2
            && crate::typechecker::subtyping::identical(&projection.receiver, receiver)
            && crate::typechecker::subtyping::identical(&projection.parameters[1], error)
    }
}

impl Inference<'_> {
    /// An abstract resource's continuation must return its chosen wrapper.
    /// A concrete resource's wrapper is checked after implementation selection.
    pub(super) fn check_generic_use_continuations(
        &mut self,
        expressions: &[TypedExpr],
        result: &Type,
    ) {
        let mut check = ContinuationCheck {
            result,
            usable: Fqn::from_dotted("standard.prelude.Usable").unwrap(),
        };
        for expression in expressions {
            check.visit_expr(expression, self.diagnostics);
        }
    }
}
