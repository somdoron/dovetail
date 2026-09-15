use crate::common::diagnostics::Diagnostics;
use crate::common::span::{Span, Spanned};
use crate::common::types::{MangledName, TypeParamName, Variance};
use crate::parser::ast::{Expr, FieldInit, FieldPattern};
use crate::typechecker::registry::{EnumTypeSignature, RecordTypeSignature};
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind, TypedPattern};
use std::collections::BTreeMap;

use super::Inference;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;

impl Inference<'_> {
    fn enum_payload_substitution(
        enum_sig: &EnumTypeSignature,
        enum_type: Option<&Type>,
    ) -> TypeParamSubstitution {
        match enum_type {
            Some(Type::GenericEnum { fqn, type_args, .. }) if *fqn == enum_sig.fqn => {
                let parameters: Vec<_> = type_args.iter().map(|(_, ty)| ty.clone()).collect();
                TypeParamSubstitution::from_pairs(&enum_sig.type_params, &parameters)
            }
            _ => TypeParamSubstitution::new(),
        }
    }

    fn record_payload_definition(
        &mut self,
        payload: &Type,
        span: &Span,
    ) -> Option<(RecordTypeSignature, Vec<Type>)> {
        let (fqn, parameters) = match payload {
            Type::Record(fqn, _) => (fqn, vec![]),
            Type::GenericRecord { fqn, type_args, .. } => {
                (fqn, type_args.iter().map(|(_, ty)| ty.clone()).collect())
            }
            _ => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "variant does not have record-style payload: brace syntax requires a known record type, found '{}'",
                        payload,
                    ),
                );
                return None;
            }
        };
        match self
            .registry
            .lookup_record_type(fqn, &self.package_path, &self.current_file)
        {
            Some(definition) => Some((definition.clone(), parameters)),
            None => {
                self.diagnostics.error(
                    span.clone(),
                    format!("record payload type '{}' is not accessible", fqn),
                );
                None
            }
        }
    }

    pub(super) fn infer_record_payload_pattern(
        &mut self,
        enum_sig: &EnumTypeSignature,
        variant_index: usize,
        payload: &Type,
        fields: &[FieldPattern],
        span: &Span,
        subject_type: &Type,
    ) -> TypedPattern {
        if !subject_type.is_error()
            && !matches!(subject_type,
                Type::Enum(fqn, _) | Type::GenericEnum { fqn, .. } if *fqn == enum_sig.fqn)
        {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "type mismatch in enum pattern: expected '{}', found '{}'",
                    subject_type, enum_sig.fqn,
                ),
            );
            return TypedPattern::Wildcard;
        }
        let substitution = Self::enum_payload_substitution(enum_sig, Some(subject_type));
        let payload = apply_substitution(&substitution, payload);
        let Some((definition, _)) = self.record_payload_definition(&payload, span) else {
            return TypedPattern::Wildcard;
        };
        let name = Spanned::new(definition.fqn.to_string(), span.clone());
        let record_pattern = self
            .infer_resolved_record_pattern(&definition, &name, &[], fields, span, &payload)
            .unwrap_or(TypedPattern::Wildcard);
        TypedPattern::EnumVariant {
            enum_type: subject_type.clone(),
            variant_name: enum_sig.variants[variant_index].0.clone(),
            variant_index: variant_index as u32,
            payload_patterns: vec![record_pattern],
        }
    }

    pub(super) fn infer_record_payload_create(
        &mut self,
        enum_sig: &EnumTypeSignature,
        variant_name: &Spanned<String>,
        payload_template: &Type,
        fields: &[FieldInit],
        span: &Span,
    ) -> TypedExpr {
        let expected = Self::enum_payload_substitution(enum_sig, self.expected_type.as_ref());
        let payload = apply_substitution(&expected, payload_template);
        let Some((definition, parameters)) = self.record_payload_definition(&payload, span) else {
            return self.error_expr(span);
        };
        self.check_private_type_access(
            &definition.fqn,
            definition.construction_private,
            "record",
            span,
            "construct",
        );
        let record = self.infer_resolved_record_payload(&definition, &parameters, fields, span);
        if record.ty.is_error() {
            return record;
        }

        let mut inferred = TypeParamSubstitution::new();
        inferred.unify(payload_template, &record.ty);
        // Expected types supply parameters absent from this variant's payload.
        for parameter in &enum_sig.type_params {
            if inferred.get(parameter).is_none() {
                if let Some(ty) = expected.get(parameter) {
                    inferred.insert(parameter.clone(), ty.clone());
                }
            }
        }
        let parameters = inferred
            .resolve_type_params(&enum_sig.type_params)
            .or_else(|| {
                inferred.resolve_with_variance_defaults(
                    &enum_sig.type_params,
                    &enum_sig.type_param_variances,
                )
            });
        let Some(parameters) = parameters else {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "cannot infer type arguments for generic enum '{}'",
                    enum_sig.fqn,
                ),
            );
            return self.error_expr(span);
        };
        let substitution = TypeParamSubstitution::from_pairs(&enum_sig.type_params, &parameters);
        self.check_assignable(
            span.clone(),
            &apply_substitution(&substitution, payload_template),
            &record.ty,
        );
        let enum_type = if enum_sig.type_params.is_empty() {
            Type::Enum(enum_sig.fqn.clone(), MangledName::for_type(&enum_sig.fqn))
        } else {
            self.resolve_generic_enum_type(&enum_sig.fqn, enum_sig, &parameters)
        };
        TypedExpr {
            ty: enum_type,
            kind: TypedExprKind::EnumCreate {
                fqn: enum_sig.fqn.clone(),
                variant_name: variant_name.value.clone(),
                args: vec![record],
                type_params: parameters,
            },
            span: span.clone(),
        }
    }

    fn infer_resolved_record_payload(
        &mut self,
        definition: &RecordTypeSignature,
        parameters: &[Type],
        fields: &[FieldInit],
        span: &Span,
    ) -> TypedExpr {
        let substitution = TypeParamSubstitution::from_pairs(&definition.type_params, parameters);
        let template_fields: Vec<_> = definition
            .fields
            .iter()
            .map(|(name, ty)| (name.clone(), apply_substitution(&substitution, ty)))
            .collect();
        let resolved_parameters =
            self.infer_record_payload_parameters(&template_fields, parameters, fields);
        if resolved_parameters.iter().any(Type::contains_type_variable) {
            self.diagnostics.error(span.clone(), format!(
                "cannot infer type arguments for record payload '{}'; provide an expected enum type",
                definition.fqn,
            ));
            return self.error_expr(span);
        }
        let record_type = if definition.type_params.is_empty() {
            Type::Record(
                definition.fqn.clone(),
                MangledName::for_type(&definition.fqn),
            )
        } else {
            self.resolve_generic_record(&definition.fqn, definition, &resolved_parameters, span)
        };
        let substitution =
            TypeParamSubstitution::from_pairs(&definition.type_params, &resolved_parameters);
        let concrete_fields: Vec<_> = definition
            .fields
            .iter()
            .map(|(name, ty)| (name.clone(), apply_substitution(&substitution, ty)))
            .collect();
        let name = Spanned::new(definition.fqn.to_string(), span.clone());
        let checked_fields = self.check_record_fields(&concrete_fields, &name, fields, None, span);
        TypedExpr {
            ty: record_type,
            kind: TypedExprKind::RecordCreate {
                fqn: definition.fqn.clone(),
                fields: checked_fields,
                type_params: resolved_parameters,
            },
            span: span.clone(),
        }
    }

    fn infer_record_payload_parameters(
        &self,
        template_fields: &[(String, Type)],
        parameters: &[Type],
        fields: &[FieldInit],
    ) -> Vec<Type> {
        if !parameters.iter().any(Type::contains_type_variable) {
            return parameters.to_vec();
        }
        let mut inferred = TypeParamSubstitution::new();
        let mut bounds = BTreeMap::<TypeParamName, PayloadBounds>::new();
        // Gather independent evidence from every field before contextual probes
        // can turn provisional upper bounds into producer evidence. Repeat both
        // phases with Any/Never defaults after concrete evidence stabilizes.
        for (allow_defaults, allow_upper_context) in
            [(false, false), (false, true), (true, false), (true, true)]
        {
            loop {
                let mut changed = false;
                for field in fields {
                    let Some((_, template)) = template_fields
                        .iter()
                        .find(|(name, _)| *name == field.name.value)
                    else {
                        continue;
                    };
                    let actual = self.probe_record_payload_evidence(
                        &field.value,
                        template,
                        &inferred,
                        &bounds,
                        allow_defaults,
                        allow_upper_context,
                    );
                    let Some(actual) = actual else {
                        continue;
                    };
                    let mut constraints = Vec::new();
                    collect_payload_constraints(
                        template,
                        &actual,
                        Variance::Covariant,
                        &mut constraints,
                    );
                    for (parameter, ty, variance) in constraints {
                        if ty.contains_type_variable()
                            || (inferred.get(&parameter).is_none()
                                && !allow_defaults
                                && ty.contains_any_or_never())
                        {
                            continue;
                        }
                        let bound = bounds.entry(parameter.clone()).or_default();
                        let combined = self.merge_record_payload_bound(bound, ty, variance);
                        if inferred.get(&parameter) != Some(&combined) {
                            inferred.insert(parameter, combined);
                            changed = true;
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
        }
        parameters
            .iter()
            .map(|ty| apply_substitution(&inferred, ty))
            .collect()
    }

    fn probe_record_payload_evidence(
        &self,
        expression: &Expr,
        template: &Type,
        inferred: &TypeParamSubstitution,
        bounds: &BTreeMap<TypeParamName, PayloadBounds>,
        allow_defaults: bool,
        allow_upper_context: bool,
    ) -> Option<Type> {
        let expected = apply_substitution(inferred, template);
        let expected = if matches!(expected, Type::TypeVariable(..))
            || (allow_defaults && expected.contains_type_variable())
        {
            None
        } else {
            Some(expected)
        };
        let contextual = expected.is_some();
        // Upper bounds constrain consumers but must not widen producers.
        // Use only established lower bounds for the first probe, preserving
        // independent concrete evidence inside partially defaulted types.
        let mut lower = TypeParamSubstitution::new();
        for (parameter, bound) in bounds {
            if let Some(ty) = &bound.lower {
                lower.insert(parameter.clone(), ty.clone());
            }
        }
        let partial = apply_substitution(&lower, template);
        let partial = (!matches!(partial, Type::TypeVariable(..))).then_some(partial);
        let independent = self
            .probe_record_payload_field(expression, partial)
            .filter(|ty| !ty.contains_type_variable());
        if independent.is_some() || !allow_upper_context {
            return independent;
        }
        self.probe_record_payload_field(expression, expected)
            .or_else(|| {
                contextual
                    .then(|| self.probe_record_payload_field(expression, None))
                    .flatten()
            })
    }

    fn merge_record_payload_bound(
        &self,
        bound: &mut PayloadBounds,
        ty: Type,
        variance: Variance,
    ) -> Type {
        if variance != Variance::Contravariant {
            bound.lower = Some(match &bound.lower {
                None => ty.clone(),
                Some(lower) => self
                    .least_upper_bound(lower, &ty)
                    .unwrap_or_else(|| lower.clone()),
            });
        }
        if variance != Variance::Covariant {
            bound.upper = Some(match &bound.upper {
                Some(upper) if !self.is_assignable(upper, &ty) => upper.clone(),
                _ => ty,
            });
        }
        // Values supply lower bounds; callback inputs supply upper bounds.
        // Prefer a lower bound when both exist and let the final field check
        // diagnose incompatible constraints.
        bound
            .lower
            .as_ref()
            .or(bound.upper.as_ref())
            .unwrap()
            .clone()
    }

    /// Probe in a separate inference context. Failed attempts must not leave
    /// diagnostics, local bindings, type references, or generated state behind.
    /// The actual field expression is checked once after its type is resolved.
    fn probe_record_payload_field(
        &self,
        expression: &Expr,
        expected: Option<Type>,
    ) -> Option<Type> {
        let mut diagnostics = Diagnostics::new();
        let mut probe = Inference {
            scopes: self.scopes.clone(),
            class_type_defs: self.class_type_defs.clone(),
            current_type_params: self.current_type_params.clone(),
            type_param_counter: self.type_param_counter,
            expected_type: expected,
            loop_depth: self.loop_depth,
            function_return_type: self.function_return_type.clone(),
            async_return_type: self.async_return_type.clone(),
            block_wrapped_error: self.block_wrapped_error.clone(),
            container_name: self.container_name.clone(),
            current_module_name: self.current_module_name.clone(),
            unresolved_method_type_params: self.unresolved_method_type_params.clone(),
            typechecking_class: self.typechecking_class.clone(),
            ..Inference::new(
                self.package_path.clone(),
                self.current_file.clone(),
                self.registry,
                self.import_scope,
                &mut diagnostics,
            )
        };
        let ty = probe.infer_expr(expression).ty;
        (!diagnostics.has_errors() && !ty.is_error()).then_some(ty)
    }
}

#[derive(Default)]
struct PayloadBounds {
    lower: Option<Type>,
    upper: Option<Type>,
}

fn compose_variance(outer: Variance, inner: Variance) -> Variance {
    match (outer, inner) {
        (Variance::Invariant, _) | (_, Variance::Invariant) => Variance::Invariant,
        (Variance::Covariant, variance) | (variance, Variance::Covariant) => variance,
        (Variance::Contravariant, Variance::Contravariant) => Variance::Covariant,
    }
}

/// Retain each occurrence's direction instead of flattening callback inputs
/// and produced values into the same substitution.
fn collect_payload_constraints(
    template: &Type,
    actual: &Type,
    variance: Variance,
    constraints: &mut Vec<(TypeParamName, Type, Variance)>,
) {
    match (template, actual) {
        (Type::TypeVariable(name, _), actual) => {
            constraints.push((name.clone(), actual.clone(), variance));
        }
        (Type::Function(expected_params, expected_return), Type::Function(params, ret)) => {
            for (expected, actual) in expected_params.iter().zip(params) {
                collect_payload_constraints(
                    expected,
                    actual,
                    compose_variance(variance, Variance::Contravariant),
                    constraints,
                );
            }
            collect_payload_constraints(expected_return, ret, variance, constraints);
        }
        (Type::Array(expected), Type::Array(actual)) => {
            collect_payload_constraints(expected, actual, Variance::Invariant, constraints);
        }
        (Type::Tuple(expected, _), Type::Tuple(actual, _)) => {
            for (expected, actual) in expected.iter().zip(actual) {
                collect_payload_constraints(expected, actual, variance, constraints);
            }
        }
        (
            Type::GenericRecord {
                fqn: ef,
                type_args: expected,
                ..
            },
            Type::GenericRecord {
                fqn: af,
                type_args: actual,
                ..
            },
        )
        | (
            Type::GenericEnum {
                fqn: ef,
                type_args: expected,
                ..
            },
            Type::GenericEnum {
                fqn: af,
                type_args: actual,
                ..
            },
        )
        | (
            Type::GenericClass {
                fqn: ef,
                type_args: expected,
                ..
            },
            Type::GenericClass {
                fqn: af,
                type_args: actual,
                ..
            },
        )
        | (
            Type::GenericNewtype {
                fqn: ef,
                type_args: expected,
                ..
            },
            Type::GenericNewtype {
                fqn: af,
                type_args: actual,
                ..
            },
        ) if ef == af => {
            for ((inner, expected), (_, actual)) in expected.iter().zip(actual) {
                collect_payload_constraints(
                    expected,
                    actual,
                    compose_variance(variance, *inner),
                    constraints,
                );
            }
        }
        (
            Type::InterfaceObject {
                traits: expected, ..
            },
            Type::InterfaceObject { traits: actual, .. },
        ) => {
            for expected in expected {
                if let Some(actual) = actual
                    .iter()
                    .find(|actual| actual.trait_fqn == expected.trait_fqn)
                {
                    for (expected, actual) in
                        expected.trait_type_args.iter().zip(&actual.trait_type_args)
                    {
                        collect_payload_constraints(
                            expected,
                            actual,
                            Variance::Invariant,
                            constraints,
                        );
                    }
                }
            }
        }
        _ => {}
    }
}
