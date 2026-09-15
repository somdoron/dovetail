use crate::common::types::{Fqn, SymbolName, TypeParamName, VarName};
use crate::parser::ast::{Expr, ExtensionDecl, FunctionDecl};

use crate::typechecker::types::{TraitBounds, Type, TypedExtMethod, TypedExtensionBlock, TypedParam};

use super::Inference;

impl Inference<'_> {
    /// Infer types for all methods in an extension declaration.
    /// Generic extensions: type-check all method bodies only (scenario 2), do not insert.
    /// Non-generic extensions: generic methods type-check only (scenario 1); non-generic methods
    /// collected as `TypedExtMethod` and pushed into `self.extension_blocks`.
    pub(super) fn infer_extension(&mut self, ext: &ExtensionDecl) {
        if !ext.type_params.is_empty() {
            self.typecheck_generic_extension(ext);
            return;
        }

        let for_type = self.resolve_type_expr(&ext.for_type);
        if for_type.is_error() {
            return;
        }
        // Intersection for-types were rejected at collect; skip quietly here.
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            return;
        }

        let ext_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(ext.name.value.clone()),
        };

        let mut ext_methods: Vec<TypedExtMethod> = Vec::new();
        let mut ext_properties: Vec<TypedExtMethod> = Vec::new();

        for method in &ext.methods {
            if matches!(method.body, Expr::Intrinsic(_)) {
                continue;
            }

            if !method.type_params.is_empty() {
                if let Some(typed_method) = self.typecheck_generic_extension_method(method) {
                    ext_methods.push(typed_method);
                }
                continue;
            }

            // Non-generic method: full inference, collect as TypedExtMethod.
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

            self.validate_async_constraints(method, &return_type);

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
            let body = self.infer_expr(&method.body);
            self.async_return_type = prev_async_return;
            self.pop_scope();

            self.pop_scope();

            self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);

            let body = self.wrap_async_body(body, &return_type, method.is_async);

            let method_sym = SymbolName(method.name.value.clone());
            ext_methods.push(TypedExtMethod {
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

        // Infer property bodies (non-intrinsic)
        for property in &ext.properties {
            let property_body = property.body.as_ref().unwrap();
            if matches!(property_body, Expr::Intrinsic(_)) {
                continue;
            }

            let property_type = self.resolve_type_expr(&property.return_type);

            // Resolve parameter types from explicit params
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

            self.check_assignable(body.span.clone(), &property_type, &body.ty);

            let prop_sym = SymbolName(property.name.value.clone());
            ext_properties.push(TypedExtMethod {
                name: prop_sym,
                method_type_params: vec![],
                params: typed_params,
                return_type: property_type,
                body,
                span: property.span.clone(),
                is_async: false,
                visibility: property.visibility,
            });
        }

        self.extension_blocks.push(TypedExtensionBlock {
            ext_fqn,
            for_type,
            type_params: vec![],
            trait_bounds: TraitBounds::empty(),
            methods: ext_methods,
            properties: ext_properties,
            span: ext.name.span.clone(),
        });
    }

    /// Type-check all method and property bodies of a generic extension; retain typed bodies.
    fn typecheck_generic_extension(&mut self, ext: &ExtensionDecl) {
        let ext_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(ext.name.value.clone()),
        };
        let ext_type_params: Vec<TypeParamName> = ext
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let ext_trait_bounds =
            self.resolve_trait_bounds_from_where_clause(&ext.where_clause, &ext_type_params);
        let type_param_map = self.type_param_map(&ext_type_params, &ext_trait_bounds);
        let prev_type_params = std::mem::take(&mut self.current_type_params);
        for tp in &ext_type_params {
            if let Some(ty) = type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }
        let for_type = self.resolve_type_expr(&ext.for_type);
        if matches!(&for_type, Type::InterfaceObject { traits, .. } if traits.len() > 1) {
            return;
        }
        let mut ext_methods: Vec<TypedExtMethod> = Vec::new();
        for method in &ext.methods {
            if matches!(method.body, Expr::Intrinsic(_)) {
                continue;
            }
            let method_sym = SymbolName(method.name.value.clone());
            let method_type_params: Vec<TypeParamName> = method
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();
            let all_type_params: Vec<TypeParamName> = ext_type_params
                .iter()
                .chain(method_type_params.iter())
                .cloned()
                .collect();
            let method_bounds =
                self.resolve_trait_bounds_from_where_clause(&method.where_clause, &all_type_params);
            let mut combined_bounds = ext_trait_bounds.clone();
            combined_bounds.merge(&method_bounds);
            let method_type_param_map = self.type_param_map(&all_type_params, &combined_bounds);
            let prev_method_params = std::mem::take(&mut self.current_type_params);
            for tp in &all_type_params {
                if let Some(ty) = method_type_param_map.get(&tp.0) {
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
            self.validate_async_constraints(method, &return_type);
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

            let body = self.wrap_async_body(body, &return_type, method.is_async);
            ext_methods.push(TypedExtMethod {
                name: method_sym,
                method_type_params,
                params: typed_params,
                return_type,
                body,
                span: method.span.clone(),
                is_async: method.is_async,
                visibility: method.visibility,
            });

            self.current_type_params = prev_method_params;
        }
        let mut ext_properties: Vec<TypedExtMethod> = Vec::new();
        for property in &ext.properties {
            let property_body = property.body.as_ref().unwrap();
            if matches!(property_body, Expr::Intrinsic(_)) {
                continue;
            }
            let prop_sym = SymbolName(property.name.value.clone());
            let prev_method_params = std::mem::take(&mut self.current_type_params);
            for tp in &ext_type_params {
                if let Some(ty) = type_param_map.get(&tp.0) {
                    self.current_type_params.insert(tp.clone(), ty.clone());
                }
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
            self.push_scope();
            for param in &typed_params {
                self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
            }
            self.push_scope();
            let return_type = self.resolve_type_expr(&property.return_type);
            let prev_expected = self.expected_type.take();
            self.expected_type = Some(return_type.clone());
            let prev_fn_return = self.function_return_type.take();
            self.function_return_type = Some(return_type.clone());
            let body = self.infer_expr(property_body);
            self.pop_scope();
            self.pop_scope();
            self.check_assignable(body.span.clone(), &return_type, &body.ty);
            self.function_return_type = prev_fn_return;
            self.expected_type = prev_expected;

            ext_properties.push(TypedExtMethod {
                name: prop_sym,
                method_type_params: vec![],
                params: typed_params,
                return_type,
                body,
                span: property.span.clone(),
                is_async: false,
                visibility: property.visibility,
            });

            self.current_type_params = prev_method_params;
        }
        self.extension_blocks.push(TypedExtensionBlock {
            ext_fqn,
            for_type,
            type_params: ext_type_params.clone(),
            trait_bounds: ext_trait_bounds.clone(),
            methods: ext_methods,
            properties: ext_properties,
            span: ext.name.span.clone(),
        });
        self.current_type_params = prev_type_params;
    }

    /// Type-check a generic method on a non-generic extension and return the typed body.
    fn typecheck_generic_extension_method(&mut self, method: &FunctionDecl) -> Option<TypedExtMethod> {
        let type_params: Vec<TypeParamName> = method
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let trait_bounds =
            self.resolve_trait_bounds_from_where_clause(&method.where_clause, &type_params);
        let type_param_map = self.type_param_map(&type_params, &trait_bounds);
        let prev_type_params = std::mem::take(&mut self.current_type_params);
        for tp in &type_params {
            if let Some(ty) = type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }
        let return_type = match &method.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };
        self.validate_async_constraints(method, &return_type);
        let body_expected_type = if method.is_async {
            self.resolve_awaitable_value_type(&return_type).unwrap_or(return_type.clone())
        } else {
            return_type.clone()
        };
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

        let body = self.wrap_async_body(body, &return_type, method.is_async);
        Some(TypedExtMethod {
            name: SymbolName(method.name.value.clone()),
            method_type_params: type_params,
            params: typed_params,
            return_type,
            body,
            span: method.span.clone(),
            is_async: method.is_async,
            visibility: method.visibility,
        })
    }
}
