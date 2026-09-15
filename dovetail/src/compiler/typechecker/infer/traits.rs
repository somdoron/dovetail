use std::collections::BTreeMap;

use crate::common::span::Span;
use crate::common::types::{Fqn, SymbolName, TypeParamName};
use crate::parser::ast::TraitConstraint;
use crate::typechecker::registry::ImplBlockSignature;
use crate::typechecker::types::{
    BoundKind, NamedTraitBound, ResolvedImplMethod, TraitBound, TraitBounds, Type,
};

use super::Inference;
use super::type_param_substitution::TypeParamSubstitution;
use super::types::SymbolKind;

const MAX_TRAIT_CHECK_DEPTH: usize = 16;

/// Outcome of trait-directed impl-method resolution
/// (`resolve_trait_impl_method_for_type_detailed`).
/// Outcome of resolving one of the sugar traits (`Usable`, `Awaitable`)
/// for a type: found (with the extracted payload), ambiguous among distinct
/// sub-trait providers, or absent.
pub(super) enum SugarTraitResolution<T> {
    Found(T),
    Ambiguous(Vec<Fqn>),
    NotFound,
}

#[allow(
    clippy::large_enum_variant,
    reason = "Keep compiler data inline without adding allocations to this representation."
)]
pub(super) enum ImplMethodResolution {
    Found {
        resolved: ResolvedImplMethod,
        /// Substituted non-self params for arg checking; `None` when the
        /// generic path already validated the args during unification (or the
        /// class fallback resolved only the return type).
        params: Option<Vec<(String, Type)>>,
        return_type: Type,
    },
    Ambiguous,
    NotFound,
}

/// The declared application and associated types after implementation inference.
pub(super) struct ImplAssociatedTypes {
    pub(super) trait_args: Vec<Type>,
    pub(super) types: BTreeMap<String, Type>,
}

impl Inference<'_> {
    /// For a generic bound `T: I` instantiated with an intersection type: a
    /// bare-Self-returning member of `I` must produce `T` (the full
    /// intersection), but dynamic dispatch through the component vtable can
    /// only rebuild `I` itself. Returns the offending member name when the
    /// concrete type is a multi-component interface object and the trait
    /// declares such a member.
    /// When `concrete_type` satisfies `trait_fqn` only via sub-trait
    /// providers (no direct impl) and more than one distinct provider exists,
    /// return the providers — the choice of implementation is ambiguous and
    /// the user must implement the trait directly.
    pub(super) fn ambiguous_trait_providers(
        &self,
        trait_fqn: &Fqn,
        trait_type_args: &[Type],
        concrete_type: &Type,
    ) -> Option<Vec<Fqn>> {
        // Type params / interface objects dispatch statically through their
        // bound or component — no provider choice is made for them here.
        if matches!(
            concrete_type,
            Type::TypeVariable(..) | Type::GenericParam(..) | Type::InterfaceObject { .. }
        ) {
            return None;
        }
        let type_fqn = concrete_type.try_to_fqn()?;
        let args_compatible =
            |provided: &[Type]| trait_type_args.is_empty() || provided == trait_type_args;
        let applicable_blocks: Vec<_> = self
            .registry
            .find_providing_impl_blocks(trait_fqn, &type_fqn)
            .into_iter()
            .filter_map(|(block, via)| {
                let args = via
                    .as_ref()
                    .map_or(&block.trait_type_args, |(_, args)| args);
                let args = self.applicable_impl_args(block, concrete_type, args)?;
                if !args_compatible(&args) {
                    return None;
                }
                let provider_args =
                    self.applicable_impl_args(block, concrete_type, &block.trait_type_args)?;
                Some(((block.trait_fqn.clone(), provider_args), via.is_none()))
            })
            .collect();
        let class_applications: Vec<_> = self
            .registry
            .get_class_type(&type_fqn)
            .map(|sig| {
                let concrete_args = Self::extract_type_args(concrete_type).unwrap_or_default();
                let sub = TypeParamSubstitution::from_pairs(&sig.type_params, &concrete_args);
                sig.trait_impls
                    .iter()
                    .filter_map(|(fqn, args)| {
                        let args: Vec<_> = args
                            .iter()
                            .map(|arg| super::generics::apply_substitution(&sub, arg))
                            .collect();
                        let provided_args = if fqn == trait_fqn {
                            args.clone()
                        } else {
                            self.registry.super_closure_args(fqn, &args, trait_fqn)?
                        };
                        args_compatible(&provided_args).then_some((fqn.clone(), args))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if applicable_blocks.iter().any(|(_, direct)| *direct)
            || class_applications.iter().any(|(fqn, _)| fqn == trait_fqn)
        {
            return None;
        }
        let mut providers = Vec::new();
        for provider in applicable_blocks
            .iter()
            .map(|(provider, _)| provider)
            .chain(class_applications.iter())
        {
            if !providers.contains(provider) {
                providers.push(provider.clone());
            }
        }
        (providers.len() > 1).then(|| providers.into_iter().map(|(fqn, _)| fqn).collect())
    }

    /// Resolve an impl's provided application only when its receiver and
    /// where-clause bounds apply to this concrete type.
    fn applicable_impl_args(
        &self,
        block: &crate::typechecker::registry::ImplBlockSignature,
        concrete_type: &Type,
        provided_args: &[Type],
    ) -> Option<Vec<Type>> {
        if block.type_params.is_empty() {
            return (block.for_type == *concrete_type).then(|| provided_args.to_vec());
        }
        let mut sub = TypeParamSubstitution::new();
        if !sub.unify(&block.for_type, concrete_type) {
            return None;
        }
        self.infer_associated_bound_types(&block.trait_bounds, &mut sub);
        let type_args = sub.resolve_type_params(&block.type_params)?;
        let sub = TypeParamSubstitution::from_pairs(&block.type_params, &type_args);
        for (param, bounds) in block.trait_bounds.iter() {
            let Some(index) = block.type_params.iter().position(|p| p == param) else {
                continue;
            };
            for bound in bounds {
                let Some(bound) = bound.named() else {
                    if !type_args[index].is_class_reference() {
                        return None;
                    }
                    continue;
                };
                let mut bound = bound.clone();
                bound.associated_types = bound
                    .associated_types
                    .iter()
                    .map(|(n, t)| (n.clone(), super::generics::apply_substitution(&sub, t)))
                    .collect();
                let args: Vec<_> = bound
                    .type_args
                    .iter()
                    .map(|arg| super::generics::apply_substitution(&sub, arg))
                    .collect();
                if !self.type_satisfies_bound(&bound, &args, &type_args[index], 0) {
                    return None;
                }
            }
        }
        Some(
            provided_args
                .iter()
                .map(|arg| super::generics::apply_substitution(&sub, arg))
                .collect(),
        )
    }

    fn intersection_blocks_self_returning_bound(
        &self,
        trait_fqn: &Fqn,
        concrete_type: &Type,
    ) -> Option<String> {
        let Type::InterfaceObject { traits, .. } = concrete_type else {
            return None;
        };
        if traits.len() <= 1 {
            return None;
        }
        let sig = self.registry.lookup_trait(trait_fqn, &trait_fqn.package)?;
        sig.methods
            .iter()
            .find(|m| matches!(m.return_type, Type::SelfType))
            .map(|m| m.name.clone())
            .or_else(|| {
                sig.properties
                    .iter()
                    .find(|p| matches!(p.return_type, Type::SelfType))
                    .map(|p| p.name.clone())
            })
    }

    /// Apply the appropriate relation for a resolved where-clause bound.
    /// Subtype bounds use assignability; trait bounds use implementation lookup.
    pub(super) fn type_satisfies_bound(
        &self,
        bound: &NamedTraitBound,
        type_args: &[Type],
        concrete_type: &Type,
        depth: usize,
    ) -> bool {
        if bound.kind != BoundKind::SubtypeOf {
            return self.type_satisfies_trait_with_associated_types(
                &bound.trait_fqn,
                type_args,
                &bound.associated_types,
                concrete_type,
                depth,
            );
        }
        if let Some(primitive) = bound.primitive_subtype() {
            return crate::typechecker::subtyping::is_subtype(
                self.registry,
                concrete_type,
                &primitive,
            );
        }
        match concrete_type {
            Type::Class(fqn, _) | Type::GenericClass { fqn, .. } => {
                self.registry.class_is_subtype(fqn, &bound.trait_fqn)
            }
            Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) => bounds
                .iter()
                .filter_map(TraitBound::named)
                .any(|candidate| {
                    candidate.kind == BoundKind::SubtypeOf
                        && self
                            .registry
                            .class_is_subtype(&candidate.trait_fqn, &bound.trait_fqn)
                }),
            _ => false,
        }
    }

    /// Check whether a concrete type satisfies a trait bound with specific type args.
    /// When `trait_type_args` is empty, just checks that the type implements the trait.
    /// When non-empty, verifies the impl matches those specific type args
    /// (e.g., `From<Int32>` not just `From<anything>`).
    /// Uses depth-limited recursion to prevent infinite loops.
    ///
    /// NOTE: This is a pure check — it does NOT instantiate generic blocks. When trait
    /// object codegen is implemented (Phase 9), `is_assignable` or `InterfaceObjectCoerce`
    /// insertion should trigger instantiation of the trait's methods for the concrete type.
    pub(super) fn type_satisfies_trait(
        &self,
        trait_fqn: &Fqn,
        trait_type_args: &[Type],
        concrete_type: &Type,
        depth: usize,
    ) -> bool {
        self.type_satisfies_trait_with_associated_types(
            trait_fqn,
            trait_type_args,
            &Default::default(),
            concrete_type,
            depth,
        )
    }

    pub(super) fn type_satisfies_trait_with_associated_types(
        &self,
        trait_fqn: &Fqn,
        trait_type_args: &[Type],
        associated_types: &std::collections::BTreeMap<String, Type>,
        concrete_type: &Type,
        depth: usize,
    ) -> bool {
        if depth > MAX_TRAIT_CHECK_DEPTH {
            return false;
        }
        if crate::typechecker::types::is_tuple_constraint(trait_fqn) {
            return trait_type_args.is_empty()
                && associated_types.is_empty()
                && concrete_type.is_tuple();
        }

        // If the concrete type is a type parameter, check its declared bounds.
        // When the expected trait application has type args, the bound's args
        // must match — `T: Producer<Int32>` does not satisfy `Producer<String>`.
        if let Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _) = concrete_type {
            let args_match = |args: &[Type]| {
                trait_type_args.is_empty()
                    || (args.len() == trait_type_args.len()
                        && args
                            .iter()
                            .zip(trait_type_args.iter())
                            .all(|(x, y)| self.is_assignable(x, y) && self.is_assignable(y, x)))
            };
            return bounds.iter().filter_map(TraitBound::named).any(|b| {
                if !associated_types.iter().all(|(name, ty)| {
                    b.associated_types.get(name).is_some_and(|actual| {
                        self.is_assignable(actual, ty) && self.is_assignable(ty, actual)
                    })
                }) {
                    return false;
                }
                if b.trait_fqn == *trait_fqn && args_match(&b.type_args) {
                    return true;
                }
                // `T: B` satisfies `T: A` when A is in B's super closure
                // (with the closure-substituted args matching).
                self.registry
                    .super_closure_args(&b.trait_fqn, &b.type_args, trait_fqn)
                    .is_some_and(|args| args_match(&args))
            });
        }

        if concrete_type.is_error() {
            return false;
        }

        // An interface-object value satisfies a trait iff that trait is one of
        // its components (with assignable type args): `(A and B)` satisfies `A`.
        // Also keeps `to_fqn()` (single-FQN) off intersection types.
        if let Type::InterfaceObject { traits, .. } = concrete_type {
            // Component type args are invariant (erased slots flow both ways).
            return associated_types.is_empty()
                && traits.iter().any(|c| {
                    if c.trait_fqn == *trait_fqn && c.trait_type_args == trait_type_args {
                        return true;
                    }
                    // A `B`-object satisfies `A` when A is in B's super closure.
                    self.registry
                        .super_closure_args(&c.trait_fqn, &c.trait_type_args, trait_fqn)
                        .is_some_and(|args| args == trait_type_args)
                });
        }

        let type_fqn = concrete_type.to_fqn();
        let impls = self.registry.find_impl_blocks(trait_fqn, &type_fqn);

        // Operator implementations are chosen by their operands. An Output
        // equality must not make an otherwise ambiguous application acceptable.
        let is_indexed_or_binary_operator = trait_fqn.package.to_string() == "standard.prelude"
            && matches!(
                trait_fqn.symbol.0.as_str(),
                "Add" | "Sub" | "Mul" | "Div" | "Concat" | "Index" | "IndexSet"
            )
            && !trait_type_args.is_empty();
        if is_indexed_or_binary_operator || !associated_types.is_empty() {
            let mut active = vec![(
                trait_fqn.clone(),
                concrete_type.clone(),
                trait_type_args.to_vec(),
            )];
            let outputs = self.associated_outputs_for_trait(
                trait_fqn,
                concrete_type,
                trait_type_args,
                depth,
                &mut active,
            );
            return (!is_indexed_or_binary_operator || outputs.len() == 1)
                && outputs.iter().any(|output| {
                    associated_types
                        .iter()
                        .all(|(name, expected)| output.get(name) == Some(expected))
                });
        }

        for info in impls {
            if info.type_params.is_empty() {
                if info.for_type != *concrete_type
                    || !associated_types.iter().all(|(name, ty)| {
                        info.associated_type_defs
                            .get(name)
                            .is_some_and(|(params, actual)| params.is_empty() && actual == ty)
                    })
                {
                    continue;
                }
                // Concrete impl
                if trait_type_args.is_empty() {
                    // No type args to check — always satisfies
                    return true;
                }
                // Check that impl's trait_type_args match the expected ones
                if info.trait_type_args.len() == trait_type_args.len()
                    && info
                        .trait_type_args
                        .iter()
                        .zip(trait_type_args.iter())
                        .all(|(impl_arg, expected_arg)| self.is_assignable(impl_arg, expected_arg))
                {
                    return true;
                }
                continue;
            }

            // Generic impl — unify for_type against concrete_type
            let mut sub = TypeParamSubstitution::new();
            if !sub.unify(&info.for_type, concrete_type) {
                continue;
            }
            // Some implementation parameters occur only in the trait arguments,
            // such as Add<Value<R>> for a non-generic receiver.
            if !trait_type_args.is_empty()
                && (info.trait_type_args.len() != trait_type_args.len()
                    || !info
                        .trait_type_args
                        .iter()
                        .zip(trait_type_args)
                        .all(|(pattern, actual)| sub.unify(pattern, actual)))
            {
                continue;
            }

            if sub.resolve_type_params(&info.type_params).is_none() {
                self.infer_associated_bound_types_at_depth(
                    &info.trait_bounds,
                    &mut sub,
                    depth + 1,
                    &mut Vec::new(),
                );
            }
            let resolved = sub.resolve_type_params(&info.type_params);
            if let Some(type_args) = resolved {
                // If we have expected trait type args, substitute and verify
                if !trait_type_args.is_empty() {
                    let resolved_sub =
                        TypeParamSubstitution::from_pairs(&info.type_params, &type_args);
                    let substituted_trait_args: Vec<Type> = info
                        .trait_type_args
                        .iter()
                        .map(|t| super::generics::apply_substitution(&resolved_sub, t))
                        .collect();

                    let args_match = substituted_trait_args.len() == trait_type_args.len()
                        && substituted_trait_args
                            .iter()
                            .zip(trait_type_args.iter())
                            .all(|(impl_arg, expected_arg)| {
                                self.is_assignable(impl_arg, expected_arg)
                            });

                    if !args_match {
                        continue;
                    }
                }

                let output_sub = TypeParamSubstitution::from_pairs(&info.type_params, &type_args);
                if !associated_types.iter().all(|(name, ty)| {
                    info.associated_type_defs
                        .get(name)
                        .is_some_and(|(params, actual)| {
                            params.is_empty()
                                && super::generics::apply_substitution(&output_sub, actual) == *ty
                        })
                }) {
                    continue;
                }

                // Check all where-clause bounds recursively
                let resolved_sub = TypeParamSubstitution::from_pairs(&info.type_params, &type_args);
                let all_bounds_ok = info.trait_bounds.iter().all(|(tp, required_bounds)| {
                    let idx = info.type_params.iter().position(|p| p == tp);
                    let Some(i) = idx else { return true };
                    let bound_type = &type_args[i];
                    required_bounds.iter().all(|bound| {
                        let Some(bound) = bound.named() else {
                            return bound_type.is_class_reference();
                        };
                        if bound.kind == BoundKind::SubtypeOf {
                            return self.is_assignable(
                                &bound.primitive_subtype().unwrap_or_else(|| {
                                    Type::Class(
                                        bound.trait_fqn.clone(),
                                        crate::common::types::MangledName::for_type(
                                            &bound.trait_fqn,
                                        ),
                                    )
                                }),
                                bound_type,
                            );
                        }
                        let substituted_args: Vec<Type> = bound
                            .type_args
                            .iter()
                            .map(|t| super::generics::apply_substitution(&resolved_sub, t))
                            .collect();
                        self.type_satisfies_trait_with_associated_types(
                            &bound.trait_fqn,
                            &substituted_args,
                            &bound
                                .associated_types
                                .iter()
                                .map(|(n, t)| {
                                    (
                                        n.clone(),
                                        super::generics::apply_substitution(&resolved_sub, t),
                                    )
                                })
                                .collect(),
                            bound_type,
                            if concrete_type.has_tuple_subterm(bound_type) {
                                depth
                            } else {
                                depth + 1
                            },
                        )
                    })
                });
                if all_bounds_ok {
                    return true;
                }
            }
        }

        // Sub-trait providers: `implement B for T` satisfies A when A is in
        // B's super closure ("B satisfies A everywhere").
        for (block, via) in self
            .registry
            .find_providing_impl_blocks(trait_fqn, &type_fqn)
        {
            let Some((_, via_args)) = via else { continue }; // direct blocks handled above
            if block.type_params.is_empty() {
                if !associated_types.iter().all(|(name, ty)| {
                    block
                        .associated_type_defs
                        .get(name)
                        .is_some_and(|(params, actual)| params.is_empty() && actual == ty)
                }) {
                    continue;
                }
                if block.for_type != *concrete_type {
                    continue;
                }
                // Non-generic provider: via_args are concrete already.
                if trait_type_args.is_empty()
                    || (via_args.len() == trait_type_args.len()
                        && via_args
                            .iter()
                            .zip(trait_type_args.iter())
                            .all(|(a, e)| self.is_assignable(a, e)))
                {
                    return true;
                }
                continue;
            }
            // Generic provider: unify for_type, then substitute and verify.
            let mut sub = TypeParamSubstitution::new();
            if !sub.unify(&block.for_type, concrete_type) {
                continue;
            }
            if !trait_type_args.is_empty()
                && (via_args.len() != trait_type_args.len()
                    || !via_args
                        .iter()
                        .zip(trait_type_args)
                        .all(|(p, a)| sub.unify(p, a)))
            {
                continue;
            }
            self.infer_associated_bound_types_at_depth(
                &block.trait_bounds,
                &mut sub,
                depth + 1,
                &mut Vec::new(),
            );
            let Some(type_args) = sub.resolve_type_params(&block.type_params) else {
                continue;
            };
            let resolved_sub = TypeParamSubstitution::from_pairs(&block.type_params, &type_args);
            if !trait_type_args.is_empty() {
                let substituted: Vec<Type> = via_args
                    .iter()
                    .map(|t| super::generics::apply_substitution(&resolved_sub, t))
                    .collect();
                let args_match = substituted.len() == trait_type_args.len()
                    && substituted
                        .iter()
                        .zip(trait_type_args.iter())
                        .all(|(a, e)| self.is_assignable(a, e));
                if !args_match {
                    continue;
                }
            }
            if !associated_types.iter().all(|(name, ty)| {
                block
                    .associated_type_defs
                    .get(name)
                    .is_some_and(|(params, actual)| {
                        params.is_empty()
                            && super::generics::apply_substitution(&resolved_sub, actual) == *ty
                    })
            }) {
                continue;
            }
            let all_bounds_ok = block.trait_bounds.iter().all(|(tp, required_bounds)| {
                let idx = block.type_params.iter().position(|p| p == tp);
                let Some(i) = idx else { return true };
                let bound_type = &type_args[i];
                required_bounds.iter().all(|bound| {
                    let Some(bound) = bound.named() else {
                        return bound_type.is_class_reference();
                    };
                    let mut bound = bound.clone();
                    bound.associated_types = bound
                        .associated_types
                        .iter()
                        .map(|(n, t)| {
                            (
                                n.clone(),
                                super::generics::apply_substitution(&resolved_sub, t),
                            )
                        })
                        .collect();
                    let substituted_args: Vec<Type> = bound
                        .type_args
                        .iter()
                        .map(|t| super::generics::apply_substitution(&resolved_sub, t))
                        .collect();
                    let bound_depth = if concrete_type.has_tuple_subterm(bound_type) {
                        depth
                    } else {
                        depth + 1
                    };
                    self.type_satisfies_bound(&bound, &substituted_args, bound_type, bound_depth)
                })
            });
            if all_bounds_ok {
                return true;
            }
        }

        if let Some(class_sig) = self
            .registry
            .get_class_type(&type_fqn)
            .filter(|_| associated_types.is_empty())
        {
            for (impl_trait_fqn, impl_trait_type_args) in &class_sig.trait_impls {
                if impl_trait_fqn != trait_fqn {
                    // `class C implements B` satisfies A via B's super closure.
                    let effective_impl_args: Vec<Type> = if !class_sig.type_params.is_empty() {
                        match concrete_type {
                            Type::GenericClass {
                                type_args: class_type_args,
                                ..
                            } => {
                                let concrete_args: Vec<Type> =
                                    class_type_args.iter().map(|(_, t)| t.clone()).collect();
                                let sub = TypeParamSubstitution::from_pairs(
                                    &class_sig.type_params,
                                    &concrete_args,
                                );
                                impl_trait_type_args
                                    .iter()
                                    .map(|t| super::generics::apply_substitution(&sub, t))
                                    .collect()
                            }
                            _ => impl_trait_type_args.clone(),
                        }
                    } else {
                        impl_trait_type_args.clone()
                    };
                    if let Some(via_args) = self.registry.super_closure_args(
                        impl_trait_fqn,
                        &effective_impl_args,
                        trait_fqn,
                    ) && (trait_type_args.is_empty()
                        || (via_args.len() == trait_type_args.len()
                            && via_args
                                .iter()
                                .zip(trait_type_args.iter())
                                .all(|(a, e)| self.is_assignable(a, e))))
                    {
                        return true;
                    }
                    continue;
                }
                if trait_type_args.is_empty() {
                    return true;
                }
                let effective_args = if !class_sig.type_params.is_empty() {
                    match concrete_type {
                        Type::GenericClass {
                            type_args: class_type_args,
                            ..
                        } => {
                            let concrete_args: Vec<Type> =
                                class_type_args.iter().map(|(_, t)| t.clone()).collect();
                            let sub = TypeParamSubstitution::from_pairs(
                                &class_sig.type_params,
                                &concrete_args,
                            );
                            impl_trait_type_args
                                .iter()
                                .map(|t| super::generics::apply_substitution(&sub, t))
                                .collect::<Vec<_>>()
                        }
                        _ => impl_trait_type_args.clone(),
                    }
                } else {
                    impl_trait_type_args.clone()
                };
                if effective_args.len() == trait_type_args.len()
                    && effective_args
                        .iter()
                        .zip(trait_type_args.iter())
                        .all(|(impl_arg, expected_arg)| self.is_assignable(impl_arg, expected_arg))
                {
                    return true;
                }
            }
        }

        false
    }

    /// Resolve a trait impl method for a concrete receiver type.
    /// Tries generic impls first (via `resolve_generic_trait_impl_instance`),
    /// falls back to concrete impl lookup in the registry.
    /// Returns `(ResolvedImplMethod, return_type)` or `None`.
    pub(super) fn resolve_trait_impl_method_for_type(
        &mut self,
        receiver_ty: &Type,
        trait_fqn: &Fqn,
        method_name: &str,
        arg_types: &[&Type],
    ) -> Option<(ResolvedImplMethod, Type)> {
        match self.resolve_trait_impl_method_for_type_detailed(
            receiver_ty,
            trait_fqn,
            method_name,
            arg_types,
            &[],
        ) {
            ImplMethodResolution::Found {
                resolved,
                return_type,
                ..
            } => Some((resolved, return_type)),
            ImplMethodResolution::Ambiguous | ImplMethodResolution::NotFound => None,
        }
    }

    /// Can `block` apply to `concrete`? Non-generic blocks apply to their
    /// exact for_type only (sibling instantiations share a base FQN); generic
    /// blocks apply when their for_type unifies with the concrete type.
    fn impl_block_applies(
        block: &crate::typechecker::registry::ImplBlockSignature,
        concrete: &Type,
    ) -> bool {
        if block.type_params.is_empty() {
            block.for_type == *concrete
        } else {
            let mut sub = TypeParamSubstitution::new();
            sub.unify(&block.for_type, concrete)
        }
    }

    /// Trait-directed impl method resolution with enough detail for explicit
    /// `TraitName.method(receiver, args...)` calls: substituted params for arg
    /// checking (when the concrete path resolved them) and a distinguishable
    /// ambiguous outcome. `required_trait_args`, when non-empty, restricts
    /// resolution to blocks of that trait application (the explicit
    /// `Conv<Bool>.tag(r)` form on sibling instantiations).
    pub(super) fn resolve_trait_impl_method_for_type_detailed(
        &mut self,
        receiver_ty: &Type,
        trait_fqn: &Fqn,
        method_name: &str,
        arg_types: &[&Type],
        required_trait_args: &[Type],
    ) -> ImplMethodResolution {
        if receiver_ty.is_error() {
            return ImplMethodResolution::NotFound;
        }
        if let Some(bounds) = self.type_parameter_trait_applications(receiver_ty, trait_fqn) {
            return self.resolve_bound_trait_method(
                receiver_ty,
                trait_fqn,
                &bounds,
                method_name,
                arg_types,
                required_trait_args,
            );
        }
        // Intersections have no single FQN and no direct impl blocks.
        let Some(type_fqn) = receiver_ty.try_to_fqn() else {
            return ImplMethodResolution::NotFound;
        };

        let method_sym = SymbolName(method_name.to_string());

        // Try generic impl resolution first (handles unification, trait bounds).
        // Keep only results belonging to the requested trait — the lookup is
        // name-keyed and may surface a different trait's method.
        let generic_results: Vec<_> = self
            .resolve_generic_trait_impl_instance(
                receiver_ty,
                &method_sym,
                arg_types,
                &[],
                Some((trait_fqn, required_trait_args)),
            )
            .into_iter()
            .filter_map(|r| match r {
                super::ResolvedFunction::ImplMethod {
                    resolved,
                    return_type,
                } if resolved.trait_fqn == *trait_fqn
                    && (required_trait_args.is_empty()
                        || resolved.trait_type_params == required_trait_args) =>
                {
                    Some((resolved, return_type))
                }
                _ => None,
            })
            .collect();

        // Args-aware direct gate: a direct impl of a DIFFERENT application
        // (`Conv<Int32>` when the caller asked for `Conv<Bool>`) must not hide
        // providers of the requested one.
        // For_type-aware: a sibling block (`A for Wrap<Int32>` when the
        // receiver is `Wrap<String>`) is not a direct impl of THIS type.
        let has_exact_direct = self
            .registry
            .find_impl_blocks(trait_fqn, &type_fqn)
            .into_iter()
            .any(|b| {
                self.applicable_impl_args(b, receiver_ty, &b.trait_type_args)
                    .is_some()
                    && (required_trait_args.is_empty() || b.trait_type_args == required_trait_args)
            })
            || self.registry.get_class_type(&type_fqn).is_some_and(|sig| {
                sig.trait_impls.iter().any(|(t, args)| {
                    t == trait_fqn
                        && (required_trait_args.is_empty() || args == required_trait_args)
                })
            });
        // A generic direct impl MIGHT instantiate to the requested application
        // — the resolution below unifies and filters by substituted args. It
        // might also unify to a DIFFERENT application (`Conv<T> for Wrap<T>`
        // giving only `Conv<Int32>` for `Wrap<Int32>` when `Conv<Bool>` was
        // asked): then the tail falls back to provider routing.
        let generic_direct_candidate = !required_trait_args.is_empty()
            && !has_exact_direct
            && (self
                .registry
                .find_impl_blocks(trait_fqn, &type_fqn)
                .into_iter()
                .any(|b| {
                    !b.type_params.is_empty()
                        && self
                            .applicable_impl_args(b, receiver_ty, &b.trait_type_args)
                            .is_some()
                })
                || self.registry.get_class_type(&type_fqn).is_some_and(|sig| {
                    sig.trait_impls.iter().any(|(t, args)| {
                        t == trait_fqn && args.iter().any(|a| a.contains_type_parameter())
                    })
                }));
        if !has_exact_direct && !generic_direct_candidate && generic_results.is_empty() {
            return self.route_through_super_providers(
                receiver_ty,
                &type_fqn,
                trait_fqn,
                method_name,
                arg_types,
                required_trait_args,
            );
        }

        if generic_results.len() > 1 {
            return ImplMethodResolution::Ambiguous;
        }
        let generic_candidate = generic_results.into_iter().next();

        // Concrete impl — build ResolvedImplMethod from registry info.
        // Filter to impls of the requested trait, then disambiguate among them
        // by arg types (a type may have multiple impls of the same trait with
        // different trait-type-args, e.g. `Div<String> for Path` and
        // `Div<Path> for Path`).
        let concrete_impls = self.registry.find_impl_method(&type_fqn, &method_sym);
        let candidates: Vec<_> = concrete_impls
            .iter()
            .filter(|(b, m)| {
                b.trait_fqn == *trait_fqn
                    && self.is_assignable(&b.for_type, receiver_ty)
                    && b.type_params.is_empty()
                    && b.for_type == *receiver_ty
                    && m.method_type_params.is_empty()
                    && (required_trait_args.is_empty() || b.trait_type_args == required_trait_args)
            })
            .collect();
        let mut matching = candidates.into_iter().filter(|(_, m)| {
            let non_self = if m.params.first().is_some_and(|p| p.0 == "self") {
                &m.params[1..]
            } else {
                &m.params[..]
            };
            non_self.len() == arg_types.len()
                && non_self
                    .iter()
                    .zip(arg_types)
                    .all(|((_, p), a)| self.is_assignable(p, a))
        });
        let pick = matching.next();
        if matching.next().is_some() || (pick.is_some() && generic_candidate.is_some()) {
            return ImplMethodResolution::Ambiguous;
        }
        if let Some((block, m)) = pick {
            let resolved = ResolvedImplMethod {
                trait_fqn: block.trait_fqn.clone(),
                trait_type_params: block.trait_type_args.clone(),
                for_type: receiver_ty.clone(),
                method_name: m.dispatch_name.clone(),
                method_type_params: vec![],
            };
            let non_self_params: Vec<(String, Type)> =
                if !m.params.is_empty() && m.params[0].0 == "self" {
                    m.params[1..].to_vec()
                } else {
                    m.params.clone()
                };
            return ImplMethodResolution::Found {
                resolved,
                params: Some(non_self_params),
                return_type: m.return_type.clone(),
            };
        }

        if let Some((resolved, return_type)) = generic_candidate {
            return ImplMethodResolution::Found {
                resolved,
                params: None,
                return_type,
            };
        }

        if let Some(class_sig) = self.registry.get_class_type(&type_fqn) {
            // Substitute the class's type args into an implemented trait's
            // args, then require a match against the REQUESTED application —
            // `Conv<Bool>.tag(h)` on `Holder<Int32> implements Conv<T>` must
            // not silently bind Conv<Int32>.
            let substitute_impl_args = |impl_trait_type_args: &Vec<Type>| -> Vec<Type> {
                if class_sig.type_params.is_empty() {
                    return impl_trait_type_args.clone();
                }
                match receiver_ty {
                    Type::GenericClass {
                        type_args: class_type_args,
                        ..
                    } => {
                        let concrete_args: Vec<Type> =
                            class_type_args.iter().map(|(_, t)| t.clone()).collect();
                        let sub = TypeParamSubstitution::from_pairs(
                            &class_sig.type_params,
                            &concrete_args,
                        );
                        impl_trait_type_args
                            .iter()
                            .map(|t| super::generics::apply_substitution(&sub, t))
                            .collect()
                    }
                    _ => impl_trait_type_args.clone(),
                }
            };
            let matching_trait_impl = class_sig.trait_impls.iter().find_map(|(t, args)| {
                if t != trait_fqn {
                    return None;
                }
                let substituted = substitute_impl_args(args);
                if !required_trait_args.is_empty() && substituted != required_trait_args {
                    return None;
                }
                Some(substituted)
            });
            if let Some(trait_type_params) = matching_trait_impl {
                let raw_return_type = {
                    let mut result = None;
                    let mut current_fqn = type_fqn.clone();
                    while let Some(sig) = self.registry.get_class_type(&current_fqn) {
                        if let Some(overloads) = sig.instance_methods.get(&method_sym)
                            && let Some(s) = overloads.first()
                        {
                            result = Some(s.return_type.clone());
                            break;
                        }
                        if let Some(defs) = sig.generic_instance_methods.get(&method_sym)
                            && let Some(d) = defs.first()
                        {
                            result = Some(d.return_type.clone());
                            break;
                        }
                        match &sig.parent_class {
                            Some(parent) => current_fqn = parent.clone(),
                            None => break,
                        }
                    }
                    result
                };

                if let Some(raw_return_type) = raw_return_type {
                    let return_type = if !class_sig.type_params.is_empty() {
                        match receiver_ty {
                            Type::GenericClass {
                                type_args: class_type_args,
                                ..
                            } => {
                                let concrete_args: Vec<Type> =
                                    class_type_args.iter().map(|(_, t)| t.clone()).collect();
                                let sub = TypeParamSubstitution::from_pairs(
                                    &class_sig.type_params,
                                    &concrete_args,
                                );
                                super::generics::apply_substitution(&sub, &raw_return_type)
                            }
                            _ => raw_return_type,
                        }
                    } else {
                        raw_return_type
                    };

                    let resolved = ResolvedImplMethod {
                        trait_fqn: trait_fqn.clone(),
                        trait_type_params,
                        for_type: receiver_ty.clone(),
                        method_name: method_sym,
                        method_type_params: vec![],
                    };
                    return ImplMethodResolution::Found {
                        resolved,
                        params: None,
                        return_type,
                    };
                }
            }
        }

        if generic_direct_candidate {
            // The generic direct impl did not instantiate to the requested
            // application for this receiver — a sub-trait provider may still
            // supply it.
            return self.route_through_super_providers(
                receiver_ty,
                &type_fqn,
                trait_fqn,
                method_name,
                arg_types,
                required_trait_args,
            );
        }
        ImplMethodResolution::NotFound
    }

    /// `extends` routing: no usable direct impl, but a sub-trait provider may
    /// carry the member inline. Direct impls always win; a unique provider
    /// routes; several distinct providers are ambiguous. Only providers of
    /// the REQUESTED trait application compete — `Conv<Bool>.tag(r)` must not
    /// route through a `Conv<Int32>` provider, and explicit args uniquely
    /// select among providers of different applications.
    fn route_through_super_providers(
        &mut self,
        receiver_ty: &Type,
        type_fqn: &Fqn,
        trait_fqn: &Fqn,
        method_name: &str,
        arg_types: &[&Type],
        required_trait_args: &[Type],
    ) -> ImplMethodResolution {
        let args_match =
            |args: &[Type]| required_trait_args.is_empty() || args == required_trait_args;
        let mut providers: Vec<(Fqn, Vec<Type>)> = Vec::new();
        for (block, via) in self
            .registry
            .find_providing_impl_blocks(trait_fqn, type_fqn)
        {
            let Some((_, closure_args)) = via else {
                continue;
            };
            let Some(args) = self.applicable_impl_args(block, receiver_ty, &closure_args) else {
                continue;
            };
            if !args_match(&args) {
                continue;
            }
            let Some(provider_args) =
                self.applicable_impl_args(block, receiver_ty, &block.trait_type_args)
            else {
                continue;
            };
            let entry = (block.trait_fqn.clone(), provider_args);
            if !providers.contains(&entry) {
                providers.push(entry);
            }
        }
        if let Some(class_sig) = self.registry.get_class_type(type_fqn) {
            let concrete_args = Self::extract_type_args(receiver_ty).unwrap_or_default();
            let sub = TypeParamSubstitution::from_pairs(&class_sig.type_params, &concrete_args);
            for (impl_trait_fqn, impl_args) in &class_sig.trait_impls {
                if impl_trait_fqn == trait_fqn {
                    continue;
                }
                let impl_args: Vec<_> = impl_args
                    .iter()
                    .map(|arg| super::generics::apply_substitution(&sub, arg))
                    .collect();
                let Some(args) =
                    self.registry
                        .super_closure_args(impl_trait_fqn, &impl_args, trait_fqn)
                else {
                    continue;
                };
                if args_match(&args) {
                    let entry = (impl_trait_fqn.clone(), impl_args);
                    if !providers.contains(&entry) {
                        providers.push(entry);
                    }
                }
            }
        }
        match providers.len() {
            0 => ImplMethodResolution::NotFound,
            1 => {
                // Explicit args survive the filter only after concretizing the
                // provider's application (unified for generic blocks), so
                // forwarding them is safe; an unconstrained call keeps the
                // unconstrained recursion (a generic provider's raw args must
                // not be required).
                let (provider_fqn, provider_args) = &providers[0];
                let recurse_args: &[Type] = if required_trait_args.is_empty()
                    || provider_args.iter().any(|t| t.contains_type_parameter())
                {
                    &[]
                } else {
                    provider_args
                };
                self.resolve_trait_impl_method_for_type_detailed(
                    receiver_ty,
                    provider_fqn,
                    method_name,
                    arg_types,
                    recurse_args,
                )
            }
            _ => ImplMethodResolution::Ambiguous,
        }
    }

    /// Resolve the requested application through a parameter's direct or inherited bounds.
    pub(super) fn type_parameter_trait_applications(
        &self,
        receiver: &Type,
        trait_fqn: &Fqn,
    ) -> Option<Vec<NamedTraitBound>> {
        let (Type::TypeVariable(_, bounds) | Type::GenericParam(_, bounds, _)) = receiver else {
            return None;
        };
        let mut direct = Vec::new();
        let mut inherited = Vec::new();
        for bound in bounds.iter().filter_map(TraitBound::named) {
            let is_direct = bound.trait_fqn == *trait_fqn;
            let args = if is_direct {
                bound.type_args.clone()
            } else {
                let Some(args) =
                    self.registry
                        .super_closure_args(&bound.trait_fqn, &bound.type_args, trait_fqn)
                else {
                    continue;
                };
                args
            };
            let application = NamedTraitBound {
                trait_fqn: trait_fqn.clone(),
                type_args: args.iter().map(|arg| self.scoped_bound_type(arg)).collect(),
                associated_types: bound
                    .associated_types
                    .iter()
                    .map(|(name, ty)| (name.clone(), self.scoped_bound_type(ty)))
                    .collect(),
                kind: bound.kind.clone(),
            };
            let target = if is_direct {
                &mut direct
            } else {
                &mut inherited
            };
            if !target.contains(&application) {
                target.push(application);
            }
        }
        inherited.retain(|provided| {
            !direct
                .iter()
                .any(|bound| bound.type_args == provided.type_args)
        });
        direct.extend(inherited);
        Some(direct)
    }

    fn resolve_bound_trait_method(
        &self,
        receiver: &Type,
        trait_fqn: &Fqn,
        bounds: &[NamedTraitBound],
        method_name: &str,
        arg_types: &[&Type],
        required_args: &[Type],
    ) -> ImplMethodResolution {
        let Some(signature) = self.registry.lookup_trait(trait_fqn, &self.package_path) else {
            return ImplMethodResolution::NotFound;
        };
        let mut matches = Vec::new();
        for bound in bounds {
            if !required_args.is_empty() && bound.type_args != required_args {
                continue;
            }
            let mut substitution =
                TypeParamSubstitution::from_pairs(&signature.type_params, &bound.type_args)
                    .with_self_type(receiver.clone());
            for (name, ty) in &bound.associated_types {
                substitution.insert(TypeParamName(name.clone()), ty.clone());
            }
            for associated in &signature.associated_types {
                let parameters = associated
                    .type_params
                    .iter()
                    .map(|name| Type::TypeVariable(name.clone(), vec![]))
                    .collect();
                if let Some(projection) = crate::typechecker::associated_types::from_bound(
                    receiver,
                    bound,
                    &associated.name,
                    parameters,
                    self.registry,
                ) {
                    substitution.insert(TypeParamName(associated.name.clone()), projection);
                }
            }
            for method in &signature.methods {
                if method.name != method_name || !method.type_params.is_empty() {
                    continue;
                }
                let non_self = if method
                    .params
                    .first()
                    .is_some_and(|(name, _)| name == "self")
                {
                    &method.params[1..]
                } else {
                    &method.params[..]
                };
                let params: Vec<_> = non_self
                    .iter()
                    .map(|(name, ty)| {
                        (
                            name.clone(),
                            super::generics::apply_substitution(&substitution, ty),
                        )
                    })
                    .collect();
                if params.len() != arg_types.len()
                    || !params
                        .iter()
                        .zip(arg_types)
                        .all(|((_, expected), actual)| self.is_assignable(expected, actual))
                {
                    continue;
                }
                let return_type =
                    super::generics::apply_substitution(&substitution, &method.return_type);
                matches.push(ImplMethodResolution::Found {
                    resolved: ResolvedImplMethod {
                        trait_fqn: trait_fqn.clone(),
                        trait_type_params: bound.type_args.clone(),
                        for_type: receiver.clone(),
                        method_name: SymbolName(method_name.to_string()),
                        method_type_params: vec![],
                    },
                    params: Some(params),
                    return_type,
                });
            }
        }
        match matches.len() {
            0 => ImplMethodResolution::NotFound,
            1 => matches.pop().unwrap(),
            _ => ImplMethodResolution::Ambiguous,
        }
    }

    /// Resolve EarlyReturn impl for a type. Returns (T, OnFailure, ResolvedImplMethod, unwrap_return_type) or None.
    pub(super) fn resolve_early_return_impl(
        &mut self,
        operand_ty: &Type,
        early_return_fqn: &Fqn,
    ) -> Option<(Type, Type, ResolvedImplMethod, Type)> {
        let (unwrap_resolved, return_type) =
            self.resolve_trait_impl_method_for_type(operand_ty, early_return_fqn, "unwrap", &[])?;

        let (success_type, on_failure_type) = match &return_type {
            Type::GenericEnum { type_args, .. } if type_args.len() == 2 => {
                (type_args[0].1.clone(), type_args[1].1.clone())
            }
            _ => return None,
        };

        Some((success_type, on_failure_type, unwrap_resolved, return_type))
    }

    /// Resolve `Usable<T, E>` impl for a type with a distinguishable
    /// ambiguity outcome — returns `(T, E)`: the resource type and the
    /// impl's source error type. Two
    /// distinct sub-trait providers of `Usable` with no direct impl are a
    /// use-site ambiguity error, never first-wins.
    pub(super) fn resolve_usable_impl_detailed(
        &self,
        operand_ty: &Type,
    ) -> SugarTraitResolution<(Type, Type)> {
        if operand_ty.is_error() {
            return SugarTraitResolution::NotFound;
        }
        let trait_fqn = Fqn::from_dotted("standard.prelude.Usable").unwrap();
        if let Some(bounds) = self.type_parameter_trait_applications(operand_ty, &trait_fqn) {
            let successes = bounds
                .into_iter()
                .filter_map(|bound| {
                    (bound.type_args.len() == 2).then(|| {
                        (
                            bound.trait_fqn,
                            (bound.type_args[0].clone(), bound.type_args[1].clone()),
                        )
                    })
                })
                .collect();
            return Self::decide_sugar_resolution(successes, true);
        }
        let Some(type_fqn) = operand_ty.try_to_fqn() else {
            return SugarTraitResolution::NotFound;
        };
        // "B satisfies A everywhere": with no direct Usable impl, a sub-trait
        // provider block carries it inline — its super-closure args are the
        // Usable application it provides.
        let (candidates, provided) =
            Self::direct_or_provided_impls(self.registry, &trait_fqn, &type_fqn, operand_ty);

        let mut successes: Vec<(Fqn, (Type, Type))> = Vec::new();
        for (info, trait_type_args) in candidates {
            if info.type_params.is_empty() {
                // A non-generic block applies only to its exact for_type —
                // sibling-instantiation blocks share a base FQN.
                if info.for_type == *operand_ty && trait_type_args.len() >= 2 {
                    successes.push((
                        info.trait_fqn.clone(),
                        (trait_type_args[0].clone(), trait_type_args[1].clone()),
                    ));
                }
                continue;
            }

            let mut sub = TypeParamSubstitution::new();
            if !sub.unify(&info.for_type, operand_ty) {
                continue;
            }

            let operand_type_args = Self::extract_type_args(operand_ty);
            let for_type_args = Self::extract_type_args(&info.for_type);
            if let (Some(op_args), Some(ft_args)) = (&operand_type_args, &for_type_args)
                && op_args.len() == ft_args.len()
            {
                for (ft_arg, op_arg) in ft_args.iter().zip(op_args.iter()) {
                    if let Type::TypeVariable(name, _) = ft_arg
                        && sub.get(&TypeParamName(name.0.clone())).is_none()
                    {
                        sub.insert(TypeParamName(name.0.clone()), op_arg.clone());
                    }
                }
            }

            let resolved = sub.resolve_type_params(&info.type_params);
            if let Some(type_args) = resolved {
                let resolved_sub = TypeParamSubstitution::from_pairs(&info.type_params, &type_args);
                let substituted_trait_args: Vec<Type> = trait_type_args
                    .iter()
                    .map(|t| super::generics::apply_substitution(&resolved_sub, t))
                    .collect();
                if substituted_trait_args.len() >= 2 {
                    successes.push((
                        info.trait_fqn.clone(),
                        (
                            substituted_trait_args[0].clone(),
                            substituted_trait_args[1].clone(),
                        ),
                    ));
                }
            }
        }
        Self::decide_sugar_resolution(successes, provided)
    }

    /// Shared success arbitration for the `use` / `await` sugar resolvers:
    /// zero matches → NotFound; several matches from distinct sub-trait
    /// PROVIDER blocks → Ambiguous (a direct impl can't be plural — coherence
    /// forbids overlap — so multiple direct matches keep first-wins).
    fn decide_sugar_resolution<T>(
        mut successes: Vec<(Fqn, T)>,
        provided: bool,
    ) -> SugarTraitResolution<T> {
        match successes.len() {
            0 => SugarTraitResolution::NotFound,
            1 => SugarTraitResolution::Found(successes.remove(0).1),
            _ if provided => {
                SugarTraitResolution::Ambiguous(successes.into_iter().map(|(f, _)| f).collect())
            }
            _ => SugarTraitResolution::Found(successes.remove(0).1),
        }
    }

    /// The direct impl blocks of a trait for a type, each paired with its
    /// trait application args — or, when there are none, the sub-trait
    /// provider blocks paired with the super-closure args they provide for
    /// the trait (`true` in the returned flag). Shared by the `use` / `await`
    /// sugar resolvers.
    fn direct_or_provided_impls<'r>(
        registry: &'r crate::typechecker::registry::Registry,
        trait_fqn: &Fqn,
        type_fqn: &Fqn,
        operand_ty: &Type,
    ) -> (
        Vec<(
            &'r crate::typechecker::registry::ImplBlockSignature,
            Vec<Type>,
        )>,
        bool,
    ) {
        // Only blocks that can apply to THIS operand count as direct — a
        // sibling block (`Usable for Wrap<Int32>` when the operand is
        // `Wrap<String>`) must not hide sub-trait providers.
        let direct: Vec<_> = registry
            .find_impl_blocks(trait_fqn, type_fqn)
            .into_iter()
            .filter(|b| Self::impl_block_applies(b, operand_ty))
            .map(|b| (b, b.trait_type_args.clone()))
            .collect();
        if !direct.is_empty() {
            return (direct, false);
        }
        (
            registry
                .find_providing_impl_blocks(trait_fqn, type_fqn)
                .into_iter()
                .filter_map(|(b, via)| via.map(|(_, closure_args)| (b, closure_args)))
                .collect(),
            true,
        )
    }

    /// Resolve Awaitable impl for a type and extract the inner value type T.
    /// Returns Some(T) if the type implements Awaitable<T>, None otherwise
    /// (including the provider-ambiguous case — the `await` expression site
    /// uses the detailed form to report that properly).
    pub(super) fn resolve_awaitable_value_type(&self, operand_ty: &Type) -> Option<Type> {
        match self.resolve_awaitable_value_type_detailed(operand_ty) {
            SugarTraitResolution::Found(ty) => Some(ty),
            _ => None,
        }
    }

    /// `resolve_awaitable_value_type` with a distinguishable ambiguity outcome.
    pub(super) fn resolve_awaitable_value_type_detailed(
        &self,
        operand_ty: &Type,
    ) -> SugarTraitResolution<Type> {
        if operand_ty.is_error() {
            return SugarTraitResolution::NotFound;
        }
        let awaitable_fqn = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
        let Some(type_fqn) = operand_ty.try_to_fqn() else {
            return SugarTraitResolution::NotFound;
        };
        // Sub-trait providers route here too (see resolve_usable_impl).
        let (candidates, provided) =
            Self::direct_or_provided_impls(self.registry, &awaitable_fqn, &type_fqn, operand_ty);

        let mut successes: Vec<(Fqn, Type)> = Vec::new();
        for (info, trait_type_args) in candidates {
            if info.type_params.is_empty() {
                // Concrete impl — trait_type_args[0] is T; applies only to
                // its exact for_type (sibling blocks share a base FQN).
                if info.for_type == *operand_ty && !trait_type_args.is_empty() {
                    successes.push((info.trait_fqn.clone(), trait_type_args[0].clone()));
                }
                continue;
            }

            // Generic impl — unify for_type against concrete operand type
            let mut sub = TypeParamSubstitution::new();
            if !sub.unify(&info.for_type, operand_ty) {
                continue;
            }

            // If some type params are unbound (e.g. because the operand has TypeVariable
            // type args that unify skips), fill them from the operand's type args directly.
            let operand_type_args = Self::extract_type_args(operand_ty);
            let for_type_args = Self::extract_type_args(&info.for_type);
            if let (Some(op_args), Some(ft_args)) = (&operand_type_args, &for_type_args)
                && op_args.len() == ft_args.len()
            {
                for (ft_arg, op_arg) in ft_args.iter().zip(op_args.iter()) {
                    if let Type::TypeVariable(name, _) = ft_arg
                        && sub.get(&TypeParamName(name.0.clone())).is_none()
                    {
                        sub.insert(TypeParamName(name.0.clone()), op_arg.clone());
                    }
                }
            }

            let resolved = sub.resolve_type_params(&info.type_params);
            if let Some(type_args) = resolved {
                // Substitute resolved type params into trait_type_args to get T
                let resolved_sub = TypeParamSubstitution::from_pairs(&info.type_params, &type_args);
                let substituted_trait_args: Vec<Type> = trait_type_args
                    .iter()
                    .map(|t| super::generics::apply_substitution(&resolved_sub, t))
                    .collect();
                if !substituted_trait_args.is_empty() {
                    successes.push((info.trait_fqn.clone(), substituted_trait_args[0].clone()));
                }
            }
        }

        Self::decide_sugar_resolution(successes, provided)
    }

    /// Extract type args from a generic type (enum, record, class, newtype).
    fn extract_type_args(ty: &Type) -> Option<Vec<Type>> {
        match ty {
            Type::GenericEnum { type_args, .. }
            | Type::GenericRecord { type_args, .. }
            | Type::GenericClass { type_args, .. } => {
                Some(type_args.iter().map(|(_, t)| t.clone()).collect())
            }
            Type::GenericNewtype { type_args, .. } => {
                Some(type_args.iter().map(|(_, t)| t.clone()).collect())
            }
            _ => None,
        }
    }

    /// Restore the current generic parameter bounds in types collected from a where clause.
    pub(super) fn scoped_bound_type(&self, ty: &Type) -> Type {
        let mut substitution = TypeParamSubstitution::new();
        for (name, ty) in &self.current_type_params {
            substitution.insert(name.clone(), ty.clone());
        }
        super::generics::apply_substitution(&substitution, ty)
    }

    /// Infer result parameters from uniquely determined associated types after operand inference.
    pub(super) fn infer_associated_bound_types(
        &self,
        bounds: &TraitBounds,
        substitution: &mut TypeParamSubstitution,
    ) {
        self.infer_associated_bound_types_at_depth(bounds, substitution, 0, &mut Vec::new());
    }

    fn infer_associated_bound_types_at_depth(
        &self,
        bounds: &TraitBounds,
        substitution: &mut TypeParamSubstitution,
        depth: usize,
        active: &mut Vec<(Fqn, Type, Vec<Type>)>,
    ) {
        if depth > MAX_TRAIT_CHECK_DEPTH {
            return;
        }
        use super::generics::apply_substitution;
        // Each pass can unlock another bound on the same receiver, so count
        // individual bounds rather than the number of receiver parameters.
        let bound_count: usize = bounds.iter().map(|(_, required)| required.len()).sum();
        for _ in 0..bound_count {
            for (tp, required) in bounds.iter() {
                let Some(receiver) = substitution.get(tp).cloned() else {
                    continue;
                };
                for bound in required.iter().filter_map(TraitBound::named) {
                    if bound.associated_types.is_empty() {
                        continue;
                    }
                    let args: Vec<_> = bound
                        .type_args
                        .iter()
                        .map(|t| apply_substitution(substitution, t))
                        .collect();
                    // Re-entering the same operand obligation cannot establish its
                    // output. Cut cycles before branching through more impl bounds.
                    let obligation = (bound.trait_fqn.clone(), receiver.clone(), args.clone());
                    if active.contains(&obligation) {
                        continue;
                    }
                    active.push(obligation);
                    let outputs = self
                        .associated_outputs_for_obligation(bound, &receiver, &args, depth, active);
                    active.pop();
                    if let [output] = outputs.as_slice() {
                        for (name, expected) in &bound.associated_types {
                            if let Some(actual) = output.get(name) {
                                substitution.unify(expected, actual);
                            }
                        }
                    }
                }
            }
        }
    }

    fn associated_outputs_for_obligation(
        &self,
        bound: &NamedTraitBound,
        receiver: &Type,
        args: &[Type],
        depth: usize,
        active: &mut Vec<(Fqn, Type, Vec<Type>)>,
    ) -> Vec<BTreeMap<String, Type>> {
        if let Type::TypeVariable(_, actual_bounds) | Type::GenericParam(_, actual_bounds, _) =
            receiver
        {
            let mut direct = Vec::new();
            let mut provided = Vec::new();
            for actual in actual_bounds.iter().filter_map(TraitBound::named) {
                let is_direct = actual.trait_fqn == bound.trait_fqn;
                let actual_args = if is_direct {
                    actual.type_args.clone()
                } else {
                    let Some(args) = self.registry.super_closure_args(
                        &actual.trait_fqn,
                        &actual.type_args,
                        &bound.trait_fqn,
                    ) else {
                        continue;
                    };
                    args
                };
                if actual_args.len() != args.len()
                    || !actual_args
                        .iter()
                        .zip(args)
                        .all(|(a, b)| self.is_assignable(a, b) && self.is_assignable(b, a))
                {
                    continue;
                }
                if is_direct {
                    direct.push(actual.associated_types.clone());
                } else {
                    provided.push(actual.associated_types.clone());
                }
            }
            return if direct.is_empty() { provided } else { direct };
        }
        self.associated_outputs_for_trait(&bound.trait_fqn, receiver, args, depth, active)
    }

    fn associated_outputs_for_trait(
        &self,
        trait_fqn: &Fqn,
        receiver: &Type,
        args: &[Type],
        depth: usize,
        active: &mut Vec<(Fqn, Type, Vec<Type>)>,
    ) -> Vec<BTreeMap<String, Type>> {
        let Some(fqn) = receiver.try_to_fqn() else {
            return Vec::new();
        };
        let mut direct = Vec::new();
        let mut provided = Vec::new();
        for (info, via) in self.registry.find_providing_impl_blocks(trait_fqn, &fqn) {
            let patterns = via.as_ref().map_or(&info.trait_type_args, |(_, args)| args);
            let Some(output) =
                self.associated_outputs_for_impl(info, receiver, patterns, args, depth, active)
            else {
                continue;
            };
            // Index expression keys permit coercions, but a generic bound
            // names an exact trait application used later by monomorphization.
            let exact_application = trait_fqn.package.to_string() == "standard.prelude"
                && matches!(trait_fqn.symbol.0.as_str(), "Index" | "IndexSet");
            if exact_application && output.trait_args != args {
                continue;
            }
            if via.is_none() {
                direct.push(output.types);
            } else {
                provided.push(output.types);
            }
        }
        if direct.is_empty() { provided } else { direct }
    }

    pub(super) fn associated_outputs_for_impl(
        &self,
        info: &ImplBlockSignature,
        receiver: &Type,
        provided_args: &[Type],
        args: &[Type],
        depth: usize,
        active: &mut Vec<(Fqn, Type, Vec<Type>)>,
    ) -> Option<ImplAssociatedTypes> {
        let mut sub = TypeParamSubstitution::new().with_self_type(receiver.clone());
        if !sub.unify(&info.for_type, receiver)
            || provided_args.len() != args.len()
            || !provided_args
                .iter()
                .zip(args)
                .all(|(pattern, actual)| sub.unify(pattern, actual))
        {
            return None;
        }
        if sub.resolve_type_params(&info.type_params).is_none() {
            self.infer_associated_bound_types_at_depth(
                &info.trait_bounds,
                &mut sub,
                depth + 1,
                active,
            );
        }
        let params = sub.resolve_type_params(&info.type_params)?;
        if !self
            .unsatisfied_trait_bounds_at_depth(
                &info.trait_bounds,
                &info.type_params,
                &params,
                depth + 1,
            )
            .is_empty()
        {
            return None;
        }
        Some(ImplAssociatedTypes {
            trait_args: provided_args
                .iter()
                .map(|ty| super::generics::apply_substitution(&sub, ty))
                .collect(),
            types: info
                .associated_type_defs
                .iter()
                .filter(|(_, (params, _))| params.is_empty())
                .map(|(name, (_, ty))| {
                    (name.clone(), super::generics::apply_substitution(&sub, ty))
                })
                .collect(),
        })
    }

    /// Check that concrete type args satisfy all trait bounds.
    /// Returns `true` if all bounds are satisfied, emits errors and returns `false` otherwise.
    pub(super) fn check_trait_bounds(
        &mut self,
        trait_bounds: &crate::typechecker::types::TraitBounds,
        type_params: &[TypeParamName],
        type_args: &[Type],
        span: &Span,
    ) -> bool {
        let unsatisfied = self.unsatisfied_trait_bounds(trait_bounds, type_params, type_args);
        for message in &unsatisfied {
            self.diagnostics.error(span.clone(), message.clone());
        }
        unsatisfied.is_empty()
    }

    /// The same check, reporting nothing: one message per violated bound, empty
    /// when they all hold.
    ///
    /// Overload resolution needs this. A candidate whose bounds fail is not an
    /// error on its own — a sibling overload may take the call — so the failures
    /// have to be carried until it is known that nothing resolved, and reported
    /// then, against the CALL's span rather than the definition's.
    pub(super) fn unsatisfied_trait_bounds(
        &self,
        trait_bounds: &crate::typechecker::types::TraitBounds,
        type_params: &[TypeParamName],
        concrete_type_params: &[Type],
    ) -> Vec<String> {
        self.unsatisfied_trait_bounds_at_depth(trait_bounds, type_params, concrete_type_params, 0)
    }

    fn unsatisfied_trait_bounds_at_depth(
        &self,
        trait_bounds: &TraitBounds,
        type_params: &[TypeParamName],
        concrete_type_params: &[Type],
        depth: usize,
    ) -> Vec<String> {
        let mut messages = Vec::new();
        for (tp, required_bounds) in trait_bounds.iter() {
            // Find the concrete type arg for this type param
            let idx = type_params.iter().position(|p| p == tp);
            let concrete_type = match idx {
                Some(i) => concrete_type_params[i].clone(),
                None => continue,
            };

            // Substitute type params in bound type_args with the concrete type args
            let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                type_params,
                concrete_type_params,
            );

            for bound in required_bounds {
                let Some(bound) = bound.named() else {
                    if !concrete_type.is_class_reference() {
                        messages.push(format!(
                            "type '{}' does not satisfy 'class' required by type parameter '{}'",
                            concrete_type, tp.0
                        ));
                    }
                    continue;
                };
                if bound.kind == BoundKind::SubtypeOf {
                    if !self.type_satisfies_bound(bound, &[], &concrete_type, 0) {
                        messages.push(format!(
                            "type '{}' is not a subtype of {}'{}' required by constraint on '{}'",
                            concrete_type,
                            if bound.is_class_bound() { "class " } else { "" },
                            bound.trait_fqn.symbol,
                            tp.0
                        ));
                    }
                } else {
                    let substituted_args: Vec<Type> = bound
                        .type_args
                        .iter()
                        .map(|t| super::generics::apply_substitution(&sub, t))
                        .collect();

                    let associated_types = bound
                        .associated_types
                        .iter()
                        .map(|(name, ty)| {
                            (name.clone(), super::generics::apply_substitution(&sub, ty))
                        })
                        .collect();
                    if !self.type_satisfies_trait_with_associated_types(
                        &bound.trait_fqn,
                        &substituted_args,
                        &associated_types,
                        &concrete_type,
                        depth,
                    ) {
                        let trait_display = if bound.type_args.is_empty() {
                            format!("{}", bound.trait_fqn.symbol)
                        } else {
                            let args: Vec<String> =
                                substituted_args.iter().map(|t| t.to_string()).collect();
                            format!("{}<{}>", bound.trait_fqn.symbol, args.join(", "))
                        };
                        messages.push(format!(
                            "type '{}' does not implement trait '{}' required by constraint on '{}'",
                            concrete_type, trait_display, tp.0
                        ));
                    } else if let Some(providers) = self.ambiguous_trait_providers(
                        &bound.trait_fqn,
                        &substituted_args,
                        &concrete_type,
                    ) {
                        let names: Vec<String> = providers
                            .iter()
                            .map(|f| format!("'{}'", f.symbol))
                            .collect();
                        messages.push(format!(
                            "ambiguous implementations of trait '{}' for type '{}': provided by both {}; implement '{}' directly to disambiguate",
                            bound.trait_fqn.symbol, concrete_type, names.join(" and "),
                            bound.trait_fqn.symbol,
                        ));
                    } else if let Some(member) = self
                        .intersection_blocks_self_returning_bound(&bound.trait_fqn, &concrete_type)
                    {
                        // An intersection satisfies the bound via one component, but a
                        // bare-Self-returning member would have to produce the FULL
                        // intersection — the vtable wrapper can only rebuild its own
                        // component, so this instantiation is unsound.
                        messages.push(format!(
                            "intersection type '{}' cannot satisfy constraint '{}' on '{}': member '{}' returns Self, which dynamic dispatch narrows to '{}' — upcast the value to '{}' first",
                            concrete_type, bound.trait_fqn.symbol, tp.0,
                            member, bound.trait_fqn.symbol, bound.trait_fqn.symbol
                        ));
                    }
                }
                if depth > 0 && !messages.is_empty() {
                    return messages;
                }
            }
        }
        messages
    }

    /// Add method-local evidence without replacing enclosing parameter identities.
    pub(super) fn add_current_type_param_bounds(&mut self, bounds: &TraitBounds) {
        for (name, additional) in bounds.iter() {
            if let Some(Type::GenericParam(_, existing, _) | Type::TypeVariable(_, existing)) =
                self.current_type_params.get_mut(name)
            {
                let mut merged = TraitBounds::empty();
                merged.insert(name.clone(), existing.clone());
                merged.insert(name.clone(), additional.clone());
                *existing = merged.get(name).cloned().unwrap_or_default();
            }
        }
    }

    /// Resolve where-clause trait constraints to TraitBounds (for use when type-checking
    /// generic method bodies without a pre-registered def, e.g. generic method on non-generic extension).
    pub(super) fn resolve_trait_bounds_from_where_clause(
        &mut self,
        where_clause: &[TraitConstraint],
        type_params: &[TypeParamName],
    ) -> TraitBounds {
        let saved_params = self.current_type_params.clone();
        for tp in type_params {
            self.current_type_params
                .entry(tp.clone())
                .or_insert_with(|| Type::TypeVariable(tp.clone(), vec![]));
        }
        let preliminary = self
            .current_type_params
            .iter()
            .map(|(name, ty)| (name.0.clone(), ty.clone()))
            .collect();
        let scope = crate::typechecker::bound_projections::resolve_scope(
            where_clause,
            &preliminary,
            |constraint, scope| {
                self.current_type_params = scope
                    .iter()
                    .map(|(name, ty)| (TypeParamName(name.clone()), ty.clone()))
                    .collect();
                let before = self.diagnostics.len();
                let resolved =
                    self.resolve_trait_bounds_once(std::slice::from_ref(constraint), type_params);
                let failed = self.diagnostics.len() != before;
                self.diagnostics.truncate(before);
                if failed { None } else { Some(resolved) }
            },
        );
        self.current_type_params = scope
            .into_iter()
            .map(|(name, ty)| (TypeParamName(name), ty))
            .collect();
        let result = self.resolve_trait_bounds_once(where_clause, type_params);
        self.current_type_params = saved_params;
        result
    }

    fn resolve_trait_bounds_once(
        &mut self,
        where_clause: &[TraitConstraint],
        type_params: &[TypeParamName],
    ) -> TraitBounds {
        let mut trait_bounds = TraitBounds::empty();
        for constraint in where_clause {
            let tp_name = TypeParamName(constraint.type_param.value.clone());
            if !type_params.iter().any(|p| p == &tp_name) {
                self.diagnostics.error(
                    constraint.type_param.span.clone(),
                    format!(
                        "type parameter '{}' in where clause is not declared on this item",
                        constraint.type_param.value
                    ),
                );
                continue;
            }
            let mut bounds = Vec::new();
            for trait_name in &constraint.trait_bounds {
                let crate::parser::ast::TypeBound::Named(trait_name) = trait_name else {
                    bounds.push(TraitBound::IsClass);
                    continue;
                };
                if let Some(fqn) = self.resolve_fqn(&trait_name.name.value, SymbolKind::Trait) {
                    let resolved_type_args: Vec<Type> = trait_name
                        .type_args
                        .iter()
                        .map(|te| self.resolve_type_expr(te))
                        .collect();
                    let trait_sig = self
                        .registry
                        .lookup_trait(&fqn, &self.package_path)
                        .cloned();
                    let mut associated_types = std::collections::BTreeMap::new();
                    for (name, te) in &trait_name.associated_types {
                        match trait_sig.as_ref().and_then(|sig| {
                            sig.associated_types.iter().find(|a| a.name == name.value)
                        }) {
                            Some(assoc) if assoc.type_params.is_empty() => {}
                            Some(_) => {
                                self.diagnostics.error(
                                    name.span.clone(),
                                    "cannot bind a generic associated type".to_string(),
                                );
                                continue;
                            }
                            None => {
                                self.diagnostics.error(
                                    name.span.clone(),
                                    format!("unknown associated type '{}'", name.value),
                                );
                                continue;
                            }
                        }
                        let ty = self.resolve_type_expr(te);
                        if associated_types.insert(name.value.clone(), ty).is_some() {
                            self.diagnostics.error(
                                name.span.clone(),
                                format!("duplicate associated type binding '{}'", name.value),
                            );
                        }
                    }
                    bounds.push(TraitBound::Named(
                        crate::typechecker::types::NamedTraitBound {
                            associated_types,
                            trait_fqn: fqn,
                            type_args: resolved_type_args,
                            kind: BoundKind::HasTrait,
                        },
                    ));
                } else if let Some(fqn) = self
                    .resolve_fqn(&trait_name.name.value, SymbolKind::Class)
                    .or_else(|| Type::from_primitive(&trait_name.name.value).map(|ty| ty.to_fqn()))
                {
                    // Nominal subtype bound: a class or a prelude primitive.
                    if !trait_name.type_args.is_empty() || !trait_name.associated_types.is_empty() {
                        self.diagnostics.error(
                            trait_name.name.span.clone(),
                            format!(
                                "subtype bounds cannot have type arguments or associated types (on '{}')",
                                trait_name.name.value,
                            ),
                        );
                        continue;
                    }
                    bounds.push(TraitBound::Named(
                        crate::typechecker::types::NamedTraitBound {
                            associated_types: Default::default(),
                            trait_fqn: fqn,
                            type_args: vec![],
                            kind: BoundKind::SubtypeOf,
                        },
                    ));
                } else {
                    self.diagnostics.error(
                        trait_name.name.span.clone(),
                        format!(
                            "unknown trait, class, or primitive: '{}'",
                            trait_name.name.value
                        ),
                    );
                }
            }
            if !bounds.is_empty() {
                if let Some(name) = trait_bounds.conflicting_associated_binding(&tp_name, &bounds) {
                    self.diagnostics.error(
                        constraint.span.clone(),
                        format!("conflicting associated type binding '{}'", name),
                    );
                }
                trait_bounds.insert(tp_name, bounds);
            }
        }
        trait_bounds
    }
}
