//! Element access through the read and write indexing capabilities.

use crate::common::span::Span;
use crate::common::types::Fqn;
use crate::typechecker::registry::ImplBlockSignature;
use crate::typechecker::types::{BoundKind, NamedTraitBound, Type, TypedExpr, TypedExprKind};

use super::Inference;
use super::generics::apply_substitution;
use super::traits::ImplMethodResolution;
use super::type_param_substitution::TypeParamSubstitution;

impl Inference<'_> {
    pub(super) fn infer_trait_index(
        &mut self,
        receiver: TypedExpr,
        index: TypedExpr,
        value: Option<TypedExpr>,
        span: &Span,
    ) -> TypedExpr {
        if receiver.ty.is_error()
            || index.ty.is_error()
            || value.as_ref().is_some_and(|v| v.ty.is_error())
        {
            return Self::index_error(span);
        }
        let (name, method, associated) = if value.is_some() {
            ("IndexSet", "set", "Value")
        } else {
            ("Index", "get", "Output")
        };
        let bound = NamedTraitBound {
            trait_fqn: Fqn::from_dotted(&format!("standard.prelude.{name}")).unwrap(),
            type_args: vec![index.ty.clone()],
            associated_types: Default::default(),
            kind: BoundKind::HasTrait,
        };
        let Some((key, output)) = self.index_element_type(&receiver.ty, &bound, associated, span)
        else {
            return Self::index_error(span);
        };
        if let Some(value) = &value {
            self.check_assignable(value.span.clone(), &output, &value.ty);
        }
        let mut args = vec![receiver, index];
        args.extend(value);
        // Resolve with the selected parameter types. The real arguments retain
        // their types so the ordinary coercion pass can insert conversions.
        let mut parameter_types = vec![&key];
        if args.len() == 3 {
            parameter_types.push(&output);
        }
        let expected = self.expected_type.take();
        let resolved = self.resolve_trait_impl_method_for_type_detailed(
            &args[0].ty,
            &bound.trait_fqn,
            method,
            &parameter_types,
            std::slice::from_ref(&key),
        );
        self.expected_type = expected;
        match resolved {
            ImplMethodResolution::Found {
                resolved,
                return_type,
                ..
            } => TypedExpr {
                kind: TypedExprKind::ImplFunctionCall {
                    trait_fqn: resolved.trait_fqn,
                    trait_type_params: resolved.trait_type_params,
                    for_type: resolved.for_type,
                    method_name: resolved.method_name,
                    method_type_params: resolved.method_type_params,
                    args,
                },
                ty: return_type,
                span: span.clone(),
            },
            _ => {
                self.diagnostics.error(
                    span.clone(),
                    format!("cannot resolve {name}.{method} for index operator"),
                );
                Self::index_error(span)
            }
        }
    }

    fn index_element_type(
        &mut self,
        receiver: &Type,
        bound: &NamedTraitBound,
        associated: &str,
        span: &Span,
    ) -> Option<(Type, Type)> {
        let name = &bound.trait_fqn.symbol.0;
        // Select by receiver and key before inspecting the assigned value or
        // expected result. Neither may disambiguate overlapping implementations.
        let outputs = self.index_applications(receiver, &bound.trait_fqn, &bound.type_args[0]);
        if outputs.len() != 1 {
            let message = if outputs.is_empty() {
                format!(
                    "index operator requires {name}<{}> for {}",
                    bound.type_args[0], receiver
                )
            } else {
                format!(
                    "ambiguous index operator: multiple {name}<{}> implementations for {}",
                    bound.type_args[0], receiver
                )
            };
            self.diagnostics.error(span.clone(), message);
            return None;
        }
        let Some(output) = outputs[0].associated_types.get(associated) else {
            self.diagnostics.error(span.clone(), format!(
                "index operator requires an associated type constraint: {name}<{}, {associated} = ElementType>", bound.type_args[0],
            ));
            return None;
        };
        Some((
            outputs[0].type_args[0].clone(),
            self.scoped_bound_type(output),
        ))
    }

    fn index_applications(
        &self,
        receiver: &Type,
        trait_fqn: &Fqn,
        key: &Type,
    ) -> Vec<NamedTraitBound> {
        if let Some(bounds) = self.type_parameter_trait_applications(receiver, trait_fqn) {
            return bounds
                .into_iter()
                .filter(|bound| {
                    bound.type_args.len() == 1 && self.is_assignable(&bound.type_args[0], key)
                })
                .collect();
        }
        let Some(receiver_fqn) = receiver.try_to_fqn() else {
            return Vec::new();
        };
        let mut direct = Vec::new();
        let mut provided = Vec::new();
        for (block, via) in self
            .registry
            .find_providing_impl_blocks(trait_fqn, &receiver_fqn)
        {
            let patterns = via
                .as_ref()
                .map_or(&block.trait_type_args, |(_, args)| args);
            let [pattern] = patterns.as_slice() else {
                continue;
            };
            let Some(declared_key) = self.index_impl_key(block, receiver, pattern, key) else {
                continue;
            };
            let Some(outputs) = self.associated_outputs_for_impl(
                block,
                receiver,
                patterns,
                std::slice::from_ref(&declared_key),
                0,
                &mut Vec::new(),
            ) else {
                continue;
            };
            let application = NamedTraitBound {
                trait_fqn: trait_fqn.clone(),
                type_args: vec![declared_key],
                associated_types: outputs.types,
                kind: BoundKind::HasTrait,
            };
            if via.is_none() {
                direct.push(application)
            } else {
                provided.push(application)
            }
        }
        // Match the ordinary resolver's direct-implementation preference for
        // each trait application, without hiding providers of other key types.
        provided.retain(|p| !direct.iter().any(|d| d.type_args == p.type_args));
        direct.extend(provided);
        direct
    }

    fn index_impl_key(
        &self,
        block: &ImplBlockSignature,
        receiver: &Type,
        pattern: &Type,
        actual: &Type,
    ) -> Option<Type> {
        let mut substitution = TypeParamSubstitution::new().with_self_type(receiver.clone());
        if !substitution.unify(&block.for_type, receiver) {
            return None;
        }
        // Unification infers free key parameters. Failure can still be a valid
        // coercion (a subclass or concrete interface implementer, for example).
        let mut inferred = substitution.clone();
        if inferred.unify(pattern, actual) {
            substitution = inferred
        }
        self.infer_associated_bound_types(&block.trait_bounds, &mut substitution);
        substitution.resolve_type_params(&block.type_params)?;
        let declared = apply_substitution(&substitution, pattern);
        self.is_assignable(&declared, actual).then_some(declared)
    }

    fn index_error(span: &Span) -> TypedExpr {
        TypedExpr {
            kind: TypedExprKind::UnitLiteral,
            ty: Type::Error,
            span: span.clone(),
        }
    }
}
