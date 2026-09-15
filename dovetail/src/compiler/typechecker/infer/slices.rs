use crate::common::span::{Span, Spanned};
use crate::common::types::{Fqn, SymbolName};
use crate::parser::ast::Expr;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

use super::Inference;

impl Inference<'_> {
    pub(super) fn is_slice_type(ty: &Type) -> bool {
        matches!(ty, Type::GenericNewtype { fqn, .. }
            if ["standard.prelude.Slice", "standard.prelude.ReadonlySlice"]
                .iter().any(|name| Some(fqn.clone()) == Fqn::from_dotted(name)))
    }

    pub(super) fn call_slice_method(
        &mut self,
        receiver: TypedExpr,
        method: &str,
        args: Vec<TypedExpr>,
        span: &Span,
    ) -> TypedExpr {
        let method = Spanned::new(method.to_owned(), span.clone());
        self.resolve_concrete_type_instance_method(&receiver, &method, &args, &[], span)
            .unwrap_or_else(|| {
                self.diagnostics
                    .error(span.clone(), "cannot resolve prelude Slice method");
                Self::slice_error(span)
            })
    }

    fn full_array_slice(&mut self, array: TypedExpr, span: &Span) -> TypedExpr {
        let fqn = Fqn::from_dotted("standard.prelude.Slice").unwrap();
        let Some(module) = self.registry.lookup_module(&fqn).cloned() else {
            self.diagnostics
                .error(span.clone(), "prelude Slice module is unavailable");
            return Self::slice_error(span);
        };
        let candidates = self.resolve_generic_module_static_method(
            &module,
            &SymbolName("full".into()),
            &[&array.ty],
            &[],
            &[],
            Some(span),
        );
        self.resolve_overload("Slice.full", candidates, vec![array], span)
    }

    pub(super) fn infer_slice_index(
        &mut self,
        object: &Expr,
        start: Option<&Expr>,
        end: Option<&Expr>,
        inclusive: bool,
        span: &Span,
    ) -> TypedExpr {
        let object = self.infer_expr(object);
        let start = start.map(|expr| self.infer_expr(expr));
        let end = end.map(|expr| self.infer_expr(expr));
        for bound in start.iter().chain(end.iter()) {
            self.check_assignable(bound.span.clone(), &Type::Int32, &bound.ty);
        }
        let receiver = match &object.ty {
            Type::Array(_) => self.full_array_slice(object, span),
            ty if Self::is_slice_type(ty) => object,
            Type::Error => return Self::slice_error(span),
            ty => {
                self.diagnostics.error(
                    span.clone(),
                    format!("slice operator requires Array, Slice or ReadonlySlice, found {ty}",),
                );
                return Self::slice_error(span);
            }
        };
        // Each input occurs once in this call tree. Calls evaluate the receiver
        // before their arguments, so omitted bounds need no duplicated receiver.
        let zero = || TypedExpr {
            kind: TypedExprKind::Int32Literal(0),
            ty: Type::Int32,
            span: span.clone(),
        };
        match (start, end, inclusive) {
            (start, Some(end), true) => self.call_slice_method(
                receiver,
                "sliceInclusive",
                vec![start.unwrap_or_else(zero), end],
                span,
            ),
            (Some(start), Some(end), false) => {
                self.call_slice_method(receiver, "slice", vec![start, end], span)
            }
            (Some(start), None, false) => {
                self.call_slice_method(receiver, "drop", vec![start], span)
            }
            (None, Some(end), false) => self.call_slice_method(receiver, "take", vec![end], span),
            (None, None, false) => receiver,
            (_, None, true) => Self::slice_error(span),
        }
    }

    fn slice_error(span: &Span) -> TypedExpr {
        TypedExpr {
            kind: TypedExprKind::UnitLiteral,
            ty: Type::Error,
            span: span.clone(),
        }
    }
}
