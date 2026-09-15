use std::sync::Arc;

use crate::common::span::Span;
use crate::common::types::VarName;

use crate::typechecker::types::{
    ResolvedImplMethod, Type, TypedExpr, TypedExprKind, TypedMatchArm, TypedModule, TypedPattern,
};

/// Desugar `Try` nodes into `match unwrap(operand) { Ok(x) => x, Error(r) => Return(r) }`.
///
/// This pass runs after inference and rules, before variance casts.
/// After this pass, no `Try` nodes remain in the typed AST.
pub fn desugar_try_expressions(module: &mut TypedModule) {
    for func in module.functions.values_mut() {
        func.body = walk_expr(
            std::mem::replace(&mut func.body, dummy_expr()),
        );
    }
    for func in module.default_templates.values_mut() {
        func.body = walk_expr(std::mem::replace(&mut func.body, dummy_expr()));
    }
    for global in module.globals.values_mut() {
        global.initializer = walk_expr(
            std::mem::replace(&mut global.initializer, dummy_expr()),
        );
    }
    for test in module.tests.iter_mut() {
        test.body = walk_expr(std::mem::replace(&mut test.body, dummy_expr()));
    }
    for block in &mut module.implement_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            method.body = walk_expr(std::mem::replace(&mut method.body, dummy_expr()));
        }
    }
    for block in &mut module.extension_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            method.body = walk_expr(std::mem::replace(&mut method.body, dummy_expr()));
        }
    }
}

/// Dummy expression used as a placeholder during `std::mem::replace`.
fn dummy_expr() -> TypedExpr {
    TypedExpr {
        kind: TypedExprKind::UnitLiteral,
        ty: Type::Unit,
        span: Span::point(Arc::from(""), 0, 0),
    }
}

/// Bottom-up rewrite: recurse into children, then desugar Try nodes at this level.
fn walk_expr(expr: TypedExpr) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;

    let kind = match expr.kind {
        TypedExprKind::Try {
            operand,
            unwrap_method,
            unwrap_return_type,
            return_type,
            from_method,
        } => {
            let operand = walk_expr(*operand);
            return desugar_try(operand, unwrap_method, unwrap_return_type, return_type, from_method, ty, span);
        }

        // === Recursive (same pattern as variance_cast.rs) ===

        TypedExprKind::Block(exprs) => {
            TypedExprKind::Block(exprs.into_iter().map(walk_expr).collect())
        }

        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(walk_expr(*message)),
        },

        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(walk_expr(*condition)),
            message: message.map(|m| Box::new(walk_expr(*m))),
        },

        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value: Box::new(walk_expr(*value)),
        },

        TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value,
        } => TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value: Box::new(walk_expr(*value)),
        },

        TypedExprKind::GlobalAssign { name, type_params, value } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(walk_expr(*value)),
        },

        TypedExprKind::FunctionCall { name, args, type_params } => TypedExprKind::FunctionCall {
            name,
            args: args.into_iter().map(walk_expr).collect(),
            type_params,
        },

        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(walk_expr(*left)),
            right: Box::new(walk_expr(*right)),
        },

        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(walk_expr(*operand)),
        },

        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => TypedExprKind::If {
            condition: Box::new(walk_expr(*condition)),
            then_branch: Box::new(walk_expr(*then_branch)),
            else_branch: else_branch.map(|e| Box::new(walk_expr(*e))),
        },

        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(walk_expr(*condition)),
            body: Box::new(walk_expr(*body)),
        },

        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(walk_expr(*subject)),
            arms: arms
                .into_iter()
                .map(|arm| TypedMatchArm {
                    body: Box::new(walk_expr(*arm.body)),
                    ..arm
                })
                .collect(),
        },

        TypedExprKind::RecordCreate { fqn, fields, type_params } => TypedExprKind::RecordCreate {
            fqn,
            type_params,
            fields: fields
                .into_iter()
                .map(|(name, expr)| (name, walk_expr(expr)))
                .collect(),
        },

        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements.into_iter().map(walk_expr).collect(),
        },

        TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            type_params,
            args: args.into_iter().map(walk_expr).collect(),
        },

        TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            type_params,
            args: args.into_iter().map(walk_expr).collect(),
        },

        TypedExprKind::FieldAccess {
            object,
            field_name,
            field_index,
            boxed,
        } => TypedExprKind::FieldAccess {
            object: Box::new(walk_expr(*object)),
            field_name,
            field_index,
            boxed,
        },

        TypedExprKind::FieldAssign {
            object,
            field_name,
            field_index,
            value,
            boxed,
        } => TypedExprKind::FieldAssign {
            object: Box::new(walk_expr(*object)),
            field_name,
            field_index,
            value: Box::new(walk_expr(*value)),
            boxed,
        },

        TypedExprKind::RecordWith {
            object,
            fqn,
            overrides,
            type_params,
        } => TypedExprKind::RecordWith {
            object: Box::new(walk_expr(*object)),
            fqn,
            type_params,
            overrides: overrides
                .into_iter()
                .map(|(name, idx, expr)| (name, idx, walk_expr(expr)))
                .collect(),
        },

        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements.into_iter().map(walk_expr).collect(),
        },

        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args.into_iter().map(walk_expr).collect(),
        },

        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(walk_expr(*value)),
            target_type,
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(walk_expr(*value)),
            target_type,
        },
        TypedExprKind::LetDestructure {
            pattern,
            var_ty,
            value,
        } => TypedExprKind::LetDestructure {
            pattern,
            var_ty,
            value: Box::new(walk_expr(*value)),
        },
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(walk_expr(*value)),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(walk_expr(*value)),
        },
        TypedExprKind::InterfaceObjectCoerce {
            inner,
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        } => TypedExprKind::InterfaceObjectCoerce {
            inner: Box::new(walk_expr(*inner)),
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        },
        TypedExprKind::TemplateInterfaceObjectCoerce {
            inner,
            traits,
            concrete_type,
        } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: Box::new(walk_expr(*inner)),
            traits,
            concrete_type,
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(walk_expr(*inner)),
        },
        TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name,
            member_name,
            receiver,
            args,
        } => TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name,
            member_name,
            receiver: Box::new(walk_expr(*receiver)),
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::ClassNew { mangled_name, args, type_params } => TypedExprKind::ClassNew {
            mangled_name,
            type_params,
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::ClassStructCreate { target_mangled_name, fields, type_params } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::ClassVirtualCall { object, vtable_slot, args } => TypedExprKind::ClassVirtualCall {
            object: Box::new(walk_expr(*object)),
            vtable_slot,
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::ClassSuperCall { method_mangled, args } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(walk_expr(*value)),
            return_type,
        },
        TypedExprKind::Closure { params, body, captures } => TypedExprKind::Closure {
            params,
            body: Box::new(walk_expr(*body)),
            captures,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(walk_expr(*callee)),
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::MethodRef { object, method_name, type_params } => TypedExprKind::MethodRef {
            object: Box::new(walk_expr(*object)),
            method_name,
            type_params,
        },

        TypedExprKind::Await { operand, return_type, and_then_method, map_method, source_location_mn } => TypedExprKind::Await {
            operand: Box::new(walk_expr(*operand)),
            return_type,
            and_then_method,
            map_method,
            source_location_mn,
        },

        TypedExprKind::Use { operand, inner_type, source_error, target_error, from_method } => TypedExprKind::Use {
            operand: Box::new(walk_expr(*operand)),
            inner_type, source_error, target_error, from_method,
        },

        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before desugar_try pass")
        }

        TypedExprKind::AsyncBlock { body, succeed_method } => TypedExprKind::AsyncBlock {
            body: Box::new(walk_expr(*body)),
            succeed_method,
        },

        // Leaf nodes
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
        | TypedExprKind::Continue
        | TypedExprKind::BoxToAny { .. }) => kind,

        TypedExprKind::ImplFunctionCall { trait_fqn, trait_type_params, for_type, method_name, args, method_type_params } => TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args: args.into_iter().map(walk_expr).collect(),
            method_type_params,
        },
        kind @ TypedExprKind::ImplFunctionRef { .. } => kind,
        TypedExprKind::ExtFunctionCall { ext_fqn, for_type, method_name, args, type_params } => TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args: args.into_iter().map(walk_expr).collect(),
            type_params,
        },
        kind @ TypedExprKind::ExtFunctionRef { .. } => kind,
    };

    TypedExpr { kind, ty, span }
}

/// Desugar a single Try node into:
/// ```text
/// Block([
///     Let { $try = operand },
///     Match {
///         subject: unwrap($try),
///         arms: [
///             case Ok($try_ok) => VarRef($try_ok),
///             case Error($try_err) => Return { value: VarRef($try_err) },
///         ]
///     }
/// ])
/// ```
fn desugar_try(
    operand: TypedExpr,
    unwrap_method: ResolvedImplMethod,
    unwrap_return_type: Type,
    return_type: Type,
    from_method: Option<ResolvedImplMethod>,
    success_type: Type,
    span: Span,
) -> TypedExpr {
    let synthetic_span = Span::point(Arc::from(""), 0, 0);

    let tmp_name = VarName("$try".to_string());
    let ok_name = VarName("$try_ok".to_string());
    let err_name = VarName("$try_err".to_string());

    let operand_ty = operand.ty.clone();

    // Extract Ok and Error payload types from unwrap_return_type: Result<T, OnFailure>
    let (ok_type, err_type) = match &unwrap_return_type {
        Type::GenericEnum { type_args, .. } if type_args.len() == 2 => {
            (type_args[0].1.clone(), type_args[1].1.clone())
        }
        _ => {
            // Error recovery: return the operand as-is
            return operand;
        }
    };

    // Let binding: let _tryN = operand
    let let_expr = TypedExpr {
        kind: TypedExprKind::Let {
            name: tmp_name.clone(),
            mutable: false,
            boxed: false,
            var_ty: operand_ty.clone(),
            value: Box::new(operand),
        },
        ty: Type::Unit,
        span: synthetic_span.clone(),
    };

    let unwrap_call = TypedExpr {
        kind: TypedExprKind::ImplFunctionCall {
            trait_fqn: unwrap_method.trait_fqn,
            trait_type_params: unwrap_method.trait_type_params,
            for_type: unwrap_method.for_type,
            method_name: unwrap_method.method_name,
            args: vec![TypedExpr {
                kind: TypedExprKind::VarRef {
                    name: tmp_name.clone(),
                    boxed: false,
                },
                ty: operand_ty,
                span: synthetic_span.clone(),
            }],
            method_type_params: unwrap_method.method_type_params,
        },
        ty: unwrap_return_type.clone(),
        span: synthetic_span.clone(),
    };

    // Ok arm: case Ok(_tryN_ok) => VarRef(_tryN_ok)
    let ok_arm = TypedMatchArm {
        pattern: TypedPattern::EnumVariant {
            enum_type: unwrap_return_type.clone(),
            variant_name: "Ok".to_string(),
            variant_index: 0,
            payload_patterns: vec![TypedPattern::Variable(ok_name.clone(), ok_type.clone())],
        },
        guard: None,
        body: Box::new(TypedExpr {
            kind: TypedExprKind::VarRef {
                name: ok_name,
                boxed: false,
            },
            ty: ok_type,
            span: synthetic_span.clone(),
        }),
        span: synthetic_span.clone(),
    };

    // Error arm: case Error(_tryN_err) => Return { value: from(VarRef(_tryN_err)) or VarRef(_tryN_err) }
    let err_value = TypedExpr {
        kind: TypedExprKind::VarRef {
            name: err_name.clone(),
            boxed: false,
        },
        ty: err_type.clone(),
        span: synthetic_span.clone(),
    };
    let return_value = match from_method {
        Some(from_resolved) => TypedExpr {
            kind: TypedExprKind::ImplFunctionCall {
                trait_fqn: from_resolved.trait_fqn,
                trait_type_params: from_resolved.trait_type_params,
                for_type: from_resolved.for_type,
                method_name: from_resolved.method_name,
                args: vec![err_value],
                method_type_params: from_resolved.method_type_params,
            },
            ty: return_type.clone(),
            span: synthetic_span.clone(),
        },
        None => err_value,
    };
    let err_arm = TypedMatchArm {
        pattern: TypedPattern::EnumVariant {
            enum_type: unwrap_return_type,
            variant_name: "Error".to_string(),
            variant_index: 1,
            payload_patterns: vec![TypedPattern::Variable(err_name.clone(), err_type)],
        },
        guard: None,
        body: Box::new(TypedExpr {
            kind: TypedExprKind::Return {
                value: Box::new(return_value),
                return_type,
            },
            ty: Type::Never,
            span: synthetic_span.clone(),
        }),
        span: synthetic_span.clone(),
    };

    // Match expression
    let match_expr = TypedExpr {
        kind: TypedExprKind::Match {
            subject: Box::new(unwrap_call),
            arms: vec![ok_arm, err_arm],
        },
        ty: success_type.clone(),
        span: synthetic_span,
    };

    // Block([let, match])
    TypedExpr {
        kind: TypedExprKind::Block(vec![let_expr, match_expr]),
        ty: success_type,
        span,
    }
}
