//! Representation-preserving subtyping shared by inference and runtime reification.
//! Value-producing coercions and inference error recovery belong to the caller.
use super::infer::generics::apply_substitution;
use super::infer::type_param_substitution::TypeParamSubstitution;
use super::{registry::Registry, types::Type};
use crate::common::types::{Fqn, MangledName, Variance};

pub fn class_parent(registry: &Registry, actual: &Type) -> Option<Type> {
    let fqn = actual.try_to_fqn()?;
    let sig = registry.get_class_type(&fqn)?;
    let parent = sig.parent_class.as_ref()?;
    let params = match actual {
        Type::GenericClass { type_args, .. } => type_args.iter().map(|(_, t)| t.clone()).collect(),
        _ => Vec::new(),
    };
    match &sig.parent_type_expr {
        Some(ty) => Some(apply_substitution(
            &TypeParamSubstitution::from_pairs(&sig.type_params, &params),
            ty,
        )),
        None => Some(Type::Class(parent.clone(), MangledName::for_type(parent))),
    }
}

pub fn project_class(registry: &Registry, actual: &Type, expected: &Fqn) -> Option<Type> {
    let mut current = actual.clone();
    let mut visited = std::collections::BTreeSet::new();
    loop {
        let fqn = current.try_to_fqn()?;
        if &fqn == expected {
            return Some(current);
        }
        if !visited.insert(fqn) {
            return None;
        }
        current = class_parent(registry, &current)?;
    }
}

/// Nominal type identity ignores erased layout names and variance annotations.
pub fn identical(a: &Type, b: &Type) -> bool {
    match (a, b) {
        (Type::AssociatedProjection(a), Type::AssociatedProjection(b)) => {
            a.trait_fqn == b.trait_fqn
                && a.member == b.member
                && identical(&a.receiver, &b.receiver)
                && a.trait_parameters.len() == b.trait_parameters.len()
                && a.parameters.len() == b.parameters.len()
                && a.trait_parameters
                    .iter()
                    .zip(&b.trait_parameters)
                    .all(|(a, b)| identical(a, b))
                && a.parameters
                    .iter()
                    .zip(&b.parameters)
                    .all(|(a, b)| identical(a, b))
        }
        (Type::TupleProjection(a, k), Type::TupleProjection(b, l)) => k == l && identical(a, b),
        (Type::TupleExtend(a, b), Type::TupleExtend(c, d)) => identical(a, c) && identical(b, d),
        (
            Type::TypeVariable(a, _) | Type::GenericParam(a, _, _),
            Type::TypeVariable(b, _) | Type::GenericParam(b, _, _),
        ) => a == b,
        (Type::Record(a, _), Type::Record(b, _))
        | (Type::Enum(a, _), Type::Enum(b, _))
        | (Type::Class(a, _), Type::Class(b, _))
        | (Type::Newtype(a, _), Type::Newtype(b, _)) => a == b,
        (
            Type::GenericRecord {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericRecord {
                fqn: b,
                type_args: ba,
                ..
            },
        )
        | (
            Type::GenericEnum {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericEnum {
                fqn: b,
                type_args: ba,
                ..
            },
        )
        | (
            Type::GenericClass {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericClass {
                fqn: b,
                type_args: ba,
                ..
            },
        )
        | (
            Type::GenericNewtype {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericNewtype {
                fqn: b,
                type_args: ba,
                ..
            },
        ) => {
            a == b
                && aa.len() == ba.len()
                && aa.iter().zip(ba).all(|((_, a), (_, b))| identical(a, b))
        }
        (Type::Array(a), Type::Array(b)) => identical(a, b),
        (Type::Tuple(a, _), Type::Tuple(b, _)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| identical(a, b))
        }
        (Type::Function(a, ar), Type::Function(b, br)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| identical(a, b)) && identical(ar, br)
        }
        _ => a == b,
    }
}

pub fn arguments(
    registry: &Registry,
    expected: &[(Variance, Type)],
    actual: &[(Variance, Type)],
) -> bool {
    expected.len() == actual.len()
        && expected
            .iter()
            .zip(actual)
            .all(|((v, e), (_, a))| argument(registry, *v, e, a))
}

pub fn argument(registry: &Registry, variance: Variance, expected: &Type, actual: &Type) -> bool {
    // Widening an existing value to Any preserves its representation, including
    // an already-built interface object. Interface wrapping/subsetting still
    // requires exact generic arguments in every other position.
    let widens_to_any = match variance {
        Variance::Covariant => expected.is_any(),
        Variance::Contravariant => actual.is_any(),
        Variance::Invariant => false,
    };
    if !widens_to_any
        && !actual.is_never()
        && !expected.is_never()
        && (actual.contains_interface_object() || expected.contains_interface_object())
    {
        return identical(actual, expected);
    }
    match variance {
        Variance::Invariant => identical(actual, expected),
        Variance::Covariant => is_subtype(registry, actual, expected),
        Variance::Contravariant => is_subtype(registry, expected, actual),
    }
}

pub fn is_subtype(registry: &Registry, actual: &Type, expected: &Type) -> bool {
    if actual.is_error() || expected.is_error() {
        return false;
    }
    if identical(actual, expected) || actual.is_never() || expected.is_any() {
        return true;
    }
    if let Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) = actual {
        for bound in bounds.iter().filter_map(|b| b.named()) {
            if bound.kind != super::types::BoundKind::SubtypeOf {
                continue;
            }
            if let Some(primitive) = bound.primitive_subtype() {
                if is_subtype(registry, &primitive, expected) {
                    return true;
                }
                continue;
            }
            let Some(sig) = registry.get_class_type(&bound.trait_fqn) else {
                continue;
            };
            let ty = if sig.type_params.is_empty() {
                Type::Class(
                    bound.trait_fqn.clone(),
                    MangledName::for_type(&bound.trait_fqn),
                )
            } else {
                Type::GenericClass {
                    fqn: bound.trait_fqn.clone(),
                    mangled_name: MangledName::for_type(&bound.trait_fqn),
                    type_args: sig
                        .type_param_variances
                        .iter()
                        .cloned()
                        .zip(bound.type_args.clone())
                        .collect(),
                }
            };
            if is_subtype(registry, &ty, expected) {
                return true;
            }
        }
    }
    if actual.is_class_type() && expected.is_class_type() {
        let Some(target) = expected.try_to_fqn() else {
            return false;
        };
        let Some(projected) = project_class(registry, actual, &target) else {
            return false;
        };
        return match (&projected, expected) {
            (Type::GenericClass { type_args: a, .. }, Type::GenericClass { type_args: e, .. }) => {
                arguments(registry, e, a)
            }
            (Type::Class(..), Type::Class(..)) => true,
            _ => false,
        };
    }
    match (actual, expected) {
        (
            Type::GenericRecord {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericRecord {
                fqn: e,
                type_args: ea,
                ..
            },
        )
        | (
            Type::GenericEnum {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericEnum {
                fqn: e,
                type_args: ea,
                ..
            },
        )
        | (
            Type::GenericNewtype {
                fqn: a,
                type_args: aa,
                ..
            },
            Type::GenericNewtype {
                fqn: e,
                type_args: ea,
                ..
            },
        ) => a == e && arguments(registry, ea, aa),
        (Type::Array(a), Type::Array(e)) => identical(a, e),
        (Type::TupleExtend(al, ar), Type::TupleExtend(el, er)) => {
            identical(al, el) && is_subtype(registry, ar, er)
        }
        (Type::Tuple(a, _), Type::Tuple(e, _)) => {
            a.len() == e.len() && a.iter().zip(e).all(|(a, e)| is_subtype(registry, a, e))
        }
        (Type::Function(a, ar), Type::Function(e, er)) => {
            a.len() == e.len()
                && a.iter()
                    .zip(e)
                    .all(|(a, e)| argument(registry, Variance::Contravariant, e, a))
                && argument(registry, Variance::Covariant, er, ar)
        }
        _ => false,
    }
}
