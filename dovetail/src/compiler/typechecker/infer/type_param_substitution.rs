use std::collections::BTreeMap;

use crate::common::types::{TypeParamName, Variance};
use crate::typechecker::infer::types::is_byname_fqn;
use crate::typechecker::types::Type;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExtensionInference {
    Strict,
    CollectIndependent,
    NormalizeKnown,
}

/// A mapping from type parameter names to their concrete (or identity) types.
/// Used during generic instantiation and type argument inference.
/// Optionally carries a `SelfType` replacement for trait method substitution.
#[derive(Debug, Clone)]
pub(crate) struct TypeParamSubstitution {
    bindings: BTreeMap<TypeParamName, Type>,
    self_type: Option<Type>,
    extension_inference: ExtensionInference,
}

impl TypeParamSubstitution {
    /// Empty substitution — used as starting point for unification.
    pub fn new() -> Self {
        Self {
            bindings: BTreeMap::new(),
            self_type: None,
            extension_inference: ExtensionInference::Strict,
        }
    }

    /// Build from parallel type_params and type_args slices.
    pub fn from_pairs(type_params: &[TypeParamName], type_args: &[Type]) -> Self {
        Self {
            bindings: type_params
                .iter()
                .zip(type_args.iter())
                .map(|(tp, ty)| (tp.clone(), ty.clone()))
                .collect(),
            self_type: None,
            extension_inference: ExtensionInference::Strict,
        }
    }

    /// Insert a single type parameter binding.
    pub fn insert(&mut self, name: TypeParamName, ty: Type) {
        self.bindings.insert(name, ty);
    }

    /// Set the replacement type for `SelfType`. Returns `self` for chaining.
    pub fn with_self_type(mut self, ty: Type) -> Self {
        self.self_type = Some(ty);
        self
    }

    /// Look up the concrete type for a type parameter, if bound.
    pub fn get(&self, name: &TypeParamName) -> Option<&Type> {
        self.bindings.get(name)
    }

    /// Collect bound types for each type parameter in declaration order.
    /// Returns `None` if any type parameter is unbound.
    pub fn resolve_type_params(&self, type_params: &[TypeParamName]) -> Option<Vec<Type>> {
        type_params
            .iter()
            .map(|tp| self.bindings.get(tp).cloned())
            .collect()
    }

    /// Resolve type params, defaulting covariant (`+T`) params to `Never`
    /// and contravariant (`-T`) params to `Any` when unresolved.
    /// A bound rigid generic parameter is resolved evidence, not a placeholder.
    /// Returns `None` if any invariant param remains unresolved.
    pub fn resolve_with_variance_defaults(
        &self,
        type_params: &[TypeParamName],
        variances: &[Variance],
    ) -> Option<Vec<Type>> {
        type_params
            .iter()
            .zip(variances.iter())
            .map(|(tp, variance)| match self.bindings.get(tp) {
                Some(ty) if !matches!(ty, Type::TypeVariable(..)) => Some(ty.clone()),
                _ => match variance {
                    Variance::Covariant => Some(Type::Never),
                    Variance::Contravariant => Some(Type::Any),
                    Variance::Invariant => None,
                },
            })
            .collect()
    }

    /// Get the self type replacement, if set.
    pub fn self_type(&self) -> Option<&Type> {
        self.self_type.as_ref()
    }

    /// Unify a parameter type (may contain TypeVariable) against a concrete argument type.
    /// Returns true if unification succeeds. Binds type parameters in this substitution.
    ///
    /// For concrete types (no TypeVariable/GenericParam), uses subtype-aware matching:
    /// `Never` (bottom) is compatible with any param type, and `Any` (top) accepts any arg type.
    /// Like `unify`, but accepts a `Never` argument against an already-bound
    /// TypeVariable (covariant-slot widening). Used by method-dispatch sites
    /// where the receiver pre-binds class-level type params and a closure
    /// argument may produce a narrower (`Never`-typed) value — e.g.
    /// `Resource<X, NetError>.andThen(closure-returning-Resource<Y, Never>)`.
    /// `is_assignable` re-validates the actual call afterward, so this
    /// relaxation is safe.
    pub fn unify_arg(&mut self, param_ty: &Type, arg_ty: &Type) -> bool {
        // Special-case `arg = Never` on a TypeVariable param: bind if unbound
        // (so resolve_type_params completes), accept without rebinding if
        // already bound (covariant-slot subtype acceptance).
        if *arg_ty == Type::Never {
            if let Type::TypeVariable(name, _) = param_ty {
                if !self.bindings.contains_key(name) {
                    self.bindings.insert(name.clone(), Type::Never);
                }
                return true;
            }
            return true;
        }
        match param_ty {
            Type::GenericRecord {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericRecord {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } => {
                    if param_fqn != arg_fqn || param_type_args.len() != arg_type_args.len() {
                        return false;
                    }
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify_arg(p, a))
                }
                _ => false,
            },
            Type::GenericEnum {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericEnum {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } => {
                    if param_fqn != arg_fqn || param_type_args.len() != arg_type_args.len() {
                        return false;
                    }
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify_arg(p, a))
                }
                _ => false,
            },
            Type::GenericClass {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericClass {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } => {
                    if param_fqn != arg_fqn || param_type_args.len() != arg_type_args.len() {
                        return false;
                    }
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify_arg(p, a))
                }
                _ => false,
            },
            Type::GenericNewtype {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericNewtype {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } if param_fqn == arg_fqn && param_type_args.len() == arg_type_args.len() => {
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify_arg(p, a))
                }
                // Arg isn't a matching newtype (e.g. a raw value passed for a
                // `ByName<E>` param via by-name coercion): defer to strict
                // `unify`, which knows the newtype's inner-type coercions.
                _ => self.unify(param_ty, arg_ty),
            },
            Type::Function(param_params, param_ret) => match arg_ty {
                Type::Function(arg_params, arg_ret) => {
                    param_params.len() == arg_params.len()
                        && param_params
                            .iter()
                            .zip(arg_params.iter())
                            .all(|(p, a)| self.unify_arg(p, a))
                        && self.unify_arg(param_ret, arg_ret)
                }
                _ => false,
            },
            Type::Array(param_elem) => match arg_ty {
                Type::Array(arg_elem) => self.unify_arg(param_elem, arg_elem),
                _ => false,
            },
            Type::Tuple(param_elems, _) => match arg_ty {
                Type::Tuple(arg_elems, _) => {
                    param_elems.len() == arg_elems.len()
                        && param_elems
                            .iter()
                            .zip(arg_elems.iter())
                            .all(|(p, a)| self.unify_arg(p, a))
                }
                _ => false,
            },
            Type::TypeVariable(name, _) => {
                if let Some(existing) = self.bindings.get(name) {
                    *existing == *arg_ty
                } else {
                    self.bindings.insert(name.clone(), arg_ty.clone());
                    true
                }
            }
            _ => self.unify(param_ty, arg_ty),
        }
    }

    /// Infer independent bindings first, then normalize extensions whose left
    /// operands became known. Repeat to resolve dependencies between arguments.
    /// Callers must validate all substituted parameters afterward: unresolved
    /// extension constraints are deferred, never inverted to infer a left shape.
    pub fn infer_from_arguments<'a>(
        &mut self,
        arguments: impl Iterator<Item = (&'a Type, &'a Type)> + Clone,
    ) -> bool {
        self.extension_inference = ExtensionInference::CollectIndependent;
        let mut matched = true;
        for (param, arg) in arguments.clone() {
            matched &= self.unify(param, arg);
        }
        self.extension_inference = ExtensionInference::NormalizeKnown;
        loop {
            let previous_count = self.bindings.len();
            for (param, arg) in arguments.clone() {
                self.unify(param, arg);
            }
            if self.bindings.len() == previous_count {
                self.extension_inference = ExtensionInference::Strict;
                return matched;
            }
        }
    }

    pub fn unify(&mut self, param_ty: &Type, arg_ty: &Type) -> bool {
        if let Type::TupleExtend(left, right) = param_ty {
            if self.extension_inference != ExtensionInference::CollectIndependent
                && let Some((prefix, last)) = param_ty.split_tuple_extension(arg_ty)
            {
                return self.unify(left, &prefix) && self.unify(right, &last);
            }
            if self.extension_inference == ExtensionInference::CollectIndependent {
                return true;
            }
            let normalized = super::generics::apply_substitution(self, param_ty);
            if !matches!(normalized, Type::TupleExtend(..)) {
                return self.unify(&normalized, arg_ty);
            }
            if self.extension_inference == ExtensionInference::NormalizeKnown {
                return true;
            }
            // Unrestricted reverse inference remains unsupported. Compare symbolic
            // forms without inferring operands outside the constrained split above.
            return crate::typechecker::subtyping::identical(&normalized, arg_ty);
        }

        // TypeVariable on the arg side means a registry placeholder leaked through
        if matches!(arg_ty, Type::TypeVariable(_, _)) {
            // ...except that a trait DEFAULT body types `self` as `Self`, a
            // bounded TypeVariable, and passing it to a generic function is
            // legitimate. Bind a bare type-param on the param side so
            // `resolve_type_params` completes — otherwise the type param
            // stays unbound, overload resolution finds no candidates, and the
            // call becomes an unreported error node that reaches codegen.
            if let (Type::TypeVariable(name, _), Type::TypeVariable(arg_name, _)) =
                (param_ty, arg_ty)
            {
                // Never bind a variable to ITSELF — an identity binding is
                // self-referential and would loop during substitution.
                if name != arg_name && !self.bindings.contains_key(name) {
                    self.bindings.insert(name.clone(), arg_ty.clone());
                }
            }
            return true;
        }
        if matches!(param_ty, Type::AssociatedProjection(_)) {
            let normalized = super::generics::apply_substitution(self, param_ty);
            return if matches!(normalized, Type::AssociatedProjection(_)) {
                crate::typechecker::subtyping::identical(&normalized, arg_ty)
            } else {
                self.unify(&normalized, arg_ty)
            };
        }
        match param_ty {
            Type::TypeVariable(name, _) => {
                if let Some(existing) = self.bindings.get(name) {
                    *existing == *arg_ty
                } else {
                    self.bindings.insert(name.clone(), arg_ty.clone());
                    true
                }
            }
            // Generic record types: match by FQN and recursively unify type args
            Type::GenericRecord {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericRecord {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } => {
                    if param_fqn != arg_fqn || param_type_args.len() != arg_type_args.len() {
                        return false;
                    }
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify(p, a))
                }
                _ => *arg_ty == Type::Never,
            },
            // Generic enum types: match by FQN and recursively unify type args
            Type::GenericEnum {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericEnum {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } => {
                    if param_fqn != arg_fqn || param_type_args.len() != arg_type_args.len() {
                        return false;
                    }
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify(p, a))
                }
                _ => *arg_ty == Type::Never,
            },
            // Generic class types: match by FQN and recursively unify type args
            Type::GenericClass {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericClass {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } => {
                    if param_fqn != arg_fqn || param_type_args.len() != arg_type_args.len() {
                        return false;
                    }
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify(p, a))
                }
                _ => *arg_ty == Type::Never,
            },
            // Generic newtype types: match by FQN and recursively unify type args
            Type::GenericNewtype {
                fqn: param_fqn,
                type_args: param_type_args,
                ..
            } => match arg_ty {
                Type::GenericNewtype {
                    fqn: arg_fqn,
                    type_args: arg_type_args,
                    ..
                } if param_fqn == arg_fqn => {
                    if param_type_args.len() != arg_type_args.len() {
                        return false;
                    }
                    param_type_args
                        .iter()
                        .zip(arg_type_args.iter())
                        .all(|((.., p), (.., a))| self.unify(p, a))
                }
                _ => {
                    // ByName<T> coercion: unify the inner type arg with the argument
                    if is_byname_fqn(param_fqn) && param_type_args.len() == 1 {
                        self.unify(&param_type_args[0].1, arg_ty)
                    } else {
                        *arg_ty == Type::Never
                    }
                }
            },
            // Array types: recursively unify element types
            Type::Array(param_elem) => match arg_ty {
                Type::Array(arg_elem) => self.unify(param_elem, arg_elem),
                _ => *arg_ty == Type::Never,
            },
            // Tuple types: recursively unify element types
            Type::Tuple(param_elems, _) => match arg_ty {
                Type::Tuple(arg_elems, _) => {
                    param_elems.len() == arg_elems.len()
                        && param_elems
                            .iter()
                            .zip(arg_elems.iter())
                            .all(|(p, a)| self.unify(p, a))
                }
                _ => *arg_ty == Type::Never,
            },
            // Function types: recursively unify param types and return type
            Type::Function(param_params, param_ret) => match arg_ty {
                Type::Function(arg_params, arg_ret) => {
                    param_params.len() == arg_params.len()
                        && param_params
                            .iter()
                            .zip(arg_params.iter())
                            .all(|(p, a)| self.unify(p, a))
                        && self.unify(param_ret, arg_ret)
                }
                _ => *arg_ty == Type::Never,
            },
            // Interface object types: match component-wise (same sorted set of
            // FQNs) and recursively unify each component's type args.
            Type::InterfaceObject {
                traits: param_traits,
                ..
            } => match arg_ty {
                Type::InterfaceObject {
                    traits: arg_traits, ..
                } => {
                    if param_traits.len() != arg_traits.len() {
                        return false;
                    }
                    param_traits.iter().zip(arg_traits.iter()).all(|(p, a)| {
                        p.trait_fqn == a.trait_fqn
                            && p.trait_type_args.len() == a.trait_type_args.len()
                            && p.trait_type_args
                                .iter()
                                .zip(a.trait_type_args.iter())
                                .all(|(pt, at)| self.unify(pt, at))
                    })
                }
                _ => *arg_ty == Type::Never,
            },
            // Concrete types: exact match, or Never/Any subtyping.
            // Never (bottom type) is assignable to any type; Any (top type) accepts any arg.
            _ => *param_ty == *arg_ty || *arg_ty == Type::Never || *param_ty == Type::Any,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn argument_inference_normalizes_known_left_without_inverting_unknown_left() {
        let t_name = TypeParamName("T".into());
        let u_name = TypeParamName("U".into());
        let t = Type::TypeVariable(t_name.clone(), vec![]);
        let u = Type::TypeVariable(u_name.clone(), vec![]);
        let extension = Type::tuple_extend(t.clone(), u);
        let pair = Type::tuple_extend(Type::Int32, Type::Bool);
        for arguments in [
            [(&t, &Type::Int32), (&extension, &pair)],
            [(&extension, &pair), (&t, &Type::Int32)],
        ] {
            let mut sub = TypeParamSubstitution::new();
            assert!(sub.infer_from_arguments(arguments.into_iter()));
            assert_eq!(sub.get(&t_name), Some(&Type::Int32));
            assert_eq!(sub.get(&u_name), Some(&Type::Bool));
        }
        let mut sub = TypeParamSubstitution::new();
        assert!(sub.infer_from_arguments([(&extension, &pair)].into_iter()));
        assert_eq!(sub.get(&t_name), None);
        assert_eq!(sub.get(&u_name), None);
        assert!(!sub.unify(&extension, &pair));
    }

    #[test]
    fn unify_type_param_with_nested_same_name() {
        // Unifying T with Array<T> succeeds — the inner T may be from a different
        // scope (e.g., impl's T vs caller's T). No occurs check needed since
        // all types in Dovetail are well-formed (no infinite types).
        let mut sub = TypeParamSubstitution::new();
        let t = TypeParamName("T".to_string());
        let param = Type::TypeVariable(t.clone(), vec![]);
        let arg = Type::Array(Box::new(Type::TypeVariable(t.clone(), vec![])));
        assert!(sub.unify(&param, &arg));
        assert_eq!(sub.get(&t), Some(&arg));
    }

    #[test]
    fn occurs_check_allows_identity_binding() {
        let mut sub = TypeParamSubstitution::new();
        let t = TypeParamName("T".to_string());
        // Unifying T with T (identity): arg is TypeVariable (registry placeholder),
        // so we skip creating a binding and return true.
        let param = Type::TypeVariable(t.clone(), vec![]);
        let arg = Type::TypeVariable(t.clone(), vec![]);
        assert!(sub.unify(&param, &arg));
        assert_eq!(sub.get(&t), None);
    }

    #[test]
    fn unify_type_param_with_concrete_succeeds() {
        let mut sub = TypeParamSubstitution::new();
        let t = TypeParamName("T".to_string());
        let param = Type::TypeVariable(t.clone(), vec![]);
        assert!(sub.unify(&param, &Type::Int32));
        assert_eq!(sub.get(&t), Some(&Type::Int32));
    }

    #[test]
    fn unify_type_param_consistent_rebind() {
        let mut sub = TypeParamSubstitution::new();
        let t = TypeParamName("T".to_string());
        let param = Type::TypeVariable(t.clone(), vec![]);
        assert!(sub.unify(&param, &Type::Int32));
        // Same binding again is OK
        assert!(sub.unify(&param, &Type::Int32));
        // Different binding fails
        assert!(!sub.unify(&param, &Type::Bool));
    }

    #[test]
    fn unify_never_arg_matches_any_concrete_param() {
        let mut sub = TypeParamSubstitution::new();
        // Never arg should unify with any concrete param type
        assert!(sub.unify(&Type::Int32, &Type::Never));
        assert!(sub.unify(&Type::Bool, &Type::Never));
        assert!(sub.unify(&Type::String, &Type::Never));
    }

    #[test]
    fn unify_any_param_matches_any_arg() {
        let mut sub = TypeParamSubstitution::new();
        // Any param should accept any arg type
        assert!(sub.unify(&Type::Any, &Type::Int32));
        assert!(sub.unify(&Type::Any, &Type::Never));
        assert!(sub.unify(&Type::Any, &Type::String));
    }

    #[test]
    fn unify_type_param_binds_never() {
        let mut sub = TypeParamSubstitution::new();
        let t = TypeParamName("T".to_string());
        let param = Type::TypeVariable(t.clone(), vec![]);
        // TypeVariable should still bind to Never (not short-circuit)
        assert!(sub.unify(&param, &Type::Never));
        assert_eq!(sub.get(&t), Some(&Type::Never));
    }

    #[test]
    fn variance_defaults_preserve_rigid_generic_bindings() {
        let bound = TypeParamName("T".into());
        let missing = TypeParamName("E".into());
        let caller_type = Type::GenericParam(TypeParamName("Value".into()), vec![], 7);
        let mut substitution = TypeParamSubstitution::new();
        assert!(substitution.unify(&Type::TypeVariable(bound.clone(), vec![]), &caller_type,));

        for variance in [
            Variance::Covariant,
            Variance::Contravariant,
            Variance::Invariant,
        ] {
            assert_eq!(
                substitution.resolve_with_variance_defaults(
                    &[bound.clone(), missing.clone()],
                    &[variance, Variance::Covariant],
                ),
                Some(vec![caller_type.clone(), Type::Never]),
            );
        }
    }

    #[test]
    fn variance_defaults_still_apply_to_unresolved_placeholders() {
        let parameter = TypeParamName("T".into());
        let mut substitution = TypeParamSubstitution::new();
        substitution.insert(
            parameter.clone(),
            Type::TypeVariable(TypeParamName("Unknown".into()), vec![]),
        );

        for (variance, expected) in [
            (Variance::Covariant, Some(vec![Type::Never])),
            (Variance::Contravariant, Some(vec![Type::Any])),
            (Variance::Invariant, None),
        ] {
            assert_eq!(
                substitution
                    .resolve_with_variance_defaults(std::slice::from_ref(&parameter), &[variance],),
                expected,
            );
        }
    }
}
