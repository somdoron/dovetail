use std::collections::BTreeMap;

use crate::common::types::{SymbolName, TypeParamName};
use crate::parser::ast::{Expr, ImplementDecl};

use crate::common::types::Fqn;
use crate::typechecker::registry::{
    GenericFunctionDef, ImplBlockSignature, ImplMethodSignature, TraitSignature,
};
use crate::typechecker::types::{NamedTraitBound, TraitBound, TraitBounds, Type};

use super::Collector;

impl Collector<'_> {
    /// Collect an implement block: validate methods against the trait and register.
    pub(super) fn collect_implement(&mut self, impl_decl: &ImplementDecl) {
        if !impl_decl.type_params.is_empty() {
            self.collect_generic_implement(impl_decl);
            return;
        }

        // 1. Resolve the for-type
        let for_type = self.resolve_type_expr(&impl_decl.for_type);
        if for_type.is_error() {
            return;
        }
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            self.diagnostics.error(
                impl_decl.for_type.span(),
                "cannot implement a trait for an intersection type; implement each interface's contract separately".to_string(),
            );
            return;
        }
        let Some(type_fqn) = for_type.try_to_fqn() else {
            self.diagnostics.error(
                impl_decl.for_type.span(),
                "implement blocks for a bare type parameter are not supported".to_string(),
            );
            return;
        };

        // 2. Resolve the trait through imports (same pattern as resolve_fqn in inference)
        let (trait_fqn, trait_sig) = match self.resolve_trait(&impl_decl.trait_name.value) {
            Some(result) => result,
            None => {
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!("unknown trait: '{}'", impl_decl.trait_name.value),
                );
                return;
            }
        };

        if crate::typechecker::types::is_tuple_constraint(&trait_fqn) {
            self.diagnostics.error(
                impl_decl.span.clone(),
                "Tuple is a built-in structural constraint and cannot be implemented",
            );
            return;
        }

        // 2b. Validate and resolve trait type args
        let trait_type_param_count = trait_sig.type_params.len();
        let impl_type_arg_count = impl_decl.trait_type_args.len();

        if trait_type_param_count > 0 && impl_type_arg_count == 0 {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                format!(
                    "trait '{}' has {} type parameter(s), but no type arguments were provided",
                    impl_decl.trait_name.value, trait_type_param_count
                ),
            );
            return;
        }
        if trait_type_param_count == 0 && impl_type_arg_count > 0 {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                format!(
                    "trait '{}' has no type parameters, but {} type argument(s) were provided",
                    impl_decl.trait_name.value, impl_type_arg_count
                ),
            );
            return;
        }
        if trait_type_param_count != impl_type_arg_count {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                format!(
                    "trait '{}' expects {} type argument(s), but {} were provided",
                    impl_decl.trait_name.value, trait_type_param_count, impl_type_arg_count
                ),
            );
            return;
        }

        // Resolve trait type args and build substitution map
        let resolved_trait_type_args: Vec<Type> = impl_decl
            .trait_type_args
            .iter()
            .map(|te| self.resolve_type_expr(te))
            .collect();

        let mut trait_subst: BTreeMap<TypeParamName, Type> = trait_sig
            .type_params
            .iter()
            .zip(resolved_trait_type_args.iter())
            .map(|(param, arg)| (param.clone(), arg.clone()))
            .collect();
        trait_subst.insert(TypeParamName("Self".to_string()), for_type.clone());

        // 2c. Process associated type definitions
        let mut associated_type_defs = BTreeMap::new();
        let mut seen_assoc_types = std::collections::BTreeSet::new();

        for assoc_def in &impl_decl.associated_types {
            if !seen_assoc_types.insert(assoc_def.name.value.clone()) {
                self.diagnostics.error(
                    assoc_def.name.span.clone(),
                    format!(
                        "duplicate associated type definition '{}'",
                        assoc_def.name.value
                    ),
                );
                continue;
            }

            let trait_assoc = trait_sig
                .associated_types
                .iter()
                .find(|a| a.name == assoc_def.name.value);
            let trait_assoc = match trait_assoc {
                Some(a) => a,
                None => {
                    self.diagnostics.error(
                        assoc_def.name.span.clone(),
                        format!(
                            "associated type '{}' is not declared in trait '{}'",
                            assoc_def.name.value, impl_decl.trait_name.value
                        ),
                    );
                    continue;
                }
            };

            // Validate GAT type param count
            let expected_tp_count = trait_assoc.type_params.len();
            let actual_tp_count = assoc_def.type_params.len();
            if actual_tp_count != expected_tp_count {
                self.diagnostics.error(
                    assoc_def.name.span.clone(),
                    format!(
                        "associated type '{}' expects {} type parameter(s), but {} were provided",
                        assoc_def.name.value, expected_tp_count, actual_tp_count
                    ),
                );
                continue;
            }

            // Validate GAT type param names match
            let gat_type_params: Vec<TypeParamName> = assoc_def
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            let mut names_ok = true;
            for (trait_tp, impl_tp) in trait_assoc.type_params.iter().zip(gat_type_params.iter()) {
                if *trait_tp != *impl_tp {
                    self.diagnostics.error(
                        assoc_def.name.span.clone(),
                        format!(
                            "type parameter name mismatch for associated type '{}': expected '{}', found '{}'",
                            assoc_def.name.value, trait_tp, impl_tp
                        ),
                    );
                    names_ok = false;
                }
            }
            if !names_ok {
                continue;
            }

            // Resolve GAT body with GAT params in scope
            let resolved_type = if gat_type_params.is_empty() {
                let resolved = self.resolve_type_expr(&assoc_def.type_expr);
                trait_subst.insert(
                    TypeParamName(assoc_def.name.value.clone()),
                    resolved.clone(),
                );
                resolved
            } else {
                let mut gat_resolve_map = BTreeMap::new();
                for tp in &gat_type_params {
                    gat_resolve_map.insert(tp.0.clone(), Type::TypeVariable(tp.clone(), vec![]));
                }
                self.resolve_type_expr_with_type_params(&assoc_def.type_expr, &gat_resolve_map)
            };

            associated_type_defs.insert(
                assoc_def.name.value.clone(),
                (gat_type_params, resolved_type),
            );
        }

        // Check completeness: every trait associated type must be defined
        for assoc in &trait_sig.associated_types {
            if !seen_assoc_types.contains(&assoc.name) {
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!(
                        "missing associated type definition '{}' from trait '{}'",
                        assoc.name, impl_decl.trait_name.value
                    ),
                );
            }
        }

        // Build GAT definitions map for expand_gats
        let gat_defs: BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)> = associated_type_defs
            .iter()
            .filter(|(_, (params, _))| !params.is_empty())
            .map(|(name, (params, ty))| (TypeParamName(name.clone()), (params.clone(), ty.clone())))
            .collect();

        // 3. Substitute Self → for_type and trait type params → concrete types in trait method signatures.
        //    Also expand GATs after normal substitution.
        //    Carry type_params and trait_bounds for generic method validation.
        #[allow(clippy::type_complexity)]
        let substituted_methods: Vec<(
            String,
            Vec<TypeParamName>,
            Vec<(String, Type)>,
            Type,
            TraitBounds,
        )> = trait_sig
            .methods
            .iter()
            .map(|m| {
                let params: Vec<(String, Type)> = m
                    .params
                    .iter()
                    .map(|(name, ty)| {
                        let ty = substitute_trait_type_params(ty, &trait_subst);
                        let ty = substitute_self(&ty, &for_type);
                        let ty = expand_gats(&ty, &gat_defs);
                        (name.clone(), ty)
                    })
                    .collect();
                let return_type = substitute_trait_type_params(&m.return_type, &trait_subst);
                let return_type = substitute_self(&return_type, &for_type);
                let return_type = expand_gats(&return_type, &gat_defs);
                (
                    m.name.clone(),
                    m.type_params.clone(),
                    params,
                    return_type,
                    m.trait_bounds.clone(),
                )
            })
            .collect();

        // 4. Validate each impl method against the trait
        let mut collected_methods: Vec<ImplMethodSignature> = Vec::new();
        let mut collected_properties: Vec<ImplMethodSignature> = Vec::new();
        let mut matched_trait_methods = std::collections::BTreeSet::new();

        for method in &impl_decl.methods {
            // Find matching trait method by name
            // With `extends`, flattening can create same-name members with
            // distinct parameter lists (§1.2). Prefer the entry whose param
            // count matches; fall back to the first same-name entry so the
            // existing signature-mismatch diagnostics still fire.
            let parameter_names: Vec<_> = impl_decl
                .type_params
                .iter()
                .chain(&method.type_params)
                .map(|name| TypeParamName(name.value.clone()))
                .collect();
            let match_scope = super::implementation_matching::MethodScope {
                parameters: &parameter_names,
                enclosing_bounds: &TraitBounds::empty(),
                trait_substitution: &trait_subst,
                associated_types: &gat_defs,
            };
            let trait_method = substituted_methods
                .iter()
                .zip(trait_sig.methods.iter())
                .filter(|((name, _, _, _, _), _)| *name == method.name.value)
                .find(|(_, contract)| {
                    self.implementation_matches_method(
                        method,
                        &contract.params,
                        &contract.type_params,
                        &contract.trait_bounds,
                        &match_scope,
                    )
                })
                .or_else(|| {
                    substituted_methods
                        .iter()
                        .zip(trait_sig.methods.iter())
                        .find(|((name, _, _, _, _), _)| *name == method.name.value)
                });

            let (
                (
                    trait_name,
                    trait_method_type_params,
                    trait_params,
                    trait_return_type,
                    trait_method_bounds,
                ),
                original_contract,
            ) = match trait_method {
                Some(m) => m,
                None => {
                    self.diagnostics.error(
                        method.name.span.clone(),
                        format!(
                            "method '{}' is not a member of trait '{}'",
                            method.name.value, impl_decl.trait_name.value
                        ),
                    );
                    continue;
                }
            };

            matched_trait_methods.insert(member_signature_key(trait_name, trait_params));

            // Collect impl method type params
            let impl_method_type_params: Vec<TypeParamName> = method
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            if !trait_method_type_params.is_empty() {
                // ── Generic trait method ──────────────────────────────────
                // Validate impl method has matching type param count
                if impl_method_type_params.len() != trait_method_type_params.len() {
                    self.diagnostics.error(
                        method.name.span.clone(),
                        format!(
                            "method '{}' has {} type parameter(s), but trait '{}' expects {}",
                            method.name.value,
                            impl_method_type_params.len(),
                            impl_decl.trait_name.value,
                            trait_method_type_params.len()
                        ),
                    );
                    continue;
                }

                // Resolve impl method trait bounds from where clause
                let contract_bounds = expand_trait_bound_gats(
                    &rename_method_bounds(
                        trait_method_bounds,
                        trait_method_type_params,
                        &impl_method_type_params,
                        &trait_subst,
                    ),
                    &gat_defs,
                );
                let contract_scope =
                    Type::type_param_map(&impl_method_type_params, &contract_bounds);
                let mut impl_method_trait_bounds = self.resolve_trait_bounds_in_scope(
                    &method.where_clause,
                    &impl_method_type_params,
                    &contract_scope,
                );
                impl_method_trait_bounds.merge(&contract_bounds);

                // Build method-level type param map for resolving impl method types
                let method_type_params_map =
                    Type::type_param_map(&impl_method_type_params, &impl_method_trait_bounds);

                // Resolve impl method param types with type params in scope
                let params: Vec<(String, Type)> = method
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.resolve_type_expr_with_type_params(
                            &p.type_annotation,
                            &method_type_params_map,
                        );
                        (p.name.value.clone(), ty)
                    })
                    .collect();

                let return_type = match &method.return_type {
                    Some(type_expr) => {
                        self.resolve_type_expr_with_type_params(type_expr, &method_type_params_map)
                    }
                    None => Type::Unit,
                };

                // Validate parameter count
                if params.len() != trait_params.len() {
                    self.diagnostics.error(
                        method.name.span.clone(),
                        format!(
                            "method '{}' has {} parameter(s), but trait '{}' expects {}",
                            method.name.value,
                            params.len(),
                            impl_decl.trait_name.value,
                            trait_params.len()
                        ),
                    );
                    continue;
                }

                // Substitute both scopes together to preserve names in enclosing arguments.
                let mut contract_substitution = trait_subst.clone();
                contract_substitution.extend(
                    trait_method_type_params.iter().cloned().zip(
                        impl_method_type_params
                            .iter()
                            .cloned()
                            .map(|name| Type::TypeVariable(name, vec![])),
                    ),
                );

                // Validate parameter types (rename trait type params for comparison)
                let mut params_ok = true;
                for (i, ((_, impl_ty), (_, trait_ty))) in params
                    .iter()
                    .zip(original_contract.params.iter())
                    .enumerate()
                {
                    let renamed_trait_ty = expand_gats(
                        &substitute_trait_type_params(trait_ty, &contract_substitution),
                        &gat_defs,
                    );
                    if !crate::typechecker::subtyping::identical(impl_ty, &renamed_trait_ty) {
                        self.diagnostics.error(
                            method.params[i].span.clone(),
                            format!(
                                "parameter type mismatch for '{}' in method '{}': expected '{}', found '{}'",
                                method.params[i].name.value,
                                method.name.value,
                                renamed_trait_ty,
                                impl_ty
                            ),
                        );
                        params_ok = false;
                    }
                }

                // Validate return type
                let renamed_trait_return = expand_gats(
                    &substitute_trait_type_params(
                        &original_contract.return_type,
                        &contract_substitution,
                    ),
                    &gat_defs,
                );
                if !crate::typechecker::subtyping::identical(&return_type, &renamed_trait_return) {
                    let span = method
                        .return_type
                        .as_ref()
                        .map(|rt| rt.span())
                        .unwrap_or_else(|| method.name.span.clone());
                    self.diagnostics.error(
                        span,
                        format!(
                            "return type mismatch for method '{}': expected '{}', found '{}'",
                            method.name.value, renamed_trait_return, return_type
                        ),
                    );
                    params_ok = false;
                }

                if !params_ok {
                    continue;
                }

                collected_methods.push(ImplMethodSignature {
                    dispatch_name: trait_sig.method_dispatch_name(original_contract),
                    name: SymbolName(method.name.value.clone()),
                    visibility: method.visibility,
                    method_type_params: impl_method_type_params.clone(),
                    params: params.clone(),
                    return_type: return_type.clone(),
                    span: method.name.span.clone(),
                    is_async: method.is_async,
                    is_intrinsic: matches!(method.body, Expr::Intrinsic(_)),
                    is_property: false,
                    trait_bounds: impl_method_trait_bounds.clone(),
                    is_default: false,
                });
                // Register GenericFunctionDef for on-demand instantiation at call sites
                let impl_method_fqn = Fqn {
                    package: self.package_path.clone(),
                    symbol: SymbolName(format!(
                        "{}${}.{}",
                        trait_fqn.symbol, type_fqn.symbol, method.name.value
                    )),
                };
                self.package_registry.register_generic_function(
                    impl_method_fqn,
                    GenericFunctionDef {
                        visibility: method.visibility,
                        type_params: impl_method_type_params,
                        params,
                        return_type,
                        body: method.body.clone(),
                        span: method.name.span.clone(),
                        container_name: None,
                        trait_bounds: impl_method_trait_bounds,
                        is_async: method.is_async,
                        is_intrinsic: matches!(method.body, Expr::Intrinsic(_)),
                    },
                );
            } else {
                // ── Non-generic trait method ──────────────────────────────
                // Error if impl method declares type params that the trait method doesn't have
                if !impl_method_type_params.is_empty() {
                    self.diagnostics.error(
                        method.name.span.clone(),
                        format!(
                            "method '{}' has {} type parameter(s), but trait '{}' expects 0",
                            method.name.value,
                            impl_method_type_params.len(),
                            impl_decl.trait_name.value,
                        ),
                    );
                    continue;
                }

                // Resolve impl method param types
                let params: Vec<(String, Type)> = method
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.resolve_type_expr(&p.type_annotation);
                        (p.name.value.clone(), ty)
                    })
                    .collect();

                let return_type = match &method.return_type {
                    Some(type_expr) => self.resolve_type_expr(type_expr),
                    None => Type::Unit,
                };

                // Validate parameter count
                if params.len() != trait_params.len() {
                    self.diagnostics.error(
                        method.name.span.clone(),
                        format!(
                            "method '{}' has {} parameter(s), but trait '{}' expects {}",
                            method.name.value,
                            params.len(),
                            impl_decl.trait_name.value,
                            trait_params.len()
                        ),
                    );
                    continue;
                }

                // Validate parameter types
                let mut params_ok = true;
                for (i, ((_, impl_ty), (_, trait_ty))) in
                    params.iter().zip(trait_params.iter()).enumerate()
                {
                    if *impl_ty != *trait_ty {
                        self.diagnostics.error(
                            method.params[i].span.clone(),
                            format!(
                                "parameter type mismatch for '{}' in method '{}': expected '{}', found '{}'",
                                method.params[i].name.value,
                                method.name.value,
                                trait_ty,
                                impl_ty
                            ),
                        );
                        params_ok = false;
                    }
                }

                // Validate return type
                if return_type != *trait_return_type {
                    let span = method
                        .return_type
                        .as_ref()
                        .map(|rt| rt.span())
                        .unwrap_or_else(|| method.name.span.clone());
                    self.diagnostics.error(
                        span,
                        format!(
                            "return type mismatch for method '{}': expected '{}', found '{}'",
                            method.name.value, trait_return_type, return_type
                        ),
                    );
                    params_ok = false;
                }

                if !params_ok {
                    continue;
                }

                collected_methods.push(ImplMethodSignature {
                    dispatch_name: trait_sig.method_dispatch_name(original_contract),
                    name: SymbolName(method.name.value.clone()),
                    visibility: method.visibility,
                    method_type_params: vec![],
                    params,
                    return_type,
                    span: method.name.span.clone(),
                    is_async: method.is_async,
                    is_intrinsic: matches!(method.body, Expr::Intrinsic(_)),
                    is_property: false,
                    trait_bounds: TraitBounds::default(),
                    is_default: false,
                });
            }
        }

        // 4b. Validate each impl property against the trait
        let substituted_properties: Vec<_> = trait_sig
            .properties
            .iter()
            .map(|p| {
                let params: Vec<(String, Type)> = p
                    .params
                    .iter()
                    .map(|(name, ty)| {
                        let ty = substitute_trait_type_params(ty, &trait_subst);
                        let ty = substitute_self(&ty, &for_type);
                        let ty = expand_gats(&ty, &gat_defs);
                        (name.clone(), ty)
                    })
                    .collect();
                let return_type = substitute_trait_type_params(&p.return_type, &trait_subst);
                let return_type = substitute_self(&return_type, &for_type);
                let return_type = expand_gats(&return_type, &gat_defs);
                (p.name.clone(), params, return_type)
            })
            .collect();

        let mut matched_trait_properties = std::collections::BTreeSet::new();

        for property in &impl_decl.properties {
            let trait_property = substituted_properties
                .iter()
                .find(|(name, _, _)| *name == property.name.value);

            let (trait_name, trait_params, trait_return_type) = match trait_property {
                Some(p) => p,
                None => {
                    self.diagnostics.error(
                        property.name.span.clone(),
                        format!(
                            "property '{}' is not a member of trait '{}'",
                            property.name.value, impl_decl.trait_name.value
                        ),
                    );
                    continue;
                }
            };

            matched_trait_properties.insert(trait_name.clone());

            // Resolve impl property param types
            let params: Vec<(String, Type)> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    (p.name.value.clone(), ty)
                })
                .collect();

            let return_type = self.resolve_type_expr(&property.return_type);

            // Validate parameter count
            if params.len() != trait_params.len() {
                self.diagnostics.error(
                    property.name.span.clone(),
                    format!(
                        "property '{}' has {} parameter(s), but trait '{}' expects {}",
                        property.name.value,
                        params.len(),
                        impl_decl.trait_name.value,
                        trait_params.len()
                    ),
                );
                continue;
            }

            // Validate parameter types
            let mut params_ok = true;
            for (i, ((_, impl_ty), (_, trait_ty))) in
                params.iter().zip(trait_params.iter()).enumerate()
            {
                if *impl_ty != *trait_ty {
                    self.diagnostics.error(
                        property.params[i].span.clone(),
                        format!(
                            "parameter type mismatch for '{}' in property '{}': expected '{}', found '{}'",
                            property.params[i].name.value,
                            property.name.value,
                            trait_ty,
                            impl_ty
                        ),
                    );
                    params_ok = false;
                }
            }

            // Validate return type
            if return_type != *trait_return_type {
                self.diagnostics.error(
                    property.return_type.span(),
                    format!(
                        "return type mismatch for property '{}': expected '{}', found '{}'",
                        property.name.value, trait_return_type, return_type
                    ),
                );
                params_ok = false;
            }

            if !params_ok {
                continue;
            }

            let body = match property.body.as_ref() {
                Some(b) => b,
                None => continue,
            };
            collected_properties.push(ImplMethodSignature {
                dispatch_name: SymbolName(property.name.value.clone()),
                name: SymbolName(property.name.value.clone()),
                visibility: property.visibility,
                method_type_params: vec![],
                params,
                return_type,
                span: property.name.span.clone(),
                is_async: false,
                is_intrinsic: matches!(body, Expr::Intrinsic(_)),
                is_property: true,
                trait_bounds: TraitBounds::default(),
                is_default: false,
            });
        }

        // 5. Check completeness: all trait methods must be implemented —
        // unless the trait provides a default body, in which case an
        // ImplMethodSignature is synthesized and the function is materialized
        // from the default template at monomorphize.
        for ((trait_method_name, method_parameters, m_params, m_return, _), trait_method_sig) in
            substituted_methods.iter().zip(trait_sig.methods.iter())
        {
            if !matched_trait_methods.contains(&member_signature_key(trait_method_name, m_params)) {
                if trait_method_sig.default_source.is_some() {
                    collected_methods.push(ImplMethodSignature {
                        dispatch_name: trait_sig.method_dispatch_name(trait_method_sig),
                        name: SymbolName(trait_method_name.clone()),
                        visibility: trait_sig.visibility,
                        method_type_params: method_parameters.clone(),
                        params: m_params.clone(),
                        return_type: m_return.clone(),
                        span: impl_decl.trait_name.span.clone(),
                        is_async: false,
                        is_intrinsic: false,
                        is_property: false,
                        trait_bounds: expand_trait_bound_gats(
                            &substitute_trait_bounds(&trait_method_sig.trait_bounds, &trait_subst),
                            &gat_defs,
                        ),
                        is_default: true,
                    });
                    continue;
                }
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!(
                        "missing implementation of method '{}' from trait '{}'",
                        trait_method_name, impl_decl.trait_name.value
                    ),
                );
            }
        }

        // 5b. Check completeness: all trait properties must be implemented
        // (same default escape hatch).
        for ((trait_prop_name, p_params, p_return), trait_prop_sig) in substituted_properties
            .iter()
            .zip(trait_sig.properties.iter())
        {
            if !matched_trait_properties.contains(trait_prop_name) {
                if trait_prop_sig.default_source.is_some() {
                    collected_properties.push(ImplMethodSignature {
                        dispatch_name: SymbolName(trait_prop_name.clone()),
                        name: SymbolName(trait_prop_name.clone()),
                        visibility: trait_sig.visibility,
                        method_type_params: vec![],
                        params: p_params.clone(),
                        return_type: p_return.clone(),
                        span: impl_decl.trait_name.span.clone(),
                        is_async: false,
                        is_intrinsic: false,
                        is_property: true,
                        trait_bounds: TraitBounds::default(),
                        is_default: true,
                    });
                    continue;
                }
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!(
                        "missing implementation of property '{}' from trait '{}'",
                        trait_prop_name, impl_decl.trait_name.value
                    ),
                );
            }
        }

        // 6. Check for duplicate trait impl before registering. Exact
        // duplicates only (same full for-type): sibling instantiations are
        // legal, and generic-vs-concrete overlap is reported by the
        // coherence rule with both locations instead.
        let merged_has_dup = self.dependency_registry.has_exact_trait_impl(
            &trait_fqn,
            &for_type,
            &resolved_trait_type_args,
        );
        let pkg_has_dup = self.package_registry.has_exact_trait_impl(
            &trait_fqn,
            &for_type,
            &resolved_trait_type_args,
        );

        if merged_has_dup || pkg_has_dup {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                format!(
                    "type '{}' already implements trait '{}'",
                    type_fqn.symbol.0, impl_decl.trait_name.value
                ),
            );
        } else {
            self.package_registry
                .register_implement_block(ImplBlockSignature {
                    trait_fqn: trait_fqn.clone(),
                    type_fqn: type_fqn.clone(),
                    for_type: for_type.clone(),
                    type_params: vec![],
                    trait_type_args: resolved_trait_type_args,
                    trait_bounds: TraitBounds::default(),
                    methods: collected_methods,
                    properties: collected_properties,
                    associated_type_defs,
                    span: impl_decl.span.clone(),
                    source_file: impl_decl.trait_name.span.file.clone(),
                    package: self.package_path.clone(),
                });
        }
    }

    /// Collect a generic implement block: `implement <T> Trait for Type<T> where T: Bound = ...`
    /// Block-level type params are in scope for for_type, trait type args, and method signatures.
    /// Bodies are stored as AST and instantiated at call sites (same pattern as generic extensions).
    fn collect_generic_implement(&mut self, impl_decl: &ImplementDecl) {
        // 1. Build block-level type params and bounds
        let type_params: Vec<TypeParamName> = impl_decl
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let impl_trait_bounds = self.resolve_trait_bounds(&impl_decl.where_clause, &type_params);
        let type_params_map = Type::type_param_map(&type_params, &impl_trait_bounds);

        // 2. Resolve for_type with type params in scope
        let for_type =
            self.resolve_type_expr_with_type_params(&impl_decl.for_type, &type_params_map);
        if for_type.contains_tuple_extension() && !for_type.is_recursive_tuple_head(&type_params) {
            self.diagnostics.error(
                impl_decl.for_type.span(),
                "symbolic tuple extension implementation heads are not supported yet".to_string(),
            );
            return;
        }
        if for_type.is_error() {
            return;
        }
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            self.diagnostics.error(
                impl_decl.for_type.span(),
                "cannot implement a trait for an intersection type; implement each interface's contract separately".to_string(),
            );
            return;
        }
        let Some(type_fqn) = for_type.try_to_fqn() else {
            self.diagnostics.error(
                impl_decl.for_type.span(),
                "implement blocks for a bare type parameter are not supported".to_string(),
            );
            return;
        };

        // 3. Resolve the trait
        let (trait_fqn, trait_sig) = match self.resolve_trait(&impl_decl.trait_name.value) {
            Some(result) => result,
            None => {
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!("unknown trait: '{}'", impl_decl.trait_name.value),
                );
                return;
            }
        };

        if crate::typechecker::types::is_tuple_constraint(&trait_fqn) {
            self.diagnostics.error(
                impl_decl.span.clone(),
                "Tuple is a built-in structural constraint and cannot be implemented",
            );
            return;
        }

        // 4. Validate and resolve trait type args with type params in scope
        let trait_type_param_count = trait_sig.type_params.len();
        let impl_type_arg_count = impl_decl.trait_type_args.len();

        if trait_type_param_count > 0 && impl_type_arg_count == 0 {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                format!(
                    "trait '{}' has {} type parameter(s), but no type arguments were provided",
                    impl_decl.trait_name.value, trait_type_param_count
                ),
            );
            return;
        }
        if trait_type_param_count == 0 && impl_type_arg_count > 0 {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                format!(
                    "trait '{}' has no type parameters, but {} type argument(s) were provided",
                    impl_decl.trait_name.value, impl_type_arg_count
                ),
            );
            return;
        }
        if trait_type_param_count != impl_type_arg_count {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                format!(
                    "trait '{}' expects {} type argument(s), but {} were provided",
                    impl_decl.trait_name.value, trait_type_param_count, impl_type_arg_count
                ),
            );
            return;
        }

        let resolved_trait_type_args: Vec<Type> = impl_decl
            .trait_type_args
            .iter()
            .map(|te| self.resolve_type_expr_with_type_params(te, &type_params_map))
            .collect();
        if resolved_trait_type_args
            .iter()
            .any(Type::contains_tuple_extension)
        {
            self.diagnostics.error(
                impl_decl.trait_name.span.clone(),
                "symbolic tuple extension implementation heads are not supported yet (including trait arguments)".to_string(),
            );
            return;
        }

        // 5. Build trait substitution map and substitute Self + trait type params
        let mut trait_subst: BTreeMap<TypeParamName, Type> = trait_sig
            .type_params
            .iter()
            .zip(resolved_trait_type_args.iter())
            .map(|(param, arg)| (param.clone(), arg.clone()))
            .collect();
        trait_subst.insert(TypeParamName("Self".to_string()), for_type.clone());

        // 5b. Process associated type definitions (with block-level type params in scope)
        let mut associated_type_defs = BTreeMap::new();
        let mut seen_assoc_types = std::collections::BTreeSet::new();

        for assoc_def in &impl_decl.associated_types {
            if !seen_assoc_types.insert(assoc_def.name.value.clone()) {
                self.diagnostics.error(
                    assoc_def.name.span.clone(),
                    format!(
                        "duplicate associated type definition '{}'",
                        assoc_def.name.value
                    ),
                );
                continue;
            }

            let trait_assoc = trait_sig
                .associated_types
                .iter()
                .find(|a| a.name == assoc_def.name.value);
            let trait_assoc = match trait_assoc {
                Some(a) => a,
                None => {
                    self.diagnostics.error(
                        assoc_def.name.span.clone(),
                        format!(
                            "associated type '{}' is not declared in trait '{}'",
                            assoc_def.name.value, impl_decl.trait_name.value
                        ),
                    );
                    continue;
                }
            };

            // Validate GAT type param count
            let expected_tp_count = trait_assoc.type_params.len();
            let actual_tp_count = assoc_def.type_params.len();
            if actual_tp_count != expected_tp_count {
                self.diagnostics.error(
                    assoc_def.name.span.clone(),
                    format!(
                        "associated type '{}' expects {} type parameter(s), but {} were provided",
                        assoc_def.name.value, expected_tp_count, actual_tp_count
                    ),
                );
                continue;
            }

            // Validate GAT type param names match
            let gat_type_params: Vec<TypeParamName> = assoc_def
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            let mut names_ok = true;
            for (trait_tp, impl_tp) in trait_assoc.type_params.iter().zip(gat_type_params.iter()) {
                if *trait_tp != *impl_tp {
                    self.diagnostics.error(
                        assoc_def.name.span.clone(),
                        format!(
                            "type parameter name mismatch for associated type '{}': expected '{}', found '{}'",
                            assoc_def.name.value, trait_tp, impl_tp
                        ),
                    );
                    names_ok = false;
                }
            }
            if !names_ok {
                continue;
            }

            // Resolve GAT body with GAT params + block-level params in scope
            let resolved_type = if gat_type_params.is_empty() {
                let resolved =
                    self.resolve_type_expr_with_type_params(&assoc_def.type_expr, &type_params_map);
                trait_subst.insert(
                    TypeParamName(assoc_def.name.value.clone()),
                    resolved.clone(),
                );
                resolved
            } else {
                let mut gat_resolve_map = type_params_map.clone();
                for tp in &gat_type_params {
                    gat_resolve_map.insert(tp.0.clone(), Type::TypeVariable(tp.clone(), vec![]));
                }
                self.resolve_type_expr_with_type_params(&assoc_def.type_expr, &gat_resolve_map)
            };

            associated_type_defs.insert(
                assoc_def.name.value.clone(),
                (gat_type_params, resolved_type),
            );
        }

        // Check completeness: every trait associated type must be defined
        for assoc in &trait_sig.associated_types {
            if !seen_assoc_types.contains(&assoc.name) {
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!(
                        "missing associated type definition '{}' from trait '{}'",
                        assoc.name, impl_decl.trait_name.value
                    ),
                );
            }
        }

        // Build GAT definitions map for expand_gats
        let gat_defs: BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)> = associated_type_defs
            .iter()
            .filter(|(_, (params, _))| !params.is_empty())
            .map(|(name, (params, ty))| (TypeParamName(name.clone()), (params.clone(), ty.clone())))
            .collect();

        #[allow(clippy::type_complexity)]
        let substituted_methods: Vec<(
            String,
            Vec<TypeParamName>,
            Vec<(String, Type)>,
            Type,
            TraitBounds,
        )> = trait_sig
            .methods
            .iter()
            .map(|m| {
                let params: Vec<(String, Type)> = m
                    .params
                    .iter()
                    .map(|(name, ty)| {
                        let ty = substitute_trait_type_params(ty, &trait_subst);
                        let ty = substitute_self(&ty, &for_type);
                        let ty = expand_gats(&ty, &gat_defs);
                        (name.clone(), ty)
                    })
                    .collect();
                let return_type = substitute_trait_type_params(&m.return_type, &trait_subst);
                let return_type = substitute_self(&return_type, &for_type);
                let return_type = expand_gats(&return_type, &gat_defs);
                (
                    m.name.clone(),
                    m.type_params.clone(),
                    params,
                    return_type,
                    m.trait_bounds.clone(),
                )
            })
            .collect();

        let substituted_properties: Vec<_> = trait_sig
            .properties
            .iter()
            .map(|p| {
                let params: Vec<(String, Type)> = p
                    .params
                    .iter()
                    .map(|(name, ty)| {
                        let ty = substitute_trait_type_params(ty, &trait_subst);
                        let ty = substitute_self(&ty, &for_type);
                        let ty = expand_gats(&ty, &gat_defs);
                        (name.clone(), ty)
                    })
                    .collect();
                let return_type = substitute_trait_type_params(&p.return_type, &trait_subst);
                let return_type = substitute_self(&return_type, &for_type);
                let return_type = expand_gats(&return_type, &gat_defs);
                (p.name.clone(), params, return_type)
            })
            .collect();

        // 6. Validate and register each method
        let mut collected_methods: Vec<ImplMethodSignature> = Vec::new();
        let mut collected_properties: Vec<ImplMethodSignature> = Vec::new();
        let mut matched_trait_methods = std::collections::BTreeSet::new();

        for method in &impl_decl.methods {
            // With `extends`, flattening can create same-name members with
            // distinct parameter lists (§1.2). Prefer the entry whose param
            // count matches; fall back to the first same-name entry so the
            // existing signature-mismatch diagnostics still fire.
            let parameter_names: Vec<_> = impl_decl
                .type_params
                .iter()
                .chain(&method.type_params)
                .map(|name| TypeParamName(name.value.clone()))
                .collect();
            let match_scope = super::implementation_matching::MethodScope {
                parameters: &parameter_names,
                enclosing_bounds: &impl_trait_bounds,
                trait_substitution: &trait_subst,
                associated_types: &gat_defs,
            };
            let trait_method = substituted_methods
                .iter()
                .zip(trait_sig.methods.iter())
                .filter(|((name, _, _, _, _), _)| *name == method.name.value)
                .find(|(_, contract)| {
                    self.implementation_matches_method(
                        method,
                        &contract.params,
                        &contract.type_params,
                        &contract.trait_bounds,
                        &match_scope,
                    )
                })
                .or_else(|| {
                    substituted_methods
                        .iter()
                        .zip(trait_sig.methods.iter())
                        .find(|((name, _, _, _, _), _)| *name == method.name.value)
                });

            let (
                (
                    trait_name,
                    trait_method_type_params,
                    trait_params,
                    _trait_return_type,
                    trait_method_bounds,
                ),
                original_contract,
            ) = match trait_method {
                Some(m) => m,
                None => {
                    self.diagnostics.error(
                        method.name.span.clone(),
                        format!(
                            "method '{}' is not a member of trait '{}'",
                            method.name.value, impl_decl.trait_name.value
                        ),
                    );
                    continue;
                }
            };

            matched_trait_methods.insert(member_signature_key(trait_name, trait_params));

            // Build method-level type params
            let method_type_params: Vec<TypeParamName> = method
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            // Validate method-level type param count
            if method_type_params.len() != trait_method_type_params.len() {
                self.diagnostics.error(
                    method.name.span.clone(),
                    format!(
                        "method '{}' has {} type parameter(s), but trait '{}' expects {}",
                        method.name.value,
                        method_type_params.len(),
                        impl_decl.trait_name.value,
                        trait_method_type_params.len()
                    ),
                );
                continue;
            }

            // Build combined type params (block-level + method-level)
            let all_method_type_params: Vec<TypeParamName> = type_params
                .iter()
                .chain(method_type_params.iter())
                .cloned()
                .collect();

            // Resolve method-level trait bounds over combined params, merge with impl block bounds
            let mut contract_bounds = impl_trait_bounds.clone();
            contract_bounds.merge(&expand_trait_bound_gats(
                &rename_method_bounds(
                    trait_method_bounds,
                    trait_method_type_params,
                    &method_type_params,
                    &trait_subst,
                ),
                &gat_defs,
            ));
            let contract_scope = Type::type_param_map(&all_method_type_params, &contract_bounds);
            let mut method_trait_bounds = self.resolve_trait_bounds_in_scope(
                &method.where_clause,
                &all_method_type_params,
                &contract_scope,
            );
            method_trait_bounds.merge(&contract_bounds);

            // Build combined type_params_map (block + method)
            let combined_type_params_map =
                Type::type_param_map(&all_method_type_params, &method_trait_bounds);

            // Resolve param types and return type with combined map
            let params: Vec<(String, Type)> = method
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr_with_type_params(
                        &p.type_annotation,
                        &combined_type_params_map,
                    );
                    (p.name.value.clone(), ty)
                })
                .collect();

            let return_type = match &method.return_type {
                Some(type_expr) => {
                    self.resolve_type_expr_with_type_params(type_expr, &combined_type_params_map)
                }
                None => Type::Unit,
            };

            // Validate parameter count
            if params.len() != trait_params.len() {
                self.diagnostics.error(
                    method.name.span.clone(),
                    format!(
                        "method '{}' has {} parameter(s), but trait '{}' expects {}",
                        method.name.value,
                        params.len(),
                        impl_decl.trait_name.value,
                        trait_params.len()
                    ),
                );
                continue;
            }

            // Substitute both scopes together to preserve names in enclosing arguments.
            let mut contract_substitution = trait_subst.clone();
            contract_substitution.extend(
                trait_method_type_params.iter().cloned().zip(
                    method_type_params
                        .iter()
                        .cloned()
                        .map(|name| Type::TypeVariable(name, vec![])),
                ),
            );

            // Validate parameter types
            let mut params_ok = true;
            for (i, ((_, impl_ty), (_, trait_ty))) in params
                .iter()
                .zip(original_contract.params.iter())
                .enumerate()
            {
                let renamed_trait_ty = expand_gats(
                    &substitute_trait_type_params(trait_ty, &contract_substitution),
                    &gat_defs,
                );
                if !crate::typechecker::subtyping::identical(impl_ty, &renamed_trait_ty) {
                    self.diagnostics.error(
                        method.params[i].span.clone(),
                        format!(
                            "parameter type mismatch for '{}' in method '{}': expected '{}', found '{}'",
                            method.params[i].name.value,
                            method.name.value,
                            renamed_trait_ty,
                            impl_ty
                        ),
                    );
                    params_ok = false;
                }
            }

            // Validate return type
            let renamed_trait_return = expand_gats(
                &substitute_trait_type_params(
                    &original_contract.return_type,
                    &contract_substitution,
                ),
                &gat_defs,
            );
            if !crate::typechecker::subtyping::identical(&return_type, &renamed_trait_return) {
                let span = method
                    .return_type
                    .as_ref()
                    .map(|rt| rt.span())
                    .unwrap_or_else(|| method.name.span.clone());
                self.diagnostics.error(
                    span,
                    format!(
                        "return type mismatch for method '{}': expected '{}', found '{}'",
                        method.name.value, renamed_trait_return, return_type
                    ),
                );
                params_ok = false;
            }

            if !params_ok {
                continue;
            }

            collected_methods.push(ImplMethodSignature {
                dispatch_name: trait_sig.method_dispatch_name(original_contract),
                name: SymbolName(method.name.value.clone()),
                visibility: method.visibility,
                method_type_params: method_type_params.clone(),
                params: params.clone(),
                return_type: return_type.clone(),
                span: method.name.span.clone(),
                is_async: method.is_async,
                is_intrinsic: matches!(method.body, Expr::Intrinsic(_)),
                is_property: false,
                trait_bounds: method_trait_bounds.clone(),
                is_default: false,
            });
            // Register GenericFunctionDef for on-demand instantiation at call sites
            let all_type_params: Vec<TypeParamName> = type_params
                .iter()
                .chain(method_type_params.iter())
                .cloned()
                .collect();
            let mut combined_bounds = impl_trait_bounds.clone();
            combined_bounds.merge(&method_trait_bounds);
            let impl_method_fqn = Fqn {
                package: self.package_path.clone(),
                symbol: SymbolName(format!(
                    "{}${}.{}",
                    trait_fqn.symbol, type_fqn.symbol, method.name.value
                )),
            };
            self.package_registry.register_generic_function(
                impl_method_fqn,
                GenericFunctionDef {
                    visibility: method.visibility,
                    type_params: all_type_params,
                    params,
                    return_type,
                    body: method.body.clone(),
                    span: method.name.span.clone(),
                    container_name: None,
                    trait_bounds: combined_bounds,
                    is_async: method.is_async,
                    is_intrinsic: matches!(method.body, Expr::Intrinsic(_)),
                },
            );
        }

        // 6b. Validate and register each property
        let mut matched_trait_properties = std::collections::BTreeSet::new();

        for property in &impl_decl.properties {
            let trait_property = substituted_properties
                .iter()
                .find(|(name, _, _)| *name == property.name.value);

            let (trait_name, trait_params, trait_return_type) = match trait_property {
                Some(p) => p,
                None => {
                    self.diagnostics.error(
                        property.name.span.clone(),
                        format!(
                            "property '{}' is not a member of trait '{}'",
                            property.name.value, impl_decl.trait_name.value
                        ),
                    );
                    continue;
                }
            };

            matched_trait_properties.insert(trait_name.clone());

            // Resolve property param types with block-level type params in scope
            let params: Vec<(String, Type)> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self
                        .resolve_type_expr_with_type_params(&p.type_annotation, &type_params_map);
                    (p.name.value.clone(), ty)
                })
                .collect();

            let return_type =
                self.resolve_type_expr_with_type_params(&property.return_type, &type_params_map);

            // Validate parameter count
            if params.len() != trait_params.len() {
                self.diagnostics.error(
                    property.name.span.clone(),
                    format!(
                        "property '{}' has {} parameter(s), but trait '{}' expects {}",
                        property.name.value,
                        params.len(),
                        impl_decl.trait_name.value,
                        trait_params.len()
                    ),
                );
                continue;
            }

            // Validate parameter types
            let mut params_ok = true;
            for (i, ((_, impl_ty), (_, trait_ty))) in
                params.iter().zip(trait_params.iter()).enumerate()
            {
                if *impl_ty != *trait_ty {
                    self.diagnostics.error(
                        property.params[i].span.clone(),
                        format!(
                            "parameter type mismatch for '{}' in property '{}': expected '{}', found '{}'",
                            property.params[i].name.value,
                            property.name.value,
                            trait_ty,
                            impl_ty
                        ),
                    );
                    params_ok = false;
                }
            }

            // Validate return type
            if return_type != *trait_return_type {
                self.diagnostics.error(
                    property.return_type.span(),
                    format!(
                        "return type mismatch for property '{}': expected '{}', found '{}'",
                        property.name.value, trait_return_type, return_type
                    ),
                );
                params_ok = false;
            }

            if !params_ok {
                continue;
            }

            let body = match property.body.clone() {
                Some(b) => b,
                None => continue,
            };
            collected_properties.push(ImplMethodSignature {
                dispatch_name: SymbolName(property.name.value.clone()),
                name: SymbolName(property.name.value.clone()),
                visibility: property.visibility,
                method_type_params: vec![],
                params: params.clone(),
                return_type: return_type.clone(),
                span: property.name.span.clone(),
                is_async: false,
                is_intrinsic: matches!(body, Expr::Intrinsic(_)),
                is_property: true,
                trait_bounds: impl_trait_bounds.clone(),
                is_default: false,
            });
            // Register GenericFunctionDef for on-demand instantiation at call sites
            let impl_method_fqn = Fqn {
                package: self.package_path.clone(),
                symbol: SymbolName(format!(
                    "{}${}.{}",
                    trait_fqn.symbol, type_fqn.symbol, property.name.value
                )),
            };
            self.package_registry.register_generic_function(
                impl_method_fqn,
                GenericFunctionDef {
                    visibility: property.visibility,
                    type_params: type_params.clone(),
                    params,
                    return_type,
                    body: body.clone(),
                    span: property.name.span.clone(),
                    container_name: None,
                    trait_bounds: impl_trait_bounds.clone(),
                    is_async: false,
                    is_intrinsic: matches!(body, Expr::Intrinsic(_)),
                },
            );
        }

        // 5. Check completeness: all trait methods must be implemented —
        // unless the trait provides a default body, in which case an
        // ImplMethodSignature is synthesized and the function is materialized
        // from the default template at monomorphize.
        for ((trait_method_name, method_parameters, m_params, m_return, _), trait_method_sig) in
            substituted_methods.iter().zip(trait_sig.methods.iter())
        {
            if !matched_trait_methods.contains(&member_signature_key(trait_method_name, m_params)) {
                if trait_method_sig.default_source.is_some() {
                    collected_methods.push(ImplMethodSignature {
                        dispatch_name: trait_sig.method_dispatch_name(trait_method_sig),
                        name: SymbolName(trait_method_name.clone()),
                        visibility: trait_sig.visibility,
                        method_type_params: method_parameters.clone(),
                        params: m_params.clone(),
                        return_type: m_return.clone(),
                        span: impl_decl.trait_name.span.clone(),
                        is_async: false,
                        is_intrinsic: false,
                        is_property: false,
                        trait_bounds: expand_trait_bound_gats(
                            &substitute_trait_bounds(&trait_method_sig.trait_bounds, &trait_subst),
                            &gat_defs,
                        ),
                        is_default: true,
                    });
                    continue;
                }
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!(
                        "missing implementation of method '{}' from trait '{}'",
                        trait_method_name, impl_decl.trait_name.value
                    ),
                );
            }
        }

        // 5b. Check completeness: all trait properties must be implemented
        // (same default escape hatch).
        for ((trait_prop_name, p_params, p_return), trait_prop_sig) in substituted_properties
            .iter()
            .zip(trait_sig.properties.iter())
        {
            if !matched_trait_properties.contains(trait_prop_name) {
                if trait_prop_sig.default_source.is_some() {
                    collected_properties.push(ImplMethodSignature {
                        dispatch_name: SymbolName(trait_prop_name.clone()),
                        name: SymbolName(trait_prop_name.clone()),
                        visibility: trait_sig.visibility,
                        method_type_params: vec![],
                        params: p_params.clone(),
                        return_type: p_return.clone(),
                        span: impl_decl.trait_name.span.clone(),
                        is_async: false,
                        is_intrinsic: false,
                        is_property: true,
                        trait_bounds: TraitBounds::default(),
                        is_default: true,
                    });
                    continue;
                }
                self.diagnostics.error(
                    impl_decl.trait_name.span.clone(),
                    format!(
                        "missing implementation of property '{}' from trait '{}'",
                        trait_prop_name, impl_decl.trait_name.value
                    ),
                );
            }
        }

        // 8. Register collected implement block
        self.package_registry
            .register_implement_block(ImplBlockSignature {
                trait_fqn,
                type_fqn,
                for_type,
                type_params,
                trait_type_args: resolved_trait_type_args,
                trait_bounds: impl_trait_bounds,
                methods: collected_methods,
                properties: collected_properties,
                associated_type_defs,
                span: impl_decl.span.clone(),
                source_file: impl_decl.trait_name.span.file.clone(),
                package: self.package_path.clone(),
            });
    }

    /// Resolve a trait name to its FQN and signature using import-aware resolution.
    pub(super) fn resolve_trait(
        &self,
        name: &str,
    ) -> Option<(crate::common::types::Fqn, TraitSignature)> {
        let fqn = self.resolve_name_to_fqn(name, |fqn| {
            self.package_registry
                .lookup_trait(fqn, &self.package_path)
                .is_some()
                || self
                    .dependency_registry
                    .lookup_trait(fqn, &self.package_path)
                    .is_some()
        })?;

        let sig = self
            .package_registry
            .lookup_trait(&fqn, &self.package_path)
            .or_else(|| {
                self.dependency_registry
                    .lookup_trait(&fqn, &self.package_path)
            })?;

        Some((fqn, sig.clone()))
    }

    /// Resolve a class name to its FQN using import-aware resolution.
    /// Uses the type registry (not ClassTypeSignature) so it works during pre-registration
    /// when full class signatures may not yet be available.
    pub(super) fn resolve_class(&self, name: &str) -> Option<crate::common::types::Fqn> {
        self.resolve_name_to_fqn(name, |fqn| {
            let is_class = |ty: Option<&Type>| matches!(ty, Some(Type::Class(..)));
            is_class(self.package_registry.lookup_type_by_fqn(fqn))
                || is_class(self.dependency_registry.lookup_type_by_fqn(fqn))
        })
    }
}

/// Replace `Type::SelfType` with the concrete for-type.
pub(super) fn substitute_self(ty: &Type, for_type: &Type) -> Type {
    match ty {
        Type::SelfType => for_type.clone(),
        Type::Array(elem) => Type::Array(Box::new(substitute_self(elem, for_type))),
        Type::GenericRecord { fqn, type_args, .. } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, a)| (*v, substitute_self(a, for_type)))
                .collect();
            let new_mn = crate::common::types::MangledName::for_type(fqn);
            Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: new_mn,
                type_args: new_type_args,
            }
        }
        Type::GenericEnum { fqn, type_args, .. } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, a)| (*v, substitute_self(a, for_type)))
                .collect();
            let new_mn = crate::common::types::MangledName::for_type(fqn);
            Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: new_mn,
                type_args: new_type_args,
            }
        }
        Type::TupleExtend(left, right) => Type::tuple_extend(
            substitute_self(left, for_type),
            substitute_self(right, for_type),
        ),
        Type::AssociatedProjection(projection) => projection
            .map(|ty| substitute_self(ty, for_type))
            .into_type(),
        Type::TupleProjection(receiver, kind) => {
            Type::tuple_projection(substitute_self(receiver, for_type), *kind)
        }
        Type::Tuple(types, _) => {
            let new_types: Vec<Type> = types.iter().map(|t| substitute_self(t, for_type)).collect();
            let mn = crate::common::types::MangledName::for_tuple(&new_types);
            Type::Tuple(new_types, mn)
        }
        Type::TypeConstructor { name, type_args } => {
            let new_args: Vec<Type> = type_args
                .iter()
                .map(|t| substitute_self(t, for_type))
                .collect();
            Type::TypeConstructor {
                name: name.clone(),
                type_args: new_args,
            }
        }
        Type::GenericNewtype {
            fqn,
            type_args,
            concrete_inner_type,
        } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, a)| (*v, substitute_self(a, for_type)))
                .collect();
            let new_inner = Box::new(substitute_self(concrete_inner_type, for_type));
            Type::GenericNewtype {
                fqn: fqn.clone(),
                type_args: new_type_args,
                concrete_inner_type: new_inner,
            }
        }
        Type::Function(params, ret) => {
            let new_params: Vec<Type> = params
                .iter()
                .map(|t| substitute_self(t, for_type))
                .collect();
            let new_ret = Box::new(substitute_self(ret, for_type));
            Type::Function(new_params, new_ret)
        }
        // Primitive types and others pass through unchanged
        other => other.clone(),
    }
}

/// Key identifying one member among same-name overloads created by `extends`
/// flattening: the member name plus its rendered param types.
pub(super) fn member_signature_key(name: &str, params: &[(String, Type)]) -> (String, Vec<String>) {
    (
        name.to_string(),
        params.iter().map(|(_, t)| t.to_string()).collect(),
    )
}

/// Replace trait-level type parameters with concrete types from the substitution map.
/// Used when implementing a generic trait: `implement From<Int32> for MyRec` replaces T → Int32.
pub(crate) fn substitute_trait_type_params(
    ty: &Type,
    subst: &BTreeMap<TypeParamName, Type>,
) -> Type {
    match ty {
        Type::SelfType => subst
            .get(&TypeParamName("Self".to_string()))
            .cloned()
            .unwrap_or(Type::SelfType),
        Type::TypeVariable(name, bounds) => {
            if let Some(concrete) = subst.get(name) {
                concrete.clone()
            } else {
                Type::TypeVariable(name.clone(), substitute_bound_types(bounds, subst))
            }
        }
        Type::GenericParam(name, bounds, id) => subst.get(name).cloned().unwrap_or_else(|| {
            Type::GenericParam(name.clone(), substitute_bound_types(bounds, subst), *id)
        }),
        Type::Array(elem) => Type::Array(Box::new(substitute_trait_type_params(elem, subst))),
        Type::GenericRecord { fqn, type_args, .. } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, substitute_trait_type_params(t, subst)))
                .collect();
            let new_mn = crate::common::types::MangledName::for_type(fqn);
            Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: new_mn,
                type_args: new_type_args,
            }
        }
        Type::GenericEnum { fqn, type_args, .. } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, substitute_trait_type_params(t, subst)))
                .collect();
            let new_mn = crate::common::types::MangledName::for_type(fqn);
            Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: new_mn,
                type_args: new_type_args,
            }
        }
        Type::TupleExtend(left, right) => Type::tuple_extend(
            substitute_trait_type_params(left, subst),
            substitute_trait_type_params(right, subst),
        ),
        Type::AssociatedProjection(projection) => projection
            .map(|ty| substitute_trait_type_params(ty, subst))
            .into_type(),
        Type::TupleProjection(receiver, kind) => {
            Type::tuple_projection(substitute_trait_type_params(receiver, subst), *kind)
        }
        Type::Tuple(types, _) => {
            let new_types: Vec<Type> = types
                .iter()
                .map(|t| substitute_trait_type_params(t, subst))
                .collect();
            let mn = crate::common::types::MangledName::for_tuple(&new_types);
            Type::Tuple(new_types, mn)
        }
        Type::TypeConstructor { name, type_args } => {
            let new_args: Vec<Type> = type_args
                .iter()
                .map(|t| substitute_trait_type_params(t, subst))
                .collect();
            Type::TypeConstructor {
                name: name.clone(),
                type_args: new_args,
            }
        }
        Type::Function(params, ret) => {
            let new_params: Vec<Type> = params
                .iter()
                .map(|t| substitute_trait_type_params(t, subst))
                .collect();
            let new_ret = Box::new(substitute_trait_type_params(ret, subst));
            Type::Function(new_params, new_ret)
        }
        Type::InterfaceObject { traits, .. } => Type::interface_intersection(
            traits
                .iter()
                .map(|c| {
                    (
                        c.trait_fqn.clone(),
                        c.trait_type_args
                            .iter()
                            .map(|t| substitute_trait_type_params(t, subst))
                            .collect(),
                    )
                })
                .collect(),
        ),
        Type::GenericClass {
            fqn,
            mangled_name: _,
            type_args,
        } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, substitute_trait_type_params(t, subst)))
                .collect();
            let new_mn = crate::common::types::MangledName::for_type(fqn);
            Type::GenericClass {
                fqn: fqn.clone(),
                mangled_name: new_mn,
                type_args: new_type_args,
            }
        }
        Type::Newtype(fqn, inner) => Type::Newtype(
            fqn.clone(),
            Box::new(substitute_trait_type_params(inner, subst)),
        ),
        Type::GenericNewtype {
            fqn,
            type_args,
            concrete_inner_type,
        } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, substitute_trait_type_params(t, subst)))
                .collect();
            let new_inner = Box::new(substitute_trait_type_params(concrete_inner_type, subst));
            Type::GenericNewtype {
                fqn: fqn.clone(),
                type_args: new_type_args,
                concrete_inner_type: new_inner,
            }
        }
        other => other.clone(),
    }
}

pub(crate) fn substitute_trait_bounds(
    bounds: &TraitBounds,
    subst: &BTreeMap<TypeParamName, Type>,
) -> TraitBounds {
    let mut result = TraitBounds::empty();
    for (param, bounds) in bounds.iter() {
        result.insert(param.clone(), substitute_bound_types(bounds, subst));
    }
    result
}

fn substitute_bound_types(
    bounds: &[TraitBound],
    subst: &BTreeMap<TypeParamName, Type>,
) -> Vec<TraitBound> {
    bounds
        .iter()
        .map(|bound| match bound {
            TraitBound::IsClass => TraitBound::IsClass,
            TraitBound::Named(bound) => TraitBound::Named(NamedTraitBound {
                trait_fqn: bound.trait_fqn.clone(),
                kind: bound.kind.clone(),
                type_args: bound
                    .type_args
                    .iter()
                    .map(|ty| substitute_trait_type_params(ty, subst))
                    .collect(),
                associated_types: bound
                    .associated_types
                    .iter()
                    .map(|(name, ty)| (name.clone(), substitute_trait_type_params(ty, subst)))
                    .collect(),
            }),
        })
        .collect()
}

/// Align method-bound keys and their referenced types by parameter position.
pub(crate) fn rename_method_bounds(
    bounds: &TraitBounds,
    original: &[TypeParamName],
    renamed: &[TypeParamName],
    enclosing_substitution: &BTreeMap<TypeParamName, Type>,
) -> TraitBounds {
    // Substitute both scopes simultaneously so newly inserted enclosing arguments
    // cannot be captured by a method parameter with the same source name.
    let mut substitution = enclosing_substitution.clone();
    substitution.extend(
        original.iter().cloned().zip(
            renamed
                .iter()
                .cloned()
                .map(|name| Type::TypeVariable(name, vec![])),
        ),
    );
    let substituted = substitute_trait_bounds(bounds, &substitution);
    let mut result = TraitBounds::empty();
    for (name, bounds) in substituted.iter() {
        let name = original
            .iter()
            .position(|parameter| parameter == name)
            .and_then(|index| renamed.get(index))
            .unwrap_or(name);
        result.insert(name.clone(), bounds.clone());
    }
    result
}

/// Expand associated constructors in the evidence required by a method contract.
pub(crate) fn expand_trait_bound_gats(
    bounds: &TraitBounds,
    definitions: &BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)>,
) -> TraitBounds {
    let mut result = TraitBounds::empty();
    for (parameter, requirements) in bounds.iter() {
        result.insert(
            parameter.clone(),
            expand_bound_gats(requirements, definitions),
        );
    }
    result
}

fn expand_bound_gats(
    requirements: &[TraitBound],
    definitions: &BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)>,
) -> Vec<TraitBound> {
    requirements
        .iter()
        .map(|requirement| match requirement {
            TraitBound::IsClass => TraitBound::IsClass,
            TraitBound::Named(bound) => TraitBound::Named(NamedTraitBound {
                trait_fqn: bound.trait_fqn.clone(),
                kind: bound.kind.clone(),
                type_args: bound
                    .type_args
                    .iter()
                    .map(|ty| expand_gats(ty, definitions))
                    .collect(),
                associated_types: bound
                    .associated_types
                    .iter()
                    .map(|(name, ty)| (name.clone(), expand_gats(ty, definitions)))
                    .collect(),
            }),
        })
        .collect()
}

/// Expand generic associated type references to their concrete types.
/// For each `TypeConstructor { name, type_args }`, looks up the GAT definition,
/// substitutes the GAT's type params with the provided type args, and returns the result.
pub(crate) fn expand_gats(
    ty: &Type,
    gat_defs: &BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)>,
) -> Type {
    if gat_defs.is_empty() {
        return ty.clone();
    }
    match ty {
        Type::TypeConstructor { name, type_args } => {
            if let Some((params, body)) = gat_defs.get(name) {
                let expanded_args: Vec<Type> =
                    type_args.iter().map(|a| expand_gats(a, gat_defs)).collect();
                let subst: BTreeMap<TypeParamName, Type> = params
                    .iter()
                    .zip(expanded_args.iter())
                    .map(|(p, a)| (p.clone(), a.clone()))
                    .collect();
                substitute_trait_type_params(body, &subst)
            } else {
                Type::TypeConstructor {
                    name: name.clone(),
                    type_args: type_args
                        .iter()
                        .map(|argument| expand_gats(argument, gat_defs))
                        .collect(),
                }
            }
        }
        Type::Array(elem) => Type::Array(Box::new(expand_gats(elem, gat_defs))),
        Type::Newtype(fqn, inner) => {
            Type::Newtype(fqn.clone(), Box::new(expand_gats(inner, gat_defs)))
        }
        Type::TypeVariable(name, bounds) => {
            Type::TypeVariable(name.clone(), expand_bound_gats(bounds, gat_defs))
        }
        Type::GenericParam(name, bounds, identity) => {
            Type::GenericParam(name.clone(), expand_bound_gats(bounds, gat_defs), *identity)
        }
        Type::GenericClass { fqn, type_args, .. } => Type::GenericClass {
            fqn: fqn.clone(),
            mangled_name: crate::common::types::MangledName::for_type(fqn),
            type_args: type_args
                .iter()
                .map(|(variance, argument)| (*variance, expand_gats(argument, gat_defs)))
                .collect(),
        },
        Type::InterfaceObject { traits, .. } => Type::interface_intersection(
            traits
                .iter()
                .map(|component| {
                    (
                        component.trait_fqn.clone(),
                        component
                            .trait_type_args
                            .iter()
                            .map(|argument| expand_gats(argument, gat_defs))
                            .collect(),
                    )
                })
                .collect(),
        ),
        Type::GenericRecord { fqn, type_args, .. } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, expand_gats(t, gat_defs)))
                .collect();
            let new_mn = crate::common::types::MangledName::for_type(fqn);
            Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: new_mn,
                type_args: new_type_args,
            }
        }
        Type::GenericEnum { fqn, type_args, .. } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, expand_gats(t, gat_defs)))
                .collect();
            let new_mn = crate::common::types::MangledName::for_type(fqn);
            Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: new_mn,
                type_args: new_type_args,
            }
        }
        Type::TupleExtend(left, right) => {
            Type::tuple_extend(expand_gats(left, gat_defs), expand_gats(right, gat_defs))
        }
        Type::AssociatedProjection(projection) => {
            projection.map(|ty| expand_gats(ty, gat_defs)).into_type()
        }
        Type::TupleProjection(receiver, kind) => {
            Type::tuple_projection(expand_gats(receiver, gat_defs), *kind)
        }
        Type::Tuple(types, _) => {
            let new_types: Vec<Type> = types.iter().map(|t| expand_gats(t, gat_defs)).collect();
            let mn = crate::common::types::MangledName::for_tuple(&new_types);
            Type::Tuple(new_types, mn)
        }
        Type::GenericNewtype {
            fqn,
            type_args,
            concrete_inner_type,
        } => {
            let new_type_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, expand_gats(t, gat_defs)))
                .collect();
            let new_inner = Box::new(expand_gats(concrete_inner_type, gat_defs));
            Type::GenericNewtype {
                fqn: fqn.clone(),
                type_args: new_type_args,
                concrete_inner_type: new_inner,
            }
        }
        Type::Function(params, ret) => {
            let new_params: Vec<Type> = params.iter().map(|t| expand_gats(t, gat_defs)).collect();
            let new_ret = Box::new(expand_gats(ret, gat_defs));
            Type::Function(new_params, new_ret)
        }
        other => other.clone(),
    }
}
