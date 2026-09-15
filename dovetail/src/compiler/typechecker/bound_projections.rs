//! Establish projection evidence before resolving dependent where-clause types.

use std::collections::BTreeMap;

use super::types::{TraitBounds, Type};
use crate::parser::ast::{TraitConstraint, TypeBound};

type Scope = BTreeMap<String, Type>;

pub(super) fn resolve_scope(
    constraints: &[TraitConstraint],
    preliminary: &Scope,
    mut resolve: impl FnMut(&TraitConstraint, &Scope) -> Option<TraitBounds>,
) -> Scope {
    let mut scope = preliminary.clone();
    if !has_dependent_types(constraints) {
        return scope;
    }

    // Resolve individual bounds as their dependencies become available.
    // A worklist avoids declaration-order dependence and never recursively
    // expands mutually dependent constraints. The caller's final pass reports
    // any constraints that cannot be resolved, including invalid declarations.
    let mut pending: Vec<_> = constraints
        .iter()
        .flat_map(|constraint| {
            constraint.trait_bounds.iter().map(|bound| TraitConstraint {
                type_param: constraint.type_param.clone(),
                trait_bounds: vec![bound.clone()],
                span: constraint.span.clone(),
            })
        })
        .collect();
    let mut evidence = TraitBounds::empty();
    loop {
        let before = pending.len();
        pending.retain(|constraint| {
            let Some(resolved) = resolve(constraint, &scope) else {
                return true;
            };
            evidence.merge(&resolved);
            add_evidence(&mut scope, preliminary, &evidence);
            false
        });
        if pending.len() == before || pending.is_empty() {
            return scope;
        }
    }
}

fn add_evidence(scope: &mut Scope, preliminary: &Scope, evidence: &TraitBounds) {
    for (parameter, bounds) in evidence.iter() {
        let Some(ty) = scope.get_mut(&parameter.0) else {
            continue;
        };
        let (Type::TypeVariable(_, current) | Type::GenericParam(_, current, _)) = ty else {
            continue;
        };
        let mut merged = TraitBounds::empty();
        if let Some(Type::TypeVariable(_, original) | Type::GenericParam(_, original, _)) =
            preliminary.get(&parameter.0)
        {
            merged.insert(parameter.clone(), original.clone());
        }
        merged.insert(parameter.clone(), bounds.clone());
        *current = merged.get(parameter).cloned().unwrap_or_default();
    }
}

fn has_dependent_types(constraints: &[TraitConstraint]) -> bool {
    // Type aliases can hide projections, so inspect every bound application
    // containing types rather than only explicitly dotted source references.
    constraints.iter().any(|constraint| {
        constraint.trait_bounds.iter().any(|bound| match bound {
            TypeBound::Named(bound) => {
                !bound.type_args.is_empty() || !bound.associated_types.is_empty()
            }
            TypeBound::Class(_) => false,
        })
    })
}
