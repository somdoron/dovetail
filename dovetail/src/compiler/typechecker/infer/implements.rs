use std::collections::BTreeMap;

use crate::common::types::{Fqn, SymbolName, TypeParamName, VarName};
use crate::parser::ast::{Expr, FunctionDecl, ImplementDecl};

use crate::typechecker::types::{TraitBounds, Type, TypedImplementBlock, TypedImplMethod, TypedParam};

use super::Inference;

impl Inference<'_> {
    fn implementation_dispatch_name(&self, trait_fqn: &Fqn, for_type: &Type, name: &str, parameters: &[TypedParam]) -> SymbolName {
        let Some(receiver) = for_type.try_to_fqn() else { return SymbolName(name.to_string()) };
        self.registry.find_impl_method(&receiver, &SymbolName(name.to_string())).into_iter()
            .find(|(block, method)| block.trait_fqn == *trait_fqn && method.params.len() == parameters.len()
                && method.params.iter().zip(parameters).all(|((_, expected), actual)| expected.to_string() == actual.ty.to_string()))
            .map(|(_, method)| method.dispatch_name.clone()).unwrap_or_else(|| SymbolName(name.to_string()))
    }

    /// An implementation may rely on its block and the declared method contract,
    /// but it cannot demand additional evidence from callers of that contract.
    pub(super) fn implementation_method_bounds(
        &mut self,
        method: &FunctionDecl,
        for_type: &Type,
        trait_fqn: &Fqn,
        trait_type_args: &[Type],
        enclosing_params: &[TypeParamName],
        enclosing_bounds: &TraitBounds,
        associated_types: &BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)>,
    ) -> TraitBounds {
        use crate::typechecker::collect::{expand_trait_bound_gats, rename_method_bounds};
        let method_params: Vec<_> = method
            .type_params
            .iter()
            .map(|parameter| TypeParamName(parameter.value.clone()))
            .collect();
        let all_params: Vec<_> = enclosing_params
            .iter()
            .chain(&method_params)
            .cloned()
            .collect();
        let mut available = enclosing_bounds.clone();
        if let Some(signature) = self
            .registry
            .lookup_trait(trait_fqn, &self.package_path)
            .cloned()
        {
            let mut enclosing_substitution: BTreeMap<_, _> = signature
                .type_params
                .iter()
                .cloned()
                .zip(trait_type_args.iter().cloned())
                .collect();
            enclosing_substitution.insert(TypeParamName("Self".to_string()), for_type.clone());
            enclosing_substitution.extend(
                associated_types
                    .iter()
                    .filter(|(_, (parameters, _))| parameters.is_empty())
                    .map(|(name, (_, ty))| (name.clone(), ty.clone())),
            );
            for contract in &signature.methods {
                if contract.name != method.name.value
                    || contract.params.len() != method.params.len()
                    || contract.type_params.len() != method_params.len()
                {
                    continue;
                }
                let mut candidate_bounds = enclosing_bounds.clone();
                candidate_bounds.merge(&expand_trait_bound_gats(
                    &rename_method_bounds(
                        &contract.trait_bounds,
                        &contract.type_params,
                        &method_params,
                        &enclosing_substitution,
                    ),
                    associated_types,
                ));
                let Some(declared_parameters) = self.implementation_parameter_types(
                    method,
                    for_type,
                    &all_params,
                    &candidate_bounds,
                ) else {
                    continue;
                };
                let mut substitution = enclosing_substitution.clone();
                substitution.extend(
                    contract.type_params.iter().cloned().zip(
                        method_params
                            .iter()
                            .map(|name| Type::TypeVariable(name.clone(), vec![])),
                    ),
                );
                if contract.params.iter().zip(&declared_parameters).all(
                    |((_, expected), actual)| {
                        let expected = crate::typechecker::collect::substitute_trait_type_params(
                            expected,
                            &substitution,
                        );
                        let expected = crate::typechecker::collect::expand_gats(
                            &expected, associated_types,
                        );
                        crate::typechecker::subtyping::identical(&expected, actual)
                    },
                ) {
                    available = candidate_bounds;
                    break;
                }
            }
        }
        let previous_scope = self.current_type_params.clone();
        let evidence = Type::type_param_map(&all_params, &available);
        self.current_type_params.extend(
            evidence
                .iter()
                .map(|(name, ty)| (TypeParamName(name.clone()), ty.clone())),
        );
        let required =
            self.resolve_trait_bounds_from_where_clause(&method.where_clause, &all_params);
        self.current_type_params = previous_scope;
        let arguments: Vec<_> = all_params
            .iter()
            .map(|name| evidence[&name.0].clone())
            .collect();
        for failure in self.unsatisfied_trait_bounds(&required, &all_params, &arguments) {
            self.diagnostics.error(
                method.name.span.clone(),
                format!(
                    "method '{}' cannot strengthen its trait contract: {failure}",
                    method.name.value,
                ),
            );
        }
        available.merge(&required);
        available
    }

    fn implementation_parameter_types(
        &mut self,
        method: &FunctionDecl,
        receiver: &Type,
        parameters: &[TypeParamName],
        bounds: &TraitBounds,
    ) -> Option<Vec<Type>> {
        let previous_scope = self.current_type_params.clone();
        let scope = Type::type_param_map(parameters, bounds);
        self.current_type_params.extend(
            scope
                .into_iter()
                .map(|(name, ty)| (TypeParamName(name), ty)),
        );
        let before = self.diagnostics.len();
        let result = method
            .params
            .iter()
            .map(|parameter| {
                if parameter.name.value == "self" {
                    receiver.clone()
                } else {
                    self.resolve_type_expr(&parameter.type_annotation)
                }
            })
            .collect();
        let failed = self.diagnostics.len() != before;
        self.diagnostics.truncate(before);
        self.current_type_params = previous_scope;
        if failed { None } else { Some(result) }
    }

    fn implementation_associated_types(
        &mut self,
        declaration: &ImplementDecl,
    ) -> BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)> {
        let mut definitions = BTreeMap::new();
        for definition in &declaration.associated_types {
            let parameters: Vec<_> = definition.type_params.iter()
                .map(|parameter| TypeParamName(parameter.value.clone())).collect();
            let previous = self.current_type_params.clone();
            let scope = self.type_param_map(&parameters, &TraitBounds::empty());
            self.current_type_params.extend(scope.into_iter().map(|(name, ty)| (TypeParamName(name), ty)));
            let body = self.resolve_type_expr(&definition.type_expr);
            self.current_type_params = previous;
            definitions.insert(TypeParamName(definition.name.value.clone()), (parameters, body));
        }
        definitions
    }

    /// Infer types for all methods in an implement block.
    /// Follows the same pattern as `infer_extension`.
    pub(super) fn infer_implement(&mut self, impl_decl: &ImplementDecl) {
        if !impl_decl.type_params.is_empty() {
            self.typecheck_generic_impl_block(impl_decl);
            return;
        }

        let trait_fqn = match self.resolve_trait_fqn(&impl_decl.trait_name.value) {
            Some(fqn) => fqn,
            None => return,
        };

        let for_type = self.resolve_type_expr(&impl_decl.for_type);
        if for_type.is_error() {
            return;
        }
        // Intersection for-types were rejected at collect; skip quietly here.
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            return;
        }

        let resolved_trait_type_args: Vec<Type> = impl_decl
            .trait_type_args
            .iter()
            .map(|te| self.resolve_type_expr(te))
            .collect();

        let associated_types = self.implementation_associated_types(impl_decl);

        let mut impl_methods: Vec<TypedImplMethod> = Vec::new();
        let mut impl_properties: Vec<TypedImplMethod> = Vec::new();

        for method in &impl_decl.methods {
            let primitive_op = crate::typechecker::types::primitive_binary_operator(
                &for_type.to_fqn(),
                &method.name.value,
            );
            if matches!(method.body, Expr::Intrinsic(_)) && primitive_op.is_none() {
                continue;
            }

            if !method.type_params.is_empty() {
                if let Some(typed_method) = self.typecheck_generic_impl_method(method, &for_type, &trait_fqn, &resolved_trait_type_args, &associated_types) {
                    impl_methods.push(typed_method);
                }
                continue;
            }

            self.implementation_method_bounds(method, &for_type, &trait_fqn, &resolved_trait_type_args, &[], &TraitBounds::empty(), &associated_types);
            // Non-generic method: full inference and insert.
            let typed_params: Vec<TypedParam> = method
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    TypedParam {
                        name: p.name.value.clone(),
                        ty,
                        span: p.span.clone(),
                    }
                })
                .collect();

            self.push_scope();
            for param in &typed_params {
                self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
            }

            let return_type = match &method.return_type {
                Some(type_expr) => self.resolve_type_expr(type_expr),
                None => Type::Unit,
            };

            let body_expected_type = if method.is_async {
                self.resolve_awaitable_value_type(&return_type).unwrap_or(return_type.clone())
            } else {
                return_type.clone()
            };

            self.push_scope();
            let prev_async_return = self.async_return_type.take();
            if method.is_async {
                self.async_return_type = Some(return_type.clone());
            }
            // Make the method's declared return type visible to expressions
            // that need it — `try`/`orReturn` check it to decide whether the
            // enclosing function returns Result/Option. The generic paths
            // already do this; the non-generic path was missing the set/restore.
            let prev_fn_return = self.function_return_type.take();
            self.function_return_type = Some(return_type.clone());
            let body = if matches!(method.body, Expr::Intrinsic(_)) {
                use crate::typechecker::types::{TypedExpr, TypedExprKind};
                let args: Vec<_> = typed_params
                    .iter()
                    .map(|p| TypedExpr {
                        kind: TypedExprKind::VarRef {
                            name: VarName(p.name.clone()),
                            boxed: false,
                        },
                        ty: p.ty.clone(),
                        span: p.span.clone(),
                    })
                    .collect();
                TypedExpr {
                    kind: TypedExprKind::BinaryOp {
                        op: primitive_op.unwrap(),
                        left: Box::new(args[0].clone()),
                        right: Box::new(args[1].clone()),
                    },
                    ty: return_type.clone(),
                    span: method.body.span().clone(),
                }
            } else {
                self.infer_expr(&method.body)
            };
            self.function_return_type = prev_fn_return;
            self.async_return_type = prev_async_return;
            self.pop_scope();

            self.pop_scope();

            self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);

            let body = self.wrap_async_body(body, &return_type, method.is_async);

            let method_sym = self.implementation_dispatch_name(&trait_fqn, &for_type, &method.name.value, &typed_params);
            impl_methods.push(TypedImplMethod {
                name: method_sym,
                method_type_params: vec![],
                params: typed_params,
                return_type,
                body,
                span: method.span.clone(),
                is_async: method.is_async,
                visibility: method.visibility,
            });
        }

        // Infer property bodies
        for property in &impl_decl.properties {
            let property_body = property.body.as_ref().unwrap();
            if matches!(property_body, Expr::Intrinsic(_)) {
                continue;
            }

            let typed_params: Vec<TypedParam> = property
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    TypedParam {
                        name: p.name.value.clone(),
                        ty,
                        span: p.span.clone(),
                    }
                })
                .collect();

            // Param scope
            self.push_scope();
            for param in &typed_params {
                self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
            }

            // Body scope
            self.push_scope();
            let body = self.infer_expr(property_body);
            self.pop_scope();

            self.pop_scope();

            let return_type = self.resolve_type_expr(&property.return_type);

            self.check_assignable(body.span.clone(), &return_type, &body.ty);

            let prop_sym = SymbolName(property.name.value.clone());
            impl_properties.push(TypedImplMethod {
                name: prop_sym,
                method_type_params: vec![],
                params: typed_params,
                return_type,
                body,
                span: property.span.clone(),
                is_async: false,
                visibility: property.visibility,
            });
        }

        self.implement_blocks.push(TypedImplementBlock {
            trait_fqn,
            type_fqn: for_type.to_fqn(),
            for_type,
            type_params: vec![],
            trait_type_args: resolved_trait_type_args,
            trait_bounds: TraitBounds::empty(),
            methods: impl_methods,
            properties: impl_properties,
            span: impl_decl.span.clone(),
        });
    }

    /// Type-check all method bodies of a generic impl block and retain typed bodies.
    fn typecheck_generic_impl_block(&mut self, impl_decl: &ImplementDecl) {
        let impl_type_params: Vec<TypeParamName> = impl_decl
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let impl_trait_bounds =
            self.resolve_trait_bounds_from_where_clause(&impl_decl.where_clause, &impl_type_params);
        let block_type_param_map = self.type_param_map(&impl_type_params, &impl_trait_bounds);
        let prev_type_params = std::mem::take(&mut self.current_type_params);
        for tp in &impl_type_params {
            if let Some(ty) = block_type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }
        let for_type = self.resolve_type_expr(&impl_decl.for_type);
        if matches!(for_type, Type::TypeVariable(..) | Type::GenericParam(..)) {
            // Collection diagnosed unsupported blanket implementation heads.
            self.current_type_params = prev_type_params;
            return;
        }
        // Intersection for-types were rejected at collect; skip quietly here.
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            self.current_type_params = prev_type_params;
            return;
        }
        if !for_type.is_error() {
            let type_fqn = for_type.to_fqn();
            let trait_fqn = match self.resolve_trait_fqn(&impl_decl.trait_name.value) {
                Some(fqn) => fqn,
                None => {
                    self.current_type_params = prev_type_params;
                    return;
                }
            };

            let resolved_trait_type_args: Vec<Type> = impl_decl
                .trait_type_args
                .iter()
                .map(|te| self.resolve_type_expr(te))
                .collect();

            let associated_types = self.implementation_associated_types(impl_decl);
            let mut impl_methods: Vec<TypedImplMethod> = Vec::new();

            for method in &impl_decl.methods {
                if matches!(method.body, Expr::Intrinsic(_)) {
                    continue;
                }
                let method_type_params: Vec<TypeParamName> = method
                    .type_params
                    .iter()
                    .map(|tp| TypeParamName(tp.value.clone()))
                    .collect();
                let all_type_params: Vec<TypeParamName> = impl_type_params
                    .iter()
                    .chain(method_type_params.iter())
                    .cloned()
                    .collect();
                let method_trait_bounds = self.implementation_method_bounds(
                    method, &for_type, &trait_fqn, &resolved_trait_type_args, &impl_type_params, &impl_trait_bounds, &associated_types,
                );
                let mut combined_bounds = impl_trait_bounds.clone();
                combined_bounds.merge(&method_trait_bounds);
                let type_param_map = self.type_param_map(&all_type_params, &combined_bounds);
                let method_prev_type_params = std::mem::take(&mut self.current_type_params);
                for tp in &all_type_params {
                    if let Some(ty) = type_param_map.get(&tp.0) {
                        self.current_type_params.insert(tp.clone(), ty.clone());
                    }
                }
                let typed_params: Vec<TypedParam> = method
                    .params
                    .iter()
                    .map(|p| {
                        let ty = self.resolve_type_expr(&p.type_annotation);
                        TypedParam {
                            name: p.name.value.clone(),
                            ty,
                            span: p.span.clone(),
                        }
                    })
                    .collect();
                self.push_scope();
                for param in &typed_params {
                    self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
                }
                self.push_scope();
                let return_type = match &method.return_type {
                    Some(type_expr) => self.resolve_type_expr(type_expr),
                    None => Type::Unit,
                };
                let body_expected_type = if method.is_async {
                    self.resolve_awaitable_value_type(&return_type).unwrap_or(return_type.clone())
                } else {
                    return_type.clone()
                };
                let prev_expected = self.expected_type.take();
                self.expected_type = Some(body_expected_type.clone());
                let prev_fn_return = self.function_return_type.take();
                self.function_return_type = Some(return_type.clone());
                let prev_async_return = self.async_return_type.take();
                if method.is_async {
                    self.async_return_type = Some(return_type.clone());
                }
                let body = self.infer_expr(&method.body);
                // Restore — a stale expected type here leaks the BLOCK's type
                // params into later declarations' bodies (order-dependent
                // false "type mismatch" errors).
                self.expected_type = prev_expected;
                self.pop_scope();
                self.pop_scope();
                self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);
                self.async_return_type = prev_async_return;
                self.function_return_type = prev_fn_return;

                let method_sym = self.implementation_dispatch_name(&trait_fqn, &for_type, &method.name.value, &typed_params);
                impl_methods.push(TypedImplMethod {
                    name: method_sym,
                    method_type_params,
                    params: typed_params,
                    return_type,
                    body,
                    span: method.span.clone(),
                    is_async: method.is_async,
                    visibility: method.visibility,
                });

                self.current_type_params = method_prev_type_params;
            }

            // Infer property bodies for generic impl blocks
            let mut impl_properties: Vec<TypedImplMethod> = Vec::new();
            for property in &impl_decl.properties {
                let property_body = property.body.as_ref().unwrap();
                if matches!(property_body, Expr::Intrinsic(_)) {
                    continue;
                }

                let prop_sym = SymbolName(property.name.value.clone());
                {
                    let typed_params: Vec<TypedParam> = property
                        .params
                        .iter()
                        .map(|p| {
                            let ty = self.resolve_type_expr(&p.type_annotation);
                            TypedParam {
                                name: p.name.value.clone(),
                                ty,
                                span: p.span.clone(),
                            }
                        })
                        .collect();

                    self.push_scope();
                    for param in &typed_params {
                        self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
                    }
                    self.push_scope();
                    let return_type = self.resolve_type_expr(&property.return_type);
                    let prev_expected = self.expected_type.take();
                    self.expected_type = Some(return_type.clone());
                    let body = self.infer_expr(property_body);
                    self.expected_type = prev_expected;
                    self.pop_scope();
                    self.pop_scope();
                    self.check_assignable(body.span.clone(), &return_type, &body.ty);

                    impl_properties.push(TypedImplMethod {
                        name: prop_sym,
                        method_type_params: vec![],
                        params: typed_params,
                        return_type,
                        body,
                        span: property.span.clone(),
                        is_async: false,
                        visibility: property.visibility,
                    });
                }
            }

            self.implement_blocks.push(TypedImplementBlock {
                trait_fqn,
                type_fqn,
                for_type,
                type_params: impl_type_params.clone(),
                trait_type_args: resolved_trait_type_args,
                trait_bounds: impl_trait_bounds,
                methods: impl_methods,
                properties: impl_properties,
                span: impl_decl.span.clone(),
            });
        }
        self.current_type_params = prev_type_params;
    }

    /// Type-check a generic method on a non-generic impl block.
    /// Returns the typed method and inserts a template TypedFunction for monomorphize.
    fn typecheck_generic_impl_method(
        &mut self,
        method: &FunctionDecl,
        for_type: &Type,
        trait_fqn: &Fqn,
        trait_type_args: &[Type],
        associated_types: &BTreeMap<TypeParamName, (Vec<TypeParamName>, Type)>,
    ) -> Option<TypedImplMethod> {
        if for_type.is_error() {
            return None;
        }
        let method_type_params: Vec<TypeParamName> = method
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let method_trait_bounds = self.implementation_method_bounds(
            method, for_type, trait_fqn, trait_type_args, &[], &TraitBounds::empty(), associated_types,
        );
        let type_param_map = self.type_param_map(&method_type_params, &method_trait_bounds);
        let prev_type_params = std::mem::take(&mut self.current_type_params);
        for tp in &method_type_params {
            if let Some(ty) = type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }
        {
            let typed_params: Vec<TypedParam> = method
                .params
                .iter()
                .map(|p| {
                    let ty = self.resolve_type_expr(&p.type_annotation);
                    TypedParam {
                        name: p.name.value.clone(),
                        ty,
                        span: p.span.clone(),
                    }
                })
                .collect();
            self.push_scope();
            for param in &typed_params {
                self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
            }
            self.push_scope();
            let return_type = match &method.return_type {
                Some(type_expr) => self.resolve_type_expr(type_expr),
                None => Type::Unit,
            };
            let body_expected_type = if method.is_async {
                self.resolve_awaitable_value_type(&return_type).unwrap_or(return_type.clone())
            } else {
                return_type.clone()
            };
            let prev_expected = self.expected_type.take();
            self.expected_type = Some(body_expected_type.clone());
            let prev_fn_return = self.function_return_type.take();
            self.function_return_type = Some(return_type.clone());
            let prev_async_return = self.async_return_type.take();
            if method.is_async {
                self.async_return_type = Some(return_type.clone());
            }
            let body = self.infer_expr(&method.body);
            self.pop_scope();
            self.pop_scope();
            self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);
            self.async_return_type = prev_async_return;
            self.function_return_type = prev_fn_return;
            self.expected_type = prev_expected;
            self.current_type_params = prev_type_params;

            // Store template TypedFunction for monomorphize to instantiate.
            // Only method-level type_params (the impl block is non-generic).
            let type_fqn = for_type.to_fqn();
            let method_sym = self.implementation_dispatch_name(&trait_fqn, &for_type, &method.name.value, &typed_params);
            let template_mn = crate::typechecker::types::impl_member_mangled_name(
                trait_fqn,
                for_type,
                &[],
                &method_sym,
                trait_type_args,
            );
            let display_name = super::make_display_name(&format!("{}${}.{}", trait_fqn, type_fqn, method_sym), &typed_params);
            let typed_func = crate::typechecker::types::TypedFunction {
                visibility: method.visibility,
                name: template_mn.clone(),
                source_name: method_sym.0.clone(),
                type_params: method_type_params.clone(),
                params: typed_params.clone(),
                return_type: return_type.clone(),
                body,
                span: method.span.clone(),
                vtable_self_type: None,
                is_async: method.is_async,
                display_name,
            };
            self.typed_functions.insert(template_mn, typed_func);

            // The real body lives in the template TypedFunction above.
            // Use a unit literal placeholder for the TypedImplMethod.
            let placeholder_body = crate::typechecker::types::TypedExpr {
                kind: crate::typechecker::types::TypedExprKind::UnitLiteral,
                ty: Type::Unit,
                span: method.span.clone(),
            };
            Some(TypedImplMethod {
                name: method_sym,
                method_type_params,
                params: typed_params,
                return_type,
                body: placeholder_body,
                span: method.span.clone(),
                is_async: method.is_async,
                visibility: method.visibility,
            })
        }
    }
}
