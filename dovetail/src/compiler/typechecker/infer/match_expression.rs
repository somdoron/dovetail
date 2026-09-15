use crate::common::types::{MangledName, VarName, Variance};
use crate::parser::ast::{Expr, FieldPattern, MatchArm, Pattern, TypeExpr};

use crate::typechecker::registry::VariantPayload;
use crate::typechecker::types::{
    Type, TypedExpr, TypedExprKind, TypedFieldPattern, TypedMatchArm, TypedPattern,
};

use super::generics::apply_substitution;
use super::Inference;

impl Inference<'_> {
    fn infer_literal_pattern(&mut self, expr: &Expr) -> TypedExpr {
        if matches!(expr, Expr::ExactNumberLiteral(..)) {
            self.diagnostics.error(
                expr.span(),
                "BigInt and Decimal literal patterns are not supported; use an equality guard".to_string(),
            );
        }
        self.infer_expr(expr)
    }

    /// Returns true if `actual` is assignable to `expected`.
    /// Handles: Never → any, Error suppression, structural comparison for generic types
    /// with variance, Array, Tuple, InterfaceObject identity, and implicit concrete → trait
    /// object coercion when the concrete type implements the trait.
    #[allow(clippy::only_used_in_recursion)]
    fn is_assignable_for_pattern(&self, expected: &Type, actual: &Type) -> bool {
        // Subject (expected) is a type parameter: runtime type unknown at type-check time.
        if matches!(expected, Type::TypeVariable(_, _) | Type::GenericParam(_, _, _)) {
            return true;
        }
        if actual == expected || actual.is_error() || expected.is_error() || actual.is_never() || expected.is_any() {
            return true;
        }
        // TypeParameter identity: same name → assignable (used in generic bodies)
        if let (Type::TypeVariable(a, _) | Type::GenericParam(a, _, _), Type::TypeVariable(b, _) | Type::GenericParam(b, _, _)) = (expected, actual) {
            return a == b;
        }
        // Structural comparison for GenericRecord types: match FQN + type args with variance.
        if let (
            Type::GenericRecord { fqn: fqn_e, type_args: args_e, .. },
            Type::GenericRecord { fqn: fqn_a, type_args: args_a, .. },
        ) = (expected, actual)
        {
            if fqn_e != fqn_a || args_e.len() != args_a.len() {
                return false;
            }
            return args_e.iter().zip(args_a.iter()).all(|((v_e, e), (_, a))| {
                self.check_variance_for_pattern(*v_e, e, a)
            });
        }
        // Structural comparison for GenericEnum types: match FQN + type args with variance.
        if let (
            Type::GenericEnum { fqn: fqn_e, type_args: args_e, .. },
            Type::GenericEnum { fqn: fqn_a, type_args: args_a, .. },
        ) = (expected, actual)
        {
            if fqn_e != fqn_a || args_e.len() != args_a.len() {
                return false;
            }
            return args_e.iter().zip(args_a.iter()).all(|((v_e, e), (_, a))| {
                self.check_variance_for_pattern(*v_e, e, a)
            });
        }
        // Interface object → interface object: superset → subset (see is_assignable).
        if let (
            Type::InterfaceObject { traits: traits_e, .. },
            Type::InterfaceObject { traits: traits_a, .. },
        ) = (expected, actual)
        {
            // Component type args are invariant (see is_assignable); the
            // super closure covers extends-upcasts.
            return traits_e.iter().all(|ce| {
                traits_a.iter().any(|ca| {
                    (ce.trait_fqn == ca.trait_fqn && ce.trait_type_args == ca.trait_type_args)
                        || self
                            .registry
                            .super_closure_args(&ca.trait_fqn, &ca.trait_type_args, &ce.trait_fqn)
                            .is_some_and(|args| args == ce.trait_type_args)
                })
            });
        }
        // Structural comparison for Array types
        if let (Type::Array(elem_e), Type::Array(elem_a)) = (expected, actual) {
            return self.is_assignable_for_pattern(elem_e, elem_a);
        }
        // Structural comparison for Tuple types
        if let (Type::Tuple(types_e, _), Type::Tuple(types_a, _)) = (expected, actual) {
            if types_e.len() != types_a.len() {
                return false;
            }
            return types_e.iter().zip(types_a.iter()).all(|(e, a)| self.is_assignable_for_pattern(e, a));
        }
        // Implicit coercion: concrete type → interface object type (all
        // components; a type parameter satisfies via its declared bounds).
        if let Type::InterfaceObject { traits, .. } = expected {
            if traits
                .iter()
                .all(|c| self.type_satisfies_trait(&c.trait_fqn, &c.trait_type_args, actual, 0))
            {
                return true;
            }
        }
        false
    }

    /// Check assignability for a single type argument position given its variance.
    fn check_variance_for_pattern(&self, variance: Variance, expected: &Type, actual: &Type) -> bool {
        if !actual.contains_type_parameter() && !expected.contains_type_parameter() && !actual.is_error() && !expected.is_error() {
            return crate::typechecker::subtyping::argument(self.registry, variance, expected, actual);
        }
        // See check_variance: exact matches whenever an interface object
        // appears at any depth; Never/Error still flow through.
        if !matches!(actual, Type::Never | Type::Error)
            && !matches!(expected, Type::Never | Type::Error)
            && (expected.contains_interface_object() || actual.contains_interface_object())
        {
            return expected == actual;
        }
        match variance {
            Variance::Covariant => self.is_assignable_for_pattern(expected, actual),
            Variance::Contravariant => self.is_assignable_for_pattern(actual, expected),
            Variance::Invariant => {
                self.is_assignable_for_pattern(expected, actual)
                    && self.is_assignable_for_pattern(actual, expected)
            }
        }
    }

    pub(super) fn infer_match_expr(
        &mut self,
        subject: &Expr,
        arms: &[MatchArm],
        span: &crate::common::span::Span,
    ) -> TypedExpr {
        let parent_expected = self.expected_type.clone();
        self.expected_type = None;
        let typed_subject = self.infer_expr(subject);
        self.expected_type = parent_expected.clone();
        let subject_ty = typed_subject.ty.clone();

        // Reject literal patterns on types not yet supported by codegen
        let has_literal_pattern = arms
            .iter()
            .any(|arm| matches!(arm.pattern, Pattern::Literal(_, _)));
        if has_literal_pattern {
            if let Type::String = subject_ty {
                self.diagnostics.error(
                    span.clone(),
                    "match with literal patterns on 'String' is not supported yet".to_string(),
                );
            }
        }

        let mut typed_arms = Vec::new();
        let mut result_ty: Option<Type> = None;

        for arm in arms {
            self.push_scope();

            // Type-check the pattern against the subject type.
            // Returns None when the arm is statically unreachable (e.g., an instantiated
            // generic function where the pattern type can never match the concrete scrutinee).
            let typed_pattern = match &arm.pattern {
                Pattern::Wildcard(_) => Some(TypedPattern::Wildcard),
                Pattern::Variable(name, _pat_span) => {
                    // Check if this is a bare no-payload variant of the scrutinee's enum
                    if let Some(typed_pat) = self.try_promote_variable_to_variant(name, &subject_ty) {
                        Some(typed_pat)
                    } else {
                        let var_name = VarName(name.clone());
                        self.define_variable(var_name.clone(), subject_ty.clone(), false);
                        Some(TypedPattern::Variable(var_name, subject_ty.clone()))
                    }
                }
                Pattern::Literal(lit_expr, pat_span) => {
                    let typed_lit = self.infer_literal_pattern(lit_expr);
                    // A literal pattern against an interface-object scrutinee is
                    // a downcast test — unsupported (the coercion-tolerant
                    // assignability check would wrongly accept it).
                    if matches!(subject_ty, Type::InterfaceObject { .. }) {
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "cannot match a literal pattern against interface type '{}'; interface objects do not support downcasts",
                                subject_ty
                            ),
                        );
                    } else if !subject_ty.is_error() && !typed_lit.ty.is_error() {
                        self.check_assignable(pat_span.clone(), &subject_ty, &typed_lit.ty);
                    }
                    Some(TypedPattern::Literal(Box::new(typed_lit)))
                }
                Pattern::TypeAnnotated {
                    binding,
                    type_expr,
                    span: pat_span,
                } => self.infer_type_annotated_pattern(
                    binding, type_expr, pat_span, &subject_ty,
                ),
                Pattern::Record {
                    type_name,
                    type_params,
                    fields,
                    span: pat_span,
                } => self.infer_record_pattern(
                    type_name, type_params, fields, pat_span, &subject_ty,
                ),
                Pattern::EnumVariant {
                    type_name,
                    variant_name,
                    span: pat_span,
                } => self.infer_enum_variant_no_args_pattern(
                    type_name, variant_name, pat_span, &subject_ty,
                ),
                Pattern::EnumVariantTuple {
                    type_name,
                    variant_name,
                    payload_patterns,
                    span: pat_span,
                } => self.infer_enum_variant_pattern(
                    type_name, variant_name, payload_patterns, pat_span, &subject_ty,
                ),
                Pattern::EnumVariantRecord {
                    type_name,
                    variant_name,
                    fields,
                    span: pat_span,
                } => self.infer_enum_variant_record_pattern(
                    type_name, variant_name, fields, pat_span, &subject_ty,
                ),
                Pattern::Tuple(sub_pats, pat_span) => {
                    match &subject_ty {
                        Type::Tuple(elem_types, _mn) => {
                            if sub_pats.len() != elem_types.len() {
                                self.diagnostics.error(
                                    pat_span.clone(),
                                    format!(
                                        "tuple pattern has {} elements but the tuple has {}",
                                        sub_pats.len(),
                                        elem_types.len()
                                    ),
                                );
                                Some(TypedPattern::Wildcard)
                            } else {
                                let element_patterns: Vec<TypedPattern> = sub_pats
                                    .iter()
                                    .zip(elem_types.iter())
                                    .map(|(sp, et)| self.infer_sub_pattern(sp, et))
                                    .collect();
                                Some(TypedPattern::Tuple {
                                    element_patterns,
                                    tuple_type: subject_ty.clone(),
                                })
                            }
                        }
                        Type::Error => Some(TypedPattern::Wildcard),
                        _ => {
                            self.diagnostics.error(
                                pat_span.clone(),
                                format!(
                                    "cannot match tuple pattern against non-tuple type '{}'",
                                    subject_ty
                                ),
                            );
                            Some(TypedPattern::Wildcard)
                        }
                    }
                }
            };

            // Skip unreachable arms (pattern type can never match the scrutinee)
            let Some(typed_pattern) = typed_pattern else {
                self.pop_scope();
                continue;
            };

            // Type-check guard if present
            let typed_guard = arm.guard.as_ref().map(|guard_expr| {
                let typed_guard = self.infer_expr(guard_expr);
                self.check_assignable(typed_guard.span.clone(), &Type::Bool, &typed_guard.ty);
                Box::new(typed_guard)
            });

            // Type-check arm body with parent expected type propagated
            self.expected_type = parent_expected.clone();
            let typed_body = self.infer_expr(&arm.body);

            // Unify arm body types (same logic as if-else)
            result_ty = Some(match result_ty {
                None => typed_body.ty.clone(),
                Some(prev) => {
                    if prev.is_error() || typed_body.ty.is_error() {
                        Type::Error
                    } else if prev.is_never() {
                        typed_body.ty.clone()
                    } else if let Some(expected @ Type::InterfaceObject { .. }) = parent_expected
                        .as_ref()
                        .filter(|e| {
                            matches!(e, Type::InterfaceObject { .. })
                                && !typed_body.ty.is_never()
                                && self.is_assignable(e, &prev)
                                && self.is_assignable(e, &typed_body.ty)
                        })
                        .cloned()
                    {
                        // The context annotates an interface-object type and
                        // every arm coerces to it: use it. Arm-pairwise
                        // unification could otherwise pick a SUB-interface
                        // (a concrete arm implements it too), silently
                        // rerouting the concrete arm's dispatch through the
                        // sub's vtable against the direct-impl-wins tie-break.
                        expected
                    } else if typed_body.ty.is_never() || self.is_assignable(&prev, &typed_body.ty)
                    {
                        prev
                    } else if self.is_assignable(&typed_body.ty, &prev) {
                        typed_body.ty.clone()
                    } else if let Some(lub) = self.least_upper_bound(&prev, &typed_body.ty) {
                        lub
                    } else {
                        self.diagnostics.error(
                            typed_body.span.clone(),
                            format!(
                                "type mismatch: expected '{}', found '{}'",
                                prev, typed_body.ty
                            ),
                        );
                        Type::Error
                    }
                }
            });

            self.pop_scope();

            typed_arms.push(TypedMatchArm {
                pattern: typed_pattern,
                guard: typed_guard,
                body: Box::new(typed_body),
                span: arm.span.clone(),
            });
        }

        let final_ty = result_ty.unwrap_or(Type::Unit);

        // Second pass: re-infer arm bodies whose type differs from the unified
        // result type (e.g. `None` defaulting to `Option<Never>` instead of
        // `Option<String>`). Without this, different arms may produce distinct
        // WASM struct types for what should be the same tuple/generic type.
        if !final_ty.is_error() && !final_ty.is_never() {
            for (typed_arm, orig_arm) in typed_arms.iter_mut().zip(arms.iter()) {
                let body_ty = &typed_arm.body.ty;
                if *body_ty != final_ty
                    && !body_ty.is_error()
                    && !body_ty.is_never()
                    && self.is_assignable(&final_ty, body_ty)
                {
                    self.push_scope();
                    self.define_pattern_bindings(&typed_arm.pattern);
                    let saved = self.expected_type.take();
                    self.expected_type = Some(final_ty.clone());
                    let new_body = self.infer_expr(&orig_arm.body);
                    self.expected_type = saved;
                    self.pop_scope();
                    typed_arm.body = Box::new(new_body);
                }
            }
        }

        self.expected_type = parent_expected;

        TypedExpr {
            kind: TypedExprKind::Match {
                subject: Box::new(typed_subject),
                arms: typed_arms,
            },
            ty: final_ty,
            span: span.clone(),
        }
    }

    /// Re-establish pattern bindings in the current scope for re-inference.
    fn define_pattern_bindings(&mut self, pattern: &TypedPattern) {
        match pattern {
            TypedPattern::Variable(name, ty) => {
                self.define_variable(name.clone(), ty.clone(), false);
            }
            TypedPattern::Tuple { element_patterns, .. } => {
                for sub_pat in element_patterns {
                    self.define_pattern_bindings(sub_pat);
                }
            }
            TypedPattern::EnumVariant { payload_patterns, .. } => {
                for sub_pat in payload_patterns {
                    self.define_pattern_bindings(sub_pat);
                }
            }
            TypedPattern::EnumVariantRecord { field_patterns, .. } => {
                for fp in field_patterns {
                    self.define_pattern_bindings(&fp.pattern);
                }
            }
            TypedPattern::Record { fields, .. } => {
                for fp in fields {
                    self.define_pattern_bindings(&fp.pattern);
                }
            }
            TypedPattern::TypeAnnotated { binding, ty } => {
                self.define_variable(binding.clone(), ty.clone(), false);
            }
            TypedPattern::Newtype { inner_pattern, .. } => {
                self.define_pattern_bindings(inner_pattern);
            }
            TypedPattern::Wildcard | TypedPattern::Literal(_) => {}
        }
    }

    /// Infer a type-annotated pattern: `case b: SomeType =>`.
    /// Returns `None` when the arm is statically unreachable (pattern type can
    /// never match the concrete scrutinee, e.g., in an instantiated generic function).
    fn infer_type_annotated_pattern(
        &mut self,
        binding: &str,
        type_expr: &TypeExpr,
        pat_span: &crate::common::span::Span,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        let expected_ty = self.resolve_type_expr(type_expr);
        if expected_ty.is_error() {
            return Some(TypedPattern::Wildcard);
        }

        // Runtime type pattern on Any: validate target is concrete, then return early.
        if subject_ty.is_any() {
            if expected_ty.is_any() {
                self.diagnostics.error(
                    pat_span.clone(),
                    "type pattern target must be a concrete type, found 'Any'".to_string(),
                );
                return Some(TypedPattern::Wildcard);
            }
            if expected_ty.is_never() {
                self.diagnostics.error(
                    pat_span.clone(),
                    "type pattern target must be a concrete type, found 'Never'".to_string(),
                );
                return Some(TypedPattern::Wildcard);
            }
            // An interface-object target would test fat-pointer identity, not
            // "implements" — same rejection as the `is` gate.
            if matches!(expected_ty, Type::InterfaceObject { .. }) {
                self.diagnostics.error(
                    pat_span.clone(),
                    format!(
                        "type pattern cannot test interface type '{}'; interface objects carry no runtime type information — test the concrete type instead",
                        expected_ty
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
            let var_name = VarName(binding.to_string());
            self.define_variable(var_name.clone(), expected_ty.clone(), false);
            return Some(TypedPattern::TypeAnnotated {
                binding: var_name,
                ty: expected_ty,
            });
        }

        // Runtime type pattern on class hierarchy: validate target is a related class.
        if subject_ty.is_class_type() {
            if !expected_ty.is_class_type() {
                if !expected_ty.is_error() {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!(
                            "type pattern on class subject requires a class target type, found '{}'",
                            expected_ty
                        ),
                    );
                }
                return Some(TypedPattern::Wildcard);
            }
            if subject_ty.is_error() || expected_ty.is_error() {
                return Some(TypedPattern::Wildcard);
            }
            let subject_fqn = subject_ty.to_fqn();
            let target_fqn = expected_ty.to_fqn();
            if crate::typechecker::subtyping::is_subtype(self.registry, subject_ty, &expected_ty) {
                self.diagnostics.error(
                    pat_span.clone(),
                    format!(
                        "type pattern on '{}' for '{}' is always true — the subject is already a subtype of the target",
                        subject_ty, expected_ty
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
            if !self.registry.class_is_subtype(&target_fqn, &subject_fqn) && !self.registry.class_is_subtype(&subject_fqn, &target_fqn) {
                self.diagnostics.error(
                    pat_span.clone(),
                    format!(
                        "type pattern between unrelated classes '{}' and '{}' will never match",
                        subject_ty, expected_ty
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
            let var_name = VarName(binding.to_string());
            self.define_variable(var_name.clone(), expected_ty.clone(), false);
            return Some(TypedPattern::TypeAnnotated {
                binding: var_name,
                ty: expected_ty,
            });
        }

        if !subject_ty.is_error() && !self.is_assignable_for_pattern(subject_ty, &expected_ty) {
            self.diagnostics.error(
                pat_span.clone(),
                format!(
                    "type mismatch in type-annotated pattern: scrutinee is '{}', \
                     pattern type '{}' cannot match",
                    subject_ty, expected_ty
                ),
            );
            return Some(TypedPattern::Wildcard);
        }

        let var_name = VarName(binding.to_string());
        self.define_variable(var_name.clone(), expected_ty.clone(), false);

        Some(TypedPattern::TypeAnnotated {
            binding: var_name,
            ty: expected_ty,
        })
    }

    /// Infer an enum variant pattern in a match arm.
    /// Returns `None` when the arm is statically unreachable.
    fn infer_enum_variant_pattern(
        &mut self,
        type_name: &crate::common::span::Spanned<String>,
        variant_name: &crate::common::span::Spanned<String>,
        payload_patterns: &[Pattern],
        pat_span: &crate::common::span::Span,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        // Resolve the enum type — bare variant patterns have empty type_name
        let enum_sig = if type_name.value.is_empty() {
            // Bare variant pattern: check if scrutinee is a newtype first
            if matches!(subject_ty, Type::Newtype(..) | Type::GenericNewtype { .. }) {
                return self.infer_newtype_pattern(
                    variant_name, payload_patterns, pat_span, subject_ty,
                );
            }
            // Bare variant pattern: resolve from scrutinee type
            let scrutinee_fqn = match subject_ty {
                Type::Enum(fqn, _) | Type::GenericEnum { fqn, .. } => fqn.clone(),
                _ => {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!(
                            "bare variant pattern '{}' requires an enum scrutinee, found '{}'",
                            variant_name.value, subject_ty
                        ),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            };
            match self.registry.lookup_enum_type(&scrutinee_fqn, &self.package_path, &self.current_file) {
                Some(sig) => sig.clone(),
                None => {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!("unknown enum type: '{}'", scrutinee_fqn),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            }
        } else {
            match self.resolve_enum_type(&type_name.value) {
                Some(sig) => sig,
                None => {
                    self.diagnostics.error(
                        type_name.span.clone(),
                        format!("unknown enum type: '{}'", type_name.value),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            }
        };

        let fqn = enum_sig.fqn.clone();

        // Find the variant
        let variant_pos = enum_sig
            .variants
            .iter()
            .position(|(name, _)| name == &variant_name.value);
        let (variant_index, payload_types) = match variant_pos {
            Some(idx) => {
                let types = match &enum_sig.variants[idx].1 {
                    VariantPayload::Tuple(types) => types.clone(),
                    VariantPayload::None => {
                        let display_enum = if type_name.value.is_empty() {
                            fqn.symbol.0.as_str()
                        } else {
                            type_name.value.as_str()
                        };
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "variant '{}.{}' has no fields; use '{}.{}' without parentheses",
                                display_enum, variant_name.value,
                                display_enum, variant_name.value
                            ),
                        );
                        return Some(TypedPattern::Wildcard);
                    }
                    VariantPayload::Record(_) => {
                        let display_enum = if type_name.value.is_empty() {
                            fqn.symbol.0.as_str()
                        } else {
                            type_name.value.as_str()
                        };
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "variant '{}.{}' requires record-style pattern with {{ }}, not ()",
                                display_enum,
                                variant_name.value
                            ),
                        );
                        return Some(TypedPattern::Wildcard);
                    }
                };
                (idx as u32, types)
            }
            None => {
                let display_enum = if type_name.value.is_empty() {
                    fqn.symbol.0.as_str()
                } else {
                    type_name.value.as_str()
                };
                self.diagnostics.error(
                    variant_name.span.clone(),
                    format!(
                        "no variant '{}' in enum '{}'",
                        variant_name.value, display_enum
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
        };

        // Check payload count
        if payload_patterns.len() != payload_types.len() {
            let display_enum = if type_name.value.is_empty() {
                fqn.symbol.0.as_str()
            } else {
                type_name.value.as_str()
            };
            self.diagnostics.error(
                pat_span.clone(),
                format!(
                    "variant '{}.{}' expects {} payload pattern(s), found {}",
                    display_enum,
                    variant_name.value,
                    payload_types.len(),
                    payload_patterns.len()
                ),
            );
            return Some(TypedPattern::Wildcard);
        }

        // Build substitution from scrutinee's type args for generic enums
        let substitution = match subject_ty {
            Type::GenericEnum { type_args: scrutinee_type_args, .. } => {
                let stripped_args: Vec<Type> = scrutinee_type_args
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect();
                Some(super::type_param_substitution::TypeParamSubstitution::from_pairs(
                    &enum_sig.type_params,
                    &stripped_args,
                ))
            }
            _ => None,
        };

        // Substitute payload types if we have a generic enum substitution
        let concrete_payload_types: Vec<Type> = if let Some(ref sub) = substitution {
            payload_types
                .iter()
                .map(|ty| apply_substitution(sub, ty))
                .collect()
        } else {
            payload_types.clone()
        };

        // Check subject type compatibility
        if !subject_ty.is_error() {
            let subject_fqn_matches = match subject_ty {
                Type::Enum(sfqn, _) | Type::GenericEnum { fqn: sfqn, .. } => *sfqn == fqn,
                _ => false,
            };
            if !subject_fqn_matches {
                self.diagnostics.error(
                    pat_span.clone(),
                    format!(
                        "type mismatch in enum pattern: expected '{}', found '{}'",
                        subject_ty,
                        if type_name.value.is_empty() { &fqn.symbol.0 } else { &type_name.value }
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
        }

        // Type-check payload sub-patterns
        let mut typed_payload = Vec::new();
        for (sub_pat, payload_ty) in payload_patterns.iter().zip(concrete_payload_types.iter()) {
            typed_payload.push(self.infer_sub_pattern(sub_pat, payload_ty));
        }

        let enum_type = match subject_ty {
            Type::GenericEnum { .. } => subject_ty.clone(),
            _ => Type::Enum(fqn.clone(), MangledName::for_type(&fqn)),
        };

        Some(TypedPattern::EnumVariant {
            enum_type,
            variant_name: variant_name.value.clone(),
            variant_index,
            payload_patterns: typed_payload,
        })
    }

    /// Promote a bare no-payload variant name (e.g. `None`, `Nil`) to the corresponding
    /// enum variant pattern when the scrutinee is an enum that has a matching variant.
    fn try_promote_variable_to_variant(
        &self,
        name: &str,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        let scrutinee_fqn = match subject_ty {
            Type::Enum(fqn, _) | Type::GenericEnum { fqn, .. } => fqn,
            _ => return None,
        };
        let enum_sig = self
            .registry
            .lookup_enum_type(scrutinee_fqn, &self.package_path, &self.current_file)?;
        let variant_pos = enum_sig
            .variants
            .iter()
            .position(|(v, payload)| v == name && matches!(payload, VariantPayload::None));
        let variant_index = variant_pos? as u32;
        let enum_type = match subject_ty {
            Type::GenericEnum { .. } => subject_ty.clone(),
            _ => Type::Enum(scrutinee_fqn.clone(), MangledName::for_type(scrutinee_fqn)),
        };
        Some(TypedPattern::EnumVariant {
            enum_type,
            variant_name: name.to_string(),
            variant_index,
            payload_patterns: vec![],
        })
    }

    fn infer_newtype_pattern(
        &mut self,
        variant_name: &crate::common::span::Spanned<String>,
        payload_patterns: &[Pattern],
        pat_span: &crate::common::span::Span,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        let (nt_fqn, inner_ty) = match subject_ty {
            Type::Newtype(fqn, inner) => (fqn.clone(), inner.as_ref().clone()),
            Type::GenericNewtype { fqn, concrete_inner_type, .. } => (fqn.clone(), concrete_inner_type.as_ref().clone()),
            _ => unreachable!(),
        };

        // Check private newtype access
        if let Some(sig) = self.registry.lookup_newtype_type(&nt_fqn, &self.package_path, &self.current_file) {
            let sig = sig.clone();
            if !self.check_newtype_inner_access(&sig, pat_span, "pattern match on") {
                return Some(TypedPattern::Wildcard);
            }
        }

        // Constructor name must match the newtype name
        if variant_name.value != nt_fqn.symbol.0 {
            self.diagnostics.error(
                variant_name.span.clone(),
                format!(
                    "expected newtype constructor '{}', found '{}'",
                    nt_fqn.symbol.0, variant_name.value
                ),
            );
            return Some(TypedPattern::Wildcard);
        }

        // Must have exactly 1 sub-pattern
        if payload_patterns.len() != 1 {
            self.diagnostics.error(
                pat_span.clone(),
                format!(
                    "newtype '{}' pattern expects 1 sub-pattern, found {}",
                    nt_fqn.symbol.0, payload_patterns.len()
                ),
            );
            return Some(TypedPattern::Wildcard);
        }

        let inner_pattern = self.infer_sub_pattern(&payload_patterns[0], &inner_ty);

        Some(TypedPattern::Newtype {
            newtype_ty: subject_ty.clone(),
            inner_pattern: Box::new(inner_pattern),
        })
    }

    /// Infer a no-args enum variant pattern (e.g. `case Color.Red` or bare `None`).
    fn infer_enum_variant_no_args_pattern(
        &mut self,
        type_name: &crate::common::span::Spanned<String>,
        variant_name: &crate::common::span::Spanned<String>,
        pat_span: &crate::common::span::Span,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        // Resolve the enum type — bare variant patterns have empty type_name
        let enum_sig = if type_name.value.is_empty() {
            let scrutinee_fqn = match subject_ty {
                Type::Enum(fqn, _) | Type::GenericEnum { fqn, .. } => fqn.clone(),
                _ => {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!(
                            "bare variant pattern '{}' requires an enum scrutinee, found '{}'",
                            variant_name.value, subject_ty
                        ),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            };
            match self.registry.lookup_enum_type(&scrutinee_fqn, &self.package_path, &self.current_file) {
                Some(sig) => sig.clone(),
                None => {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!("unknown enum type: '{}'", scrutinee_fqn),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            }
        } else {
            match self.resolve_enum_type(&type_name.value) {
                Some(sig) => sig,
                None => {
                    self.diagnostics.error(
                        type_name.span.clone(),
                        format!("unknown enum type: '{}'", type_name.value),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            }
        };

        let fqn = enum_sig.fqn.clone();

        // Find the variant and validate it has no payload
        let variant_pos = enum_sig
            .variants
            .iter()
            .position(|(name, _)| name == &variant_name.value);
        let variant_index = match variant_pos {
            Some(idx) => {
                let display_enum = if type_name.value.is_empty() {
                    fqn.symbol.0.as_str()
                } else {
                    type_name.value.as_str()
                };
                match &enum_sig.variants[idx].1 {
                    VariantPayload::None => idx as u32,
                    VariantPayload::Tuple(types) => {
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "variant '{}.{}' expects {} argument(s); use '{}.{}(...)' with parentheses",
                                display_enum, variant_name.value,
                                types.len(),
                                display_enum, variant_name.value
                            ),
                        );
                        return Some(TypedPattern::Wildcard);
                    }
                    VariantPayload::Record(_) => {
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "variant '{}.{}' has record-style fields; use '{}.{}' with {{ }}",
                                display_enum, variant_name.value,
                                display_enum, variant_name.value
                            ),
                        );
                        return Some(TypedPattern::Wildcard);
                    }
                }
            }
            None => {
                let display_enum = if type_name.value.is_empty() {
                    fqn.symbol.0.as_str()
                } else {
                    type_name.value.as_str()
                };
                self.diagnostics.error(
                    variant_name.span.clone(),
                    format!(
                        "no variant '{}' in enum '{}'",
                        variant_name.value, display_enum
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
        };

        // Check subject type compatibility
        if !subject_ty.is_error() {
            let subject_fqn_matches = match subject_ty {
                Type::Enum(sfqn, _) | Type::GenericEnum { fqn: sfqn, .. } => *sfqn == fqn,
                _ => false,
            };
            if !subject_fqn_matches {
                let display_enum = if type_name.value.is_empty() {
                    &fqn.symbol.0
                } else {
                    &type_name.value
                };
                self.diagnostics.error(
                    pat_span.clone(),
                    format!(
                        "type mismatch in enum pattern: expected '{}', found '{}'",
                        subject_ty, display_enum
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
        }

        let enum_type = match subject_ty {
            Type::GenericEnum { .. } => subject_ty.clone(),
            _ => Type::Enum(fqn.clone(), MangledName::for_type(&fqn)),
        };

        Some(TypedPattern::EnumVariant {
            enum_type,
            variant_name: variant_name.value.clone(),
            variant_index,
            payload_patterns: vec![],
        })
    }

    /// Infer a record pattern in a match arm.
    /// Returns `None` when the arm is statically unreachable.
    fn infer_record_pattern(
        &mut self,
        type_name: &crate::common::span::Spanned<String>,
        type_args: &[TypeExpr],
        fields: &[FieldPattern],
        pat_span: &crate::common::span::Span,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        // Bare brace patterns are disambiguated by the scrutinee, just like
        // bare positional variant patterns.
        if type_args.is_empty() && matches!(subject_ty, Type::Enum(..) | Type::GenericEnum { .. })
        {
            let enum_name = crate::common::span::Spanned::new(String::new(), type_name.span.clone());
            return self.infer_enum_variant_record_pattern(
                &enum_name,
                type_name,
                fields,
                pat_span,
                subject_ty,
            );
        }

        // Resolve record type
        let info = match self.resolve_record_type(&type_name.value) {
            Some(info) => info,
            None => {
                let msg =
                    if let Some(fqn) = self.registry.suggest_import_for_record(&type_name.value) {
                        format!(
                            "unknown record type: '{}'; try adding 'import {}'",
                            type_name.value, fqn
                        )
                    } else {
                        format!("unknown record type: '{}'", type_name.value)
                    };
                self.diagnostics.error(type_name.span.clone(), msg);
                return Some(TypedPattern::Wildcard);
            }
        };

        self.infer_resolved_record_pattern(&info, type_name, type_args, fields, pat_span, subject_ty)
    }

    pub(super) fn infer_resolved_record_pattern(
        &mut self,
        info: &crate::typechecker::registry::RecordTypeSignature,
        type_name: &crate::common::span::Spanned<String>,
        type_args: &[TypeExpr],
        fields: &[FieldPattern],
        pat_span: &crate::common::span::Span,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        let fqn = info.fqn.clone();

        // Resolve type arguments, substitute field types, and build the record Type
        let (record_ty, field_types) = if !type_args.is_empty() {
            // Validate type arg count
            if type_args.len() != info.type_params.len() {
                self.diagnostics.error(
                    pat_span.clone(),
                    format!(
                        "expected {} type argument(s) for '{}', found {}",
                        info.type_params.len(),
                        type_name.value,
                        type_args.len()
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }

            // Resolve type args
            let resolved = match self.resolve_type_args(type_args) {
                Some(args) => args,
                None => return Some(TypedPattern::Wildcard),
            };

            // Resolve the generic record type
            let instantiated_ty = self.resolve_generic_record(&fqn, info, &resolved, pat_span);

            // Substitute type params in field types to get concrete field types
            let substitution = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                &info.type_params,
                &resolved,
            );
            let concrete_fields = info.fields.iter().map(|(n, ty)| (n.clone(), apply_substitution(&substitution, ty))).collect();

            (instantiated_ty, concrete_fields)
        } else if let Type::GenericRecord { type_args: scrutinee_type_args, .. } = subject_ty
        {
            // Static path: infer type args from scrutinee type
            let stripped_args: Vec<Type> = scrutinee_type_args
                .iter()
                .map(|(_, t)| t.clone())
                .collect();
            let substitution = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                &info.type_params,
                &stripped_args,
            );
            let concrete_fields = info.fields.iter().map(|(n, ty)| (n.clone(), apply_substitution(&substitution, ty))).collect();
            (subject_ty.clone(), concrete_fields)
        } else {
            // Non-generic record
            let mn = MangledName::for_type(&fqn);
            (Type::Record(fqn.clone(), mn), info.fields.clone())
        };

        // A record pattern against an interface-object scrutinee would be a
        // downcast test, which interface objects don't support — the pattern
        // "matching" via the concrete→interface coercion rule is wrong here.
        if matches!(subject_ty, Type::InterfaceObject { .. }) {
            self.diagnostics.error(
                pat_span.clone(),
                format!(
                    "cannot match a record pattern against interface type '{}'; interface objects do not support downcasts",
                    subject_ty
                ),
            );
            return Some(TypedPattern::Wildcard);
        }

        // Check pattern type (record) is assignable to subject (scrutinee).
        if !subject_ty.is_error() && !self.is_assignable_for_pattern(subject_ty, &record_ty) {
            self.diagnostics.error(
                pat_span.clone(),
                format!(
                    "type mismatch in record pattern: expected '{}', found '{}'",
                    subject_ty, type_name.value
                ),
            );
            return Some(TypedPattern::Wildcard);
        }

        let mut typed_fields = Vec::new();
        let mut seen = std::collections::BTreeSet::new();

        for field_pat in fields {
            let field_name = &field_pat.name.value;

            // Check for duplicate fields
            if !seen.insert(field_name.clone()) {
                self.diagnostics.error(
                    field_pat.name.span.clone(),
                    format!("duplicate field '{}' in record pattern", field_name),
                );
                continue;
            }

            // Find field in the (possibly substituted) field list
            if let Some((idx, (_, field_ty))) = field_types
                .iter()
                .enumerate()
                .find(|(_, (name, _))| name == field_name)
            {
                let typed_sub_pattern = self.infer_field_sub_pattern(
                    &field_pat.pattern,
                    field_name,
                    field_ty,
                );

                typed_fields.push(TypedFieldPattern {
                    field_name: field_name.clone(),
                    field_index: idx as u32,
                    pattern: typed_sub_pattern,
                });
            } else {
                self.diagnostics.error(
                    field_pat.name.span.clone(),
                    format!("no field '{}' on record '{}'", field_name, type_name.value),
                );
            }
        }

        Some(TypedPattern::Record {
            ty: record_ty,
            fields: typed_fields,
        })
    }

    /// Infer an enum variant record pattern in a match arm: `Shape.Point { x = px, y = py }`.
    /// Returns `None` when the arm is statically unreachable.
    fn infer_enum_variant_record_pattern(
        &mut self,
        type_name: &crate::common::span::Spanned<String>,
        variant_name: &crate::common::span::Spanned<String>,
        field_patterns: &[FieldPattern],
        pat_span: &crate::common::span::Span,
        subject_ty: &Type,
    ) -> Option<TypedPattern> {
        // Resolve the enum type -- bare variant patterns have empty type_name
        let enum_sig = if type_name.value.is_empty() {
            let scrutinee_fqn = match subject_ty {
                Type::Enum(fqn, _) | Type::GenericEnum { fqn, .. } => fqn.clone(),
                _ => {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!(
                            "bare variant pattern '{}' requires an enum scrutinee, found '{}'",
                            variant_name.value, subject_ty
                        ),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            };
            match self.registry.lookup_enum_type(&scrutinee_fqn, &self.package_path, &self.current_file) {
                Some(sig) => sig.clone(),
                None => {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!("unknown enum type: '{}'", scrutinee_fqn),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            }
        } else {
            match self.resolve_enum_type(&type_name.value) {
                Some(sig) => sig,
                None => {
                    self.diagnostics.error(
                        type_name.span.clone(),
                        format!("unknown enum type: '{}'", type_name.value),
                    );
                    return Some(TypedPattern::Wildcard);
                }
            }
        };

        let fqn = enum_sig.fqn.clone();

        // Find the variant
        let variant_pos = enum_sig
            .variants
            .iter()
            .position(|(name, _)| name == &variant_name.value);
        let (variant_index, expected_fields) = match variant_pos {
            Some(idx) => {
                let fields = match &enum_sig.variants[idx].1 {
                    VariantPayload::Record(f) => f.clone(),
                    VariantPayload::Tuple(payload) if payload.len() == 1 => {
                        return Some(self.infer_record_payload_pattern(
                            &enum_sig,
                            idx,
                            &payload[0],
                            field_patterns,
                            pat_span,
                            subject_ty,
                        ));
                    }
                    VariantPayload::None | VariantPayload::Tuple(_) => {
                        let display_enum = if type_name.value.is_empty() {
                            fqn.symbol.0.as_str()
                        } else {
                            type_name.value.as_str()
                        };
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "variant '{}.{}' does not have record-style payload",
                                display_enum, variant_name.value
                            ),
                        );
                        return Some(TypedPattern::Wildcard);
                    }
                };
                (idx as u32, fields)
            }
            None => {
                let display_enum = if type_name.value.is_empty() {
                    fqn.symbol.0.as_str()
                } else {
                    type_name.value.as_str()
                };
                self.diagnostics.error(
                    variant_name.span.clone(),
                    format!("no variant '{}' in enum '{}'", variant_name.value, display_enum),
                );
                return Some(TypedPattern::Wildcard);
            }
        };

        // Build substitution from scrutinee's type args for generic enums
        let substitution = match subject_ty {
            Type::GenericEnum { type_args: scrutinee_type_args, .. } => {
                let stripped_args: Vec<Type> = scrutinee_type_args
                    .iter()
                    .map(|(_, t)| t.clone())
                    .collect();
                Some(super::type_param_substitution::TypeParamSubstitution::from_pairs(
                    &enum_sig.type_params,
                    &stripped_args,
                ))
            }
            _ => None,
        };

        // Substitute field types if we have a generic enum substitution
        let concrete_fields: Vec<(String, Type)> = if let Some(ref sub) = substitution {
            expected_fields
                .iter()
                .map(|(name, ty)| (name.clone(), apply_substitution(sub, ty)))
                .collect()
        } else {
            expected_fields.clone()
        };

        // Check subject type compatibility
        if !subject_ty.is_error() {
            let subject_fqn_matches = match subject_ty {
                Type::Enum(sfqn, _) | Type::GenericEnum { fqn: sfqn, .. } => *sfqn == fqn,
                _ => false,
            };
            if !subject_fqn_matches {
                self.diagnostics.error(
                    pat_span.clone(),
                    format!(
                        "type mismatch in enum pattern: expected '{}', found '{}'",
                        subject_ty,
                        if type_name.value.is_empty() { &fqn.symbol.0 } else { &type_name.value }
                    ),
                );
                return Some(TypedPattern::Wildcard);
            }
        }

        // Type-check field patterns
        let mut typed_fields = Vec::new();
        for field_pat in field_patterns {
            let field_name = &field_pat.name.value;
            if let Some((idx, (_, field_ty))) = concrete_fields
                .iter()
                .enumerate()
                .find(|(_, (name, _))| name == field_name)
            {
                let typed_sub_pattern = self.infer_field_sub_pattern(
                    &field_pat.pattern,
                    field_name,
                    field_ty,
                );
                typed_fields.push(TypedFieldPattern {
                    field_name: field_name.clone(),
                    field_index: idx as u32,
                    pattern: typed_sub_pattern,
                });
            } else {
                self.diagnostics.error(
                    field_pat.name.span.clone(),
                    format!(
                        "no field '{}' in variant '{}.{}'",
                        field_name,
                        if type_name.value.is_empty() { &fqn.symbol.0 } else { &type_name.value },
                        variant_name.value
                    ),
                );
            }
        }

        let enum_type = match subject_ty {
            Type::GenericEnum { .. } => subject_ty.clone(),
            _ => Type::Enum(fqn.clone(), MangledName::for_type(&fqn)),
        };

        Some(TypedPattern::EnumVariantRecord {
            enum_type,
            variant_name: variant_name.value.clone(),
            variant_index,
            field_patterns: typed_fields,
        })
    }

    /// Infer a sub-pattern inside a tuple payload or as a direct nested pattern.
    /// `expected_ty` is the type the sub-pattern must match against.
    pub(super) fn infer_sub_pattern(&mut self, sub_pat: &Pattern, expected_ty: &Type) -> TypedPattern {
        match sub_pat {
            Pattern::Wildcard(_) => TypedPattern::Wildcard,
            Pattern::Variable(name, _) => {
                // Check if this is a bare no-payload variant (e.g., `None` inside `Some(None)`)
                if let Some(typed_pat) = self.try_promote_variable_to_variant(name, expected_ty) {
                    return typed_pat;
                }
                let var_name = VarName(name.clone());
                self.define_variable(var_name.clone(), expected_ty.clone(), false);
                TypedPattern::Variable(var_name, expected_ty.clone())
            }
            Pattern::Literal(lit_expr, lit_span) => {
                let typed_lit = self.infer_literal_pattern(lit_expr);
                if !expected_ty.is_error() && !typed_lit.ty.is_error() {
                    self.check_assignable(lit_span.clone(), expected_ty, &typed_lit.ty);
                }
                TypedPattern::Literal(Box::new(typed_lit))
            }
            Pattern::EnumVariant {
                type_name,
                variant_name,
                span,
            } => self
                .infer_enum_variant_no_args_pattern(type_name, variant_name, span, expected_ty)
                .unwrap_or(TypedPattern::Wildcard),
            Pattern::EnumVariantTuple {
                type_name,
                variant_name,
                payload_patterns,
                span,
            } => self
                .infer_enum_variant_pattern(
                    type_name,
                    variant_name,
                    payload_patterns,
                    span,
                    expected_ty,
                )
                .unwrap_or(TypedPattern::Wildcard),
            Pattern::EnumVariantRecord {
                type_name,
                variant_name,
                fields,
                span,
            } => self
                .infer_enum_variant_record_pattern(
                    type_name,
                    variant_name,
                    fields,
                    span,
                    expected_ty,
                )
                .unwrap_or(TypedPattern::Wildcard),
            Pattern::Record {
                type_name,
                type_params,
                fields,
                span,
            } => self
                .infer_record_pattern(type_name, type_params, fields, span, expected_ty)
                .unwrap_or(TypedPattern::Wildcard),
            Pattern::TypeAnnotated {
                binding,
                type_expr,
                span,
            } => self
                .infer_type_annotated_pattern(binding, type_expr, span, expected_ty)
                .unwrap_or(TypedPattern::Wildcard),
            Pattern::Tuple(sub_pats, pat_span) => {
                match expected_ty {
                    Type::Tuple(elem_types, _mn) => {
                        if sub_pats.len() != elem_types.len() {
                            self.diagnostics.error(
                                pat_span.clone(),
                                format!(
                                    "tuple pattern has {} elements but the tuple has {}",
                                    sub_pats.len(),
                                    elem_types.len()
                                ),
                            );
                            TypedPattern::Wildcard
                        } else {
                            let element_patterns: Vec<TypedPattern> = sub_pats
                                .iter()
                                .zip(elem_types.iter())
                                .map(|(sp, et)| self.infer_sub_pattern(sp, et))
                                .collect();
                            TypedPattern::Tuple {
                                element_patterns,
                                tuple_type: expected_ty.clone(),
                            }
                        }
                    }
                    Type::Error => TypedPattern::Wildcard,
                    _ => {
                        self.diagnostics.error(
                            pat_span.clone(),
                            format!(
                                "cannot match tuple pattern against non-tuple type '{}'",
                                expected_ty
                            ),
                        );
                        TypedPattern::Wildcard
                    }
                }
            }
        }
    }

    /// Infer a sub-pattern for a record/enum-record field.
    /// `field_pattern` is `None` for bare shorthand (`{ x }`), `Some(pat)` for `{ x = pat }`.
    fn infer_field_sub_pattern(
        &mut self,
        field_pattern: &Option<Pattern>,
        field_name: &str,
        field_ty: &Type,
    ) -> TypedPattern {
        match field_pattern {
            None => {
                // Bare identifier shorthand: bind field to same-name variable
                let var_name = VarName(field_name.to_string());
                self.define_variable(var_name.clone(), field_ty.clone(), false);
                TypedPattern::Variable(var_name, field_ty.clone())
            }
            Some(sub_pattern) => self.infer_sub_pattern(sub_pattern, field_ty),
        }
    }
}
