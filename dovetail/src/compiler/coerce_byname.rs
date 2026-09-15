use std::collections::BTreeMap;
use std::sync::Arc;

use crate::common::span::Span;
use crate::common::types::{MangledName, TypeParamName};
use crate::typechecker::infer::types::is_byname_fqn;
use crate::typechecker::types::{
    Type, TypedExpr, TypedExprKind, TypedMatchArm, TypedModule,
};

/// Holds function parameter info for ByName coercion lookups.
struct FnParamInfo {
    /// Concrete function params: MangledName → param types.
    concrete: BTreeMap<MangledName, Vec<Type>>,
    /// Template function params: MangledName → (type_params, param types with TypeParameter).
    /// Used as fallback for concrete names not yet in `concrete` (created by monomorphize later).
    templates: BTreeMap<MangledName, (Vec<TypeParamName>, Vec<Type>)>,
}

impl FnParamInfo {
    /// Look up param types for a function call. For concrete functions, returns directly.
    /// For calls to monomorphize-created functions (name contains `#`), derives the
    /// template name, looks up the template, and substitutes type_args to get concrete params.
    fn lookup(&self, name: &MangledName, type_args: &[Type]) -> Option<Vec<Type>> {
        if let Some(params) = self.concrete.get(name) {
            return Some(params.clone());
        }
        // Try template fallback: concrete name = template_name#type_arg1,type_arg2,...
        if !type_args.is_empty() {
            let template_name_str = name.0.split('#').next()?;
            let template_mn = MangledName(template_name_str.to_string());
            if let Some((type_params, template_params)) = self.templates.get(&template_mn) {
                if type_params.len() != type_args.len() {
                    return None;
                }
                let sub: BTreeMap<TypeParamName, Type> = type_params
                    .iter()
                    .zip(type_args.iter())
                    .map(|(tp, t)| (tp.clone(), t.clone()))
                    .collect();
                let concrete_params = template_params
                    .iter()
                    .map(|ty| crate::monomorphize::substitute::apply_type_substitution(ty, &sub))
                    .collect();
                return Some(concrete_params);
            }
        }
        // Also try template fallback when type_args is empty but name contains '#'
        if name.0.contains('#') {
            let template_name_str = name.0.split('#').next()?;
            let template_mn = MangledName(template_name_str.to_string());
            if let Some(params) = self.concrete.get(&template_mn) {
                return Some(params.clone());
            }
        }
        None
    }
}

/// Walk the typed module and wrap arguments that need implicit ByName conversion.
/// Must run BEFORE capture analysis so synthetic closures get their captures filled in.
pub fn coerce_byname_args(module: &mut TypedModule) {
    let concrete: BTreeMap<MangledName, Vec<Type>> = module
        .functions
        .iter()
        .chain(module.function_templates.iter())
        .map(|(mn, f)| (mn.clone(), f.params.iter().map(|p| p.ty.clone()).collect()))
        .collect();
    let templates: BTreeMap<MangledName, (Vec<TypeParamName>, Vec<Type>)> = module
        .functions
        .iter()
        .chain(module.function_templates.iter())
        .filter(|(_, f)| !f.type_params.is_empty())
        .map(|(mn, f)| {
            (
                mn.clone(),
                (
                    f.type_params.clone(),
                    f.params.iter().map(|p| p.ty.clone()).collect(),
                ),
            )
        })
        .collect();
    let function_params = FnParamInfo { concrete, templates };

    for func in module.functions.values_mut().chain(module.function_templates.values_mut()) {
        func.body = walk_expr(
            std::mem::replace(&mut func.body, dummy_expr()),
            &function_params,
        );
    }
    for global in module.globals.values_mut() {
        global.initializer = walk_expr(
            std::mem::replace(&mut global.initializer, dummy_expr()),
            &function_params,
        );
    }
    for test in module.tests.iter_mut() {
        test.body = walk_expr(
            std::mem::replace(&mut test.body, dummy_expr()),
            &function_params,
        );
    }
    for block in &mut module.implement_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            method.body = walk_expr(
                std::mem::replace(&mut method.body, dummy_expr()),
                &function_params,
            );
        }
    }
    for block in &mut module.extension_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            method.body = walk_expr(
                std::mem::replace(&mut method.body, dummy_expr()),
                &function_params,
            );
        }
    }
}

fn dummy_expr() -> TypedExpr {
    TypedExpr {
        kind: TypedExprKind::UnitLiteral,
        ty: Type::Unit,
        span: Span::point(Arc::from(""), 0, 0),
    }
}

fn walk_expr(expr: TypedExpr, fn_params: &FnParamInfo) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;

    let kind = match expr.kind {
        TypedExprKind::FunctionCall { name, args, type_params } => {
            let args: Vec<TypedExpr> = args.into_iter().map(|a| walk_expr(a, fn_params)).collect();
            let args = if let Some(param_types) = fn_params.lookup(&name, &type_params) {
                coerce_args(args, &param_types)
            } else {
                args
            };
            TypedExprKind::FunctionCall { name, args, type_params }
        }

        TypedExprKind::Block(exprs) => {
            TypedExprKind::Block(exprs.into_iter().map(|e| walk_expr(e, fn_params)).collect())
        }

        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(walk_expr(*message, fn_params)),
        },

        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(walk_expr(*condition, fn_params)),
            message: message.map(|m| Box::new(walk_expr(*m, fn_params))),
        },

        TypedExprKind::Let { name, mutable, boxed, var_ty, value } => TypedExprKind::Let {
            name, mutable, boxed, var_ty,
            value: Box::new(walk_expr(*value, fn_params)),
        },

        TypedExprKind::Assign { name, target_ty, boxed, value } => TypedExprKind::Assign {
            name, target_ty, boxed,
            value: Box::new(walk_expr(*value, fn_params)),
        },

        TypedExprKind::GlobalAssign { name, type_params, value } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(walk_expr(*value, fn_params)),
        },

        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(walk_expr(*left, fn_params)),
            right: Box::new(walk_expr(*right, fn_params)),
        },

        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(walk_expr(*operand, fn_params)),
        },

        TypedExprKind::If { condition, then_branch, else_branch } => TypedExprKind::If {
            condition: Box::new(walk_expr(*condition, fn_params)),
            then_branch: Box::new(walk_expr(*then_branch, fn_params)),
            else_branch: else_branch.map(|e| Box::new(walk_expr(*e, fn_params))),
        },

        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(walk_expr(*condition, fn_params)),
            body: Box::new(walk_expr(*body, fn_params)),
        },

        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(walk_expr(*subject, fn_params)),
            arms: arms.into_iter().map(|arm| TypedMatchArm {
                body: Box::new(walk_expr(*arm.body, fn_params)),
                ..arm
            }).collect(),
        },

        TypedExprKind::RecordCreate { fqn, fields, type_params } => TypedExprKind::RecordCreate {
            fqn,
            type_params,
            fields: fields.into_iter().map(|(n, e)| (n, walk_expr(e, fn_params))).collect(),
        },

        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements.into_iter().map(|e| walk_expr(e, fn_params)).collect(),
        },

        TypedExprKind::EnumCreate { fqn, variant_name, args, type_params } => TypedExprKind::EnumCreate {
            fqn, variant_name,
            type_params,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },

        TypedExprKind::EnumVariantRecordCreate { fqn, variant_name, args, type_params } => TypedExprKind::EnumVariantRecordCreate {
            fqn, variant_name,
            type_params,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },

        TypedExprKind::FieldAccess { object, field_name, field_index, boxed } => TypedExprKind::FieldAccess {
            object: Box::new(walk_expr(*object, fn_params)),
            field_name, field_index, boxed,
        },

        TypedExprKind::FieldAssign { object, field_name, field_index, value, boxed } => TypedExprKind::FieldAssign {
            object: Box::new(walk_expr(*object, fn_params)),
            field_name, field_index, boxed,
            value: Box::new(walk_expr(*value, fn_params)),
        },

        TypedExprKind::RecordWith { object, fqn, overrides, type_params } => TypedExprKind::RecordWith {
            object: Box::new(walk_expr(*object, fn_params)),
            fqn,
            type_params,
            overrides: overrides.into_iter().map(|(name, idx, expr)| (name, idx, walk_expr(expr, fn_params))).collect(),
        },

        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements.into_iter().map(|e| walk_expr(e, fn_params)).collect(),
        },

        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },

        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(walk_expr(*value, fn_params)),
            target_type,
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(walk_expr(*value, fn_params)),
            target_type,
        },

        TypedExprKind::LetDestructure { pattern, var_ty, value } => TypedExprKind::LetDestructure {
            pattern, var_ty,
            value: Box::new(walk_expr(*value, fn_params)),
        },

        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(walk_expr(*value, fn_params)),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(walk_expr(*value, fn_params)),
        },

        TypedExprKind::InterfaceObjectCoerce { inner, interface_mangled_name, concrete_type, vtable_methods } => TypedExprKind::InterfaceObjectCoerce {
            inner: Box::new(walk_expr(*inner, fn_params)),
            interface_mangled_name, concrete_type, vtable_methods,
        },
        TypedExprKind::TemplateInterfaceObjectCoerce { inner, traits, concrete_type } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: Box::new(walk_expr(*inner, fn_params)),
            traits, concrete_type,
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(walk_expr(*inner, fn_params)),
        },
        TypedExprKind::InterfaceObjectMethodCall { interface_mangled_name, method_name, member_name, receiver, args } => TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name, method_name, member_name,
            receiver: Box::new(walk_expr(*receiver, fn_params)),
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },

        TypedExprKind::ClassNew { mangled_name, args, type_params } => TypedExprKind::ClassNew {
            mangled_name,
            type_params,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },
        TypedExprKind::ClassStructCreate { target_mangled_name, fields, type_params } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields.into_iter().map(|e| walk_expr(e, fn_params)).collect(),
        },
        TypedExprKind::ClassVirtualCall { object, vtable_slot, args } => TypedExprKind::ClassVirtualCall {
            object: Box::new(walk_expr(*object, fn_params)),
            vtable_slot,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },
        TypedExprKind::ClassSuperCall { method_mangled, args } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },

        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(walk_expr(*value, fn_params)),
            return_type,
        },
        TypedExprKind::Closure { params, body, captures } => TypedExprKind::Closure {
            params,
            body: Box::new(walk_expr(*body, fn_params)),
            captures,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(walk_expr(*callee, fn_params)),
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
        },
        TypedExprKind::MethodRef { object, method_name, type_params } => TypedExprKind::MethodRef {
            object: Box::new(walk_expr(*object, fn_params)),
            method_name,
            type_params,
        },

        TypedExprKind::Await { operand, return_type, and_then_method, map_method, source_location_mn } => TypedExprKind::Await {
            operand: Box::new(walk_expr(*operand, fn_params)),
            return_type, and_then_method, map_method, source_location_mn,
        },
        TypedExprKind::Try { operand, unwrap_method, unwrap_return_type, return_type, from_method } => TypedExprKind::Try {
            operand: Box::new(walk_expr(*operand, fn_params)),
            unwrap_method, unwrap_return_type, return_type, from_method,
        },
        TypedExprKind::Use { .. } => {
            unreachable!("Use nodes should be desugared before ByName coercion pass")
        }
        TypedExprKind::AsyncBlock { body, succeed_method } => TypedExprKind::AsyncBlock {
            body: Box::new(walk_expr(*body, fn_params)),
            succeed_method,
        },

        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: Box::new(walk_expr(*inner, fn_params)),
        },

        TypedExprKind::ForLoop { pattern, iterable, iterator_method, iterator_type, element_type, body } => TypedExprKind::ForLoop {
            pattern,
            iterable: Box::new(walk_expr(*iterable, fn_params)),
            iterator_method, iterator_type, element_type,
            body: Box::new(walk_expr(*body, fn_params)),
        },

        kind @ (TypedExprKind::UnitLiteral
        | TypedExprKind::BoolLiteral(_)
        | TypedExprKind::StringLiteral(_)
        | TypedExprKind::CharLiteral(_)
        | TypedExprKind::Int8Literal(_)
        | TypedExprKind::Int16Literal(_)
        | TypedExprKind::Int32Literal(_)
        | TypedExprKind::Int64Literal(_)
        | TypedExprKind::Uint8Literal(_)
        | TypedExprKind::Uint16Literal(_)
        | TypedExprKind::Uint32Literal(_)
        | TypedExprKind::Uint64Literal(_)
        | TypedExprKind::Uint128Literal(_)
        | TypedExprKind::Float32Literal(_)
        | TypedExprKind::Float64Literal(_)
        | TypedExprKind::VarRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::Break
        | TypedExprKind::Continue) => kind,

        TypedExprKind::ImplFunctionCall { trait_fqn, trait_type_params, for_type, method_name, args, method_type_params } => TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
            method_type_params,
        },
        kind @ TypedExprKind::ImplFunctionRef { .. } => kind,
        TypedExprKind::ExtFunctionCall { ext_fqn, for_type, method_name, args, type_params } => TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args: args.into_iter().map(|a| walk_expr(a, fn_params)).collect(),
            type_params,
        },
        kind @ TypedExprKind::ExtFunctionRef { .. } => kind,
    };

    TypedExpr { kind, ty, span }
}

fn needs_byname_coercion(actual: &Type, expected: &Type) -> bool {
    if let Type::GenericNewtype { fqn, type_args, .. } = expected {
        if is_byname_fqn(fqn) && !type_args.is_empty() {
            if let Type::GenericNewtype { fqn: a_fqn, .. } = actual {
                return !is_byname_fqn(a_fqn);
            }
            return true;
        }
    }
    false
}

fn coerce_args(args: Vec<TypedExpr>, param_types: &[Type]) -> Vec<TypedExpr> {
    args.into_iter()
        .zip(param_types.iter())
        .map(|(arg, expected)| {
            if needs_byname_coercion(&arg.ty, expected) {
                wrap_in_byname(arg, expected.clone())
            } else {
                arg
            }
        })
        .collect()
}

fn wrap_in_byname(expr: TypedExpr, byname_ty: Type) -> TypedExpr {
    let span = expr.span.clone();
    let inner_ty = expr.ty.clone();
    let closure_ty = Type::Function(vec![], Box::new(inner_ty));
    let closure = TypedExpr {
        kind: TypedExprKind::Closure {
            params: vec![],
            body: Box::new(expr),
            captures: vec![],
        },
        ty: closure_ty,
        span: span.clone(),
    };
    TypedExpr {
        kind: TypedExprKind::NewtypeCreate {
            value: Box::new(closure),
        },
        ty: byname_ty,
        span,
    }
}
