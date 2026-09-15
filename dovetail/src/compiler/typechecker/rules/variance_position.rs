use std::collections::BTreeMap;

use crate::common::diagnostics::Diagnostics;
use crate::common::span::Span;
use crate::common::types::{TypeParamName, Variance};
use crate::typechecker::registry::{ClassTypeSignature, Registry, VariantPayload};
use crate::typechecker::types::{TraitBounds, Type};

/// Check that type parameters are used in positions consistent with their declared variance.
pub(super) fn check_variance_positions(registry: &Registry, merged_registry: &Registry, diagnostics: &mut Diagnostics) {
    // Check generic records
    for sig in registry.generic_record_types() {
        let declared = build_variance_map(&sig.type_params, &sig.type_param_variances);
        if declared.values().all(|v| *v == Variance::Invariant) {
            continue;
        }
        let type_name = &sig.fqn.symbol.0;
        for (_field_name, field_ty) in &sig.fields {
            check_type_in_position(
                field_ty,
                Variance::Covariant,
                &declared,
                type_name,
                &sig.span,
                diagnostics,
            );
        }
    }

    // Check generic newtypes
    for sig in registry.generic_newtype_types() {
        let declared = build_variance_map(&sig.type_params, &sig.type_param_variances);
        if declared.values().all(|v| *v == Variance::Invariant) {
            continue;
        }
        let type_name = &sig.fqn.symbol.0;
        check_type_in_position(
            &sig.inner_type,
            Variance::Covariant,
            &declared,
            type_name,
            &sig.span,
            diagnostics,
        );
    }

    // Check generic enums
    for sig in registry.generic_enum_types() {
        let declared = build_variance_map(&sig.type_params, &sig.type_param_variances);
        if declared.values().all(|v| *v == Variance::Invariant) {
            continue;
        }
        let type_name = &sig.fqn.symbol.0;
        for (_variant_name, payload) in &sig.variants {
            match payload {
                VariantPayload::None => {}
                VariantPayload::Tuple(types) => {
                    for ty in types {
                        check_type_in_position(
                            ty,
                            Variance::Covariant,
                            &declared,
                            type_name,
                            &sig.span,
                            diagnostics,
                        );
                    }
                }
                VariantPayload::Record(fields) => {
                    for (_name, ty) in fields {
                        check_type_in_position(
                            ty,
                            Variance::Covariant,
                            &declared,
                            type_name,
                            &sig.span,
                            diagnostics,
                        );
                    }
                }
            }
        }
    }

    // Check generic classes
    for sig in registry.generic_class_types() {
        let declared = build_variance_map(&sig.type_params, &sig.type_param_variances);
        if declared.values().all(|v| *v == Variance::Invariant) {
            continue;
        }
        let type_name = &sig.fqn.symbol.0;

        if let Some(parent) = &sig.parent_type_expr {
            check_type_in_position(parent, Variance::Covariant, &declared, type_name, &sig.span, diagnostics);
        }

        // Check fields: covariant for immutable, invariant for mutable
        for field in &sig.fields {
            let position = if field.mutable {
                Variance::Invariant
            } else {
                Variance::Covariant
            };
            check_type_in_position(
                &field.ty,
                position,
                &declared,
                type_name,
                &sig.span,
                diagnostics,
            );
        }

        // Check generic instance methods (on generic classes, all methods are stored here)
        for (_method_name, overloads) in &sig.generic_instance_methods {
            for def in overloads {
                if !sig.is_final && !def.is_final_method && def.method_type_params.is_empty() {
                    check_virtual_method_bounds(
                        &def.trait_bounds, sig, merged_registry, &declared, diagnostics,
                    );
                }
                // Check params (skip "self")
                for (param_name, param_ty) in &def.params {
                    if param_name == "self" {
                        continue;
                    }
                    check_type_in_position(
                        param_ty,
                        Variance::Contravariant,
                        &declared,
                        type_name,
                        &sig.span,
                        diagnostics,
                    );
                }
                // Check return type
                check_type_in_position(
                    &def.return_type,
                    Variance::Covariant,
                    &declared,
                    type_name,
                    &sig.span,
                    diagnostics,
                );
            }
        }
    }
}

/// A virtual call uses the original instantiation's method body. Changing a
/// conditional requirement through variance could make a stub callable, so
/// parameters occurring in those requirements must remain invariant.
fn check_virtual_method_bounds(
    method_bounds: &TraitBounds,
    class: &ClassTypeSignature,
    registry: &Registry,
    declared: &BTreeMap<TypeParamName, Variance>,
    diagnostics: &mut Diagnostics,
) {
    let evidence = Type::type_param_map(&class.type_params, &class.trait_bounds);
    let arguments: Vec<_> = class.type_params.iter().map(|parameter| evidence[&parameter.0].clone()).collect();
    let type_name = &class.fqn.symbol.0;
    let span = &class.span;
    for (parameter, bounds) in method_bounds.iter() {
        for bound in bounds {
            let mut requirement = TraitBounds::empty();
            requirement.insert(parameter.clone(), vec![bound.clone()]);
            if crate::typechecker::infer::generic_bounds_satisfied(
                registry, &class.fqn.package, &requirement, &class.type_params, &arguments,
            ) {
                continue;
            }
            check_type_in_position(
                &Type::TypeVariable(parameter.clone(), vec![]), Variance::Invariant,
                declared, type_name, span, diagnostics,
            );
            if let Some(bound) = bound.named() {
                for argument in bound.type_args.iter().chain(bound.associated_types.values()) {
                    check_type_in_position(
                        argument, Variance::Invariant, declared, type_name, span, diagnostics,
                    );
                }
            }
        }
    }
}

fn build_variance_map(
    type_params: &[TypeParamName],
    variances: &[Variance],
) -> BTreeMap<TypeParamName, Variance> {
    type_params
        .iter()
        .zip(variances.iter())
        .map(|(name, var)| (name.clone(), *var))
        .collect()
}

/// Compose two variances: the position variance and a parameter's declared variance.
fn compose_variance(position: Variance, param_variance: Variance) -> Variance {
    match (position, param_variance) {
        (Variance::Invariant, _) | (_, Variance::Invariant) => Variance::Invariant,
        (Variance::Covariant, v) => v,
        (Variance::Contravariant, Variance::Covariant) => Variance::Contravariant,
        (Variance::Contravariant, Variance::Contravariant) => Variance::Covariant,
    }
}

/// Check whether a declared variance is compatible with the position it appears in.
fn is_compatible(declared: Variance, position: Variance) -> bool {
    match declared {
        Variance::Invariant => true,
        Variance::Covariant => position == Variance::Covariant,
        Variance::Contravariant => position == Variance::Contravariant,
    }
}

/// Walk `ty` in `position`, reporting any declared type parameter that lands
/// somewhere its variance does not allow.
///
/// The generic arms below read each argument's variance straight out of the
/// resolved type. That is deliberate and load-bearing: `Type::Generic*` stores
/// `(Variance, Type)` pairs that `collect` filled in from the referenced
/// definition, having searched BOTH the package's own registry and the merged
/// dependency registry. Re-deriving the variance here from a registry instead
/// silently answered `Invariant` for every type declared in another package —
/// this phase only ever receives the package's own registry — so no covariant
/// type could hold a prelude `Option<T>` or `Result<T, E>` in any position.
/// The variance is already in the type; take it from there.
fn check_type_in_position(
    ty: &Type,
    position: Variance,
    declared: &BTreeMap<TypeParamName, Variance>,
    type_name: &str,
    span: &Span,
    diagnostics: &mut Diagnostics,
) {
    match ty {
        Type::TypeVariable(name, _bounds) | Type::GenericParam(name, _bounds, _) => {
            if let Some(&decl_var) = declared.get(name) {
                if !is_compatible(decl_var, position) {
                    diagnostics.error(
                        span.clone(),
                        format!(
                            "{} type parameter '{}' of '{}' cannot appear in {} position",
                            decl_var, name, type_name, position,
                        ),
                    );
                }
            }
        }
        Type::GenericRecord { type_args, .. } => {
            for (arg_var, arg_ty) in type_args.iter() {
                let composed = compose_variance(position, *arg_var);
                check_type_in_position(
                    arg_ty,
                    composed,
                    declared,
                    type_name,
                    span,
                    diagnostics,
                );
            }
        }
        Type::GenericEnum { type_args, .. } => {
            for (arg_var, arg_ty) in type_args.iter() {
                let composed = compose_variance(position, *arg_var);
                check_type_in_position(
                    arg_ty,
                    composed,
                    declared,
                    type_name,
                    span,
                    diagnostics,
                );
            }
        }
        Type::GenericNewtype { type_args, .. } => {
            for (arg_var, arg_ty) in type_args.iter() {
                let composed = compose_variance(position, *arg_var);
                check_type_in_position(
                    arg_ty,
                    composed,
                    declared,
                    type_name,
                    span,
                    diagnostics,
                );
            }
        }
        Type::GenericClass { type_args, .. } => {
            for (arg_var, arg_ty) in type_args.iter() {
                let composed = compose_variance(position, *arg_var);
                check_type_in_position(
                    arg_ty,
                    composed,
                    declared,
                    type_name,
                    span,
                    diagnostics,
                );
            }
        }
        Type::Array(elem) => {
            // Arrays are mutable, so invariant
            check_type_in_position(
                elem,
                Variance::Invariant,
                declared,
                type_name,
                span,
                diagnostics,
            );
        }
        Type::Function(params, ret) => {
            // Params are in contravariant position (flip), return is covariant (same)
            let flipped = match position {
                Variance::Covariant => Variance::Contravariant,
                Variance::Contravariant => Variance::Covariant,
                Variance::Invariant => Variance::Invariant,
            };
            for param in params {
                check_type_in_position(
                    param,
                    flipped,
                    declared,
                    type_name,
                    span,
                    diagnostics,
                );
            }
            check_type_in_position(
                ret,
                position,
                declared,
                type_name,
                span,
                diagnostics,
            );
        }
        Type::AssociatedProjection(projection) => {
            for parameter in projection.types() {
                check_type_in_position(parameter, Variance::Invariant, declared, type_name, span, diagnostics);
            }
        }
        Type::TupleProjection(receiver, _) => {
            check_type_in_position(receiver, Variance::Invariant, declared, type_name, span, diagnostics);
        }
        Type::TupleExtend(left, right) => {
            // Changing the left shape can change arity: extension is not covariant there.
            check_type_in_position(left, Variance::Invariant, declared, type_name, span, diagnostics);
            check_type_in_position(right, position, declared, type_name, span, diagnostics);
        }
        Type::Tuple(types, _) => {
            for t in types {
                check_type_in_position(
                    t,
                    position,
                    declared,
                    type_name,
                    span,
                    diagnostics,
                );
            }
        }
        // Primitives, Unit, Never, Any, Record, Enum, etc. — no type parameters to check
        _ => {}
    }
}
