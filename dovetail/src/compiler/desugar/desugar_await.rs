use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::common::span::Span;
use super::desugar_use::is_resource_use_result;
use crate::common::types::{MangledName, SymbolName, VarName, Variance};

use crate::common::types::Fqn;
use crate::typechecker::types::{
    ResolvedImplMethod, Type, TypeDef, TypedClosureParam, TypedExpr, TypedExprKind, TypedMatchArm,
    TypedModule,
};

/// Desugar `Await` nodes into `andThen`/`map` chains.
///
/// For each async function whose body is wrapped in `AsyncBlock`:
/// - If no awaits in body: wrap body using the `succeed` method from AsyncBlock
/// - If awaits present: transform into andThen/map chains
/// - Defer the lowered function body through Awaitable.defer
///
/// Also handles async closures nested inside any function body.
///
/// Runs after `desugar_try`, before `capture::analyze_captures`.
pub fn desugar_await_expressions(module: &mut TypedModule) {
    super::awaitable::snapshot(module);
    // Snapshot the class parent chain for `widens_to`'s covariant-widening
    // check (see `CLASS_PARENTS`). Overwritten wholesale on every call, so a
    // stale map from a previous module can never leak into this pass.
    let class_parents: HashMap<MangledName, MangledName> = module
        .types
        .iter()
        .filter_map(|(mangled, def)| match def {
            TypeDef::Class(class) => class
                .parent_mangled_name
                .clone()
                .map(|parent| (mangled.clone(), parent)),
            _ => None,
        })
        .collect();
    CLASS_PARENTS.with(|slot| *slot.borrow_mut() = class_parents);

    for func in module.default_templates.values_mut() {
        let body = std::mem::replace(&mut func.body, dummy_expr());
        func.body = walk_expr_for_async_closures(body);
    }
    let function_keys: Vec<MangledName> = module.functions.keys().cloned().collect();
    for key in function_keys {
        let func = module.functions.get_mut(&key).unwrap();
        // Only process functions whose body is wrapped in AsyncBlock
        let (inner_body, succeed_method) = match &func.body.kind {
            TypedExprKind::AsyncBlock { .. } => {
                // Extract the AsyncBlock fields
                let body = std::mem::replace(&mut func.body, dummy_expr());
                match body.kind {
                    TypedExprKind::AsyncBlock { body, succeed_method } => (*body, succeed_method),
                    _ => unreachable!(),
                }
            }
            _ => {
                // Even non-async functions may contain async closures — walk the body
                let body = std::mem::replace(&mut func.body, dummy_expr());
                func.body = walk_expr_for_async_closures(body);
                continue;
            }
        };

        let return_type = func.return_type.clone();
        let succeed_method = Some(succeed_method);
        let fn_name = func.source_name.clone();

        let new_body = if contains_await(&inner_body) {
            desugar_body(inner_body, &return_type, &succeed_method, &fn_name)
        } else {
            // No awaits: wrap body using the Awaitable succeed method —
            // UNLESS the body already produces an Async<T, E> value matching
            // the return type (e.g., a `Usable.use(...)` call after
            // use-desugar lifted the continuation closure). In that case
            // wrapping would double-wrap into `Async<Async<T, E>, Never>`.
            maybe_wrap_in_succeed(inner_body, &return_type, &succeed_method)
        };

        func.body = defer_async_body(walk_expr_for_async_closures(new_body), &return_type, &fn_name);
    }
    for test in &mut module.tests {
        test.body = walk_expr_for_async_closures(std::mem::replace(&mut test.body, dummy_expr()));
    }
    for global in module.globals.values_mut() {
        global.initializer = walk_expr_for_async_closures(std::mem::replace(&mut global.initializer, dummy_expr()));
    }
    for block in &mut module.implement_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            let body = std::mem::replace(&mut method.body, dummy_expr());
            match body.kind {
                TypedExprKind::AsyncBlock { body: inner_body, succeed_method } => {
                    let return_type = method.return_type.clone();
                    let succeed = Some(succeed_method);
                    let fn_name = method.name.0.clone();
                    let new_body = if contains_await(&inner_body) {
                        desugar_body(*inner_body, &return_type, &succeed, &fn_name)
                    } else {
                        maybe_wrap_in_succeed(*inner_body, &return_type, &succeed)
                    };
                    method.body = defer_async_body(walk_expr_for_async_closures(new_body), &return_type, &fn_name);
                }
                _ => {
                    method.body = walk_expr_for_async_closures(TypedExpr { kind: body.kind, ty: body.ty, span: body.span });
                }
            }
        }
    }
    for block in &mut module.extension_blocks {
        for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
            let body = std::mem::replace(&mut method.body, dummy_expr());
            match body.kind {
                TypedExprKind::AsyncBlock { body: inner_body, succeed_method } => {
                    let return_type = method.return_type.clone();
                    let succeed = Some(succeed_method);
                    let fn_name = method.name.0.clone();
                    let new_body = if contains_await(&inner_body) {
                        desugar_body(*inner_body, &return_type, &succeed, &fn_name)
                    } else {
                        maybe_wrap_in_succeed(*inner_body, &return_type, &succeed)
                    };
                    method.body = defer_async_body(walk_expr_for_async_closures(new_body), &return_type, &fn_name);
                }
                _ => {
                    method.body = walk_expr_for_async_closures(TypedExpr { kind: body.kind, ty: body.ty, span: body.span });
                }
            }
        }
    }
}

/// Defer the entire function body, including local initialization, so each
/// execution of a saved computation gets fresh state.
fn defer_async_body(body: TypedExpr, return_type: &Type, function_name: &str) -> TypedExpr {
    let span = body.span.clone();
    let closure_type = Type::Function(vec![], Box::new(return_type.clone()));
    let closure = TypedExpr {
        kind: TypedExprKind::Closure {
            params: vec![],
            body: Box::new(body),
            captures: vec![],
        },
        ty: closure_type.clone(),
        span: span.clone(),
    };
    let deferred = TypedExpr {
        kind: TypedExprKind::NewtypeCreate { value: Box::new(closure) },
        ty: Type::GenericNewtype {
            fqn: Fqn::from_dotted("standard.prelude.ByName").unwrap(),
            type_args: vec![(Variance::Covariant, return_type.clone())],
            concrete_inner_type: Box::new(closure_type),
        },
        span: span.clone(),
    };
    let location = Fqn::from_dotted("standard.prelude.SourceLocation").unwrap();
    let trace = build_source_location(&span, function_name, &MangledName::for_type(&location));
    let success = super::awaitable::success_type(return_type)
        .expect("checked async return type implements Awaitable");
    let method = synth_awaitable_method("defer", return_type.clone(), success, vec![]);
    TypedExpr {
        kind: TypedExprKind::ImplFunctionCall {
            trait_fqn: method.trait_fqn,
            trait_type_params: method.trait_type_params,
            for_type: method.for_type,
            method_name: method.method_name,
            args: vec![deferred, trace],
            method_type_params: method.method_type_params,
        },
        ty: return_type.clone(),
        span,
    }
}

/// Wrap a source result in succeed, except when resource-use lowering has
/// already lifted it. An ordinary Awaitable-valued result must stay nested.
fn maybe_wrap_in_succeed(
    body: TypedExpr,
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
) -> TypedExpr {
    // Only resource-use lowering has already wrapped a source-level result.
    // Type equality alone is insufficient when the success type is Any.
    if is_resource_use_result(&body)
        && (body.ty == *return_type || already_lifted(&body.ty, return_type))
    {
        return body;
    }
    // The Block's outer `ty` may be stale (set by typecheck before use-desugar
    // replaced trailing statements with a `Usable.use(...)` call). Check the
    // last statement's actual ty — if THAT matches return_type, the body has
    // already been lifted into Async.
    if let TypedExprKind::Block(stmts) = &body.kind
        && let Some(last) = stmts.last()
        && is_resource_use_result(last)
        && (last.ty == *return_type || already_lifted(&last.ty, return_type))
    {
        return TypedExpr { ty: return_type.clone(), ..body };
    }
    wrap_in_succeed(body, return_type, succeed_method)
}

/// Whether `actual` is already an Awaitable of the shape `return_type` wants, so
/// wrapping it in `succeed` would produce `Async<Async<T, E>, Never>`.
///
/// Equality is too strict, because `Async<out T, out E>` is covariant and a body
/// may legitimately end in a narrower one. The case that matters in practice is
/// a `Never` component: `use res` whose continuation ends in `await Async.fail(e)`
/// has type `Async<Never, E>` in a function returning `Async<Unit, E>`, and
/// wrapping that fed the `use` call itself to `succeed`, which then failed wasm
/// validation (`expected i32, found (ref ...)`).
fn already_lifted(actual: &Type, return_type: &Type) -> bool {
    super::awaitable::success_type(actual).is_some()
        && super::awaitable::success_type(return_type).is_some()
        && actual.try_to_fqn() == return_type.try_to_fqn()
        && widens_to(actual, return_type)
}

fn wrapper_parameters(ty: &Type) -> Option<&[(Variance, Type)]> {
    match ty {
        Type::GenericClass { type_args, .. } | Type::GenericEnum { type_args, .. }
        | Type::GenericRecord { type_args, .. } | Type::GenericNewtype { type_args, .. } => Some(type_args),
        _ => None,
    }
}

/// `Never` is uninhabited, so it stands in for any type in a covariant position;
/// equal types trivially widen; and a class widens to any of its ancestors —
/// `Async<out T, out E>` is covariant, and a narrower Async VALUE is valid where
/// the wider Async is expected (it's the extra `succeed` wrap that poisons
/// codegen). Reached e.g. when use-desugar stamps a `Usable.use(...)` call with
/// the continuation's own narrower success type (`Async<Dog, E>` in a function
/// declared `Async<Animal, E>`).
///
/// The ancestor walk is deliberately conservative: it only recognizes concrete
/// (non-generic) classes whose parent chain is recorded in the module's
/// TypeDefs (snapshotted into `CLASS_PARENTS` at pass start). Anything it does
/// not recognize keeps the current wrap-in-succeed behavior — a too-conservative
/// answer only re-emits the wrap that was emitted before, never an unsound skip.
fn widens_to(actual: &Type, expected: &Type) -> bool {
    if matches!(actual, Type::Never) || matches!(expected, Type::Any) || actual == expected {
        return true;
    }
    if let (Some(actual_params), Some(expected_params)) =
        (wrapper_parameters(actual), wrapper_parameters(expected))
        && actual.try_to_fqn() == expected.try_to_fqn()
        && actual_params.len() == expected_params.len()
    {
        return actual_params.iter().zip(expected_params).all(|((variance, actual), (_, expected))| {
            match variance {
                Variance::Covariant => widens_to(actual, expected),
                Variance::Contravariant => widens_to(expected, actual),
                Variance::Invariant => actual == expected,
            }
        });
    }
    let (actual_mn, expected_mn) = match (actual, expected) {
        (Type::Class(_, a), Type::Class(_, e)) => (a.clone(), e),
        _ => return false,
    };
    CLASS_PARENTS.with(|slot| {
        let parents = slot.borrow();
        let mut current = actual_mn;
        // Bounded walk — defensive against malformed (cyclic) hierarchies.
        for _ in 0..MAX_CLASS_HIERARCHY_DEPTH {
            match parents.get(&current) {
                Some(parent) if parent == expected_mn => return true,
                Some(parent) => current = parent.clone(),
                None => return false,
            }
        }
        false
    })
}

/// Upper bound on class-hierarchy depth for the `widens_to` ancestor walk.
const MAX_CLASS_HIERARCHY_DEPTH: usize = 64;

thread_local! {
    /// Child → parent class mangled names for the current module, snapshotted
    /// by `desugar_await_expressions` from `module.types`. A thread-local
    /// rather than a threaded parameter because the covariance check sits under
    /// `walk_expr_for_async_closures`, which has ~60 recursive call sites; the
    /// pass runs synchronously on the calling thread, and concurrent test
    /// threads each compile with their own thread-local copy.
    static CLASS_PARENTS: RefCell<HashMap<MangledName, MangledName>> =
        RefCell::new(HashMap::new());
}

/// Dummy expression used as a placeholder during `std::mem::replace`.
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

/// Walk an expression tree, desugaring AsyncBlock nodes inside closures (async closures).
fn walk_expr_for_async_closures(expr: TypedExpr) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;

    let kind = match expr.kind {
        // The core case: a closure whose body is an AsyncBlock
        TypedExprKind::Closure { params, body, captures } => {
            match body.kind {
                TypedExprKind::AsyncBlock { body: inner_body, succeed_method: _ } => {
                    // Extract the Awaitable return type from the closure's function type
                    let awaitable_ret = match &ty {
                        Type::Function(_, ret) => (**ret).clone(),
                        _ => unreachable!("closure should have function type"),
                    };
                    // Resource continuations can have a narrower result than the
                    // enclosing async block. Resolve succeed for this closure,
                    // rather than retaining the enclosing block's specialization.
                    let success = super::awaitable::success_type(&awaitable_ret)
                        .expect("async closure implements Awaitable");
                    let succeed = Some(synth_awaitable_method(
                        "succeed", awaitable_ret.clone(), success, vec![],
                    ));

                    let desugared = if contains_await(&inner_body) {
                        desugar_body(*inner_body, &awaitable_ret, &succeed, "<closure>")
                    } else {
                        // Use `maybe_wrap_in_succeed` (not `wrap_in_succeed`) so
                        // a closure body that's already an Async-valued
                        // expression — e.g. a `Usable.use(...)` call after
                        // `desugar_use` lifted the continuation — doesn't get
                        // double-wrapped into `Async<Async<T, E>, Never>`.
                        // Mirror of the function-body path at line 55.
                        maybe_wrap_in_succeed(*inner_body, &awaitable_ret, &succeed)
                    };

                    // Recurse into the desugared body in case it contains more async closures
                    let desugared = defer_async_body(
                        walk_expr_for_async_closures(desugared), &awaitable_ret, "<closure>",
                    );

                    TypedExprKind::Closure {
                        params,
                        body: Box::new(desugared),
                        captures,
                    }
                }
                _ => {
                    // Non-async closure — still walk the body for nested async closures
                    let body = TypedExpr { kind: body.kind, ty: body.ty, span: body.span };
                    TypedExprKind::Closure {
                        params,
                        body: Box::new(walk_expr_for_async_closures(body)),
                        captures,
                    }
                }
            }
        }

        // === Recursive cases (same pattern as desugar_try.rs walk_expr) ===

        TypedExprKind::Block(exprs) => {
            TypedExprKind::Block(exprs.into_iter().map(walk_expr_for_async_closures).collect())
        }

        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(walk_expr_for_async_closures(*message)),
        },

        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(walk_expr_for_async_closures(*condition)),
            message: message.map(|m| Box::new(walk_expr_for_async_closures(*m))),
        },

        TypedExprKind::Let {
            name, mutable, boxed, var_ty, value,
        } => TypedExprKind::Let {
            name, mutable, boxed, var_ty,
            value: Box::new(walk_expr_for_async_closures(*value)),
        },

        TypedExprKind::Assign {
            name, target_ty, boxed, value,
        } => TypedExprKind::Assign {
            name, target_ty, boxed,
            value: Box::new(walk_expr_for_async_closures(*value)),
        },

        TypedExprKind::GlobalAssign { name, type_params, value } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(walk_expr_for_async_closures(*value)),
        },

        TypedExprKind::FunctionCall { name, args, type_params } => TypedExprKind::FunctionCall {
            name,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
            type_params,
        },

        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(walk_expr_for_async_closures(*left)),
            right: Box::new(walk_expr_for_async_closures(*right)),
        },

        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(walk_expr_for_async_closures(*operand)),
        },

        TypedExprKind::If { condition, then_branch, else_branch } => TypedExprKind::If {
            condition: Box::new(walk_expr_for_async_closures(*condition)),
            then_branch: Box::new(walk_expr_for_async_closures(*then_branch)),
            else_branch: else_branch.map(|e| Box::new(walk_expr_for_async_closures(*e))),
        },

        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(walk_expr_for_async_closures(*condition)),
            body: Box::new(walk_expr_for_async_closures(*body)),
        },

        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(walk_expr_for_async_closures(*subject)),
            arms: arms.into_iter().map(|arm| crate::typechecker::types::TypedMatchArm {
                body: Box::new(walk_expr_for_async_closures(*arm.body)),
                ..arm
            }).collect(),
        },

        TypedExprKind::RecordCreate { fqn, fields, type_params } => TypedExprKind::RecordCreate {
            fqn,
            type_params,
            fields: fields.into_iter().map(|(name, expr)| (name, walk_expr_for_async_closures(expr))).collect(),
        },

        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements.into_iter().map(walk_expr_for_async_closures).collect(),
        },

        TypedExprKind::EnumCreate { fqn, variant_name, args, type_params } => TypedExprKind::EnumCreate {
            fqn, variant_name,
            type_params,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },

        TypedExprKind::EnumVariantRecordCreate { fqn, variant_name, args, type_params } => TypedExprKind::EnumVariantRecordCreate {
            fqn, variant_name,
            type_params,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },

        TypedExprKind::FieldAccess { object, field_name, field_index, boxed } => TypedExprKind::FieldAccess {
            object: Box::new(walk_expr_for_async_closures(*object)),
            field_name, field_index, boxed,
        },

        TypedExprKind::FieldAssign { object, field_name, field_index, value, boxed } => TypedExprKind::FieldAssign {
            object: Box::new(walk_expr_for_async_closures(*object)),
            field_name, field_index, boxed,
            value: Box::new(walk_expr_for_async_closures(*value)),
        },

        TypedExprKind::RecordWith { object, fqn, overrides, type_params } => TypedExprKind::RecordWith {
            object: Box::new(walk_expr_for_async_closures(*object)),
            fqn,
            type_params,
            overrides: overrides.into_iter().map(|(name, idx, expr)| (name, idx, walk_expr_for_async_closures(expr))).collect(),
        },

        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements.into_iter().map(walk_expr_for_async_closures).collect(),
        },

        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },

        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(walk_expr_for_async_closures(*value)),
            target_type,
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(walk_expr_for_async_closures(*value)),
            target_type,
        },
        TypedExprKind::LetDestructure { pattern, var_ty, value } => TypedExprKind::LetDestructure {
            pattern, var_ty,
            value: Box::new(walk_expr_for_async_closures(*value)),
        },
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(walk_expr_for_async_closures(*value)),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(walk_expr_for_async_closures(*value)),
        },
        TypedExprKind::InterfaceObjectCoerce { inner, interface_mangled_name, concrete_type, vtable_methods } => TypedExprKind::InterfaceObjectCoerce {
            inner: Box::new(walk_expr_for_async_closures(*inner)),
            interface_mangled_name, concrete_type, vtable_methods,
        },
        TypedExprKind::TemplateInterfaceObjectCoerce { inner, traits, concrete_type } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: Box::new(walk_expr_for_async_closures(*inner)),
            traits, concrete_type,
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(walk_expr_for_async_closures(*inner)),
        },
        TypedExprKind::InterfaceObjectMethodCall { interface_mangled_name, method_name, member_name, receiver, args } => TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name, method_name, member_name,
            receiver: Box::new(walk_expr_for_async_closures(*receiver)),
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },
        TypedExprKind::ClassNew { mangled_name, args, type_params } => TypedExprKind::ClassNew {
            mangled_name,
            type_params,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },
        TypedExprKind::ClassStructCreate { target_mangled_name, fields, type_params } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields.into_iter().map(walk_expr_for_async_closures).collect(),
        },
        TypedExprKind::ClassVirtualCall { object, vtable_slot, args } => TypedExprKind::ClassVirtualCall {
            object: Box::new(walk_expr_for_async_closures(*object)),
            vtable_slot,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },
        TypedExprKind::ClassSuperCall { method_mangled, args } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(walk_expr_for_async_closures(*value)),
            return_type,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(walk_expr_for_async_closures(*callee)),
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
        },
        TypedExprKind::MethodRef { object, method_name, type_params } => TypedExprKind::MethodRef {
            object: Box::new(walk_expr_for_async_closures(*object)),
            method_name,
            type_params,
        },

        TypedExprKind::Await { operand, return_type, and_then_method, map_method, source_location_mn } => TypedExprKind::Await {
            operand: Box::new(walk_expr_for_async_closures(*operand)),
            return_type, and_then_method, map_method, source_location_mn,
        },

        TypedExprKind::Try { operand, unwrap_method, unwrap_return_type, return_type, from_method } => TypedExprKind::Try {
            operand: Box::new(walk_expr_for_async_closures(*operand)),
            unwrap_method, unwrap_return_type, return_type, from_method,
        },

        TypedExprKind::Use { operand, inner_type, source_error, target_error, from_method } => TypedExprKind::Use {
            operand: Box::new(walk_expr_for_async_closures(*operand)),
            inner_type, source_error, target_error, from_method,
        },

        TypedExprKind::AsyncBlock { body, succeed_method } => TypedExprKind::AsyncBlock {
            body: Box::new(walk_expr_for_async_closures(*body)),
            succeed_method,
        },

        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before desugar_await pass")
        }

        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: Box::new(walk_expr_for_async_closures(*inner)),
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
        | TypedExprKind::Continue) => kind,

        TypedExprKind::ImplFunctionCall { trait_fqn, trait_type_params, for_type, method_name, args, method_type_params } => TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
            method_type_params,
        },
        kind @ TypedExprKind::ImplFunctionRef { .. } => kind,
        TypedExprKind::ExtFunctionCall { ext_fqn, for_type, method_name, args, type_params } => TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args: args.into_iter().map(walk_expr_for_async_closures).collect(),
            type_params,
        },
        kind @ TypedExprKind::ExtFunctionRef { .. } => kind,
    };

    TypedExpr { kind, ty, span }
}

fn wrap_in_succeed(body: TypedExpr, return_type: &Type, succeed_method: &Option<ResolvedImplMethod>) -> TypedExpr {
    let succeed = match succeed_method {
        Some(m) => m.clone(),
        None => return body,
    };

    let span = body.span.clone();
    TypedExpr {
        kind: TypedExprKind::ImplFunctionCall {
            trait_fqn: succeed.trait_fqn,
            trait_type_params: succeed.trait_type_params,
            for_type: succeed.for_type,
            method_name: succeed.method_name,
            args: vec![body],
            method_type_params: succeed.method_type_params,
        },
        ty: return_type.clone(),
        span,
    }
}

/// Desugar a body that contains await expressions.
fn desugar_body(body: TypedExpr, return_type: &Type, succeed_method: &Option<ResolvedImplMethod>, function_name: &str) -> TypedExpr {
    match body.kind {
        TypedExprKind::Block(stmts) => desugar_stmts(stmts, return_type, body.span, succeed_method, function_name),
        _ => {
            // Single expression body — wrap in a vec
            desugar_stmts(vec![body.clone()], return_type, body.span, succeed_method, function_name)
        }
    }
}

/// Data extracted from an Await node by `extract_first_await`. Carries just
/// enough info to bind the awaited value to a fresh temp and continue the
/// surrounding expression. The new linear desugar reconstructs trait method
/// resolutions on demand via `synth_awaitable_method`, so the original Await's
/// pre-resolved `and_then`/`map`/`source_location_mn` are no longer threaded
/// through.
struct AwaitData {
    /// The expression whose value the continuation binds. For an `await`, this
    /// is the Async-valued operand directly. For a hoisted control-flow
    /// expression (`needs_lift == true`), this is the whole `If`/`Match`/`While`
    /// node, which `lower_expr_to_async` lifts to Async before chaining.
    operand: TypedExpr,
    /// The unwrapped value type (Await's outer `ty`).
    inner_type: Type,
    /// A fresh `VarName` already produced for the replacement `VarRef`.
    /// `lower_expr_to_async` reuses it as the andThen closure's parameter name.
    var_name: Option<VarName>,
    /// When true, `operand` is a control-flow expression (`If`/`Match`/`While`)
    /// that still contains awaits and must be lifted to `Async<inner_type, E>`
    /// via the branching machinery before it can be used as the andThen stem.
    /// When false, `operand` is already an Async-valued expression.
    needs_lift: bool,
}

/// Counter for generating unique temp variable names.
static AWAIT_COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

fn fresh_await_var() -> VarName {
    let n = AWAIT_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    VarName(format!("$await_{}", n))
}

/// Build a SourceLocation record expression from an await span and function name.
fn build_source_location(await_span: &Span, function_name: &str, source_location_mn: &MangledName) -> TypedExpr {
    let source_location_fqn = Fqn::from_dotted("standard.prelude.SourceLocation").unwrap();
    let source_location_type = Type::Record(source_location_fqn.clone(), source_location_mn.clone());
    TypedExpr {
        kind: TypedExprKind::RecordCreate {
            fqn: source_location_fqn,
            fields: vec![
                ("file".to_string(), TypedExpr {
                    kind: TypedExprKind::StringLiteral(await_span.file.to_string()),
                    ty: Type::String,
                    span: synthetic_span(),
                }),
                ("line".to_string(), TypedExpr {
                    kind: TypedExprKind::Int32Literal(await_span.line as i32),
                    ty: Type::Int32,
                    span: synthetic_span(),
                }),
                ("column".to_string(), TypedExpr {
                    kind: TypedExprKind::Int32Literal(await_span.column as i32),
                    ty: Type::Int32,
                    span: synthetic_span(),
                }),
                ("functionName".to_string(), TypedExpr {
                    kind: TypedExprKind::StringLiteral(function_name.to_string()),
                    ty: Type::String,
                    span: synthetic_span(),
                }),
            ],
            type_params: vec![],
        },
        ty: source_location_type,
        span: synthetic_span(),
    }
}

/// Desugar a list of statements containing await expressions.
/// Linear CPS-style desugar: walk stmts left-to-right; non-await stmts pile into
/// `before`; on the first await-containing stmt, recursively desugar the rest as
/// the Async-valued continuation, then dispatch to `lower_await_stmt` to bind
/// this stmt's await(s) and chain the continuation via `andThen`.
fn desugar_stmts(
    stmts: Vec<TypedExpr>,
    return_type: &Type,
    span: Span,
    succeed_method: &Option<ResolvedImplMethod>,
    function_name: &str,
) -> TypedExpr {
    let mut before: Vec<TypedExpr> = Vec::new();
    let mut iter = stmts.into_iter();
    while let Some(stmt) = iter.next() {
        if !contains_await(&stmt) {
            before.push(stmt);
            continue;
        }
        // Found the first await-containing statement. The remaining stmts form
        // `rest`; recursively desugar them to produce the Async continuation
        // (Async<FnT, E>), unless `rest` is empty (this stmt is at the tail).
        let rest: Vec<TypedExpr> = iter.collect();
        let continuation = if rest.is_empty() {
            None
        } else {
            Some(desugar_stmts(rest, return_type, span.clone(), succeed_method, function_name))
        };
        let async_chain = lower_await_stmt(
            stmt, continuation, return_type, succeed_method, function_name,
        );
        return prepend_before(before, async_chain, return_type, span);
    }
    // No awaits remain. Only resource-use lowering may have already lifted
    // the final result. Ordinary Awaitable values still need succeed, even
    // when their type equals the context (possible with an Any success type).
    let block_ty = before.last().map(|s| s.ty.clone()).unwrap_or(Type::Unit);
    let already_async = before.last().is_some_and(|last| {
        is_resource_use_result(last) && (block_ty == *return_type || already_lifted(&block_ty, return_type))
    });
    let block = TypedExpr {
        kind: TypedExprKind::Block(before),
        ty: block_ty,
        span: span.clone(),
    };
    if already_async {
        block
    } else {
        wrap_in_succeed(block, return_type, succeed_method)
    }
}

/// Wrap `tail` (an Async-valued expression) with the pure prefix `before`,
/// producing a Block when there's a prefix.
fn prepend_before(
    before: Vec<TypedExpr>,
    tail: TypedExpr,
    return_type: &Type,
    span: Span,
) -> TypedExpr {
    if before.is_empty() {
        tail
    } else {
        let mut all_stmts = before;
        all_stmts.push(tail);
        TypedExpr {
            kind: TypedExprKind::Block(all_stmts),
            ty: return_type.clone(),
            span,
        }
    }
}

/// Lower an await-containing statement to an Async chain that produces
/// `Async<FnT, E>` (where FnT is the outer function's success type), running
/// the statement's effects and then chaining `continuation` via `andThen`.
/// When `continuation` is `None`, this statement is at the tail and no chaining
/// is needed.
fn lower_await_stmt(
    stmt: TypedExpr,
    continuation: Option<TypedExpr>,
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    function_name: &str,
) -> TypedExpr {
    let stmt_span = stmt.span.clone();
    let stmt_ty = stmt.ty.clone();
    match stmt.kind {
        // Bare expression-statement Await: `await foo`.
        TypedExprKind::Await { operand, .. } => {
            let operand_async = *operand;
            match continuation {
                None => operand_async,
                Some(cont) => build_and_then(
                    operand_async, VarName("$tail_await".into()), stmt_ty,
                    cont, return_type, succeed_method, function_name, stmt_span,
                ),
            }
        }

        // `let x = await foo` — directly bind the await's value.
        TypedExprKind::Let { name, mutable, boxed, var_ty, value }
            if matches!(value.kind, TypedExprKind::Await { .. }) =>
        {
            let operand = match value.kind {
                TypedExprKind::Await { operand, .. } => *operand,
                _ => unreachable!(),
            };
            let cont = continuation.unwrap_or_else(|| {
                synth_unit_succeed(return_type, succeed_method, stmt_span.clone())
            });
            if mutable {
                // A mutable binding must stay a real `let mutable` so capture
                // analysis boxes it — otherwise later reassignments (e.g. in a
                // `while` body) are invisible across await suspension points,
                // because the andThen continuation param is immutable and never
                // boxed. Bind the await result to a fresh param and re-introduce
                // the mutable let at the head of the continuation.
                let tmp = VarName(format!("$mut_init${}", name.0));
                let tmp_ref = TypedExpr {
                    kind: TypedExprKind::VarRef { name: tmp.clone(), boxed: false },
                    ty: var_ty.clone(),
                    span: stmt_span.clone(),
                };
                let mutable_let = TypedExpr {
                    kind: TypedExprKind::Let {
                        name, mutable: true, boxed, var_ty: var_ty.clone(),
                        value: Box::new(tmp_ref),
                    },
                    ty: Type::Unit,
                    span: stmt_span.clone(),
                };
                let cont_ty = cont.ty.clone();
                let cont_span = cont.span.clone();
                let wrapped_cont = TypedExpr {
                    kind: TypedExprKind::Block(vec![mutable_let, cont]),
                    ty: cont_ty,
                    span: cont_span,
                };
                build_and_then(
                    operand, tmp, var_ty,
                    wrapped_cont, return_type, succeed_method, function_name, stmt_span,
                )
            } else {
                build_and_then(
                    operand, name, var_ty,
                    cont, return_type, succeed_method, function_name, stmt_span,
                )
            }
        }

        // `let x = <expr with await inside>` — recursively desugar value into
        // an Async<var_ty, E>, then chain the let-binding into the continuation.
        TypedExprKind::Let { name, mutable: _, boxed: _, var_ty, value } => {
            let value_async = lower_expr_to_async(
                *value, &var_ty, return_type, succeed_method, function_name,
            );
            let cont = continuation.unwrap_or_else(|| {
                synth_unit_succeed(return_type, succeed_method, stmt_span.clone())
            });
            build_and_then(
                value_async, name, var_ty,
                cont, return_type, succeed_method, function_name, stmt_span,
            )
        }

        // Keep destructured bindings in the same continuation as their uses.
        TypedExprKind::LetDestructure { pattern, var_ty, value } => {
            let value_async = lower_expr_to_async(
                *value, &var_ty, return_type, succeed_method, function_name,
            );
            let name = VarName("$destructure_result".into());
            let binding = TypedExpr {
                kind: TypedExprKind::LetDestructure {
                    pattern,
                    var_ty: var_ty.clone(),
                    value: Box::new(TypedExpr {
                        kind: TypedExprKind::VarRef { name: name.clone(), boxed: false },
                        ty: var_ty.clone(),
                        span: stmt_span.clone(),
                    }),
                },
                ty: Type::Unit,
                span: stmt_span.clone(),
            };
            let cont = continuation.unwrap_or_else(|| {
                synth_unit_succeed(return_type, succeed_method, stmt_span.clone())
            });
            let body = prepend_before(vec![binding], cont, return_type, stmt_span.clone());
            build_and_then(
                value_async, name, var_ty,
                body, return_type, succeed_method, function_name, stmt_span,
            )
        }

        // `while cond do body` where body contains await.
        TypedExprKind::While { condition, body } => {
            if contains_await(&condition) {
                panic!(
                    "await in while-loop condition is not supported (at {}:{}:{}). \
                     Move the awaiting expression into the body or use Async.whileLoop directly.",
                    stmt_span.file, stmt_span.line, stmt_span.column,
                );
            }
            let while_call = build_while_loop_call(
                *condition, *body, return_type, succeed_method, function_name, stmt_span.clone(),
            );
            match continuation {
                None => while_call,
                Some(cont) => build_and_then(
                    while_call, VarName("$while_unit".into()), Type::Unit,
                    cont, return_type, succeed_method, function_name, stmt_span,
                ),
            }
        }

        // `match` / `if` at statement level with await inside arms/branches.
        TypedExprKind::Match { .. } | TypedExprKind::If { .. } => {
            let inner_ty = stmt_ty.clone();
            let stmt_rebuilt = TypedExpr { kind: stmt.kind, ty: stmt_ty.clone(), span: stmt_span.clone() };
            let lifted = lift_branching_to_async(
                stmt_rebuilt, &inner_ty, return_type, succeed_method, function_name,
            );
            match continuation {
                None => lifted,
                Some(cont) => build_and_then(
                    lifted, VarName("$branch_val".into()), inner_ty,
                    cont, return_type, succeed_method, function_name, stmt_span,
                ),
            }
        }

        // Any other expression-shaped statement (FunctionCall, ImplFunctionCall,
        // BinaryOp, etc.) — lift its inner await(s) to an Async chain producing
        // its value type, then chain the continuation.
        _ => {
            let inner_ty = stmt_ty.clone();
            let stmt_rebuilt = TypedExpr { kind: stmt.kind, ty: stmt_ty, span: stmt_span.clone() };
            let stmt_async = lower_expr_to_async(
                stmt_rebuilt, &inner_ty, return_type, succeed_method, function_name,
            );
            match continuation {
                None => stmt_async,
                Some(cont) => build_and_then(
                    stmt_async, VarName("$expr_val".into()), inner_ty,
                    cont, return_type, succeed_method, function_name, stmt_span,
                ),
            }
        }
    }
}

/// Lower an expression containing await(s) into an Async-valued expression.
/// For pure (no-await) expressions, wraps in `succeed`. For `Await` itself,
/// returns its operand directly. For `Match`/`If`, uses the branching lift.
/// Otherwise, hoists the first inner await via `extract_first_await` and
/// recursively lowers the remaining expression.
fn lower_expr_to_async(
    expr: TypedExpr,
    inner_ty: &Type,
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    function_name: &str,
) -> TypedExpr {
    let span = expr.span.clone();
    let async_template = outer_awaitable_context(return_type, succeed_method);
    let async_inner_ty = super::awaitable::rebind(async_template, inner_ty.clone());

    if !contains_await(&expr) {
        let inner_succeed = synth_awaitable_method(
            "succeed", async_inner_ty.clone(), inner_ty.clone(),
            vec![],
        );
        return wrap_in_succeed(expr, &async_inner_ty, &Some(inner_succeed));
    }

    // The expression IS an Await — its operand is the Async directly.
    if let TypedExprKind::Await { operand, .. } = expr.kind {
        return *operand;
    }

    // Match/If with await inside — full branching lift.
    if matches!(expr.kind, TypedExprKind::Match { .. } | TypedExprKind::If { .. }) {
        return lift_branching_to_async(expr, inner_ty, return_type, succeed_method, function_name);
    }

    // A Block value (`let x = <multi-statement block with awaits>`) is a small
    // function body: lower it STATEMENT-WISE, exactly like `desugar_body` does
    // for the whole function, targeting `Async<inner_ty, E>` instead of the
    // function's return type. The general hoisting case below is WRONG for
    // blocks: `extract_first_await` pulls an await's operand out to become the
    // andThen stem while every preceding statement moves into the continuation
    // closure — so an operand referencing a `let` bound earlier in the block
    // (`let a = await f()` then `await g(a)`) is evaluated before its binding
    // exists ("undefined local" at codegen).
    if let TypedExprKind::Block(stmts) = expr.kind {
        let inner_succeed = synth_awaitable_method(
            "succeed", async_inner_ty.clone(), inner_ty.clone(),
            vec![],
        );
        return desugar_stmts(stmts, &async_inner_ty, span, &Some(inner_succeed), function_name);
    }

    // General case: hoist the first await inside this expression, bind via
    // andThen, and recursively lower the remainder.
    let (await_data, modified_expr) = extract_first_await(expr);
    let await_data = await_data.unwrap_or_else(|| {
        panic!(
            "lower_expr_to_async: contains_await=true but extract_first_await found none. \
             Missing arm in extract_first_await for kind: {:?}",
            modified_expr.kind
        )
    });
    let bind_name = await_data.var_name.unwrap_or_else(fresh_await_var);
    let bound_ty = await_data.inner_type;
    // For a plain await, `operand` is already Async-valued. For a hoisted
    // control-flow expression it is the raw `If`/`Match`/`While` node, which we
    // must lift to `Async<bound_ty, E>` before using it as the andThen stem.
    let operand = if await_data.needs_lift {
        lift_control_flow_to_async(
            await_data.operand, &bound_ty, return_type, succeed_method, function_name,
        )
    } else {
        await_data.operand
    };
    // The modified_expr now has a VarRef where the await was; lift it into
    // Async<inner_ty, E> via recursive call. The recursive call's return_type
    // is the SAME outer Async<FnT, E> so build_and_then knows the closure's
    // result type, but the closure body's actual type is async_inner_ty.
    // We synthesize a local andThen tied to async_inner_ty.
    let cont = lower_expr_to_async(modified_expr, inner_ty, return_type, succeed_method, function_name);
    build_and_then_typed(
        operand, bind_name, bound_ty,
        cont, &async_inner_ty, succeed_method, function_name, span,
    )
}

/// Lift a `Match` or `If` (with await in some arm/branch) into an
/// Async-valued expression. Each arm/branch body is recursively desugared
/// into `Async<inner_ty, E>`; the resulting Match/If has type `Async<inner_ty, E>`.
fn lift_branching_to_async(
    expr: TypedExpr,
    inner_ty: &Type,
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    function_name: &str,
) -> TypedExpr {
    let span = expr.span.clone();
    let async_template = outer_awaitable_context(return_type, succeed_method);
    let async_inner_ty = super::awaitable::rebind(async_template, inner_ty.clone());
    // Block-level type args for the impl `Awaitable<inner_ty> for Async<inner_ty, E>`.
    let inner_succeed = synth_awaitable_method(
        "succeed", async_inner_ty.clone(), inner_ty.clone(),
        vec![],
    );
    let inner_succeed_opt = Some(inner_succeed);

    // Evaluate an effectful subject/condition once, before choosing a branch.
    // Lifting only the arms leaves these Await nodes in otherwise lowered code.
    let head = match &expr.kind {
        TypedExprKind::Match { subject, .. } => subject.as_ref(),
        TypedExprKind::If { condition, .. } => condition.as_ref(),
        _ => unreachable!("lift_branching_to_async called on non-branching expr"),
    };
    if contains_await(head) {
        let bind_name = fresh_await_var();
        let bind_ty = head.ty.clone();
        let replacement = TypedExpr {
            kind: TypedExprKind::VarRef { name: bind_name.clone(), boxed: false },
            ty: bind_ty.clone(),
            span: head.span.clone(),
        };
        let (head, continuation_kind) = match expr.kind {
            TypedExprKind::Match { subject, arms } => (
                *subject,
                TypedExprKind::Match { subject: Box::new(replacement), arms },
            ),
            TypedExprKind::If { condition, then_branch, else_branch } => (
                *condition,
                TypedExprKind::If {
                    condition: Box::new(replacement),
                    then_branch,
                    else_branch,
                },
            ),
            _ => unreachable!("lift_branching_to_async called on non-branching expr"),
        };
        let continuation_expr = TypedExpr {
            kind: continuation_kind,
            ty: expr.ty,
            span: expr.span,
        };
        let operand = lower_expr_to_async(
            head, &bind_ty, return_type, succeed_method, function_name,
        );
        let continuation = lift_branching_to_async(
            continuation_expr, inner_ty, return_type, succeed_method, function_name,
        );
        return build_and_then(
            operand, bind_name, bind_ty, continuation, &async_inner_ty,
            &inner_succeed_opt, function_name, span,
        );
    }

    let lift_body = |body: TypedExpr| -> TypedExpr {
        if contains_await(&body) {
            desugar_body(body, &async_inner_ty, &inner_succeed_opt, function_name)
        } else {
            // `maybe_wrap_in_succeed` (not `wrap_in_succeed`) so an arm body
            // that's already Async-valued — e.g. a `Usable.use(...)` call
            // synthesized by `desugar_use` — doesn't get double-wrapped.
            // Without this, a match arm whose body is a `use` block produces
            // `Async<Async<T, E>, ...>` and the resulting heterogeneous arm
            // types cause a WASM-load failure at codegen.
            maybe_wrap_in_succeed(body, &async_inner_ty, &inner_succeed_opt)
        }
    };

    match expr.kind {
        TypedExprKind::Match { subject, arms } => {
            let new_arms: Vec<TypedMatchArm> = arms.into_iter().map(|arm| {
                let arm_span = arm.span.clone();
                let new_body = lift_body(*arm.body);
                TypedMatchArm {
                    pattern: arm.pattern,
                    guard: arm.guard,
                    body: Box::new(new_body),
                    span: arm_span,
                }
            }).collect();
            TypedExpr {
                kind: TypedExprKind::Match { subject, arms: new_arms },
                ty: async_inner_ty,
                span,
            }
        }
        TypedExprKind::If { condition, then_branch, else_branch } => {
            let new_then = Box::new(lift_body(*then_branch));
            // If there's no else: the original expression's value type is Unit
            // (only when else is omitted). Synthesize `else => succeed(())`.
            let new_else = match else_branch {
                Some(e) => Some(Box::new(lift_body(*e))),
                None => {
                    let unit_expr = TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Unit,
                        span: span.clone(),
                    };
                    Some(Box::new(wrap_in_succeed(unit_expr, &async_inner_ty, &inner_succeed_opt)))
                }
            };
            TypedExpr {
                kind: TypedExprKind::If {
                    condition,
                    then_branch: new_then,
                    else_branch: new_else,
                },
                ty: async_inner_ty,
                span,
            }
        }
        _ => unreachable!("lift_branching_to_async called on non-branching expr"),
    }
}

/// Build a `Awaitable<bind_ty>.whileLoop(cond_thunk, body_thunk, trace)` call
/// on `Async<Unit, E>`. The body is recursively desugared into Async<Unit, E>.
fn build_while_loop_call(
    condition: TypedExpr,
    body: TypedExpr,
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    function_name: &str,
    span: Span,
) -> TypedExpr {
    let async_template = outer_awaitable_context(return_type, succeed_method);
    let async_unit_ty = super::awaitable::rebind(async_template, Type::Unit);
    let succeed_unit = synth_awaitable_method(
        "succeed", async_unit_ty.clone(), Type::Unit,
        vec![],
    );
    let body_as_async = if contains_await(&body) {
        desugar_body(body, &async_unit_ty, &Some(succeed_unit.clone()), function_name)
    } else {
        wrap_in_succeed(body, &async_unit_ty, &Some(succeed_unit))
    };
    let cond_span = condition.span.clone();
    let cond_thunk = TypedExpr {
        kind: TypedExprKind::Closure {
            params: vec![],
            body: Box::new(condition),
            captures: Vec::new(),
        },
        ty: Type::Function(vec![], Box::new(Type::Bool)),
        span: cond_span,
    };
    let body_span = body_as_async.span.clone();
    let body_thunk = TypedExpr {
        kind: TypedExprKind::Closure {
            params: vec![],
            body: Box::new(body_as_async),
            captures: Vec::new(),
        },
        ty: Type::Function(vec![], Box::new(async_unit_ty.clone())),
        span: body_span,
    };
    let source_location_fqn = Fqn::from_dotted("standard.prelude.SourceLocation").unwrap();
    let source_location_mn = MangledName::for_type(&source_location_fqn);
    let trace = build_source_location(&span, function_name, &source_location_mn);
    let awaitable_fqn = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
    let block_args = super::awaitable::method("whileLoop", async_unit_ty.clone(), Type::Unit, vec![]).method_type_params;
    TypedExpr {
        kind: TypedExprKind::ImplFunctionCall {
            trait_fqn: awaitable_fqn,
            trait_type_params: vec![Type::Unit],
            for_type: async_unit_ty.clone(),
            method_name: SymbolName("whileLoop".to_string()),
            args: vec![cond_thunk, body_thunk, trace],
            method_type_params: block_args,
        },
        ty: async_unit_ty,
        span,
    }
}

/// Build `Awaitable<bind_ty>.andThen(operand, (bind_name: bind_ty) => continuation, trace)`.
/// The continuation must itself be Async-valued (typed as `return_type`).
fn build_and_then(
    operand: TypedExpr,
    bind_name: VarName,
    bind_ty: Type,
    continuation: TypedExpr,
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    function_name: &str,
    span: Span,
) -> TypedExpr {
    build_and_then_typed(operand, bind_name, bind_ty, continuation, return_type, succeed_method, function_name, span)
}

/// Like `build_and_then` but with an explicit `result_type` for the closure's
/// success type and the andThen call's result. Used when chaining into a
/// continuation typed differently from the outer function's return type
/// (e.g., inside `lower_expr_to_async` where the chain produces `Async<inner_ty, E>`).
fn build_and_then_typed(
    operand: TypedExpr,
    bind_name: VarName,
    bind_ty: Type,
    continuation: TypedExpr,
    result_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    _function_name: &str,
    span: Span,
) -> TypedExpr {
    let async_template = outer_awaitable_context(result_type, succeed_method);
    let async_bound_ty = super::awaitable::rebind(async_template, bind_ty.clone());

    let source_location_fqn = Fqn::from_dotted("standard.prelude.SourceLocation").unwrap();
    let source_location_mn = MangledName::for_type(&source_location_fqn);
    let trace = build_source_location(&span, _function_name, &source_location_mn);

    let closure_param = TypedClosureParam {
        name: bind_name,
        ty: bind_ty.clone(),
        span: span.clone(),
    };
    let closure_ty = Type::Function(vec![bind_ty.clone()], Box::new(result_type.clone()));
    let closure = TypedExpr {
        kind: TypedExprKind::Closure {
            params: vec![closure_param],
            body: Box::new(continuation),
            captures: Vec::new(),
        },
        ty: closure_ty,
        span: span.clone(),
    };

    let awaitable_fqn = Fqn::from_dotted("standard.prelude.Awaitable").unwrap();
    let result_success = super::awaitable::success_type(async_template).expect("checked Awaitable context");
    let method_type_params = super::awaitable::method(
        "andThen", async_bound_ty.clone(), bind_ty.clone(), vec![result_success],
    ).method_type_params;

    TypedExpr {
        kind: TypedExprKind::ImplFunctionCall {
            trait_fqn: awaitable_fqn,
            trait_type_params: vec![bind_ty],
            for_type: async_bound_ty,
            method_name: SymbolName("andThen".to_string()),
            args: vec![operand, closure, trace],
            method_type_params,
        },
        ty: result_type.clone(),
        span,
    }
}

/// Build a synthetic `Async<Unit, E>.succeed(())` value matching the outer
/// function's E. Used when a let-await is the tail statement and needs to
/// produce a continuation that yields Unit.
fn synth_unit_succeed(
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    span: Span,
) -> TypedExpr {
    let async_template = outer_awaitable_context(return_type, succeed_method);
    let async_unit_ty = super::awaitable::rebind(async_template, Type::Unit);
    let unit_succeed = synth_awaitable_method(
        "succeed", async_unit_ty.clone(), Type::Unit,
        vec![],
    );
    let unit_expr = TypedExpr {
        kind: TypedExprKind::UnitLiteral,
        ty: Type::Unit,
        span: span.clone(),
    };
    wrap_in_succeed(unit_expr, &async_unit_ty, &Some(unit_succeed))
}

/// Resource-use lowering can leave an unwrapped continuation result type;
/// the resolved succeed method still carries the computation context.
fn outer_awaitable_context<'a>(
    return_type: &'a Type,
    succeed_method: &'a Option<ResolvedImplMethod>,
) -> &'a Type {
    if super::awaitable::success_type(return_type).is_some() {
        return return_type;
    }
    &succeed_method.as_ref().expect("checked Awaitable context").for_type
}

/// Check if a statement is a bare `await expr` (the await IS the entire statement).
/// Depth-first walk to find the first `Await` node, replacing it with a `VarRef`.
/// Returns the AwaitData and the modified expression.
fn extract_first_await(expr: TypedExpr) -> (Option<AwaitData>, TypedExpr) {
    match expr.kind {
        TypedExprKind::Await { operand, .. } => {
            let var_name = fresh_await_var();
            let inner_type = expr.ty.clone();
            let data = AwaitData {
                operand: *operand,
                inner_type: inner_type.clone(),
                var_name: Some(var_name.clone()),
                needs_lift: false,
            };
            let var_ref = TypedExpr {
                kind: TypedExprKind::VarRef {
                    name: var_name,
                    boxed: false,
                },
                ty: inner_type,
                span: expr.span,
            };
            (Some(data), var_ref)
        }

        // Recurse into sub-expressions to find nested awaits
        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => {
            let (data, new_value) = extract_first_await(*value);
            (
                data,
                TypedExpr {
                    kind: TypedExprKind::Let {
                        name,
                        mutable,
                        boxed,
                        var_ty,
                        value: Box::new(new_value),
                    },
                    ty: expr.ty,
                    span: expr.span,
                },
            )
        }

        TypedExprKind::BinaryOp { op, left, right } => {
            let (data, new_left) = extract_first_await(*left);
            if data.is_some() {
                return (
                    data,
                    TypedExpr {
                        kind: TypedExprKind::BinaryOp {
                            op,
                            left: Box::new(new_left),
                            right,
                        },
                        ty: expr.ty,
                        span: expr.span,
                    },
                );
            }
            let (data, new_right) = extract_first_await(*right);
            (
                data,
                TypedExpr {
                    kind: TypedExprKind::BinaryOp {
                        op,
                        left: Box::new(new_left),
                        right: Box::new(new_right),
                    },
                    ty: expr.ty,
                    span: expr.span,
                },
            )
        }

        TypedExprKind::UnaryOp { op, operand } => {
            let (data, new_operand) = extract_first_await(*operand);
            (
                data,
                TypedExpr {
                    kind: TypedExprKind::UnaryOp {
                        op,
                        operand: Box::new(new_operand),
                    },
                    ty: expr.ty,
                    span: expr.span,
                },
            )
        }

        TypedExprKind::FunctionCall { name, args, type_params } => {
            let mut new_args = Vec::new();
            let mut found_data = None;
            for arg in args {
                if found_data.is_some() {
                    new_args.push(arg);
                } else {
                    let (data, new_arg) = extract_first_await(arg);
                    new_args.push(new_arg);
                    found_data = data;
                }
            }
            (
                found_data,
                TypedExpr {
                    kind: TypedExprKind::FunctionCall { name, args: new_args, type_params },
                    ty: expr.ty,
                    span: expr.span,
                },
            )
        }

        TypedExprKind::EnumCreate { fqn, variant_name, args, type_params } => {
            let mut new_args = Vec::new();
            let mut found_data = None;
            for arg in args {
                if found_data.is_some() {
                    new_args.push(arg);
                } else {
                    let (data, new_arg) = extract_first_await(arg);
                    new_args.push(new_arg);
                    found_data = data;
                }
            }
            (
                found_data,
                TypedExpr {
                    kind: TypedExprKind::EnumCreate { fqn, variant_name, args: new_args, type_params },
                    ty: expr.ty,
                    span: expr.span,
                },
            )
        }

        TypedExprKind::FieldAccess { object, field_name, field_index, boxed } => {
            let (data, new_object) = extract_first_await(*object);
            (
                data,
                TypedExpr {
                    kind: TypedExprKind::FieldAccess {
                        object: Box::new(new_object),
                        field_name,
                        field_index,
                        boxed,
                    },
                    ty: expr.ty,
                    span: expr.span,
                },
            )
        }

        // --- Single-child structural recursion ---

        TypedExprKind::NewtypeCreate { value } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::NewtypeCreate { value: Box::new(new_value) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::NewtypeValue { value } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::NewtypeValue { value: Box::new(new_value) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::BoxToAny { inner } => {
            let (data, new_inner) = extract_first_await(*inner);
            (data, TypedExpr {
                kind: TypedExprKind::BoxToAny { inner: Box::new(new_inner) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::TypeTest { value, target_type } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::TypeTest { value: Box::new(new_value), target_type },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::TypeCast { value, target_type } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::TypeCast { value: Box::new(new_value), target_type },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::Return { value, return_type } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::Return { value: Box::new(new_value), return_type },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::Panic { message } => {
            let (data, new_msg) = extract_first_await(*message);
            (data, TypedExpr {
                kind: TypedExprKind::Panic { message: Box::new(new_msg) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::Assert { condition, message } => {
            let (data, new_cond) = extract_first_await(*condition);
            if data.is_some() {
                return (data, TypedExpr {
                    kind: TypedExprKind::Assert { condition: Box::new(new_cond), message },
                    ty: expr.ty,
                    span: expr.span,
                });
            }
            match message {
                Some(msg) => {
                    let (data, new_msg) = extract_first_await(*msg);
                    (data, TypedExpr {
                        kind: TypedExprKind::Assert { condition: Box::new(new_cond), message: Some(Box::new(new_msg)) },
                        ty: expr.ty,
                        span: expr.span,
                    })
                }
                None => (None, TypedExpr {
                    kind: TypedExprKind::Assert { condition: Box::new(new_cond), message: None },
                    ty: expr.ty,
                    span: expr.span,
                }),
            }
        }

        TypedExprKind::MethodRef { object, method_name, type_params } => {
            let (data, new_object) = extract_first_await(*object);
            (data, TypedExpr {
                kind: TypedExprKind::MethodRef { object: Box::new(new_object), method_name, type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::InterfaceObjectCoerce { inner, interface_mangled_name, concrete_type, vtable_methods } => {
            let (data, new_inner) = extract_first_await(*inner);
            (data, TypedExpr {
                kind: TypedExprKind::InterfaceObjectCoerce { inner: Box::new(new_inner), interface_mangled_name, concrete_type, vtable_methods },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::TemplateInterfaceObjectCoerce { inner, traits, concrete_type } => {
            let (data, new_inner) = extract_first_await(*inner);
            (data, TypedExpr {
                kind: TypedExprKind::TemplateInterfaceObjectCoerce { inner: Box::new(new_inner), traits, concrete_type },
                ty: expr.ty,
                span: expr.span,
            })
        }
        TypedExprKind::InterfaceObjectUpcast { inner } => {
            let (data, new_inner) = extract_first_await(*inner);
            (data, TypedExpr {
                kind: TypedExprKind::InterfaceObjectUpcast { inner: Box::new(new_inner) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::Assign { name, target_ty, boxed, value } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::Assign { name, target_ty, boxed, value: Box::new(new_value) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::GlobalAssign { name, type_params, value } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::GlobalAssign { name, type_params, value: Box::new(new_value) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::LetDestructure { pattern, var_ty, value } => {
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::LetDestructure { pattern, var_ty, value: Box::new(new_value) },
                ty: expr.ty,
                span: expr.span,
            })
        }

        // --- Multi-child in-order recursion ---

        TypedExprKind::Block(stmts) => {
            let mut new_stmts = Vec::with_capacity(stmts.len());
            let mut found = None;
            for stmt in stmts {
                if found.is_some() {
                    new_stmts.push(stmt);
                } else {
                    let (d, ns) = extract_first_await(stmt);
                    new_stmts.push(ns);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::Block(new_stmts),
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ArrayLiteral { elements } => {
            let mut new_elements = Vec::with_capacity(elements.len());
            let mut found = None;
            for el in elements {
                if found.is_some() {
                    new_elements.push(el);
                } else {
                    let (d, ne) = extract_first_await(el);
                    new_elements.push(ne);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ArrayLiteral { elements: new_elements },
                ty: expr.ty,
                span: expr.span,
            })
        }

        // Tuples evaluate their elements left-to-right with no short-circuit,
        // so hoisting the first await out of a tuple literal is sound — mirror
        // the ArrayLiteral arm above.
        TypedExprKind::TupleLiteral { elements } => {
            let mut new_elements = Vec::with_capacity(elements.len());
            let mut found = None;
            for el in elements {
                if found.is_some() {
                    new_elements.push(el);
                } else {
                    let (d, ne) = extract_first_await(el);
                    new_elements.push(ne);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::TupleLiteral { elements: new_elements },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::IntrinsicCall { intrinsic, args } => {
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::IntrinsicCall { intrinsic, args: new_args },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ImplFunctionCall { trait_fqn, trait_type_params, for_type, method_name, args, method_type_params } => {
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ImplFunctionCall { trait_fqn, trait_type_params, for_type, method_name, args: new_args, method_type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ExtFunctionCall { ext_fqn, for_type, method_name, args, type_params } => {
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ExtFunctionCall { ext_fqn, for_type, method_name, args: new_args, type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ClassNew { mangled_name, args, type_params } => {
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ClassNew { mangled_name, args: new_args, type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ClassSuperCall { method_mangled, args } => {
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ClassSuperCall { method_mangled, args: new_args },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ClassStructCreate { target_mangled_name, fields, type_params } => {
            let mut new_fields = Vec::with_capacity(fields.len());
            let mut found = None;
            for f in fields {
                if found.is_some() {
                    new_fields.push(f);
                } else {
                    let (d, nf) = extract_first_await(f);
                    new_fields.push(nf);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ClassStructCreate { target_mangled_name, fields: new_fields, type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ClosureCall { callee, args } => {
            let (data, new_callee) = extract_first_await(*callee);
            if data.is_some() {
                return (data, TypedExpr {
                    kind: TypedExprKind::ClosureCall { callee: Box::new(new_callee), args },
                    ty: expr.ty,
                    span: expr.span,
                });
            }
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ClosureCall { callee: Box::new(new_callee), args: new_args },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::InterfaceObjectMethodCall { interface_mangled_name, method_name, member_name, receiver, args } => {
            let (data, new_receiver) = extract_first_await(*receiver);
            if data.is_some() {
                return (data, TypedExpr {
                    kind: TypedExprKind::InterfaceObjectMethodCall { interface_mangled_name, method_name, member_name, receiver: Box::new(new_receiver), args },
                    ty: expr.ty,
                    span: expr.span,
                });
            }
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::InterfaceObjectMethodCall { interface_mangled_name, method_name, member_name, receiver: Box::new(new_receiver), args: new_args },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::ClassVirtualCall { object, vtable_slot, args } => {
            let (data, new_object) = extract_first_await(*object);
            if data.is_some() {
                return (data, TypedExpr {
                    kind: TypedExprKind::ClassVirtualCall { object: Box::new(new_object), vtable_slot, args },
                    ty: expr.ty,
                    span: expr.span,
                });
            }
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::ClassVirtualCall { object: Box::new(new_object), vtable_slot, args: new_args },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::EnumVariantRecordCreate { fqn, variant_name, args, type_params } => {
            let mut new_args = Vec::with_capacity(args.len());
            let mut found = None;
            for arg in args {
                if found.is_some() {
                    new_args.push(arg);
                } else {
                    let (d, na) = extract_first_await(arg);
                    new_args.push(na);
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::EnumVariantRecordCreate { fqn, variant_name, args: new_args, type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::RecordCreate { fqn, fields, type_params } => {
            let mut new_fields = Vec::with_capacity(fields.len());
            let mut found = None;
            for (name, field_expr) in fields {
                if found.is_some() {
                    new_fields.push((name, field_expr));
                } else {
                    let (d, nf) = extract_first_await(field_expr);
                    new_fields.push((name, nf));
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::RecordCreate { fqn, fields: new_fields, type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::RecordWith { object, fqn, overrides, type_params } => {
            let (data, new_object) = extract_first_await(*object);
            if data.is_some() {
                return (data, TypedExpr {
                    kind: TypedExprKind::RecordWith { object: Box::new(new_object), fqn, overrides, type_params },
                    ty: expr.ty,
                    span: expr.span,
                });
            }
            let mut new_overrides = Vec::with_capacity(overrides.len());
            let mut found = None;
            for (name, idx, ov_expr) in overrides {
                if found.is_some() {
                    new_overrides.push((name, idx, ov_expr));
                } else {
                    let (d, no) = extract_first_await(ov_expr);
                    new_overrides.push((name, idx, no));
                    found = d;
                }
            }
            (found, TypedExpr {
                kind: TypedExprKind::RecordWith { object: Box::new(new_object), fqn, overrides: new_overrides, type_params },
                ty: expr.ty,
                span: expr.span,
            })
        }

        TypedExprKind::FieldAssign { object, field_name, field_index, value, boxed } => {
            let (data, new_object) = extract_first_await(*object);
            if data.is_some() {
                return (data, TypedExpr {
                    kind: TypedExprKind::FieldAssign { object: Box::new(new_object), field_name, field_index, value, boxed },
                    ty: expr.ty,
                    span: expr.span,
                });
            }
            let (data, new_value) = extract_first_await(*value);
            (data, TypedExpr {
                kind: TypedExprKind::FieldAssign { object: Box::new(new_object), field_name, field_index, value: Box::new(new_value), boxed },
                ty: expr.ty,
                span: expr.span,
            })
        }

        // Nested control-flow (`If`/`Match`/`While`) that itself contains an
        // await — e.g. `1 + (if c then await a() else await b())`. Hoisting an
        // await out of a single branch is unsound (it would run regardless of
        // the branch taken), so instead hoist the WHOLE branching expression:
        // bind its result via `andThen` and lift the branching to Async in
        // `lower_expr_to_async` (`needs_lift`). Only branchings that actually
        // contain an await are hoisted; await-free control flow falls through to
        // the catch-all and stays inline (so a later await in evaluation order
        // is still found). Top-level `If`/`Match` never reach here — they are
        // handled directly in `lower_expr_to_async` before `extract_first_await`.
        TypedExprKind::If { condition, then_branch, else_branch }
            if contains_await(&condition)
                || contains_await(&then_branch)
                || else_branch.as_ref().is_some_and(|e| contains_await(e))
                || is_resource_use_result(&then_branch)
                || else_branch.as_ref().is_some_and(|branch| is_resource_use_result(branch)) =>
        {
            let kind = TypedExprKind::If { condition, then_branch, else_branch };
            hoist_control_flow(kind, expr.ty, expr.span)
        }

        TypedExprKind::Match { subject, arms }
            if contains_await(&subject)
                || arms.iter().any(|a| contains_await(&a.body))
                || arms.iter().any(|a| is_resource_use_result(&a.body)) =>
        {
            let kind = TypedExprKind::Match { subject, arms };
            hoist_control_flow(kind, expr.ty, expr.span)
        }

        TypedExprKind::While { condition, body }
            if contains_await(&condition) || contains_await(&body) =>
        {
            let kind = TypedExprKind::While { condition, body };
            hoist_control_flow(kind, expr.ty, expr.span)
        }

        // For remaining expression types (await-free Match/If/While, Closure,
        // AsyncBlock, etc.), don't recurse here — handled at statement level by
        // `lower_await_stmt` or left inline.
        _ => (None, expr),
    }
}

/// Hoist a control-flow expression (`If`/`Match`/`While`) that contains an
/// await out of its surrounding expression: return `AwaitData` marked
/// `needs_lift` (so `lower_expr_to_async` lifts it to Async and binds it via
/// `andThen`) plus a `VarRef` to replace it in the enclosing expression.
fn hoist_control_flow(kind: TypedExprKind, ty: Type, span: Span) -> (Option<AwaitData>, TypedExpr) {
    let var_name = fresh_await_var();
    let data = AwaitData {
        operand: TypedExpr { kind, ty: ty.clone(), span: span.clone() },
        inner_type: ty.clone(),
        var_name: Some(var_name.clone()),
        needs_lift: true,
    };
    let var_ref = TypedExpr {
        kind: TypedExprKind::VarRef { name: var_name, boxed: false },
        ty,
        span,
    };
    (Some(data), var_ref)
}

/// Lift a hoisted control-flow expression to `Async<inner_ty, E>`. `If`/`Match`
/// go through `lift_branching_to_async`; `While` through `build_while_loop_call`.
fn lift_control_flow_to_async(
    expr: TypedExpr,
    inner_ty: &Type,
    return_type: &Type,
    succeed_method: &Option<ResolvedImplMethod>,
    function_name: &str,
) -> TypedExpr {
    if matches!(expr.kind, TypedExprKind::While { .. }) {
        let (condition, body, span) = match expr {
            TypedExpr { kind: TypedExprKind::While { condition, body }, span, .. } => {
                (*condition, *body, span)
            }
            _ => unreachable!(),
        };
        // Same restriction as the statement-level `while` lowering: an await in
        // the loop condition is not supported (the condition becomes a plain
        // `() => Bool` thunk with no place to suspend).
        if contains_await(&condition) {
            panic!(
                "await in while-loop condition is not supported (at {}:{}:{}). \
                 Move the awaiting expression into the body or use Async.whileLoop directly.",
                span.file, span.line, span.column,
            );
        }
        build_while_loop_call(condition, body, return_type, succeed_method, function_name, span)
    } else {
        lift_branching_to_async(expr, inner_ty, return_type, succeed_method, function_name)
    }
}

/// Resolve block parameters from the implementation, then append method parameters.
fn synth_awaitable_method(
    method_name: &str,
    for_type: Type,
    inner_ty: Type,
    method_type_params: Vec<Type>,
) -> ResolvedImplMethod {
    super::awaitable::method(method_name, for_type, inner_ty, method_type_params)
}

/// Recursively check if an expression contains any `Await` node.
fn contains_await(expr: &TypedExpr) -> bool {
    match &expr.kind {
        TypedExprKind::Await { .. } => true,
        TypedExprKind::Block(exprs) => exprs.iter().any(contains_await),
        TypedExprKind::Let { value, .. } => contains_await(value),
        TypedExprKind::Assign { value, .. } => contains_await(value),
        TypedExprKind::BinaryOp { left, right, .. } => {
            contains_await(left) || contains_await(right)
        }
        TypedExprKind::UnaryOp { operand, .. } => contains_await(operand),
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            contains_await(condition)
                || contains_await(then_branch)
                || else_branch.as_ref().is_some_and(|e| contains_await(e))
                // Same heterogeneous-branch rule as Match — if a branch was
                // use-rewritten to an Async value, lift the whole `if`.
                || is_resource_use_result(then_branch)
                || else_branch.as_ref().is_some_and(|e| is_resource_use_result(e))
        }
        TypedExprKind::While { condition, body } => {
            contains_await(condition) || contains_await(body)
        }
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::EnumCreate { args, .. }
        | TypedExprKind::ClassNew { args, .. } => args.iter().any(contains_await),
        TypedExprKind::Panic { message } => contains_await(message),
        TypedExprKind::Assert { condition, message } => {
            contains_await(condition)
                || message.as_ref().is_some_and(|m| contains_await(m))
        }
        TypedExprKind::FieldAccess { object, .. }
        | TypedExprKind::NewtypeCreate { value: object }
        | TypedExprKind::NewtypeValue { value: object }
        | TypedExprKind::BoxToAny { inner: object }
        | TypedExprKind::TypeTest { value: object, .. }
        | TypedExprKind::TypeCast { value: object, .. }
        | TypedExprKind::Return { value: object, .. }
        | TypedExprKind::MethodRef { object, .. } => contains_await(object),
        TypedExprKind::RecordCreate { fields, .. } => {
            fields.iter().any(|(_, e)| contains_await(e))
        }
        TypedExprKind::TupleLiteral { elements } => elements.iter().any(contains_await),
        TypedExprKind::Match { subject, arms } => {
            contains_await(subject)
                || arms.iter().any(|a| contains_await(&a.body))
                // After `desugar_use` rewrites a `use` inside a match arm, the
                // arm's body becomes a `Usable.use(...)` call with type
                // `Async<T, E>` while the match's overall type is still `T`.
                // Treat this as "needs branching lift" so `lift_branching_to_async`
                // unifies the arms in Async. Without this check the codegen
                // emits heterogeneous arm types and the WASM fails to load.
                || arms.iter().any(|a| is_resource_use_result(&a.body))
        }
        TypedExprKind::Closure { body, .. } => match &body.kind {
            TypedExprKind::AsyncBlock { .. } => false, // async closure's own awaits
            _ => contains_await(body),
        },
        TypedExprKind::ClosureCall { callee, args } => {
            contains_await(callee) || args.iter().any(contains_await)
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, args, .. }
        | TypedExprKind::ClassVirtualCall { object: receiver, args, .. } => {
            contains_await(receiver) || args.iter().any(contains_await)
        }
        TypedExprKind::RecordWith { object, overrides, .. } => {
            contains_await(object) || overrides.iter().any(|(_, _, e)| contains_await(e))
        }
        TypedExprKind::ArrayLiteral { elements } => elements.iter().any(contains_await),
        TypedExprKind::LetDestructure { value, .. } => contains_await(value),
        TypedExprKind::GlobalAssign { value, .. } => contains_await(value),
        TypedExprKind::Try { operand, .. } => contains_await(operand),
        TypedExprKind::Use { operand, .. } => contains_await(operand),
        TypedExprKind::InterfaceObjectCoerce { inner, .. }
        | TypedExprKind::TemplateInterfaceObjectCoerce { inner, .. }
        | TypedExprKind::InterfaceObjectUpcast { inner } => contains_await(inner),
        TypedExprKind::AsyncBlock { body, .. } => contains_await(body),
        TypedExprKind::EnumVariantRecordCreate { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::ClassStructCreate { fields: args, .. } => args.iter().any(contains_await),
        // Leaf nodes
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
        | TypedExprKind::Break
        | TypedExprKind::Continue => false,
        TypedExprKind::FieldAssign { value, .. } => contains_await(value),
        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before desugar_await pass")
        }
        TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. } => args.iter().any(contains_await),
        TypedExprKind::ImplFunctionRef { .. }
        | TypedExprKind::ExtFunctionRef { .. } => false,
    }
}
