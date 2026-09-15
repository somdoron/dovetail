use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, VarName};
use crate::parser::ast::{Expr, ModuleDecl};

use crate::typechecker::types::{Type, TypedFunction, TypedParam};

use super::Inference;
use super::types::SymbolKind;

impl Inference<'_> {
    /// Infer types for all members in a module declaration.
    pub(super) fn infer_module(&mut self, module: &ModuleDecl) {
        let module_name = module.name.value.clone();

        // Save and set container_name so that infer_function/infer_global build qualified FQNs
        let prev_container = self.container_name.take();
        self.container_name = Some(module_name.clone());
        let prev_module = self.current_module_name.replace(module_name.clone());

        if module.type_params.is_empty() {
            // Non-generic: eager inference of functions and properties
            // Skip intrinsic bodies — they are resolved at call sites.
            for func in &module.functions {
                self.infer_function(func);
            }
            for property in &module.properties {
                if property.type_params.is_empty() {
                    if let Some(body) = &property.body
                        && matches!(body, Expr::Intrinsic(_))
                    {
                        continue;
                    }
                    self.infer_module_property(property, &module_name);
                } else {
                    // Generic property on non-generic module: infer as template
                    // by converting to a FunctionDecl and calling infer_function.
                    let func_decl = self.property_to_function_decl(property);
                    self.infer_function(&func_decl);
                }
            }
        } else {
            // Generic module: infer function/property bodies as templates with TypeParameter types.
            let module_type_params: Vec<TypeParamName> = module
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();

            // Look up module trait bounds from registry
            let module_fqn = Fqn {
                package: self.package_path.clone(),
                symbol: SymbolName(module_name.clone()),
            };
            let module_trait_bounds = self
                .registry
                .lookup_module(&module_fqn)
                .map(|info| info.trait_bounds.clone())
                .unwrap_or_else(crate::typechecker::types::TraitBounds::empty);

            // Set up generic context with trait bounds
            let prev_type_params = self.current_type_params.clone();
            let module_type_param_map =
                self.type_param_map(&module_type_params, &module_trait_bounds);
            for tp in &module_type_params {
                if let Some(ty) = module_type_param_map.get(&tp.0) {
                    self.current_type_params.insert(tp.clone(), ty.clone());
                }
            }

            for func in &module.functions {
                if matches!(func.body, Expr::Intrinsic(_)) {
                    continue;
                }
                let method_context = self.current_type_params.clone();
                self.infer_module_function_template(func, &module_name, &module_type_params);
                self.current_type_params = method_context;
            }
            for property in &module.properties {
                if let Some(body) = &property.body
                    && matches!(body, Expr::Intrinsic(_))
                {
                    continue;
                }
                self.infer_module_property_template(property, &module_name, &module_type_params);
            }

            // Restore generic context
            self.current_type_params = prev_type_params;
        }

        // Infer global initializers
        if module.type_params.is_empty() {
            for global in &module.globals {
                self.infer_global(global);
            }
        } else {
            // Generic module: infer globals as templates with TypeParameter types
            let module_type_params: Vec<TypeParamName> = module
                .type_params
                .iter()
                .map(|tp| TypeParamName(tp.value.clone()))
                .collect();
            let prev_type_params = self.current_type_params.clone();
            let module_fqn = Fqn {
                package: self.package_path.clone(),
                symbol: SymbolName(module_name.clone()),
            };
            let module_trait_bounds = self
                .registry
                .lookup_module(&module_fqn)
                .map(|info| info.trait_bounds.clone())
                .unwrap_or_else(crate::typechecker::types::TraitBounds::empty);
            let module_type_param_map =
                self.type_param_map(&module_type_params, &module_trait_bounds);
            for tp in &module_type_params {
                if let Some(ty) = module_type_param_map.get(&tp.0) {
                    self.current_type_params.insert(tp.clone(), ty.clone());
                }
            }
            for global in &module.globals {
                self.infer_global_template(global, &module_type_params);
            }
            self.current_type_params = prev_type_params;
        }

        // Infer test bodies (always — tests don't use module type params)
        for test in &module.tests {
            self.infer_test(test);
        }

        // Restore previous container context
        self.container_name = prev_container;
        self.current_module_name = prev_module;
    }

    /// Infer a generic module function body and store as a template TypedFunction.
    /// The template has non-empty `type_params` (module-level + function-level)
    /// and TypeParameter types in params/return_type/body.
    fn infer_module_function_template(
        &mut self,
        func: &crate::parser::ast::FunctionDecl,
        module_name: &str,
        module_type_params: &[TypeParamName],
    ) {
        // Combine module + function type params
        let func_type_params: Vec<TypeParamName> = func
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let all_type_params: Vec<TypeParamName> = module_type_params
            .iter()
            .chain(func_type_params.iter())
            .cloned()
            .collect();

        // Resolve function-level where clause and add function's own type params
        // with their trait bounds to the context (module's already set by caller)
        let func_trait_bounds =
            self.resolve_trait_bounds_from_where_clause(&func.where_clause, &all_type_params);
        let func_type_param_map = self.type_param_map(&func_type_params, &func_trait_bounds);
        for tp in &func_type_params {
            if let Some(ty) = func_type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }
        self.add_current_type_param_bounds(&func_trait_bounds);

        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };

        let typed_params: Vec<TypedParam> = func
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

        // For async functions, body returns T (inner value), not the full Awaitable<T>
        let body_expected_type = if func.is_async {
            self.resolve_awaitable_value_type(&return_type)
                .unwrap_or(return_type.clone())
        } else {
            return_type.clone()
        };

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
        if func.is_async {
            self.async_return_type = Some(return_type.clone());
        }
        let body = self.infer_expr(&func.body);
        self.async_return_type = prev_async_return;
        self.function_return_type = prev_fn_return;
        self.expected_type = prev_expected;
        self.pop_scope();
        self.pop_scope();

        self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);

        // For async templates, wrap the body in AsyncBlock with a ResolvedImplMethod
        // containing TypeParameter types. After substitution, the types become concrete
        // and the desugar step can resolve the impl method.
        let body = self.wrap_async_body(body, &return_type, func.is_async);

        // Restore context: remove function's own type params and revert any
        // module type param bounds that were modified by the function's where clause.
        for tp in &func_type_params {
            self.current_type_params.remove(tp);
        }
        // Restore module type params to their pre-function-where-clause state
        let module_type_param_map = self.type_param_map(
            module_type_params,
            &crate::typechecker::types::TraitBounds::empty(),
        );
        // Re-lookup the module trait bounds for restoring
        let module_fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(module_name.to_string()),
        };
        if let Some(info) = self.registry.lookup_module(&module_fqn) {
            let restore_map = self.type_param_map(module_type_params, &info.trait_bounds);
            for tp in module_type_params {
                if let Some(ty) = restore_map.get(&tp.0) {
                    self.current_type_params.insert(tp.clone(), ty.clone());
                }
            }
        } else {
            for tp in module_type_params {
                if let Some(ty) = module_type_param_map.get(&tp.0) {
                    self.current_type_params.insert(tp.clone(), ty.clone());
                }
            }
        }

        // Build module-qualified FQN
        let qualified_symbol = SymbolName(format!("{}.{}", module_name, func.name.value));
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: qualified_symbol,
        };
        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let template_name = MangledName::for_function(&fqn, &param_types);

        let display_name = super::make_display_name(&fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            template_name.clone(),
            TypedFunction {
                visibility: func.visibility,
                name: template_name,
                type_params: all_type_params,
                params: typed_params,
                return_type,
                body,
                span: func.span.clone(),
                vtable_self_type: None,
                is_async: func.is_async,
                display_name,
                source_name: fqn.symbol.0.clone(),
            },
        );
    }

    /// Infer a module property body and add it as a typed function.
    fn infer_module_property(
        &mut self,
        property: &crate::parser::ast::PropertyDecl,
        module_name: &str,
    ) {
        let return_type = self.resolve_type_expr(&property.return_type);

        let body_expr = match &property.body {
            Some(expr) => expr,
            None => return,
        };

        // Resolve params (may include self for instance properties)
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
        let body = self.infer_expr(body_expr);
        self.pop_scope();

        self.check_assignable(body.span.clone(), &return_type, &body.ty);

        // Build module-qualified FQN
        let qualified_symbol = SymbolName(format!("{}.{}", module_name, property.name.value));
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: qualified_symbol,
        };

        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let name = MangledName::for_function(&fqn, &param_types);

        let display_name = super::make_display_name(&fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            name.clone(),
            TypedFunction {
                visibility: property.visibility,
                name,
                type_params: vec![],
                params: typed_params,
                return_type,
                body,
                span: property.span.clone(),
                vtable_self_type: None,
                is_async: false,
                display_name,
                source_name: fqn.symbol.0.clone(),
            },
        );
    }

    /// Infer a generic module property body as a template TypedFunction.
    fn infer_module_property_template(
        &mut self,
        property: &crate::parser::ast::PropertyDecl,
        module_name: &str,
        module_type_params: &[TypeParamName],
    ) {
        let body_expr = match &property.body {
            Some(expr) => expr,
            None => return,
        };

        // Combine module + property type params
        let prop_type_params: Vec<TypeParamName> = property
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();
        let all_type_params: Vec<TypeParamName> = module_type_params
            .iter()
            .chain(prop_type_params.iter())
            .cloned()
            .collect();

        // Add property's own type params to the context BEFORE resolving return type
        // so that property-level params like U in `property make<U>(): Wrapper<U>` are recognized
        for tp in &prop_type_params {
            self.current_type_params
                .insert(tp.clone(), Type::TypeVariable(tp.clone(), vec![]));
        }

        let return_type = self.resolve_type_expr(&property.return_type);

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
        let prev_expected = self.expected_type.take();
        self.expected_type = Some(return_type.clone());
        let prev_fn_return = self.function_return_type.take();
        self.function_return_type = Some(return_type.clone());
        let body = self.infer_expr(body_expr);
        self.function_return_type = prev_fn_return;
        self.expected_type = prev_expected;
        self.pop_scope();

        self.check_assignable(body.span.clone(), &return_type, &body.ty);

        // Remove property's own type params from context
        for tp in &prop_type_params {
            self.current_type_params.remove(tp);
        }

        // Build module-qualified FQN
        let qualified_symbol = SymbolName(format!("{}.{}", module_name, property.name.value));
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: qualified_symbol,
        };

        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let template_name = MangledName::for_function(&fqn, &param_types);

        let display_name = super::make_display_name(&fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            template_name.clone(),
            TypedFunction {
                visibility: property.visibility,
                name: template_name,
                type_params: all_type_params,
                params: typed_params,
                return_type,
                body,
                span: property.span.clone(),
                vtable_self_type: None,
                is_async: false,
                display_name,
                source_name: fqn.symbol.0.clone(),
            },
        );
    }

    /// Try to resolve an identifier as a module name.
    /// Uses resolve_fqn (import scope → same-package) then looks up the module.
    pub(super) fn resolve_module_name(
        &self,
        name: &str,
    ) -> Option<&crate::typechecker::registry::ModuleInfo> {
        let fqn = self.resolve_fqn(name, SymbolKind::Module)?;
        self.registry.lookup_module(&fqn)
    }
}
