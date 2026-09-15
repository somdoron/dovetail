use crate::common::types::{Fqn, MangledName, Variance};
use crate::typechecker::registry::EnumTypeSignature;
use crate::typechecker::types::Type;

use super::Inference;

impl Inference<'_> {
    /// Infer the type for a generic enum instantiation.
    /// Returns `Type::GenericEnum` with the concrete mangled name.
    /// TypeDef registration is deferred to the monomorphize phase.
    pub(super) fn resolve_generic_enum_type(
        &mut self,
        fqn: &Fqn,
        def: &EnumTypeSignature,
        type_args: &[Type],
    ) -> Type {
        let mangled = MangledName::for_type(fqn);

        let type_args_with_variance: Vec<(Variance, Type)> = def
            .type_param_variances
            .iter()
            .zip(type_args.iter())
            .map(|(v, t)| (*v, t.clone()))
            .collect();
        Type::GenericEnum {
            fqn: fqn.clone(),
            mangled_name: mangled,
            type_args: type_args_with_variance,
        }
    }
}
