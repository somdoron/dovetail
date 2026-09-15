use super::Inference;
use crate::common::span::Span;
use crate::common::types::{Fqn, SymbolName};
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

impl Inference<'_> {
    pub(super) fn lower_library_negation(
        &mut self,
        operand: &TypedExpr,
        span: &Span,
    ) -> Option<TypedExpr> {
        let trait_fqn = Fqn::from_dotted("standard.prelude.Neg").unwrap();
        let ty = &operand.ty;
        if let Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) = ty {
            if !bounds
                .iter()
                .filter_map(crate::typechecker::types::TraitBound::named)
                .any(|bound| bound.trait_fqn == trait_fqn)
            {
                return None;
            }
        } else {
            let expected = self.expected_type.take();
            let resolved = self.resolve_trait_impl_method_for_type(ty, &trait_fqn, "negate", &[]);
            self.expected_type = expected;
            resolved?;
        }
        Some(TypedExpr {
            kind: TypedExprKind::ImplFunctionCall {
                trait_fqn,
                trait_type_params: vec![],
                for_type: ty.clone(),
                method_name: SymbolName("negate".into()),
                args: vec![operand.clone()],
                method_type_params: vec![],
            },
            ty: ty.clone(),
            span: span.clone(),
        })
    }
}
