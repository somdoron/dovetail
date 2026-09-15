use crate::common::types::VarName;
use crate::parser::ast::{ClosureParam, ClosureParamKind, Expr};
use crate::typechecker::types::{Type, TypedClosureParam, TypedExpr, TypedExprKind, TypedPattern};

use super::Inference;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;

impl Inference<'_> {
    pub(super) fn infer_closure(
        &mut self,
        is_async: bool,
        params: &[ClosureParam],
        body: &Expr,
        span: &crate::common::span::Span,
    ) -> TypedExpr {
        let discovery = self.async_discovery.take();
        let loop_depth = std::mem::replace(&mut self.loop_depth, 0);
        let return_type = self.function_return_type.take();
        let wrapped_error = self.block_wrapped_error.take();
        self.function_return_type = match &self.expected_type {
            Some(Type::Function(_, result)) => Some((**result).clone()),
            _ => None,
        };
        let result = self.infer_closure_body(is_async, params, body, span);
        if let TypedExprKind::Closure { body, .. } = &result.kind {
            self.check_generic_use_continuations(std::slice::from_ref(body.as_ref()), &body.ty);
        }
        self.function_return_type = return_type;
        self.block_wrapped_error = wrapped_error;
        self.loop_depth = loop_depth;
        self.async_discovery = discovery;
        result
    }

    fn infer_closure_body(
        &mut self,
        is_async: bool,
        params: &[ClosureParam],
        body: &Expr,
        span: &crate::common::span::Span,
    ) -> TypedExpr {
        // Extract expected param/return types from context
        let (expected_param_types, expected_ret_type) = match &self.expected_type {
            Some(Type::Function(param_types, ret_type)) => {
                (Some(param_types.clone()), Some(*ret_type.clone()))
            }
            _ => (None, None),
        };

        // For async closures, validate we have an expected type context and that the
        // return type implements Awaitable
        let (body_expected_type_override, awaitable_ret_type) = if is_async {
            match &expected_ret_type {
                None => {
                    self.diagnostics.error(
                        span.clone(),
                        "async closure requires expected type context to determine return type"
                            .to_string(),
                    );
                    (None, None)
                }
                Some(ret_ty) => match self.resolve_awaitable_value_type(ret_ty) {
                    Some(inner_ty) => (Some(inner_ty), Some(ret_ty.clone())),
                    None => {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "async closure return type '{}' does not implement Awaitable",
                                ret_ty
                            ),
                        );
                        (None, None)
                    }
                },
            }
        } else {
            (None, None)
        };

        // Unresolved record/function parameters are inference hints, not a fixed
        // return type. Keep known input context while inferring the body result.
        let has_unresolved_ret = expected_ret_type.as_ref().is_some_and(|ret| {
            ret.contains_type_variable()
                || self
                    .unresolved_method_type_params
                    .iter()
                    .any(|n| ret.contains_type_parameter_named(n))
        });

        // Replace unresolved method type params with Never (bottom type) so the body
        // inference sees concrete types. This ensures consumers like bare variant
        // resolution (Ok, Error, Some, None) get usable expected types.
        let expected_ret_type = if has_unresolved_ret {
            expected_ret_type.map(|ret| {
                let mut sub = TypeParamSubstitution::new();
                for name in &self.unresolved_method_type_params {
                    sub.insert(name.clone(), Type::Never);
                }
                apply_substitution(&sub, &ret)
            })
        } else {
            expected_ret_type
        };

        // Validate param count if we have expected types
        if let Some(ref expected_params) = expected_param_types
            && expected_params.len() != params.len()
        {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "closure has {} parameters, but expected type has {}",
                    params.len(),
                    expected_params.len()
                ),
            );
            return TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            };
        }

        // Resolve each param type: annotation > expected > error
        // When both exist, check contravariant compatibility: expected param must be
        // assignable to annotation (callers pass expected types, closure body sees annotation).
        // Skip expected param types that contain unresolved method type params.
        let mut typed_params = Vec::new();
        let mut param_types = Vec::new();
        let mut destructure_patterns: Vec<(
            crate::parser::ast::Pattern,
            Type,
            crate::common::span::Span,
        )> = Vec::new();
        let mut synthetic_counter = 0u32;
        for (i, param) in params.iter().enumerate() {
            let expected_ty = expected_param_types.as_ref().map(|p| &p[i]).filter(|t| {
                !t.contains_type_variable()
                    && !self
                        .unresolved_method_type_params
                        .iter()
                        .any(|n| t.contains_type_parameter_named(n))
            });
            match &param.kind {
                ClosureParamKind::Name(name) => {
                    let ty = match (&param.type_annotation, expected_ty) {
                        (Some(annotation), Some(expected_ty)) => {
                            let annotated_ty = self.resolve_type_expr(annotation);
                            if !annotated_ty.is_error()
                                && !expected_ty.is_error()
                                && !self.is_assignable(&annotated_ty, expected_ty)
                            {
                                self.diagnostics.error(
                                    param.span.clone(),
                                    format!(
                                        "closure parameter '{}' has type '{}', but expected type '{}' is not assignable to it",
                                        name.value, annotated_ty, expected_ty
                                    ),
                                );
                            }
                            annotated_ty
                        }
                        (Some(annotation), None) => self.resolve_type_expr(annotation),
                        (None, Some(expected_ty)) => expected_ty.clone(),
                        (None, None) => {
                            self.diagnostics.error(
                                param.span.clone(),
                                format!(
                                    "cannot infer type of closure parameter '{}'; add a type annotation",
                                    name.value
                                ),
                            );
                            Type::Error
                        }
                    };
                    param_types.push(ty.clone());
                    typed_params.push(TypedClosureParam {
                        name: VarName(name.value.clone()),
                        ty,
                        span: param.span.clone(),
                    });
                }
                ClosureParamKind::TuplePattern(pattern) => {
                    let ty = match expected_ty {
                        Some(expected_ty) => expected_ty.clone(),
                        None => {
                            self.diagnostics.error(
                                param.span.clone(),
                                "cannot infer type of destructured closure parameter; provide expected type context".to_string(),
                            );
                            Type::Error
                        }
                    };
                    let synthetic_name = VarName(format!("__destructured_{}", synthetic_counter));
                    synthetic_counter += 1;
                    param_types.push(ty.clone());
                    typed_params.push(TypedClosureParam {
                        name: synthetic_name,
                        ty: ty.clone(),
                        span: param.span.clone(),
                    });
                    destructure_patterns.push((pattern.clone(), ty, param.span.clone()));
                }
            }
        }

        // Push scope and define params
        self.push_scope();
        for tp in &typed_params {
            self.define_variable(tp.name.clone(), tp.ty.clone(), false);
        }

        // Process destructure patterns: infer patterns and define bindings in scope
        let mut destructure_stmts: Vec<TypedExpr> = Vec::new();
        for (destructure_idx, (pattern, ty, pat_span)) in destructure_patterns.iter().enumerate() {
            let typed_pattern = match ty {
                Type::Tuple(elem_types, mn) => {
                    self.infer_tuple_destructure_pattern(pattern, elem_types, mn, ty, pat_span)
                }
                Type::Error => TypedPattern::Wildcard,
                _ => {
                    self.diagnostics.error(
                        pat_span.clone(),
                        format!(
                            "cannot destructure non-tuple type '{}' with tuple pattern",
                            ty
                        ),
                    );
                    TypedPattern::Wildcard
                }
            };
            // Find the synthetic param for this pattern
            let synthetic_param = typed_params
                .iter()
                .filter(|p| p.name.0.starts_with("__destructured_"))
                .nth(destructure_idx)
                .unwrap();
            destructure_stmts.push(TypedExpr {
                kind: TypedExprKind::LetDestructure {
                    pattern: typed_pattern,
                    var_ty: ty.clone(),
                    value: Box::new(TypedExpr {
                        kind: TypedExprKind::VarRef {
                            name: synthetic_param.name.clone(),
                            boxed: false,
                        },
                        ty: ty.clone(),
                        span: pat_span.clone(),
                    }),
                },
                ty: Type::Unit,
                span: pat_span.clone(),
            });
        }

        // For async closures with unresolved type params (e.g. U in andThen<U>),
        // use a two-pass approach:
        //   1. Dry run: infer body to determine return type and bind U. Discard result.
        //   2. Real run: re-infer body with concrete types so await nodes get correct methods.
        let (body_expected_type_override, awaitable_ret_type) = if is_async && has_unresolved_ret {
            if let (Some(body_exp), Some(awaitable_ty)) =
                (body_expected_type_override, awaitable_ret_type)
            {
                // --- Dry run ---
                self.push_scope();
                for tp in &typed_params {
                    self.define_variable(tp.name.clone(), tp.ty.clone(), false);
                }
                let saved_expected = self.expected_type.take();
                let prev_async_return = self.async_return_type.take();
                self.expected_type = Some(body_exp.clone());
                self.async_return_type = Some(awaitable_ty.clone());
                let dry_body = self.infer_expr(body);
                let body_return_type = dry_body.ty.clone();
                self.async_return_type = prev_async_return;
                self.expected_type = saved_expected;
                self.pop_scope();

                // Bind type variables from body return type
                let concrete_awaitable = if !body_return_type.is_error() {
                    let mut sub = TypeParamSubstitution::new();
                    sub.unify(&body_exp, &body_return_type);
                    apply_substitution(&sub, &awaitable_ty)
                } else {
                    awaitable_ty
                };

                // Compute concrete inner type for the real pass
                let concrete_inner = self.resolve_awaitable_value_type(&concrete_awaitable);
                (concrete_inner, Some(concrete_awaitable))
            } else {
                (None, None)
            }
        } else {
            (body_expected_type_override, awaitable_ret_type)
        };

        // --- Real pass (or only pass when types are already concrete) ---

        // Infer body with expected return type
        // For async closures, the body produces the inner T, not the full Awaitable<T>
        let saved_expected = self.expected_type.take();
        let body_expected = if is_async {
            body_expected_type_override.clone()
        } else {
            expected_ret_type.clone()
        };
        self.expected_type = body_expected
            .clone()
            .filter(|ty| !matches!(ty, Type::TypeVariable(..)));

        // For async closures, set async_return_type so `await` is allowed in the body
        let prev_async_return = self.async_return_type.take();
        if is_async {
            if let Some(ref awaitable_ty) = awaitable_ret_type {
                self.async_return_type = Some(awaitable_ty.clone());
            } else {
                // Even when the async closure had errors (e.g. unresolved type params),
                // mark the body as async so `await` expressions don't produce cascading errors.
                self.async_return_type = Some(Type::Error);
            }
        }

        let typed_body = self.infer_expr(body);
        let body_return_type = typed_body.ty.clone();

        self.async_return_type = prev_async_return;
        self.expected_type = saved_expected;

        // Check covariant return: inferred return must be assignable to expected return.
        // For async closures, check against the inner T type, not the full Awaitable.
        let check_expected = if is_async {
            &body_expected
        } else {
            &expected_ret_type
        };
        if let Some(expected_ret) = check_expected
            && !body_return_type.is_error()
            && !expected_ret.contains_error()
            && !has_unresolved_ret
            && !self.is_assignable(expected_ret, &body_return_type)
        {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "closure return type '{}' is not assignable to expected return type '{}'",
                    body_return_type, expected_ret
                ),
            );
        }

        self.pop_scope();

        // Wrap body with destructure let-bindings if any pattern params exist
        let final_body = if destructure_stmts.is_empty() {
            typed_body
        } else {
            let body_ty = typed_body.ty.clone();
            let body_span = typed_body.span.clone();
            destructure_stmts.push(typed_body);
            TypedExpr {
                kind: TypedExprKind::Block(destructure_stmts),
                ty: body_ty,
                span: body_span,
            }
        };

        // For async closures, wrap body in AsyncBlock and use Awaitable return type
        if is_async && let Some(ref awaitable_ty) = awaitable_ret_type {
            let wrapped_body = self.wrap_async_body(final_body, awaitable_ty, true);
            let fn_type = Type::Function(param_types, Box::new(awaitable_ty.clone()));
            return TypedExpr {
                kind: TypedExprKind::Closure {
                    params: typed_params,
                    body: Box::new(wrapped_body),
                    captures: Vec::new(),
                },
                ty: fn_type,
                span: span.clone(),
            };
        }

        // Widen the closure's return type to the expected one when the body is
        // assignable to it (e.g. body `FileThing`, expected `Sink` interface object;
        // or body `Int32`, expected `Any`). The closure's `ty` drives codegen of
        // its return slot, and the coerce pass inserts the matching interface-object
        // / Any-boxing on the body. Without this the closure would advertise the
        // narrower body type and produce a WASM struct-type mismatch at the call
        // boundary. Mirrors the bidirectional widening done for tuple literals.
        let return_type = match &expected_ret_type {
            Some(expected_ret)
                if !has_unresolved_ret
                    && !body_return_type.is_error()
                    && !expected_ret.contains_error()
                    && self.is_assignable(expected_ret, &body_return_type) =>
            {
                expected_ret.clone()
            }
            _ => body_return_type,
        };
        let fn_type = Type::Function(param_types, Box::new(return_type));
        TypedExpr {
            kind: TypedExprKind::Closure {
                params: typed_params,
                body: Box::new(final_body),
                captures: Vec::new(),
            },
            ty: fn_type,
            span: span.clone(),
        }
    }
}
