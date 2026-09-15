use crate::common::types::{MangledName, Variance};
use crate::typechecker::types::Type;

use super::type_param_substitution::TypeParamSubstitution;

/// Apply a substitution to a type, replacing TypeParameter and SelfType occurrences.
/// Recomputes mangled names for generic types to reflect the substituted type args.
pub(crate) fn apply_substitution(substitution: &TypeParamSubstitution, ty: &Type) -> Type {
    match ty {
        Type::SelfType => {
            if let Some(self_ty) = substitution.self_type() {
                self_ty.clone()
            } else {
                ty.clone()
            }
        }
        Type::TypeVariable(name, bounds) | Type::GenericParam(name, bounds, _) => {
            if let Some(replacement) = substitution.get(name) {
                return replacement.clone();
            }
            let bounds = bounds
                .iter()
                .map(|bound| match bound {
                    crate::typechecker::types::TraitBound::IsClass => bound.clone(),
                    crate::typechecker::types::TraitBound::Named(named) => {
                        let mut named = named.clone();
                        named.type_args = named
                            .type_args
                            .iter()
                            .map(|t| apply_substitution(substitution, t))
                            .collect();
                        named.associated_types = named
                            .associated_types
                            .iter()
                            .map(|(n, t)| (n.clone(), apply_substitution(substitution, t)))
                            .collect();
                        crate::typechecker::types::TraitBound::Named(named)
                    }
                })
                .collect();
            match ty {
                Type::GenericParam(_, _, id) => Type::GenericParam(name.clone(), bounds, *id),
                _ => Type::TypeVariable(name.clone(), bounds),
            }
        }
        Type::GenericRecord { fqn, type_args, .. } => {
            let new_args: Vec<(Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, apply_substitution(substitution, t)))
                .collect();
            Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: new_args,
            }
        }
        Type::GenericEnum { fqn, type_args, .. } => {
            let new_args: Vec<(Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, apply_substitution(substitution, t)))
                .collect();
            Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: new_args,
            }
        }
        Type::GenericClass { fqn, type_args, .. } => {
            let new_args: Vec<(Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, apply_substitution(substitution, t)))
                .collect();
            Type::GenericClass {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: new_args,
            }
        }
        Type::Array(elem) => Type::Array(Box::new(apply_substitution(substitution, elem))),
        Type::TupleExtend(left, right) => Type::tuple_extend(apply_substitution(substitution, left), apply_substitution(substitution, right)),
        Type::AssociatedProjection(projection) => projection.map(|ty| apply_substitution(substitution, ty)).into_type(),
        Type::TupleProjection(receiver, kind) => Type::tuple_projection(apply_substitution(substitution, receiver), *kind),
        Type::Tuple(types, _) => {
            let new_types: Vec<Type> = types.iter().map(|t| apply_substitution(substitution, t)).collect();
            let mn = crate::common::types::MangledName::for_tuple(&new_types);
            Type::Tuple(new_types, mn)
        }
        Type::InterfaceObject { traits, .. } => Type::interface_intersection(
            traits
                .iter()
                .map(|c| {
                    (
                        c.trait_fqn.clone(),
                        c.trait_type_args.iter().map(|t| apply_substitution(substitution, t)).collect(),
                    )
                })
                .collect(),
        ),
        Type::Function(params, ret) => {
            let new_params: Vec<Type> = params.iter().map(|t| apply_substitution(substitution, t)).collect();
            let new_ret = apply_substitution(substitution, ret);
            Type::Function(new_params, Box::new(new_ret))
        }
        Type::Newtype(fqn, inner) => {
            Type::Newtype(fqn.clone(), Box::new(apply_substitution(substitution, inner)))
        }
        Type::GenericNewtype { fqn, type_args, concrete_inner_type } => {
            let new_args: Vec<(Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, apply_substitution(substitution, t)))
                .collect();
            let new_inner = apply_substitution(substitution, concrete_inner_type);
            Type::GenericNewtype {
                fqn: fqn.clone(),
                type_args: new_args,
                concrete_inner_type: Box::new(new_inner),
            }
        }
        Type::TypeConstructor { name, type_args } => {
            let parameters = type_args.iter().map(|ty| apply_substitution(substitution, ty)).collect();
            if let Some(Type::AssociatedProjection(projection)) = substitution.get(name) {
                let mut projection = (**projection).clone();
                projection.parameters = parameters;
                projection.into_type()
            } else {
                Type::TypeConstructor { name: name.clone(), type_args: parameters }
            }
        },
        _ => ty.clone(),
    }
}

