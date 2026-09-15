use crate::common::span::Span;
use crate::common::types::{Fqn, Variance};
use crate::typechecker::registry::NewtypeSignature;
use crate::typechecker::types::Type;

use super::Inference;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;

impl Inference<'_> {
    /// Resolve a generic newtype: substitute type params in the inner type and
    /// return the concrete `Type::GenericNewtype` variant.
    /// No WASM type registration needed — newtypes are transparent.
    pub(super) fn resolve_generic_newtype(
        &mut self,
        fqn: &Fqn,
        sig: &NewtypeSignature,
        type_args: &[Type],
        span: &Span,
    ) -> Type {
        self.check_trait_bounds(&sig.trait_bounds, &sig.type_params, type_args, span);

        let substitution = TypeParamSubstitution::from_pairs(&sig.type_params, type_args);
        let concrete_inner = apply_substitution(&substitution, &sig.inner_type);

        let type_args_with_variance: Vec<(Variance, Type)> = sig
            .type_param_variances
            .iter()
            .zip(type_args.iter())
            .map(|(v, t)| (*v, t.clone()))
            .collect();

        Type::GenericNewtype {
            fqn: fqn.clone(),
            type_args: type_args_with_variance,
            concrete_inner_type: Box::new(concrete_inner),
        }
    }
}
