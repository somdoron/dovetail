//! Type-check trait members' default bodies (trait-design-appendix §4).
//!
//! A default body is checked ONCE, in the trait's own package and import
//! scope, with `Self` bound as a type variable constrained by the declaring
//! trait itself — so `self.otherMember()` resolves through the ordinary
//! trait-bound dispatch into deferred `ImplFunctionCall`s. The result is a
//! template `TypedFunction` stored in `TypedModule.default_templates`, keyed
//! `MangledName::for_trait_default(trait, member)`; monomorphize materializes
//! a per-implementing-type copy (under the standard impl-member name) whenever
//! an implementor omits the member.

use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::parser::ast::TraitDecl;
use crate::typechecker::types::{BoundKind, NamedTraitBound, TraitBound, TypedFunction, TypedParam};
use crate::typechecker::types::Type;

use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;
use super::{Inference, VarName};

impl Inference<'_> {
    fn check_default_override_contract(
        &mut self,
        owner: &crate::typechecker::registry::TraitSignature,
        method: &crate::typechecker::registry::TraitMethodSig,
    ) {
        use crate::typechecker::collect::rename_method_bounds;
        use crate::typechecker::registry::{instantiate_trait_method, same_method_parameters};
        use crate::typechecker::types::TraitBounds;
        let Some((origin, arguments)) = &method.origin else { return };
        let Some(source) = self.registry.get_trait(origin) else { return };
        let substitution: std::collections::BTreeMap<_, _> = source.type_params.iter()
            .cloned().zip(arguments.iter().cloned()).collect();
        let Some(contract) = source.methods.iter().find(|candidate| {
            if candidate.name != method.name { return false; }
            let candidate = instantiate_trait_method(candidate, &substitution);
            same_method_parameters(&candidate, method)
        }) else { return };
        let available = rename_method_bounds(
            &contract.trait_bounds, &contract.type_params, &method.type_params, &substitution,
        );
        let parameters: Vec<_> = owner.type_params.iter().chain(&method.type_params).cloned().collect();
        let evidence = Type::type_param_map(&parameters, &available);
        let arguments: Vec<_> = parameters.iter().map(|name| evidence[&name.0].clone()).collect();
        let required: TraitBounds = method.trait_bounds.clone();
        for failure in self.unsatisfied_trait_bounds(&required, &parameters, &arguments) {
            self.diagnostics.error(method.span.clone(), format!(
                "default override '{}' cannot strengthen its inherited trait contract: {failure}", method.name,
            ));
        }
    }

    pub(super) fn infer_trait_defaults(&mut self, trait_decl: &TraitDecl) {
        let has_default_bodies = trait_decl.methods.iter().any(|m| m.body.is_some())
            || trait_decl.properties.iter().any(|p| p.body.is_some());
        if !has_default_bodies {
            return;
        }

        let trait_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(trait_decl.name.value.clone()),
        };
        let Some(trait_sig) = self
            .registry
            .lookup_trait(&trait_fqn, &self.package_path)
            .cloned()
        else {
            return;
        };

        // `Self` = a type variable bounded by the declaring trait, applied at
        // its own type params (each itself a plain type variable).
        let trait_param_vars: Vec<Type> = trait_sig
            .type_params
            .iter()
            .map(|tp| Type::TypeVariable(tp.clone(), vec![]))
            .collect();
        let self_ty = Type::TypeVariable(
            TypeParamName("Self".to_string()),
            vec![TraitBound::Named(NamedTraitBound {
                associated_types: Default::default(),
                trait_fqn: trait_fqn.clone(),
                type_args: trait_param_vars.clone(),
                kind: BoundKind::HasTrait,
            })],
        );

        // Bind Self, trait parameters, and symbolic associated outputs before
        // checking the body against the trait's declared contract.
        let prev_type_params = std::mem::take(&mut self.current_type_params);
        self.current_type_params
            .insert(TypeParamName("Self".to_string()), self_ty.clone());
        for (tp, var) in trait_sig.type_params.iter().zip(trait_param_vars.iter()) {
            self.current_type_params.insert(tp.clone(), var.clone());
        }
        for assoc in &trait_sig.associated_types {
            self.current_type_params.insert(
                TypeParamName(assoc.name.clone()),
                Type::TypeVariable(TypeParamName(assoc.name.clone()), vec![]),
            );
        }

        let mut self_sub = TypeParamSubstitution::new().with_self_type(self_ty.clone());
        let Type::TypeVariable(_, self_bounds) = &self_ty else { unreachable!() };
        let self_bound = self_bounds[0].named().unwrap();
        for associated in &trait_sig.associated_types {
            let parameters = associated.type_params.iter().map(|name| Type::TypeVariable(name.clone(), vec![])).collect();
            if let Some(projection) = crate::typechecker::associated_types::from_bound(
                &self_ty, self_bound, &associated.name, parameters, self.registry,
            ) {
                self_sub.insert(TypeParamName(associated.name.clone()), projection.clone());
                self.current_type_params.insert(TypeParamName(associated.name.clone()), projection);
            }
        }
        // Template type params: Self + the trait's own params (all present in
        // the body's types; monomorphize substitutes them per implementor).
        let mut template_type_params = vec![TypeParamName("Self".to_string())];
        template_type_params.extend(trait_sig.type_params.iter().cloned());

        for method in &trait_decl.methods {
            let Some(body_ast) = &method.body else { continue };
            // Note: a default-override of an inherited member carries
            // origin = Some(super) after flattening — match by default_source
            // (this trait) instead of origin.
            let Some(sig) = trait_sig
                .methods
                .iter()
                .find(|m| m.name == method.name.value && m.default_source.as_ref() == Some(&trait_fqn))
            else {
                continue;
            };
            if sig.default_source.is_none() {
                // Declaration-site error already reported (generic/intrinsic).
                continue;
            }

            self.check_default_override_contract(&trait_sig, sig);

            let previous_method_scope = self.current_type_params.clone();
            let mut method_substitution = self_sub.clone();
            let mut method_template_parameters = template_type_params.clone();
            for parameter in &sig.type_params {
                let identity = TypeParamName(format!("$method${}", parameter.0));
                let variable = Type::TypeVariable(identity.clone(), vec![]);
                method_substitution.insert(parameter.clone(), variable.clone());
                self.current_type_params.insert(parameter.clone(), variable);
                method_template_parameters.push(identity);
            }
            for parameter in &sig.type_params {
                // Bounds can mention both enclosing and method parameters.
                let bounds = sig.trait_bounds.get(parameter).cloned().unwrap_or_default();
                let identity = TypeParamName(format!("$method${}", parameter.0));
                let bounded = Type::TypeVariable(identity, bounds);
                let bounded = apply_substitution(&method_substitution, &bounded);
                self.current_type_params.insert(parameter.clone(), bounded.clone());
                method_substitution.insert(parameter.clone(), bounded);
            }

            let typed_params: Vec<TypedParam> = sig
                .params
                .iter()
                .map(|(name, ty)| TypedParam {
                    name: name.clone(),
                    ty: apply_substitution(&method_substitution, ty),
                    span: method.span.clone(),
                })
                .collect();
            let return_type = apply_substitution(&method_substitution, &sig.return_type);

            self.push_scope();
            for param in &typed_params {
                self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
            }
            self.push_scope();
            let prev_expected = self.expected_type.take();
            let prev_fn_return = self.function_return_type.take();
            self.expected_type = Some(return_type.clone());
            self.function_return_type = Some(return_type.clone());
            let body = self.infer_expr(body_ast);
            self.pop_scope();
            self.pop_scope();
            self.check_assignable(body.span.clone(), &return_type, &body.ty);
            self.expected_type = prev_expected;
            self.function_return_type = prev_fn_return;
            self.current_type_params = previous_method_scope;

            let template_mn =
                MangledName::for_trait_default(&trait_fqn, &SymbolName(sig.name.clone()));
            let display_name = super::make_display_name(
                &format!("{}.{}", trait_fqn.symbol, sig.name),
                &typed_params,
            );
            self.default_templates.insert(
                template_mn.clone(),
                TypedFunction {
                    visibility: trait_sig.visibility,
                    name: template_mn,
                    source_name: sig.name.clone(),
                    type_params: method_template_parameters,
                    params: typed_params,
                    return_type,
                    body,
                    span: method.span.clone(),
                    vtable_self_type: None,
                    is_async: false,
                    display_name,
                },
            );
        }

        for property in &trait_decl.properties {
            let Some(body_ast) = &property.body else { continue };
            let Some(sig) = trait_sig
                .properties
                .iter()
                .find(|p| p.name == property.name.value && p.default_source.as_ref() == Some(&trait_fqn))
            else {
                continue;
            };
            if sig.default_source.is_none() {
                continue;
            }

            let typed_params: Vec<TypedParam> = sig
                .params
                .iter()
                .map(|(name, ty)| TypedParam {
                    name: name.clone(),
                    ty: apply_substitution(&self_sub, ty),
                    span: property.span.clone(),
                })
                .collect();
            let return_type = apply_substitution(&self_sub, &sig.return_type);

            self.push_scope();
            for param in &typed_params {
                self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
            }
            self.push_scope();
            let prev_expected = self.expected_type.take();
            let prev_fn_return = self.function_return_type.take();
            self.expected_type = Some(return_type.clone());
            self.function_return_type = Some(return_type.clone());
            let body = self.infer_expr(body_ast);
            self.pop_scope();
            self.pop_scope();
            self.check_assignable(body.span.clone(), &return_type, &body.ty);
            self.expected_type = prev_expected;
            self.function_return_type = prev_fn_return;

            let template_mn =
                MangledName::for_trait_default(&trait_fqn, &SymbolName(sig.name.clone()));
            let display_name = super::make_display_name(
                &format!("{}.{}", trait_fqn.symbol, sig.name),
                &typed_params,
            );
            self.default_templates.insert(
                template_mn.clone(),
                TypedFunction {
                    visibility: trait_sig.visibility,
                    name: template_mn,
                    source_name: sig.name.clone(),
                    type_params: template_type_params.clone(),
                    params: typed_params,
                    return_type,
                    body,
                    span: property.span.clone(),
                    vtable_self_type: None,
                    is_async: false,
                    display_name,
                },
            );
        }

        self.current_type_params = prev_type_params;
    }
}
