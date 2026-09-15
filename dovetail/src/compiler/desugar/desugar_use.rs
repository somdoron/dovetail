use std::sync::Arc;

use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, SymbolName, VarName};
use crate::typechecker::types::{
    ResolvedImplMethod, Type, TypedClosureParam, TypedExpr, TypedExprKind, TypedMatchArm,
    TypedModule,
};

/// Desugar `Use` nodes into `Usable.use(continuation)` calls.
///
/// The transformation captures the rest of the enclosing block as a continuation
/// closure passed to the trait's `use` method:
///
/// ```text
///   let x = use expr     →    expr.use(x => <rest>)
///   <rest>
/// ```
///
/// Multiple `use` expressions nest, producing LIFO release order. Expression-level
/// `use` (inside `(use a) + (use b)`) is extracted depth-first to fresh temps.
///
/// Runs between `desugar_try` and `desugar_await`. When inside an async function
/// (function body is `AsyncBlock`), continuation closure bodies are wrapped in
/// `AsyncBlock` too so `desugar_await` can find any `await` nodes inside them.
pub fn desugar_use_expressions(module: &mut TypedModule) {
    super::awaitable::snapshot(module);
    let function_keys: Vec<MangledName> = module.functions.keys().cloned().collect();
    for key in function_keys {
        let func = module.functions.get_mut(&key).unwrap();
        let body = std::mem::replace(&mut func.body, dummy_expr());
        func.body = process_body(body);
    }
    for func in module.default_templates.values_mut() {
        let body = std::mem::replace(&mut func.body, dummy_expr());
        func.body = process_body(body);
    }
    for global in module.globals.values_mut() {
        let init = std::mem::replace(&mut global.initializer, dummy_expr());
        global.initializer = process_body(init);
    }
    for test in module.tests.iter_mut() {
        let body = std::mem::replace(&mut test.body, dummy_expr());
        test.body = process_body(body);
    }
    for block in &mut module.implement_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            let body = std::mem::replace(&mut method.body, dummy_expr());
            method.body = process_body(body);
        }
    }
    for block in &mut module.extension_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            let body = std::mem::replace(&mut method.body, dummy_expr());
            method.body = process_body(body);
        }
    }
}

/// Entry point for processing a function/test/global/method body. Determines
/// the async context from the body's outer AsyncBlock wrapper (if any), then
/// walks the body. Bare `Use` bodies are wrapped in a synthetic block so the
/// block-level processor handles them uniformly.
fn process_body(expr: TypedExpr) -> TypedExpr {
    let async_succeed = extract_async_succeed(&expr);
    // If the body is itself a Use (e.g. `function f(): T = use r`), wrap in a
    // singleton block to route it through desugar_block.
    let needs_singleton = matches!(expr.kind, TypedExprKind::Use { .. });
    if needs_singleton {
        let ty = expr.ty.clone();
        let span = expr.span.clone();
        return desugar_block(vec![expr], ty, span, async_succeed.as_ref());
    }
    walk(expr, async_succeed.as_ref())
}

fn dummy_expr() -> TypedExpr {
    TypedExpr {
        kind: TypedExprKind::UnitLiteral,
        ty: Type::Unit,
        span: Span::point(Arc::from(""), 0, 0),
    }
}

fn synthetic_span() -> Span {
    Span::point(Arc::from(""), 0, 0)
}

static USE_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn fresh_use_var() -> VarName {
    let n = USE_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    VarName(format!("$use_{}", n))
}

/// If `expr` is an `AsyncBlock`, return its `succeed_method`. Used to wrap
/// continuation closure bodies so `desugar_await` can find inner awaits.
fn extract_async_succeed(expr: &TypedExpr) -> Option<ResolvedImplMethod> {
    match &expr.kind {
        TypedExprKind::AsyncBlock { succeed_method, .. } => Some(succeed_method.clone()),
        _ => None,
    }
}

/// Recursively walk an expression. Variants are reconstructed (mirroring the
/// `walk_expr_for_async_closures` pattern in desugar_await.rs); when we hit a
/// `Block`, run the block-level use-processing.
fn walk(expr: TypedExpr, async_succeed: Option<&ResolvedImplMethod>) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;

    let kind = match expr.kind {
        TypedExprKind::Block(stmts) => {
            // Normalize `let x = <multi-statement block ending in use>` into a
            // temp binding + plain `let x = use $tmp` BEFORE walking, so the
            // statement-level handler lifts the rest of THIS block as the
            // continuation (see `expand_let_tail_use`).
            let stmts: Vec<TypedExpr> = stmts.into_iter().flat_map(expand_let_tail_use).collect();
            let processed: Vec<TypedExpr> =
                stmts.into_iter().map(|s| walk(s, async_succeed)).collect();
            return desugar_block(processed, ty, span, async_succeed);
        }

        // Preserve Use nodes — they're only processed when found as a statement
        // by `desugar_block` (which is invoked when walking enters a Block).
        // We still walk the operand in case it contains nested Use that the
        // surrounding statement-level handler will deal with after pulling this
        // Use out via `extract_first_use`.
        TypedExprKind::Use {
            operand,
            inner_type,
            source_error,
            target_error,
            from_method,
        } => TypedExprKind::Use {
            operand: Box::new(walk(*operand, async_succeed)),
            inner_type,
            source_error,
            target_error,
            from_method,
        },

        TypedExprKind::AsyncBlock {
            body,
            succeed_method,
        } => {
            let new_body = walk(*body, Some(&succeed_method));
            TypedExprKind::AsyncBlock {
                body: Box::new(new_body),
                succeed_method,
            }
        }

        TypedExprKind::Closure {
            params,
            body,
            captures,
        } => TypedExprKind::Closure {
            params,
            // Each closure establishes its own computation boundary. Its body
            // carries an AsyncBlock only when the closure itself is async.
            body: Box::new(process_body(*body)),
            captures,
        },

        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(walk(*message, async_succeed)),
        },
        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(walk(*condition, async_succeed)),
            message: message.map(|m| Box::new(walk(*m, async_succeed))),
        },
        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => {
            let value = peel_singleton_use_blocks(*value);
            let had_use = contains_use(&value);
            let mut walked = walk(value, async_succeed);
            if had_use {
                // A NON-tail `use` inside the value block (tail uses were
                // normalized away by `peel_singleton_use_blocks` /
                // `expand_let_tail_use` before we got here) has been rewritten
                // into a `Usable.use(...)` call producing `Wrapped<U>` — for
                // async impls, `Async<U, E>` — while the binding still expects
                // the plain `U`. Await the lifted value back down.
                walked = await_lifted_let_value(walked, &var_ty, async_succeed);
            }
            TypedExprKind::Let {
                name,
                mutable,
                boxed,
                var_ty,
                value: Box::new(walked),
            }
        }
        TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value,
        } => TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value: Box::new(walk(*value, async_succeed)),
        },
        TypedExprKind::GlobalAssign {
            name,
            type_params,
            value,
        } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(walk(*value, async_succeed)),
        },
        TypedExprKind::FunctionCall {
            name,
            args,
            type_params,
        } => TypedExprKind::FunctionCall {
            name,
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
            type_params,
        },
        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(walk(*left, async_succeed)),
            right: Box::new(walk(*right, async_succeed)),
        },
        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(walk(*operand, async_succeed)),
        },
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => TypedExprKind::If {
            condition: Box::new(walk(*condition, async_succeed)),
            then_branch: Box::new(walk(*then_branch, async_succeed)),
            else_branch: else_branch.map(|e| Box::new(walk(*e, async_succeed))),
        },
        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(walk(*condition, async_succeed)),
            body: Box::new(walk(*body, async_succeed)),
        },
        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(walk(*subject, async_succeed)),
            arms: arms
                .into_iter()
                .map(|arm| TypedMatchArm {
                    body: Box::new(walk(*arm.body, async_succeed)),
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
                .map(|(n, e)| (n, walk(e, async_succeed)))
                .collect(),
        },
        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements
                .into_iter()
                .map(|e| walk(e, async_succeed))
                .collect(),
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
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
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
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::FieldAccess {
            object,
            field_name,
            field_index,
            boxed,
        } => TypedExprKind::FieldAccess {
            object: Box::new(walk(*object, async_succeed)),
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
            object: Box::new(walk(*object, async_succeed)),
            field_name,
            field_index,
            boxed,
            value: Box::new(walk(*value, async_succeed)),
        },
        TypedExprKind::RecordWith {
            object,
            fqn,
            overrides,
            type_params,
        } => TypedExprKind::RecordWith {
            object: Box::new(walk(*object, async_succeed)),
            fqn,
            type_params,
            overrides: overrides
                .into_iter()
                .map(|(n, i, e)| (n, i, walk(e, async_succeed)))
                .collect(),
        },
        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements
                .into_iter()
                .map(|e| walk(e, async_succeed))
                .collect(),
        },
        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(walk(*value, async_succeed)),
            target_type,
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(walk(*value, async_succeed)),
            target_type,
        },
        TypedExprKind::LetDestructure {
            pattern,
            var_ty,
            value,
        } => TypedExprKind::LetDestructure {
            pattern,
            var_ty,
            value: Box::new(walk(*value, async_succeed)),
        },
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(walk(*value, async_succeed)),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(walk(*value, async_succeed)),
        },
        TypedExprKind::InterfaceObjectCoerce {
            inner,
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        } => TypedExprKind::InterfaceObjectCoerce {
            inner: Box::new(walk(*inner, async_succeed)),
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        },
        TypedExprKind::TemplateInterfaceObjectCoerce {
            inner,
            traits,
            concrete_type,
        } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: Box::new(walk(*inner, async_succeed)),
            traits,
            concrete_type,
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(walk(*inner, async_succeed)),
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
            receiver: Box::new(walk(*receiver, async_succeed)),
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::ClassNew {
            mangled_name,
            args,
            type_params,
        } => TypedExprKind::ClassNew {
            mangled_name,
            type_params,
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::ClassStructCreate {
            target_mangled_name,
            fields,
            type_params,
        } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::ClassVirtualCall {
            object,
            vtable_slot,
            args,
        } => TypedExprKind::ClassVirtualCall {
            object: Box::new(walk(*object, async_succeed)),
            vtable_slot,
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(walk(*value, async_succeed)),
            return_type,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(walk(*callee, async_succeed)),
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::MethodRef {
            object,
            method_name,
            type_params,
        } => TypedExprKind::MethodRef {
            object: Box::new(walk(*object, async_succeed)),
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
            operand: Box::new(walk(*operand, async_succeed)),
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
            operand: Box::new(walk(*operand, async_succeed)),
            unwrap_method,
            unwrap_return_type,
            return_type,
            from_method,
        },
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
            method_type_params,
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
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
            type_params,
            args: args.into_iter().map(|a| walk(a, async_succeed)).collect(),
        },
        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: Box::new(walk(*inner, async_succeed)),
        },
        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before desugar_use pass")
        }

        // Leaves and refs.
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
        | TypedExprKind::ImplFunctionRef { .. }
        | TypedExprKind::ExtFunctionRef { .. }
        | TypedExprKind::Break
        | TypedExprKind::Continue) => kind,
    };

    TypedExpr { kind, ty, span }
}

/// Process a block-level list of statements. Transforms the first `Use`-bearing
/// statement and recurses on what remains.
fn desugar_block(
    stmts: Vec<TypedExpr>,
    block_ty: Type,
    block_span: Span,
    async_succeed: Option<&ResolvedImplMethod>,
) -> TypedExpr {
    let first_idx = stmts.iter().position(contains_use);
    let first_idx = match first_idx {
        Some(idx) => idx,
        None => return rebuild_block(stmts, block_ty, block_span),
    };

    let (before, use_and_rest) = stmts.split_at(first_idx);
    let before: Vec<TypedExpr> = before.to_vec();
    let mut use_and_rest: Vec<TypedExpr> = use_and_rest.to_vec();
    let use_stmt = use_and_rest.remove(0);
    let rest: Vec<TypedExpr> = use_and_rest;

    // Tail `use expr` (no continuation) — emit identity continuation. Built
    // via `build_continuation_closure` so that in async context the identity
    // body is AsyncBlock-wrapped and the closure is typed `(T) => Async<T, E>`,
    // making `emit_use_call` stamp the call `Async<T, E>` — the impl's actual
    // `Wrapped<U> = Async<U, E>` return. A bare `x => x` typed `(T) => T`
    // misstated the call's type as `T` and failed wasm validation at codegen.
    // (In sync context this builds exactly the old `(T) => T` identity.)
    if rest.is_empty() && is_tail_use(&use_stmt) {
        let data = extract_tail_use(use_stmt);
        let var_name = fresh_use_var();
        let id_body = TypedExpr {
            kind: TypedExprKind::VarRef {
                name: var_name.clone(),
                boxed: false,
            },
            ty: data.inner_type.clone(),
            span: synthetic_span(),
        };
        let id_closure = build_continuation_closure(
            var_name,
            data.inner_type.clone(),
            id_body,
            data.inner_type.clone(),
            async_succeed,
        );
        let call = emit_use_call(data, id_closure);
        return prepend_before(before, call, block_ty, block_span);
    }

    let (use_data, var_name, continuation_stmts) = extract_use_from_stmt(use_stmt, rest);

    let continuation_body = if continuation_stmts.is_empty() {
        TypedExpr {
            kind: TypedExprKind::VarRef {
                name: var_name.clone(),
                boxed: false,
            },
            ty: use_data.inner_type.clone(),
            span: synthetic_span(),
        }
    } else if continuation_stmts.len() == 1 {
        let single = continuation_stmts.into_iter().next().unwrap();
        desugar_block(
            vec![single],
            block_ty.clone(),
            synthetic_span(),
            async_succeed,
        )
    } else {
        desugar_block(
            continuation_stmts,
            block_ty.clone(),
            synthetic_span(),
            async_succeed,
        )
    };

    let continuation_ty = continuation_body.ty.clone();
    let closure = build_continuation_closure(
        var_name,
        use_data.inner_type.clone(),
        continuation_body,
        continuation_ty.clone(),
        async_succeed,
    );
    let call = emit_use_call(use_data, closure);
    // `emit_use_call` already sets `call.ty` to the wrapped continuation type
    // (Async<continuation_ty, E> in async context, plain continuation_ty in sync
    // context) — matches the closure's return type. Don't override it.
    prepend_before(before, call, block_ty, block_span)
}

fn rebuild_block(stmts: Vec<TypedExpr>, ty: Type, span: Span) -> TypedExpr {
    if stmts.len() == 1 {
        stmts.into_iter().next().unwrap()
    } else {
        TypedExpr {
            kind: TypedExprKind::Block(stmts),
            ty,
            span,
        }
    }
}

fn prepend_before(
    before: Vec<TypedExpr>,
    body: TypedExpr,
    block_ty: Type,
    block_span: Span,
) -> TypedExpr {
    if before.is_empty() {
        body
    } else {
        let body_ty = body.ty.clone();
        let mut all = before;
        all.push(body);
        TypedExpr {
            kind: TypedExprKind::Block(all),
            ty: if matches!(block_ty, Type::Unit) {
                Type::Unit
            } else {
                body_ty
            },
            span: block_span,
        }
    }
}

/// A `let` whose value starts on the line after the `=` arrives as
/// `Block([expr])` — the layout filter opens a block for the indented
/// continuation. When such a singleton chain bottoms out at a `Use`, peel the
/// wrappers so the statement-level handler sees `Let { value: Use }` and lifts
/// the REST OF THE ENCLOSING BLOCK as the continuation — exactly as it does
/// for the single-line spelling. Left wrapped, the inner block's tail-`use`
/// path fires instead: it has no continuation to lift, so it emits an identity
/// continuation, releases the resource on the spot, and types the binding as
/// the inner value while the `Usable.use` call actually produces `Wrapped<U>`
/// — which for `Resource` (Wrapped = Async) fails wasm validation at codegen.
///
/// Only chains ending in exactly a `Use` are peeled: a multi-statement value
/// block keeps its own scope, and a `Use` nested inside a larger expression
/// keeps the existing extract-to-temp path.
fn peel_singleton_use_blocks(expr: TypedExpr) -> TypedExpr {
    fn chain_ends_in_use(expr: &TypedExpr) -> bool {
        match &expr.kind {
            TypedExprKind::Use { .. } => true,
            TypedExprKind::Block(stmts) if stmts.len() == 1 => chain_ends_in_use(&stmts[0]),
            _ => false,
        }
    }
    if !chain_ends_in_use(&expr) {
        return expr;
    }
    let mut expr = expr;
    while let TypedExprKind::Block(mut stmts) = expr.kind {
        expr = stmts.pop().unwrap();
    }
    expr
}

/// A `let` whose value is a MULTI-statement block ending in `use` cannot be
/// handled by `peel_singleton_use_blocks` — the preceding statements have their
/// own scope and must run before acquisition. Rewrite
///
/// ```text
///   let x =                 let $tmp =           (inner statements keep
///       stmt1                   stmt1             their own scope; the
///       use expr        →       expr              block now yields the
///   <rest>                  let x = use $tmp      resource value)
///                           <rest>
/// ```
///
/// so the statement-level handler sees a plain `let x = use $tmp` and lifts
/// the REST OF THE ENCLOSING BLOCK as the continuation — acquisition happens
/// at the `let`, release after `<rest>` completes, exactly like the
/// single-line spelling. Left unexpanded, the inner block's tail-`use` path
/// fires instead: it has no continuation to lift, so it releases the resource
/// on the spot and (for `Wrapped = Async` impls) binds an `Async<U, E>`-valued
/// call to a variable typed as the inner value — invalid wasm at codegen.
///
/// Statements that aren't such a `let` pass through unchanged. Singleton
/// chains (`Block([Use])`) are left to `peel_singleton_use_blocks`.
fn expand_let_tail_use(stmt: TypedExpr) -> Vec<TypedExpr> {
    let is_candidate = match &stmt.kind {
        TypedExprKind::Let { value, .. } => is_multistmt_tail_use_block(value),
        _ => false,
    };
    if !is_candidate {
        return vec![stmt];
    }
    let span = stmt.span;
    let ty = stmt.ty;
    match stmt.kind {
        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => {
            let (operand_block, parts) = split_tail_use(*value);
            let tmp = fresh_use_var();
            let operand_ty = operand_block.ty.clone();
            let tmp_let = TypedExpr {
                kind: TypedExprKind::Let {
                    name: tmp.clone(),
                    mutable: false,
                    boxed: false,
                    var_ty: operand_ty.clone(),
                    value: Box::new(operand_block),
                },
                ty: Type::Unit,
                span: synthetic_span(),
            };
            let use_expr = TypedExpr {
                kind: TypedExprKind::Use {
                    operand: Box::new(TypedExpr {
                        kind: TypedExprKind::VarRef {
                            name: tmp,
                            boxed: false,
                        },
                        ty: operand_ty,
                        span: synthetic_span(),
                    }),
                    inner_type: parts.inner_type,
                    source_error: parts.source_error,
                    target_error: parts.target_error,
                    from_method: parts.from_method,
                },
                ty: parts.use_ty,
                span: parts.use_span,
            };
            let let_use = TypedExpr {
                kind: TypedExprKind::Let {
                    name,
                    mutable,
                    boxed,
                    var_ty,
                    value: Box::new(use_expr),
                },
                ty,
                span,
            };
            vec![tmp_let, let_use]
        }
        _ => unreachable!("guarded by is_candidate"),
    }
}

/// Fix up a `let` binding whose value block contained a NON-tail `use`:
///
/// ```text
///   let x =                    let x = await
///       let a = use r              (let a = use r     ← already rewritten to
///       stmt2              →        stmt2               Usable.use(r, a =>
///       value                       value)              stmt2; value))
/// ```
///
/// The inner statement-position `use` was correctly desugared by
/// `desugar_block` with the REST OF THE INNER BLOCK as its continuation — the
/// resource is released when the inner block's value is computed, before `x`
/// is bound. But for a `Wrapped<U> = Async<U, E>` impl the rewritten block now
/// evaluates to the LIFTED `Async<U, E>` while the binding (and everything
/// after it) still expects the plain `U` — invalid wasm at codegen. Mirror
/// what `build_continuation_closure` does for tail position: treat the block's
/// desugared result as the lifted Async it is, and await it. The synthetic
/// `Await` is consumed by `desugar_await` (which runs next and rebuilds its
/// method resolutions from the enclosing AsyncBlock's `succeed_method`, so the
/// pre-resolved fields carried on the node are never read).
///
/// The `Await` wraps the block's TAIL statement (the lifted `Usable.use` call)
/// rather than the whole block, so any sibling statements — including further
/// synthetic `Await`s from use-blocks nested inside this one — stay ordinary
/// statements that `desugar_await`'s generic machinery already handles, just
/// as if the user had written `let x = <block with awaits inside>`.
///
/// Sync impls (`Wrapped<U> = U`) produce no lift — the rewritten block is
/// already typed `U` — so the template match below leaves them untouched.
fn await_lifted_let_value(
    value: TypedExpr,
    var_ty: &Type,
    async_succeed: Option<&ResolvedImplMethod>,
) -> TypedExpr {
    let Some(succeed) = async_succeed else {
        return value;
    };
    // The lifted type lives on the block's TAIL statement: `prepend_before`
    // re-stamps a Unit-typed block as Unit even though its tail is the
    // Async-valued Usable.use call.
    let tail_ty = block_tail_ty(&value).clone();
    if &tail_ty == var_ty {
        return value;
    }
    let Some(inner_ty) = wrapped_inner_by_template(&tail_ty, &succeed.for_type) else {
        return value;
    };
    wrap_tail_in_await(value, &inner_ty, succeed)
}

/// Wrap the (recursively) last statement of a block chain in a synthetic
/// `Await`, restoring every enclosing Block's type to the awaited inner type.
fn wrap_tail_in_await(expr: TypedExpr, inner_ty: &Type, succeed: &ResolvedImplMethod) -> TypedExpr {
    if let TypedExprKind::Block(_) = &expr.kind {
        let span = expr.span;
        let TypedExprKind::Block(mut stmts) = expr.kind else {
            unreachable!()
        };
        if let Some(last) = stmts.pop() {
            let new_last = wrap_tail_in_await(last, inner_ty, succeed);
            stmts.push(new_last);
        }
        return TypedExpr {
            kind: TypedExprKind::Block(stmts),
            ty: inner_ty.clone(),
            span,
        };
    }
    // The tail IS the lifted Usable.use call; its own type is the wrapped
    // Async, which the Await's return_type records.
    let span = expr.span.clone();
    let lifted_ty = expr.ty.clone();
    TypedExpr {
        kind: TypedExprKind::Await {
            operand: Box::new(expr),
            return_type: lifted_ty,
            // Placeholders: `desugar_await`'s linear desugar synthesizes its
            // andThen/map resolutions and SourceLocation mangled name itself.
            and_then_method: succeed.clone(),
            map_method: succeed.clone(),
            source_location_mn: MangledName::for_type(
                &Fqn::from_dotted("standard.prelude.SourceLocation").unwrap(),
            ),
        },
        ty: inner_ty.clone(),
        span,
    }
}

/// The type of a block chain's (recursively) last statement — the value the
/// block actually evaluates to, regardless of what the Block node is stamped.
fn block_tail_ty(expr: &TypedExpr) -> &Type {
    match &expr.kind {
        TypedExprKind::Block(stmts) if !stmts.is_empty() => block_tail_ty(stmts.last().unwrap()),
        _ => &expr.ty,
    }
}

/// If `ty` is the wrapped (lifted) form the Async-shaped `template`
/// (`succeed.for_type`, e.g. `Async<X, E>`) would produce — same FQN, same `E`
/// type argument — return the wrapped inner type `U`. `None` for anything
/// else, including sync-impl (`Wrapped<U> = U`) results.
fn wrapped_inner_by_template(ty: &Type, template: &Type) -> Option<Type> {
    let (template_fqn, template_e) = match template {
        Type::GenericClass { fqn, type_args, .. } | Type::GenericEnum { fqn, type_args, .. }
            if type_args.len() == 2 =>
        {
            (fqn, &type_args[1].1)
        }
        _ => return None,
    };
    match ty {
        Type::GenericClass { fqn, type_args, .. } | Type::GenericEnum { fqn, type_args, .. }
            if type_args.len() == 2 && fqn == template_fqn && &type_args[1].1 == template_e =>
        {
            Some(type_args[0].1.clone())
        }
        _ => None,
    }
}

/// True for a `Block` whose (recursively) LAST statement is a `Use`, excluding
/// pure singleton chains (those are `peel_singleton_use_blocks`' job).
fn is_multistmt_tail_use_block(expr: &TypedExpr) -> bool {
    fn tail_ends_in_use(e: &TypedExpr) -> bool {
        match &e.kind {
            TypedExprKind::Use { .. } => true,
            TypedExprKind::Block(stmts) => stmts.last().is_some_and(tail_ends_in_use),
            _ => false,
        }
    }
    fn is_singleton_use_chain(e: &TypedExpr) -> bool {
        match &e.kind {
            TypedExprKind::Use { .. } => true,
            TypedExprKind::Block(stmts) if stmts.len() == 1 => is_singleton_use_chain(&stmts[0]),
            _ => false,
        }
    }
    matches!(expr.kind, TypedExprKind::Block(_))
        && tail_ends_in_use(expr)
        && !is_singleton_use_chain(expr)
}

/// The fields of a `Use` node minus its operand, for reconstruction at the
/// enclosing statement level by `expand_let_tail_use`.
struct TailUseParts {
    inner_type: Type,
    source_error: Type,
    target_error: Type,
    from_method: Option<ResolvedImplMethod>,
    use_ty: Type,
    use_span: Span,
}

/// Rewrite a block chain ending in `use` so it evaluates to the use's OPERAND
/// (the resource value) instead, returning the rewritten expression plus the
/// `Use` node's fields. Callers must have verified the chain's tail is a `use`
/// (`is_multistmt_tail_use_block`); anything else is unreachable.
fn split_tail_use(expr: TypedExpr) -> (TypedExpr, TailUseParts) {
    let span = expr.span;
    let ty = expr.ty;
    match expr.kind {
        TypedExprKind::Use {
            operand,
            inner_type,
            source_error,
            target_error,
            from_method,
        } => {
            let parts = TailUseParts {
                inner_type,
                source_error,
                target_error,
                from_method,
                use_ty: ty,
                use_span: span,
            };
            (*operand, parts)
        }
        TypedExprKind::Block(mut stmts) => {
            let last = stmts
                .pop()
                .expect("split_tail_use: guard ensures the block is non-empty");
            let (new_last, parts) = split_tail_use(last);
            let new_ty = new_last.ty.clone();
            stmts.push(new_last);
            (
                TypedExpr {
                    kind: TypedExprKind::Block(stmts),
                    ty: new_ty,
                    span,
                },
                parts,
            )
        }
        _ => unreachable!(
            "split_tail_use: guard (is_multistmt_tail_use_block) ensures the chain's tail is a Use"
        ),
    }
}

fn is_tail_use(stmt: &TypedExpr) -> bool {
    matches!(stmt.kind, TypedExprKind::Use { .. })
}

fn extract_tail_use(stmt: TypedExpr) -> UseData {
    match stmt.kind {
        TypedExprKind::Use {
            operand,
            inner_type,
            source_error,
            target_error,
            from_method,
        } => UseData {
            operand: *operand,
            inner_type,
            source_error,
            target_error,
            from_method,
        },
        _ => unreachable!(),
    }
}

/// The result type of a call emitted by this pass, through block tails.
/// Generated use calls have synthetic spans; an explicit source-level `.use`
/// call has a real source span and remains an ordinary computation value.
fn resource_use_result_type(expr: &TypedExpr) -> Option<&Type> {
    match &expr.kind {
        TypedExprKind::ImplFunctionCall {
            trait_fqn,
            method_name,
            ..
        } if expr.span.file.is_empty()
            && method_name.0 == "use"
            && *trait_fqn == Fqn::from_dotted("standard.prelude.Usable").unwrap() =>
        {
            Some(&expr.ty)
        }
        TypedExprKind::Block(statements) => statements.last().and_then(resource_use_result_type),
        _ => None,
    }
}

/// Branches retain their source type until await lowering lifts every arm.
/// Recognize rewritten resource results even when that source type is itself
/// an Awaitable, or a block still carries its original Unit type.
pub(super) fn is_resource_use_result(expr: &TypedExpr) -> bool {
    if resource_use_result_type(expr).is_some() {
        return true;
    }
    match &expr.kind {
        TypedExprKind::Block(statements) => statements.last().is_some_and(is_resource_use_result),
        TypedExprKind::If {
            then_branch,
            else_branch,
            ..
        } => {
            is_resource_use_result(then_branch)
                || else_branch
                    .as_ref()
                    .is_some_and(|branch| is_resource_use_result(branch))
        }
        TypedExprKind::Match { arms, .. } => {
            arms.iter().any(|arm| is_resource_use_result(&arm.body))
        }
        _ => false,
    }
}

fn build_continuation_closure(
    var_name: VarName,
    inner_type: Type,
    body: TypedExpr,
    body_ty: Type,
    async_succeed: Option<&ResolvedImplMethod>,
) -> TypedExpr {
    // A nested generated use call is already lifted. Other source results,
    // including Awaitable-valued results and branches awaiting their own lift,
    // become the success value of a fresh rebound computation.
    let wrapped_body_ty = match async_succeed {
        Some(succeed) => resource_use_result_type(&body)
            .filter(|result| super::awaitable::success_type(result).is_some())
            .cloned()
            .unwrap_or_else(|| super::awaitable::rebind(&succeed.for_type, body_ty.clone())),
        None => body_ty.clone(),
    };
    let body = match async_succeed {
        Some(succeed) => TypedExpr {
            kind: TypedExprKind::AsyncBlock {
                body: Box::new(body),
                succeed_method: succeed.clone(),
            },
            ty: wrapped_body_ty.clone(),
            span: synthetic_span(),
        },
        None => body,
    };
    TypedExpr {
        kind: TypedExprKind::Closure {
            params: vec![TypedClosureParam {
                name: var_name,
                ty: inner_type.clone(),
                span: synthetic_span(),
            }],
            body: Box::new(body),
            captures: Vec::new(),
        },
        ty: Type::Function(vec![inner_type], Box::new(wrapped_body_ty)),
        span: synthetic_span(),
    }
}

/// Emit `operand.use(closure)` as an `ImplFunctionCall` to `Usable<T>.use`.
///
/// `method_type_params` on the ImplFunctionCall must be the concatenation
/// `[block_type_args..., method_type_args...]` — that's the convention the
/// monomorphize pass expects (it slices `[block.type_params.len()..]` to
/// recover the method type args). We approximate the block type args by
/// taking the operand type's own type args (sound when the impl's `for_type`
/// is shaped like the operand type, which is the typical case).
///
/// The method's single type param `U` is recovered from the closure's return
/// type — which equals `Wrapped<U>`. For impls with `Wrapped<U> = U` (sync
/// resources like a scoped counter), U is the closure return type as-is.
/// For impls with `Wrapped<U> = Async<U, E>` (e.g. `Resource<T, E>`), U is
/// the first type argument of the closure's Async return.
fn emit_use_call(use_data: UseData, closure: TypedExpr) -> TypedExpr {
    let UseData {
        operand,
        inner_type,
        source_error,
        target_error,
        from_method,
    } = use_data;

    let wrapped_ty = match &closure.ty {
        Type::Function(_, ret) => (**ret).clone(),
        _ => Type::Error,
    };
    let u_ty = unwrap_u_from_wrapped(&wrapped_ty);
    let usable_fqn = Fqn::from_dotted("standard.prelude.Usable").unwrap();

    let mut method_type_params: Vec<Type> = operand_type_args(&operand.ty);
    method_type_params.push(u_ty);
    method_type_params.push(target_error.clone());

    let error_f = build_error_f(&source_error, &target_error, from_method.as_ref());

    TypedExpr {
        kind: TypedExprKind::ImplFunctionCall {
            trait_fqn: usable_fqn,
            trait_type_params: vec![inner_type.clone(), source_error.clone()],
            for_type: operand.ty.clone(),
            method_name: SymbolName("use".to_string()),
            args: vec![operand, closure, error_f],
            method_type_params,
        },
        ty: wrapped_ty,
        span: synthetic_span(),
    }
}

/// Synthesize the `errorF: (E) => E2` argument for a `Usable.use` call.
///
/// - `E == Never` → `(_e) => panic "unreachable"`. The closure can never be
///   invoked because Never is uninhabited; codegen would otherwise choke on
///   trying to box a `Never`-typed value to anyref.
/// - `E == E2` (non-Never) → identity `(e) => e`.
/// - `From<E> for E2` → `(e) => target.from(e)`.
fn build_error_f(
    source: &Type,
    target: &Type,
    from_method: Option<&ResolvedImplMethod>,
) -> TypedExpr {
    let var = fresh_use_var();
    let var_ref = TypedExpr {
        kind: TypedExprKind::VarRef {
            name: var.clone(),
            boxed: false,
        },
        ty: source.clone(),
        span: synthetic_span(),
    };
    let body = match from_method {
        // From-conversion path: emit `target.from(var)`.
        Some(resolved) => TypedExpr {
            kind: TypedExprKind::ImplFunctionCall {
                trait_fqn: resolved.trait_fqn.clone(),
                trait_type_params: resolved.trait_type_params.clone(),
                for_type: resolved.for_type.clone(),
                method_name: resolved.method_name.clone(),
                args: vec![var_ref],
                method_type_params: resolved.method_type_params.clone(),
            },
            ty: target.clone(),
            span: synthetic_span(),
        },
        // source == Never: the closure can never be invoked. Body panics so
        // codegen doesn't try to box a Never value to anyref.
        None if matches!(source, Type::Never) => {
            let msg = TypedExpr {
                kind: TypedExprKind::StringLiteral(
                    "unreachable: Usable errorF invoked with Never".to_string(),
                ),
                ty: Type::String,
                span: synthetic_span(),
            };
            TypedExpr {
                kind: TypedExprKind::Panic {
                    message: Box::new(msg),
                },
                ty: target.clone(),
                span: synthetic_span(),
            }
        }
        // source == target: identity.
        None => var_ref,
    };
    TypedExpr {
        kind: TypedExprKind::Closure {
            params: vec![TypedClosureParam {
                name: var,
                ty: source.clone(),
                span: synthetic_span(),
            }],
            body: Box::new(body),
            captures: Vec::new(),
        },
        ty: Type::Function(vec![source.clone()], Box::new(target.clone())),
        span: synthetic_span(),
    }
}

/// Extract type arguments from a generic operand type. Returns empty for
/// non-generic operand types (records/classes without type params).
fn operand_type_args(ty: &Type) -> Vec<Type> {
    match ty {
        Type::GenericRecord { type_args, .. }
        | Type::GenericEnum { type_args, .. }
        | Type::GenericNewtype { type_args, .. }
        | Type::GenericClass { type_args, .. } => {
            type_args.iter().map(|(_, t)| t.clone()).collect()
        }
        _ => Vec::new(),
    }
}

/// Given the closure's return type (which is `Wrapped<U>` for the chosen
/// Usable impl), recover U. We pattern-match on the known shapes:
///
/// - `Wrapped<U> = U`            → U is the closure return type itself.
/// - `Wrapped<U> = Async<U, E>`  → U is the first type argument of Async.
///
/// More impls with novel wrapped shapes will require a more general approach
/// (storing the wrapped template on the Use node at infer time).
fn unwrap_u_from_wrapped(wrapped: &Type) -> Type {
    if let Type::AssociatedProjection(projection) = wrapped
        && projection.trait_fqn == Fqn::from_dotted("standard.prelude.Usable").unwrap()
        && projection.member == "Wrapped"
        && projection.parameters.len() == 2
    {
        return projection.parameters[0].clone();
    }
    if let Type::GenericClass { fqn, type_args, .. } = wrapped
        && fqn.symbol.0 == "Async"
        && !type_args.is_empty()
    {
        return type_args[0].1.clone();
    }
    wrapped.clone()
}

struct UseData {
    operand: TypedExpr,
    inner_type: Type,
    source_error: Type,
    target_error: Type,
    from_method: Option<ResolvedImplMethod>,
}

fn extract_use_from_stmt(
    stmt: TypedExpr,
    rest: Vec<TypedExpr>,
) -> (UseData, VarName, Vec<TypedExpr>) {
    match stmt.kind {
        TypedExprKind::Let { name, value, .. }
            if matches!(value.kind, TypedExprKind::Use { .. }) =>
        {
            let data = match value.kind {
                TypedExprKind::Use {
                    operand,
                    inner_type,
                    source_error,
                    target_error,
                    from_method,
                } => UseData {
                    operand: *operand,
                    inner_type,
                    source_error,
                    target_error,
                    from_method,
                },
                _ => unreachable!(),
            };
            (data, name, rest)
        }
        _ => {
            let (data, var_name, modified_expr) =
                extract_first_use(stmt).expect("statement should contain Use");
            let mut continuation = rest;
            continuation.insert(0, modified_expr);
            (data, var_name, continuation)
        }
    }
}

/// Depth-first walk: find the first `Use` and replace it with a `VarRef`.
/// Returns the use's data, the fresh var name bound to it, and the modified
/// expression with the Use replaced.
fn extract_first_use(expr: TypedExpr) -> Option<(UseData, VarName, TypedExpr)> {
    let span = expr.span.clone();
    let ty = expr.ty.clone();
    match expr.kind {
        TypedExprKind::Use {
            operand,
            inner_type,
            source_error,
            target_error,
            from_method,
        } => {
            let var_name = fresh_use_var();
            let data = UseData {
                operand: *operand,
                inner_type: inner_type.clone(),
                source_error,
                target_error,
                from_method,
            };
            Some((
                data,
                var_name.clone(),
                TypedExpr {
                    kind: TypedExprKind::VarRef {
                        name: var_name,
                        boxed: false,
                    },
                    ty: inner_type,
                    span,
                },
            ))
        }
        TypedExprKind::BinaryOp { op, left, right } => {
            if contains_use(&left) {
                let (data, vn, new_left) = extract_first_use(*left)?;
                return Some((
                    data,
                    vn,
                    TypedExpr {
                        kind: TypedExprKind::BinaryOp {
                            op,
                            left: Box::new(new_left),
                            right,
                        },
                        ty,
                        span,
                    },
                ));
            }
            if contains_use(&right) {
                let (data, vn, new_right) = extract_first_use(*right)?;
                return Some((
                    data,
                    vn,
                    TypedExpr {
                        kind: TypedExprKind::BinaryOp {
                            op,
                            left,
                            right: Box::new(new_right),
                        },
                        ty,
                        span,
                    },
                ));
            }
            None
        }
        TypedExprKind::UnaryOp { op, operand } => {
            let (data, vn, new_operand) = extract_first_use(*operand)?;
            Some((
                data,
                vn,
                TypedExpr {
                    kind: TypedExprKind::UnaryOp {
                        op,
                        operand: Box::new(new_operand),
                    },
                    ty,
                    span,
                },
            ))
        }
        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => {
            let (data, vn, new_value) = extract_first_use(*value)?;
            Some((
                data,
                vn,
                TypedExpr {
                    kind: TypedExprKind::Let {
                        name,
                        mutable,
                        boxed,
                        var_ty,
                        value: Box::new(new_value),
                    },
                    ty,
                    span,
                },
            ))
        }
        TypedExprKind::FunctionCall {
            name,
            args,
            type_params,
        } => {
            let mut new_args = Vec::with_capacity(args.len());
            let mut extracted: Option<(UseData, VarName)> = None;
            for arg in args {
                if extracted.is_some() {
                    new_args.push(arg);
                } else if contains_use(&arg) {
                    if let Some((d, vn, new_arg)) = extract_first_use(arg) {
                        extracted = Some((d, vn));
                        new_args.push(new_arg);
                    } else {
                        unreachable!()
                    }
                } else {
                    new_args.push(arg);
                }
            }
            extracted.map(|(d, vn)| {
                (
                    d,
                    vn,
                    TypedExpr {
                        kind: TypedExprKind::FunctionCall {
                            name,
                            args: new_args,
                            type_params,
                        },
                        ty,
                        span,
                    },
                )
            })
        }
        _ => None,
    }
}

/// Recursively check whether any `Use` node appears in `expr`.
fn contains_use(expr: &TypedExpr) -> bool {
    match &expr.kind {
        TypedExprKind::Use { .. } => true,
        TypedExprKind::Block(stmts) => stmts.iter().any(contains_use),
        TypedExprKind::Let { value, .. } => contains_use(value),
        TypedExprKind::LetDestructure { value, .. } => contains_use(value),
        TypedExprKind::Assign { value, .. } => contains_use(value),
        TypedExprKind::FieldAssign { value, .. } => contains_use(value),
        TypedExprKind::GlobalAssign { value, .. } => contains_use(value),
        TypedExprKind::BinaryOp { left, right, .. } => contains_use(left) || contains_use(right),
        TypedExprKind::UnaryOp { operand, .. } => contains_use(operand),
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            contains_use(condition)
                || contains_use(then_branch)
                || else_branch.as_ref().is_some_and(|e| contains_use(e))
        }
        TypedExprKind::While { condition, body } => contains_use(condition) || contains_use(body),
        TypedExprKind::Match { subject, arms } => {
            contains_use(subject) || arms.iter().any(|a| contains_use(&a.body))
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::EnumVariantRecordCreate { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassStructCreate { fields: args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => args.iter().any(contains_use),
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            contains_use(object) || args.iter().any(contains_use)
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. } => {
            contains_use(receiver) || args.iter().any(contains_use)
        }
        TypedExprKind::ClosureCall { callee, args } => {
            contains_use(callee) || args.iter().any(contains_use)
        }
        TypedExprKind::FieldAccess { object, .. } => contains_use(object),
        TypedExprKind::RecordCreate { fields, .. } => fields.iter().any(|(_, e)| contains_use(e)),
        TypedExprKind::TupleLiteral { elements } => elements.iter().any(contains_use),
        TypedExprKind::RecordWith {
            object, overrides, ..
        } => contains_use(object) || overrides.iter().any(|(_, _, e)| contains_use(e)),
        TypedExprKind::ArrayLiteral { elements } => elements.iter().any(contains_use),
        TypedExprKind::Panic { message } => contains_use(message),
        TypedExprKind::Assert { condition, message } => {
            contains_use(condition) || message.as_ref().is_some_and(|m| contains_use(m))
        }
        TypedExprKind::Return { value, .. } => contains_use(value),
        TypedExprKind::Try { operand, .. } => contains_use(operand),
        TypedExprKind::Await { operand, .. } => contains_use(operand),
        TypedExprKind::TypeTest { value, .. } | TypedExprKind::TypeCast { value, .. } => {
            contains_use(value)
        }
        TypedExprKind::NewtypeCreate { value } | TypedExprKind::NewtypeValue { value } => {
            contains_use(value)
        }
        TypedExprKind::BoxToAny { inner }
        | TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => contains_use(inner),
        TypedExprKind::AsyncBlock { body, .. } => contains_use(body),
        TypedExprKind::Closure { body, .. } => contains_use(body),
        TypedExprKind::MethodRef { object, .. } => contains_use(object),
        TypedExprKind::ForLoop { iterable, body, .. } => {
            contains_use(iterable) || contains_use(body)
        }
        TypedExprKind::UnitLiteral
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
        | TypedExprKind::ImplFunctionRef { .. }
        | TypedExprKind::ExtFunctionRef { .. }
        | TypedExprKind::Break
        | TypedExprKind::Continue => false,
    }
}
