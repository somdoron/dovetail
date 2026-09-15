use std::sync::Arc;

use crate::common::span::Span;
use crate::common::types::{
    Fqn, InterfaceMemberName, MangledName, PackagePath, SymbolName, VarName, Variance,
};

use crate::typechecker::types::{
    ResolvedImplMethod, Type, TypedExpr, TypedExprKind, TypedMatchArm, TypedModule, TypedPattern,
};

/// Desugar `ForLoop` nodes into
/// `Block([let iter = ..., let mutable running = true, while running do match next() ...])`.
///
/// This pass runs after inference and rules, before desugar_try.
/// After this pass, no `ForLoop` nodes remain in the typed AST.
///
/// The loop ends by clearing a flag rather than by `break`. A `break` would be
/// simpler, but `desugar_await` runs later and lowers an awaiting `while` into
/// `Async.whileLoop(() => cond, () => body)` — putting the body inside a
/// closure. A `break` lifted into that closure has no enclosing loop left, and
/// codegen aborts with "break outside loop". Ending through the condition keeps
/// `for` working when its body awaits.
pub fn desugar_for_expressions(module: &mut TypedModule) {
    for func in module.functions.values_mut() {
        func.body = walk_expr(std::mem::replace(&mut func.body, dummy_expr()));
    }
    for func in module.default_templates.values_mut() {
        func.body = walk_expr(std::mem::replace(&mut func.body, dummy_expr()));
    }
    for global in module.globals.values_mut() {
        global.initializer = walk_expr(std::mem::replace(&mut global.initializer, dummy_expr()));
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

/// Bottom-up rewrite: recurse into children, then desugar ForLoop nodes at this level.
fn walk_expr(expr: TypedExpr) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;

    let kind = match expr.kind {
        TypedExprKind::ForLoop {
            pattern,
            iterable,
            iterator_method,
            iterator_type,
            element_type,
            body,
        } => {
            let iterable = walk_expr(*iterable);
            let body = walk_expr(*body);
            return desugar_for(
                pattern,
                iterable,
                iterator_method,
                iterator_type,
                element_type,
                body,
                span,
            );
        }

        // === Recursive (same pattern as desugar_try.rs) ===
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

        TypedExprKind::GlobalAssign {
            name,
            type_params,
            value,
        } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(walk_expr(*value)),
        },

        TypedExprKind::FunctionCall {
            name,
            args,
            type_params,
        } => TypedExprKind::FunctionCall {
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

        TypedExprKind::RecordCreate {
            fqn,
            fields,
            type_params,
        } => TypedExprKind::RecordCreate {
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
        TypedExprKind::ClassNew {
            mangled_name,
            args,
            type_params,
        } => TypedExprKind::ClassNew {
            mangled_name,
            type_params,
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::ClassStructCreate {
            target_mangled_name,
            fields,
            type_params,
        } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::ClassVirtualCall {
            object,
            vtable_slot,
            args,
        } => TypedExprKind::ClassVirtualCall {
            object: Box::new(walk_expr(*object)),
            vtable_slot,
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(walk_expr(*value)),
            return_type,
        },
        TypedExprKind::Closure {
            params,
            body,
            captures,
        } => TypedExprKind::Closure {
            params,
            body: Box::new(walk_expr(*body)),
            captures,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(walk_expr(*callee)),
            args: args.into_iter().map(walk_expr).collect(),
        },
        TypedExprKind::MethodRef {
            object,
            method_name,
            type_params,
        } => TypedExprKind::MethodRef {
            object: Box::new(walk_expr(*object)),
            method_name,
            type_params,
        },

        TypedExprKind::Await {
            operand,
            return_type,
            and_then_method,
            map_method,
            source_location_mn,
        } => TypedExprKind::Await {
            operand: Box::new(walk_expr(*operand)),
            return_type,
            and_then_method,
            map_method,
            source_location_mn,
        },

        TypedExprKind::Try {
            operand,
            unwrap_method,
            unwrap_return_type,
            return_type,
            from_method,
        } => TypedExprKind::Try {
            operand: Box::new(walk_expr(*operand)),
            unwrap_method,
            unwrap_return_type,
            return_type,
            from_method,
        },

        TypedExprKind::Use {
            operand,
            inner_type,
            source_error,
            target_error,
            from_method,
        } => TypedExprKind::Use {
            operand: Box::new(walk_expr(*operand)),
            inner_type,
            source_error,
            target_error,
            from_method,
        },

        TypedExprKind::AsyncBlock {
            body,
            succeed_method,
        } => TypedExprKind::AsyncBlock {
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

        TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args,
            method_type_params,
        } => TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args: args.into_iter().map(walk_expr).collect(),
            method_type_params,
        },
        kind @ TypedExprKind::ImplFunctionRef { .. } => kind,
        TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args,
            type_params,
        } => TypedExprKind::ExtFunctionCall {
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

/// Build the Option<T> type for use in desugaring.
fn option_type(element_type: &Type) -> Type {
    let option_fqn = Fqn {
        package: PackagePath(vec!["standard".into(), "prelude".into()]),
        symbol: SymbolName("Option".to_string()),
    };
    let mangled = crate::common::types::MangledName::for_type(&option_fqn);
    Type::GenericEnum {
        fqn: option_fqn,
        mangled_name: mangled,
        type_args: vec![(Variance::Covariant, element_type.clone())],
    }
}

/// Desugar a single ForLoop node into:
/// ```text
/// Block([
///     Let { $for_iter = iterable.iterator() },
///     While {
///         condition: true,
///         body: Block([
///             Let { $for_next = $for_iter.next() },
///             Match {
///                 subject: $for_next,
///                 arms: [
///                     case Some(user_pattern) => user_body,
///                     case None => break,
///                 ]
///             }
///         ])
///     }
/// ])
/// ```
fn desugar_for(
    pattern: TypedPattern,
    iterable: TypedExpr,
    iterator_method: ResolvedImplMethod,
    iterator_type: Type,
    element_type: Type,
    body: TypedExpr,
    span: Span,
) -> TypedExpr {
    let synthetic_span = Span::point(Arc::from(""), 0, 0);

    let iter_name = VarName("$for_iter".to_string());
    let next_name = VarName("$for_next".to_string());
    let running_name = VarName("$for_running".to_string());

    let iterator_fqn = Fqn {
        package: PackagePath(vec!["standard".into(), "prelude".into()]),
        symbol: SymbolName("Iterator".to_string()),
    };

    let option_ty = option_type(&element_type);

    let let_iter = TypedExpr {
        kind: TypedExprKind::Let {
            name: iter_name.clone(),
            mutable: false,
            boxed: false,
            var_ty: iterator_type.clone(),
            value: Box::new(TypedExpr {
                kind: TypedExprKind::ImplFunctionCall {
                    trait_fqn: iterator_method.trait_fqn,
                    trait_type_params: iterator_method.trait_type_params,
                    for_type: iterator_method.for_type,
                    method_name: iterator_method.method_name,
                    args: vec![iterable],
                    method_type_params: iterator_method.method_type_params,
                },
                ty: iterator_type.clone(),
                span: synthetic_span.clone(),
            }),
        },
        ty: Type::Unit,
        span: synthetic_span.clone(),
    };

    let let_running = TypedExpr {
        kind: TypedExprKind::Let {
            name: running_name.clone(),
            mutable: true,
            boxed: false,
            var_ty: Type::Bool,
            value: Box::new(TypedExpr {
                kind: TypedExprKind::BoolLiteral(true),
                ty: Type::Bool,
                span: synthetic_span.clone(),
            }),
        },
        ty: Type::Unit,
        span: synthetic_span.clone(),
    };

    // $for_iter.next() — interface object method call
    let interface_mangled_name = match &iterator_type {
        Type::InterfaceObject { mangled_name, .. } => mangled_name.clone(),
        _ => {
            let trait_mn = MangledName::for_type(&iterator_fqn);
            MangledName::for_interface_object_type(&trait_mn)
        }
    };
    let next_call = TypedExpr {
        kind: TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name: "next".to_string(),
            member_name: InterfaceMemberName::new("next", &[]),
            receiver: Box::new(TypedExpr {
                kind: TypedExprKind::VarRef {
                    name: iter_name.clone(),
                    boxed: false,
                },
                ty: iterator_type,
                span: synthetic_span.clone(),
            }),
            args: vec![],
        },
        ty: option_ty.clone(),
        span: synthetic_span.clone(),
    };

    // let $for_next = $for_iter.next()
    let let_next = TypedExpr {
        kind: TypedExprKind::Let {
            name: next_name.clone(),
            mutable: false,
            boxed: false,
            var_ty: option_ty.clone(),
            value: Box::new(next_call),
        },
        ty: Type::Unit,
        span: synthetic_span.clone(),
    };

    // Some arm: case Some(user_pattern) => user_body
    let some_arm = TypedMatchArm {
        pattern: TypedPattern::EnumVariant {
            enum_type: option_ty.clone(),
            variant_name: "Some".to_string(),
            variant_index: 0,
            payload_patterns: vec![pattern],
        },
        guard: None,
        body: Box::new(body),
        span: synthetic_span.clone(),
    };

    // None arm: case None => $for_running = false
    let none_arm = TypedMatchArm {
        pattern: TypedPattern::EnumVariant {
            enum_type: option_ty.clone(),
            variant_name: "None".to_string(),
            variant_index: 1,
            payload_patterns: vec![],
        },
        guard: None,
        body: Box::new(TypedExpr {
            kind: TypedExprKind::Assign {
                name: running_name.clone(),
                target_ty: Type::Bool,
                boxed: false,
                value: Box::new(TypedExpr {
                    kind: TypedExprKind::BoolLiteral(false),
                    ty: Type::Bool,
                    span: synthetic_span.clone(),
                }),
            },
            ty: Type::Unit,
            span: synthetic_span.clone(),
        }),
        span: synthetic_span.clone(),
    };

    // match $for_next with ...
    let match_expr = TypedExpr {
        kind: TypedExprKind::Match {
            subject: Box::new(TypedExpr {
                kind: TypedExprKind::VarRef {
                    name: next_name,
                    boxed: false,
                },
                ty: option_ty,
                span: synthetic_span.clone(),
            }),
            arms: vec![some_arm, none_arm],
        },
        ty: Type::Unit,
        span: synthetic_span.clone(),
    };

    // while $for_running do ...
    let while_expr = TypedExpr {
        kind: TypedExprKind::While {
            condition: Box::new(TypedExpr {
                kind: TypedExprKind::VarRef {
                    name: running_name,
                    boxed: false,
                },
                ty: Type::Bool,
                span: synthetic_span.clone(),
            }),
            body: Box::new(TypedExpr {
                kind: TypedExprKind::Block(vec![let_next, match_expr]),
                ty: Type::Unit,
                span: synthetic_span.clone(),
            }),
        },
        ty: Type::Unit,
        span: synthetic_span,
    };

    // Block([let_iter, let_running, while_expr])
    TypedExpr {
        kind: TypedExprKind::Block(vec![let_iter, let_running, while_expr]),
        ty: Type::Unit,
        span,
    }
}
