use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName, VarName};
use crate::parser::ast::{Expr, FunctionDecl};

use crate::typechecker::types::{
    IntrinsicKind, TraitBounds, Type, TypedExpr, TypedExprKind, TypedFunction, TypedParam,
};

use super::Inference;

impl Inference<'_> {
    fn class_identity_intrinsic(&self, func: &FunctionDecl) -> Option<IntrinsicKind> {
        if !matches!(func.body, Expr::Intrinsic(_)) {
            return None;
        }
        let module = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(self.container_name.as_ref()?.clone()),
        };
        Self::class_identity_intrinsic_kind(&module, &func.name.value)
    }

    /// These intrinsic declarations have callable templates, unlike primitives
    /// that are only lowered at direct call sites.
    pub(super) fn class_identity_intrinsic_kind(module: &Fqn, name: &str) -> Option<IntrinsicKind> {
        if module.package.to_string() != "standard.prelude" || module.symbol.0 != "ClassIdentity" {
            return None;
        }
        match name {
            "equals" => Some(IntrinsicKind::ClassIdentityEquals),
            "hash" => Some(IntrinsicKind::ClassIdentityHash),
            _ => None,
        }
    }

    /// Keep identity primitives as checked generic templates, so direct calls
    /// and function values share normal bound validation and specialization.
    fn infer_function_body(
        &mut self,
        func: &FunctionDecl,
        params: &[TypedParam],
        return_type: &Type,
    ) -> TypedExpr {
        let Some(intrinsic) = self.class_identity_intrinsic(func) else {
            return self.infer_expr(&func.body);
        };
        let args = params
            .iter()
            .map(|param| TypedExpr {
                kind: TypedExprKind::VarRef {
                    name: VarName(param.name.clone()),
                    boxed: false,
                },
                ty: param.ty.clone(),
                span: param.span.clone(),
            })
            .collect();
        TypedExpr {
            kind: TypedExprKind::IntrinsicCall { intrinsic, args },
            ty: return_type.clone(),
            span: func.span.clone(),
        }
    }

    /// Validate async function constraints: must have explicit return type that implements Awaitable.
    pub(super) fn validate_async_constraints(&mut self, func: &FunctionDecl, return_type: &Type) {
        if !func.is_async {
            return;
        }
        let Some(declared_return_type) = &func.return_type else {
            self.diagnostics.error(
                func.name.span.clone(),
                "async functions must have an explicit return type".to_string(),
            );
            return;
        };
        let trait_fqn = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
        if !self.type_satisfies_trait(&trait_fqn, &[], return_type, 0) {
            self.diagnostics.error(
                declared_return_type.span(),
                "async function return type must implement Awaitable".to_string(),
            );
        }
    }

    /// Infer types for a single function declaration.
    /// Non-generic: full inference and insert into typed_functions.
    /// Generic: type-check body as template, store with type_params for monomorphize.
    pub(super) fn infer_function(&mut self, func: &FunctionDecl) {
        // Other intrinsic functions are resolved directly in the call path.
        if matches!(func.body, Expr::Intrinsic(_)) && self.class_identity_intrinsic(func).is_none()
        {
            return;
        }
        if !func.type_params.is_empty() {
            self.infer_function_typecheck_only(func);
            return;
        }

        // Resolve parameter types
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

        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };

        self.validate_async_constraints(func, &return_type);

        // For async functions, body returns T (inner value), not the full Awaitable<T>
        let body_expected_type = if func.is_async {
            self.resolve_awaitable_value_type(&return_type)
                .unwrap_or(return_type.clone())
        } else {
            return_type.clone()
        };

        // Param scope
        self.push_scope();
        for param in &typed_params {
            self.define_variable(VarName(param.name.clone()), param.ty.clone(), false);
        }

        // Body scope
        self.push_scope();
        let prev_expected = self.expected_type.take();
        self.expected_type = Some(body_expected_type.clone());
        let prev_fn_return = self.function_return_type.take();
        self.function_return_type = Some(return_type.clone());
        let prev_async_return = self.async_return_type.take();
        if func.is_async {
            self.async_return_type = Some(return_type.clone());
        }
        let body = self.infer_function_body(func, &typed_params, &return_type);
        self.async_return_type = prev_async_return;
        self.function_return_type = prev_fn_return;
        self.expected_type = prev_expected;
        self.pop_scope();

        self.pop_scope();

        self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);

        let body = self.wrap_async_body(body, &return_type, func.is_async);

        let symbol_name = if let Some(ref module_name) = self.container_name {
            format!("{}.{}", module_name, func.name.value)
        } else {
            func.name.value.clone()
        };
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(symbol_name),
        };
        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let name = MangledName::for_function(&fqn, &param_types);

        let display_name = super::make_display_name(&fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            name.clone(),
            TypedFunction {
                visibility: func.visibility,
                source_name: fqn.symbol.0.clone(),
                name,
                type_params: vec![],
                params: typed_params,
                return_type,
                body,
                span: func.span.clone(),
                vtable_self_type: None,
                is_async: func.is_async,
                display_name,
            },
        );
    }

    /// Type-check a generic function body and insert a template TypedFunction
    /// with TypeParameter types. The template is later instantiated by monomorphize.
    fn infer_function_typecheck_only(&mut self, func: &FunctionDecl) {
        let type_params: Vec<TypeParamName> = func
            .type_params
            .iter()
            .map(|tp| TypeParamName(tp.value.clone()))
            .collect();

        let symbol_name = if let Some(ref module_name) = self.container_name {
            format!("{}.{}", module_name, func.name.value)
        } else {
            func.name.value.clone()
        };
        let fqn = Fqn {
            package: self.package_path.clone(),
            symbol: SymbolName(symbol_name),
        };
        // First pass: set up type params without bounds so where clause can resolve
        let preliminary_map = self.type_param_map(&type_params, &TraitBounds::empty());
        let prev_type_params = self.current_type_params.clone();
        for tp in &type_params {
            if let Some(ty) = preliminary_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }
        // Now resolve where clause (T is in scope)
        let trait_bounds =
            self.resolve_trait_bounds_from_where_clause(&func.where_clause, &type_params);
        // Second pass: re-create with bounds
        let type_param_map = self.type_param_map(&type_params, &trait_bounds);
        for tp in &type_params {
            if let Some(ty) = type_param_map.get(&tp.0) {
                self.current_type_params.insert(tp.clone(), ty.clone());
            }
        }

        let return_type = match &func.return_type {
            Some(type_expr) => self.resolve_type_expr(type_expr),
            None => Type::Unit,
        };

        self.validate_async_constraints(func, &return_type);

        // For async functions, body returns T (inner value), not the full Awaitable<T>
        let body_expected_type = if func.is_async {
            self.resolve_awaitable_value_type(&return_type)
                .unwrap_or(return_type.clone())
        } else {
            return_type.clone()
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
        let body = self.infer_function_body(func, &typed_params, &return_type);
        self.pop_scope();
        self.pop_scope();

        self.check_assignable(body.span.clone(), &body_expected_type, &body.ty);

        // For async templates, wrap the body in AsyncBlock with a ResolvedImplMethod
        // containing TypeParameter types. After substitution, the types become concrete
        // and the desugar step can resolve the impl method.
        let body = self.wrap_async_body(body, &return_type, func.is_async);

        // If inside a generic container (class or module), prepend its type params
        // so the template has the combined [class_params..., method_params...].
        // This matches the type_params stored on FunctionCall nodes.
        let container_type_params: Vec<TypeParamName> = prev_type_params
            .iter()
            .filter(|(name, ty)| {
                matches!(ty, Type::TypeVariable(_, _) | Type::GenericParam(_, _, _))
                    && !type_params.contains(name)
            })
            .map(|(name, _)| name.clone())
            .collect();
        let all_type_params: Vec<TypeParamName> = container_type_params
            .into_iter()
            .chain(type_params.iter().cloned())
            .collect();

        self.async_return_type = prev_async_return;
        self.function_return_type = prev_fn_return;
        self.expected_type = prev_expected;
        self.current_type_params = prev_type_params;

        let param_types: Vec<&Type> = typed_params.iter().map(|p| &p.ty).collect();
        let template_name = MangledName::for_function(&fqn, &param_types);

        let display_name = super::make_display_name(&fqn.to_string(), &typed_params);
        self.typed_functions.insert(
            template_name.clone(),
            TypedFunction {
                visibility: func.visibility,
                source_name: fqn.symbol.0.clone(),
                name: template_name,
                type_params: all_type_params,
                params: typed_params,
                return_type,
                body,
                span: func.span.clone(),
                vtable_self_type: None,
                is_async: func.is_async,
                display_name,
            },
        );
    }
}
