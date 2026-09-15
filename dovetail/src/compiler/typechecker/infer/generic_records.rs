use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, Variance};
use crate::typechecker::registry::RecordTypeSignature;
use crate::typechecker::types::Type;

use super::Inference;

impl Inference<'_> {
    /// Infer the type for a generic record instantiation.
    /// Checks trait bounds and returns `Type::GenericRecord` with the concrete mangled name.
    /// TypeDef registration is deferred to the monomorphize phase.
    pub(super) fn resolve_generic_record(
        &mut self,
        fqn: &Fqn,
        def: &RecordTypeSignature,
        type_args: &[Type],
        span: &Span,
    ) -> Type {
        self.check_trait_bounds(&def.trait_bounds, &def.type_params, type_args, span);

        let mangled = MangledName::for_type(fqn);

        let type_args_with_variance: Vec<(Variance, Type)> = def
            .type_param_variances
            .iter()
            .zip(type_args.iter())
            .map(|(v, t)| (*v, t.clone()))
            .collect();
        Type::GenericRecord {
            fqn: fqn.clone(),
            mangled_name: mangled,
            type_args: type_args_with_variance,
        }
    }
}
