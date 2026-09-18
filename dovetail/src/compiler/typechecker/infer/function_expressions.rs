use crate::common::span::Span;
use crate::common::types::{
    Fqn, InterfaceMemberName, MangledName, PackagePath, SymbolName, TypeParamName, VarName,
    Visibility,
};
use crate::parser::ast::{Expr, TypeExpr};

use crate::typechecker::registry::{
    ExtMethodSignature, ExtensionBlockSignature, FunctionSignature, ImplBlockSignature,
    ImplMethodSignature, VariantPayload,
};
use crate::typechecker::types::{
    IntrinsicKind, NamedTraitBound, TraitBound, Type, TypedExpr, TypedExprKind,
};

use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;
use super::types::SymbolKind;
use super::{Inference, ResolvedFunction};

/// Outcome of resolving a method call against one trait bound.
enum BoundMethodResolution {
    /// The bound declares a matching method; the call typed successfully.
    Match(TypedExpr),
    /// The bound declares the method but calling it is a hard error
    /// (e.g. it references an associated type); already diagnosed.
    HardError(TypedExpr),
    /// The bound does not provide a matching method.
    NoMatch,
}

impl Inference<'_> {
    /// Infer a bare function call: `func(args)` or `func<T>(args)`.
    pub(super) fn infer_bare_function_call(
        &mut self,
        name: &crate::common::span::Spanned<String>,
        type_args: &[TypeExpr],
        args: &[Expr],
        span: &Span,
    ) -> TypedExpr {
        // Local variable with function type takes priority over all global names.
        // Check early so that `let f: A => B = ...` shadows any function/type named `f`.
        if let Some(binding) = self.lookup_variable(&name.value) {
            if let Type::Function(param_types, return_type) = &binding.ty {
                let expected_arg_types: Option<Vec<Option<Type>>> =
                    if param_types.len() == args.len() {
                        Some(param_types.iter().map(|t| Some(t.clone())).collect())
                    } else {
                        None
                    };
                let typed_args = self.infer_args_with_expected(args, &expected_arg_types);
                if typed_args.iter().any(|a| a.ty.is_error()) {
                    return self.error_call(typed_args, span);
                }
                return self.infer_closure_call(name, param_types, return_type, typed_args, span);
            }
            // Variable exists but is not a function type — not callable
            self.diagnostics.error(
                span.clone(),
                format!(
                    "'{}' has type '{}' which is not callable",
                    name.value, binding.ty
                ),
            );
            let typed_args = self.infer_args_with_expected(args, &None);
            return self.error_call(typed_args, span);
        }

        // Resolve FQN first so we can determine expected arg types
        let fqn = self.resolve_fqn(&name.value, super::types::SymbolKind::Function);

        // Determine expected types for arguments from function signatures or class constructors
        let expected_arg_types = fqn.as_ref().and_then(|fqn| {
            let overloads = self.registry.lookup_function(fqn, &self.package_path, &self.current_file)?;
            let matching: Vec<_> = overloads
                .iter()
                .filter(|sig| sig.params.len() == args.len() && self.named_signature_allowed(&sig.params))
                .collect();
            if matching.is_empty() {
                return None;
            }
            let mut expected = Vec::with_capacity(args.len());
            for i in 0..args.len() {
                let first_ty = &matching[0].params[i].1;
                if matching.iter().all(|sig| sig.params[i].1 == *first_ty) {
                    expected.push(Some(first_ty.clone()));
                } else {
                    expected.push(None);
                }
            }
            Some(expected)
        }).or_else(|| {
            let definitions = self.registry.lookup_generic_function(fqn.as_ref()?, &self.package_path)?;
            let matching: Vec<_> = definitions.iter().filter(|def| def.params.len() == args.len() && self.named_signature_allowed(&def.params)).collect();
            let first = *matching.first()?;
            let mut substitution = TypeParamSubstitution::new();
            if type_args.len() == first.type_params.len() {
                for (name, argument) in first.type_params.iter().zip(type_args) {
                    substitution.insert(name.clone(), self.resolve_type_expr(argument));
                }
            }
            if let Some(expected) = &self.expected_type {
                substitution.unify(&first.return_type, expected);
            }
            Some(first.params.iter().enumerate().map(|(i, (_, ty))| {
                matching.iter().all(|def| def.params[i].1 == *ty)
                    .then(|| apply_substitution(&substitution, ty))
                    .filter(|ty| args.iter().any(|arg| matches!(arg, Expr::AsyncDo { .. }))
                        || self.resolve_awaitable_value_type(ty).is_some()
                        || matches!(ty, Type::Function(parameters, _) if parameters.iter().all(|parameter| !parameter.contains_type_parameter())))
            }).collect())
        }).or_else(|| {
            // Try class constructor param types for expected type hints.
            // Pass &[] — this hint path doesn't have explicit receiver type
            // args at this point; non-generic class returns Type::Class,
            // generic class returns None here and other paths below handle it.
            let ty = self.resolve_type_name(&name.value, &[], &name.span)?;
            let fqn = match ty {
                Type::Class(fqn, _) | Type::GenericClass { fqn, .. } => fqn,
                _ => return None,
            };
            let class_sig = self.registry.lookup_class_type(&fqn, &self.package_path)?;
            if class_sig.constructor_params.len() != args.len() {
                return None;
            }
            // For generic classes with explicit type args, substitute type params
            // so that e.g. MyBox<Int32>(MyState.Pending) infers the arg as MyState<Int32>
            if !class_sig.type_params.is_empty() && !type_args.is_empty() {
                let resolved_args: Vec<Type> = type_args.iter().map(|te| self.resolve_type_expr(te)).collect();
                if resolved_args.len() == class_sig.type_params.len() {
                    let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                        &class_sig.type_params, &resolved_args,
                    );
                    return Some(class_sig.constructor_params.iter().map(|p| {
                        Some(super::generics::apply_substitution(&sub, &p.ty))
                    }).collect());
                }
            }
            Some(class_sig.constructor_params.iter().map(|p| Some(p.ty.clone())).collect())
        }).or_else(|| {
            // Try newtype constructor inner type for expected type hints
            if args.len() != 1 {
                return None;
            }
            let newtype_fqn = self.resolve_fqn(&name.value, super::types::SymbolKind::Newtype)?;
            let sig = self.registry.lookup_newtype_type(&newtype_fqn, &self.package_path, &self.current_file)?;

            if sig.type_params.is_empty() {
                Some(vec![Some(sig.inner_type.clone())])
            } else if let Some(Type::GenericNewtype { fqn, concrete_inner_type, .. }) = self.expected_type.as_ref() {
                if *fqn == newtype_fqn {
                    Some(vec![Some((**concrete_inner_type).clone())])
                } else {
                    None
                }
            } else {
                None
            }
        });

        // Derive expected arg types from expected_type for bare variant calls (e.g. Error(Result.error(e)))
        let expected_arg_types = expected_arg_types
            .or_else(|| self.derive_bare_variant_expected_args(&name.value, args.len()));

        let typed_args = self.infer_args_with_expected(args, &expected_arg_types);

        if typed_args.iter().any(|a| a.ty.is_error()) {
            return self.error_call(typed_args, span);
        }

        if let Some(fqn) = fqn {
            let arg_types: Vec<Type> = typed_args.iter().map(|a| a.ty.clone()).collect();
            let arg_type_refs: Vec<&Type> = arg_types.iter().collect();

            // Collect non-generic candidates (skip when explicit type args are provided)
            let non_generic: Vec<ResolvedFunction> = if type_args.is_empty() {
                self.registry
                    .lookup_function(&fqn, &self.package_path, &self.current_file)
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|sig| {
                        self.named_signature_allowed(&sig.params)
                            && sig.matches_args(&arg_type_refs, |p, a| self.is_assignable(p, a))
                    })
                    .map(|sig| {
                        if sig.is_intrinsic
                            && let Some(intrinsic) = resolve_freestanding_intrinsic(&fqn)
                        {
                            return ResolvedFunction::Intrinsic {
                                intrinsic,
                                return_type: sig.return_type,
                            };
                        }
                        ResolvedFunction::Regular {
                            mangled_name: sig.mangled_name,
                            return_type: sig.return_type,
                            type_args: vec![],
                        }
                    })
                    .collect()
            } else {
                vec![]
            };

            // Resolve generic candidates (skip if non-generic already matched —
            // non-generic overloads are more specific and take priority)
            let generic = if non_generic.is_empty() {
                self.resolve_generic_function(&fqn, &arg_type_refs, type_args)
            } else {
                vec![]
            };

            let candidates: Vec<ResolvedFunction> =
                non_generic.into_iter().chain(generic).collect();

            return self.resolve_overload(&name.value, candidates, typed_args, span);
        }

        // Try bare variant call: `Some(42)` → resolve as enum variant with payload
        let typed_args = match self.try_resolve_bare_variant_call(&name.value, typed_args, span) {
            Ok(result) => return result,
            Err(typed_args) => typed_args,
        };

        // Try class constructor: `ClassName(args)` or `ClassName<T>(args)` → resolve as ClassNew
        let typed_args =
            match self.try_resolve_class_constructor(&name.value, type_args, typed_args, span) {
                Ok(result) => return result,
                Err(typed_args) => typed_args,
            };

        // Try newtype constructor: `Cents(100)` or `Wrapper<Int32>(42)` → resolve as newtype create
        match self.try_resolve_newtype_call(&name.value, type_args, typed_args, span) {
            Ok(result) => result,
            Err(typed_args) => self.undefined_function_error(&name.value, typed_args, span),
        }
    }

    /// Infer a method/qualified call: `receiver.method(args)` or `Array<Int32>.method(args)`.
    pub(super) fn infer_method_call(
        &mut self,
        receiver: &Expr,
        method: &crate::common::span::Spanned<String>,
        receiver_type_args: &[TypeExpr],
        type_params: &[TypeExpr],
        args: &[Expr],
        span: &Span,
    ) -> TypedExpr {
        // Async closures need their return context even with annotated parameters.
        // Defer them and closures with unannotated parameters until the receiver
        // supplies expected types from the method signature.
        let has_async_expression = args.iter().any(|arg| {
            matches!(
                arg,
                Expr::AsyncDo { .. } | Expr::Closure { is_async: true, .. }
            )
        });
        let static_async_context = if has_async_expression {
            self.static_async_argument_context(
                receiver,
                method,
                receiver_type_args,
                type_params,
                args.len(),
                span,
            )
        } else {
            None
        };
        let has_deferred_closures = args.iter().any(|arg| {
            (static_async_context.is_none() && matches!(arg, Expr::AsyncDo { .. }))
                || super::async_arguments::deferred_async_sibling(arg)
        });
        // Also defer ambiguous enum static method references like
        // `HttpClientError.from` when the enum has multiple `From<T>` impls.
        // Without deferral the first-pass infer_expr errors with "undefined
        // variable" because `try_resolve_static_property` has no
        // `expected_type` to disambiguate by. The second pass re-infers
        // each arg with the method's parameter type as the expected_type,
        // which lets the overload resolution pick the right impl.
        let has_deferred_overloaded_refs = args
            .iter()
            .any(|arg| self.is_ambiguous_static_method_ref(arg));

        // Derive expected arg types for enum variant constructors (Enum.Variant(args)).
        // When expected_type matches the enum, substitute type args into payload types
        // to give args correct expected types (e.g. Ok(value) gets Result<T, E> not Result<T, Never>).
        let enum_variant_expected: Option<Vec<Type>> = if let Expr::Identifier(name, _) = receiver {
            self.resolve_enum_type(name).and_then(|enum_sig| {
                if enum_sig.type_params.is_empty() {
                    return None;
                }
                let (_, payload) = enum_sig.variants.iter().find(|(v, _)| v == &method.value)?;
                let payload_types = match payload {
                    crate::typechecker::registry::VariantPayload::Tuple(types)
                        if types.len() == args.len() =>
                    {
                        types.clone()
                    }
                    _ => return None,
                };
                // Use expected_type to determine type args for the enum
                if let Some(Type::GenericEnum {
                    fqn: exp_fqn,
                    type_args: expected_args,
                    ..
                }) = &self.expected_type
                    && *exp_fqn == enum_sig.fqn
                {
                    let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                        &enum_sig.type_params,
                        &expected_args
                            .iter()
                            .map(|(_, t)| t.clone())
                            .collect::<Vec<_>>(),
                    );
                    return Some(
                        payload_types
                            .iter()
                            .map(|t| super::generics::apply_substitution(&sub, t))
                            .collect(),
                    );
                }
                None
            })
        } else {
            None
        };

        // The variant's payload template, kept even when `expected_type` gave us
        // nothing. Argument 1 of `Cons(T, List<T>)` pins `T`, so argument 2 can be
        // expected at `List<Int32>` — without this, a member that needs its type
        // from context (a generic module property like `List.empty`) fails to
        // resolve in a position where the information was available all along.
        let variant_payload_template: Option<(
            Vec<crate::common::types::TypeParamName>,
            Vec<Type>,
        )> = if enum_variant_expected.is_some() {
            None
        } else if let Expr::Identifier(name, _) = receiver {
            self.resolve_enum_type(name).and_then(|enum_sig| {
                if enum_sig.type_params.is_empty() {
                    return None;
                }
                let (_, payload) = enum_sig.variants.iter().find(|(v, _)| v == &method.value)?;
                match payload {
                    crate::typechecker::registry::VariantPayload::Tuple(types)
                        if types.len() == args.len() =>
                    {
                        Some((enum_sig.type_params.clone(), types.clone()))
                    }
                    _ => None,
                }
            })
        } else {
            None
        };
        let mut progressive_sub = super::type_param_substitution::TypeParamSubstitution::new();

        let mut has_non_deferred_error = false;
        let typed_args: Vec<TypedExpr> = args
            .iter()
            .enumerate()
            .map(|(i, arg)| {
                let is_deferred_closure = matches!(arg, Expr::AsyncDo { .. })
                    || super::async_arguments::deferred_async_sibling(arg);
                let is_deferred_ref =
                    has_deferred_overloaded_refs && self.is_ambiguous_static_method_ref(arg);
                if matches!(arg, Expr::AsyncDo { .. })
                    || (has_deferred_closures && is_deferred_closure)
                    || is_deferred_ref
                {
                    // Placeholder — will be re-inferred after receiver provides expected types
                    TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: arg.span(),
                    }
                } else {
                    // Set expected type from enum variant payload if available
                    let progressive =
                        variant_payload_template.as_ref().and_then(|(_, template)| {
                            let substituted =
                                super::generics::apply_substitution(&progressive_sub, &template[i]);
                            // Only a fully-resolved type is a useful expectation; a
                            // half-substituted one would mislead inference.
                            (!substituted.contains_type_parameter()).then_some(substituted)
                        });
                    let closure_hint = if matches!(arg, Expr::Closure { .. }) {
                        static_async_context
                            .as_ref()
                            .and_then(|hints| hints.get(i))
                            .cloned()
                            .flatten()
                            .map(|ty| extract_byname_inner(&ty).cloned().unwrap_or(ty))
                    } else {
                        None
                    };
                    let saved = if let Some(ref expected) = enum_variant_expected {
                        let saved = self.expected_type.take();
                        self.expected_type = Some(expected[i].clone());
                        Some(saved)
                    } else if let Some(expected) = progressive {
                        let saved = self.expected_type.take();
                        self.expected_type = Some(expected);
                        Some(saved)
                    } else if let Some(expected) = closure_hint {
                        let saved = self.expected_type.take();
                        self.expected_type = Some(expected);
                        Some(saved)
                    } else {
                        None
                    };
                    let typed = self.infer_expr(arg);
                    if let Some(prev) = saved {
                        self.expected_type = prev;
                    }
                    if let Some((_, template)) = variant_payload_template.as_ref() {
                        progressive_sub.unify(&template[i], &typed.ty);
                    }
                    if typed.ty.is_error() {
                        has_non_deferred_error = true;
                    }
                    typed
                }
            })
            .collect();

        let typed_args = if let Some(expected) = static_async_context {
            self.infer_deferred_async_arguments(args, typed_args, &expected)
        } else {
            typed_args
        };
        if has_non_deferred_error
            || typed_args.iter().zip(args).any(|(typed, arg)| {
                matches!(arg, Expr::AsyncDo { .. }) && !has_deferred_closures && typed.ty.is_error()
            })
        {
            return self.error_call(typed_args, span);
        }

        let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();

        // Super method call: super.method(args)
        if let Expr::Identifier(name, super_span) = receiver
            && name == "super"
        {
            return self.resolve_super_method_call(method, typed_args, super_span, span);
        }

        // 0. Try to resolve as a module-qualified function call (Module.func style).
        //    The receiver is either a source-level name or a `ResolvedTypeRef`
        //    produced by a lowering — the latter names a module by FQN, so it
        //    resolves without an import. Note this runs *before* the receiver is
        //    inferred as a value (step 3), which is what makes a type-only
        //    receiver legal here and nowhere else.
        //    The tuple carries the receiver's display form alongside the module,
        //    since a `ResolvedTypeRef` has no source name to quote in errors.
        let module_receiver: Option<(String, crate::typechecker::registry::ModuleInfo)> =
            match receiver {
                Expr::Identifier(name, _) => self
                    .resolve_module_name(name)
                    .cloned()
                    .map(|info| (name.clone(), info)),
                Expr::ResolvedTypeRef(fqn, _) => self
                    .registry
                    .lookup_module(fqn)
                    .cloned()
                    .map(|info| (fqn.to_string(), info)),
                _ => None,
            };
        if let Some((name, module_info)) = module_receiver {
            let name = &name;
            // Special-case `EmbeddedResource.bytes("literal")` — the
            // typechecker resolves the literal to a concrete resource
            // registered for the calling project and emits an
            // IntrinsicCall whose kind carries the resolved name +
            // declaring root. Codegen then lowers the call to
            // `array.new_data` over a passive WASM data segment.
            if module_info.fqn.package == PackagePath::from_dotted("standard.prelude")
                && module_info.fqn.symbol.0 == "EmbeddedResource"
                && method.value == "bytes"
            {
                return self.resolve_resource_bytes_intrinsic(args, span);
            }
            let member_sym = SymbolName(method.value.clone());
            if let Some(all_overloads) = module_info.functions.get(&member_sym) {
                // Filter to static overloads only (no `self` first param), excluding inaccessible members
                let overloads: Vec<_> = all_overloads
                    .iter()
                    .filter(|sig| sig.params.is_empty() || sig.params[0].0 != "self")
                    .filter(|sig| match sig.visibility {
                        Visibility::Public | Visibility::Protected => true,
                        Visibility::Internal => module_info.fqn.package == self.package_path,
                        Visibility::Private => sig.source_file == self.current_file,
                    })
                    .filter(|sig| {
                        self.named_signature_allowed(&sig.params)
                            && sig.matches_args(&arg_types, |p, a| self.is_assignable(p, a))
                    })
                    .cloned()
                    .collect();
                if !overloads.is_empty() {
                    let display = format!("{}.{}", name, method.value);
                    return self.resolve_overloads_with_intrinsics(
                        &module_info.fqn,
                        &member_sym,
                        overloads,
                        typed_args,
                        &display,
                        span,
                    );
                }
            }

            // Try generic module static methods
            let generic_candidates = self.resolve_generic_module_static_method(
                &module_info,
                &member_sym,
                &arg_types,
                receiver_type_args,
                type_params,
                Some(span),
            );
            if !generic_candidates.is_empty() {
                let display = format!("{}.{}", name, method.value);
                return self.resolve_overload(&display, generic_candidates, typed_args, span);
            }

            // Check if a visible static member with this name exists but didn't match
            // arg types. Report argument mismatch instead of falling through to
            // "no variant in enum". Only considers members visible from the caller's
            // context and that are static (no `self` param).
            let has_visible_static =
                module_info
                    .functions
                    .get(&member_sym)
                    .is_some_and(|overloads| {
                        overloads.iter().any(|sig| {
                            (sig.params.is_empty() || sig.params[0].0 != "self")
                                && match sig.visibility {
                                    Visibility::Public | Visibility::Protected => true,
                                    Visibility::Internal => {
                                        module_info.fqn.package == self.package_path
                                    }
                                    Visibility::Private => sig.source_file == self.current_file,
                                }
                        })
                    })
                    || module_info
                        .generic_members
                        .lookup_visible(&member_sym, &self.package_path, &self.current_file)
                        .iter()
                        .any(|def| def.params.is_empty() || def.params[0].0 != "self");
            let has_other_static = self
                .registry
                .find_impl_method(&module_info.fqn, &member_sym)
                .iter()
                .any(|(_, method)| method.params.first().is_none_or(|(name, _)| name != "self"))
                || self
                    .lookup_named_extension_methods(&module_info.fqn, &member_sym)
                    .iter()
                    .any(|(_, method)| {
                        method.params.first().is_none_or(|(name, _)| name != "self")
                    });
            if has_visible_static && !has_other_static {
                let arg_type_strs: Vec<String> = arg_types.iter().map(|t| t.to_string()).collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "no matching overload for '{}.{}' with argument types ({})",
                        name,
                        method.value,
                        arg_type_strs.join(", ")
                    ),
                );
                return self.error_call(typed_args, span);
            }
            // Function not in module — fall through to extension/other dispatch
        }

        // 0b. Try to resolve as an enum variant constructor: Enum.Variant(args)
        if let Expr::Identifier(name, _) = receiver
            && let Some(enum_sig) = self.resolve_enum_type(name)
            && let Some((_, payload)) = enum_sig.variants.iter().find(|(v, _)| v == &method.value)
        {
            self.check_private_type_access(
                &enum_sig.fqn,
                enum_sig.construction_private,
                "enum",
                span,
                "construct",
            );

            let payload_types = match payload {
                VariantPayload::Tuple(types) => types.clone(),
                VariantPayload::None => {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "variant '{}.{}' has no fields; use '{}.{}' without parentheses",
                            name, method.value, name, method.value
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }
                VariantPayload::Record(_) => {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "variant '{}.{}' requires record-style construction with {{ }}, not ()",
                            name, method.value
                        ),
                    );
                    return TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    };
                }
            };
            if typed_args.len() != payload_types.len() {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "variant '{}.{}' expects {} argument(s), found {}",
                        name,
                        method.value,
                        payload_types.len(),
                        typed_args.len()
                    ),
                );
                return TypedExpr {
                    kind: TypedExprKind::UnitLiteral,
                    ty: Type::Error,
                    span: span.clone(),
                };
            }
            // Handle generic enums
            if !enum_sig.type_params.is_empty() {
                // Unify payload template types against typed arg types
                let mut substitution = super::type_param_substitution::TypeParamSubstitution::new();
                for (payload_ty, arg) in payload_types.iter().zip(typed_args.iter()) {
                    substitution.unify(payload_ty, &arg.ty);
                }
                // Try to resolve type params from unification, then expected_type, then covariant defaults
                let resolved = substitution.resolve_type_params(&enum_sig.type_params);
                let type_args: Vec<Type> = match resolved {
                    Some(args) => args,
                    _ => {
                        // Fallback: try expected_type
                        match &self.expected_type {
                            Some(Type::GenericEnum {
                                fqn: exp_fqn,
                                type_args: expected_args,
                                ..
                            }) if *exp_fqn == enum_sig.fqn => {
                                // Merge payload unification results with expected type args.
                                // Prefer concrete bindings from payload unification (e.g. T=Int32 from Succeed(x))
                                // over the expected type args.
                                expected_args
                                    .iter()
                                    .zip(enum_sig.type_params.iter())
                                    .map(|((_, exp_ty), tp)| match substitution.get(tp) {
                                        Some(ty)
                                            if !matches!(
                                                ty,
                                                Type::TypeVariable(..) | Type::GenericParam(..)
                                            ) =>
                                        {
                                            ty.clone()
                                        }
                                        _ => exp_ty.clone(),
                                    })
                                    .collect()
                            }
                            _ => {
                                // Default covariant params to Never
                                match substitution.resolve_with_variance_defaults(
                                    &enum_sig.type_params,
                                    &enum_sig.type_param_variances,
                                ) {
                                    Some(args) => args,
                                    None => {
                                        self.diagnostics.error(
                                            span.clone(),
                                            format!(
                                                "cannot infer type arguments for generic enum '{}'",
                                                name
                                            ),
                                        );
                                        return TypedExpr {
                                            kind: TypedExprKind::UnitLiteral,
                                            ty: Type::Error,
                                            span: span.clone(),
                                        };
                                    }
                                }
                            }
                        }
                    }
                };
                let enum_ty = self.resolve_generic_enum_type(&enum_sig.fqn, &enum_sig, &type_args);
                // Substitute and check assignability of payload args
                let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                    &enum_sig.type_params,
                    &type_args,
                );
                let concrete_payload: Vec<Type> = payload_types
                    .iter()
                    .map(|t| apply_substitution(&sub, t))
                    .collect();
                for (arg, expected) in typed_args.iter().zip(concrete_payload.iter()) {
                    self.check_assignable(arg.span.clone(), expected, &arg.ty);
                }
                return TypedExpr {
                    ty: enum_ty,
                    kind: TypedExprKind::EnumCreate {
                        fqn: enum_sig.fqn.clone(),
                        variant_name: method.value.clone(),
                        args: typed_args,
                        type_params: type_args.clone(),
                    },
                    span: span.clone(),
                };
            }
            for (arg, expected) in typed_args.iter().zip(payload_types.iter()) {
                self.check_assignable(arg.span.clone(), expected, &arg.ty);
            }
            let mangled_name = MangledName::for_type(&enum_sig.fqn);
            return TypedExpr {
                ty: Type::Enum(enum_sig.fqn.clone(), mangled_name),
                kind: TypedExprKind::EnumCreate {
                    fqn: enum_sig.fqn.clone(),
                    variant_name: method.value.clone(),
                    args: typed_args,
                    type_params: vec![],
                },
                span: span.clone(),
            };
        }
        // Variant not found on this enum — fall through to subsequent
        // dispatch (class statics, qualified calls, static trait
        // methods). Lets `EnumName.method(...)` resolve to a static
        // trait method like `Color.fromJson(json)` via JsonDecoder.

        // 0d. Explicit disambiguation calls (trait-design-appendix §2.3) —
        // placed before the class-static probe because that probe resolves the
        // receiver as a type name, which diagnoses a plain trait name as
        // "cannot be used as a type" while merely being probed.
        if let Expr::Identifier(name, _) = receiver {
            // A local variable sharing a trait/extension name shadows the
            // explicit-call interpretation — fall through to normal receiver
            // inference.
            let shadowed_by_local = self.lookup_variable(name).is_some();
            // 2c. Explicit extension call: `ExtName.method(receiver, args...)`.
            let ext_target = if shadowed_by_local {
                None
            } else {
                self.import_scope.lookup(name).and_then(|r| {
                    if let crate::typechecker::imports::ImportTarget::Extension(ref f) = r.target {
                        Some(f.clone())
                    } else {
                        None
                    }
                })
            };
            if let Some(ext_fqn) = ext_target {
                return self.infer_explicit_extension_call(
                    name,
                    &ext_fqn,
                    method,
                    typed_args,
                    type_params,
                    span,
                );
            }

            // 2d. Explicit trait call: `TraitName.method(receiver, args...)` or
            // `TraitName.staticFn(args...)`. Records/enums sharing the name win
            // (modules/classes/enum variants were already claimed above).
            if let Some(trait_fqn) = if shadowed_by_local {
                None
            } else {
                self.resolve_trait_fqn(name)
            } && self.resolve_fqn(name, SymbolKind::Record).is_none()
                && self.resolve_fqn(name, SymbolKind::Enum).is_none()
                && self.resolve_fqn(name, SymbolKind::Class).is_none()
                && let Some(result) = self.infer_explicit_trait_call(
                    name,
                    &trait_fqn,
                    receiver_type_args,
                    method,
                    &typed_args,
                    type_params,
                    span,
                )
            {
                return result;
            }
            // None: the name is an interface and the member wasn't
            // found — fall through to the type-name path below.
        }

        // 0c. Try to resolve as a class static method: ClassName.method(args)
        // Drop arg_types borrow before moving typed_args; recompute below.
        drop(arg_types);
        let typed_args = if let Expr::Identifier(name, _) = receiver {
            match self.try_resolve_class_static_method(
                name,
                method,
                typed_args,
                receiver_type_args,
                type_params,
                span,
            ) {
                Ok(result) => return result,
                Err(args) => args,
            }
        } else {
            typed_args
        };
        let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();

        // 1. Try to resolve as a qualified function call (package.func style)
        if let Some(overloads) = self.resolve_qualified_call(receiver, &method.value) {
            let display_name = if let Some(path) = Self::try_flatten_to_path(receiver) {
                let mut parts = path.iter().map(|s| s.to_string()).collect::<Vec<_>>();
                parts.push(method.value.clone());
                parts.join(".")
            } else {
                method.value.clone()
            };
            let candidates = self.filter_overloads_resolved(overloads, &arg_types);
            return self.resolve_overload(&display_name, candidates, typed_args, span);
        }

        // 2. Try static extension method: resolve receiver as a type name
        if let Expr::Identifier(name, _) = receiver {
            // 2b. Try generic static extension methods (e.g. Array.fill, Array<Int32>.empty)
            let method_sym = SymbolName(method.value.clone());
            let (generic_static, has_generic_static_defs) = self.resolve_generic_static_extension(
                name,
                &method_sym,
                &arg_types,
                receiver_type_args,
                type_params,
            );
            if has_generic_static_defs {
                let display = format!("{}.{}", name, method.value);
                return self.resolve_overload(&display, generic_static, typed_args, span);
            }

            let resolved_type = self.resolve_type_name(name, receiver_type_args, span);
            if let Some(ty) = resolved_type {
                // Handle type parameter static method calls (e.g., T.from(value))
                // Look up the method from the type parameter's trait bounds
                if let Type::TypeVariable(_, ref bounds) | Type::GenericParam(_, ref bounds, _) = ty
                {
                    // Every bound that declares the static competes; a trait
                    // and a sub-trait extending it are ONE inherited
                    // declaration (origin's direct impl wins), and otherwise
                    // bound ORDER must not silently decide the callee.
                    let matching: Vec<(usize, (Type, Vec<Type>))> = bounds
                        .iter()
                        .enumerate()
                        .filter_map(|(i, bound)| {
                            let bound = bound.named()?;
                            let trait_sig = self
                                .registry
                                .lookup_trait(&bound.trait_fqn, &self.package_path)?
                                .clone();
                            let method_sig = trait_sig.methods.iter().find(|m| {
                                m.name == method.value
                                    && self.named_signature_allowed(&m.params)
                                    && (m.params.is_empty() || m.params[0].0 != "self")
                                    && m.params.len() == typed_args.len()
                            })?;
                            let mut sub = TypeParamSubstitution::new().with_self_type(ty.clone());
                            for (tp, arg) in trait_sig.type_params.iter().zip(&bound.type_args) {
                                sub.insert(tp.clone(), self.scoped_bound_type(arg));
                            }
                            for (name, ty) in &bound.associated_types {
                                sub.insert(TypeParamName(name.clone()), self.scoped_bound_type(ty));
                            }
                            for associated in &trait_sig.associated_types {
                                let parameters = associated
                                    .type_params
                                    .iter()
                                    .map(|name| Type::TypeVariable(name.clone(), vec![]))
                                    .collect();
                                if let Some(projection) =
                                    crate::typechecker::associated_types::from_bound(
                                        &ty,
                                        bound,
                                        &associated.name,
                                        parameters,
                                        self.registry,
                                    )
                                {
                                    sub.insert(TypeParamName(associated.name.clone()), projection);
                                }
                            }
                            if bound.type_args.is_empty() {
                                for (arg, (_, param)) in typed_args.iter().zip(&method_sig.params) {
                                    sub.unify(param, &arg.ty);
                                }
                            }
                            if !typed_args.iter().zip(&method_sig.params).all(
                                |(arg, (_, param))| {
                                    self.is_assignable(&apply_substitution(&sub, param), &arg.ty)
                                },
                            ) {
                                return None;
                            }
                            Some((
                                i,
                                (
                                    apply_substitution(&sub, &method_sig.return_type),
                                    sub.resolve_type_params(&trait_sig.type_params)
                                        .unwrap_or_else(|| bound.type_args.clone()),
                                ),
                            ))
                        })
                        .collect();
                    let matching = if matching.len() > 1 {
                        match self.dedup_bound_matches_by_origin(
                            &matching,
                            bounds,
                            |sig, member| {
                                sig.methods
                                    .iter()
                                    .find(|m| m.name == member)
                                    .and_then(|m| m.origin.clone())
                            },
                            &method.value,
                        ) {
                            Some(kept) => vec![matching[kept].clone()],
                            None => matching,
                        }
                    } else {
                        matching
                    };
                    if matching.len() > 1 {
                        let names: Vec<String> = matching
                            .iter()
                            .map(|(i, _)| format!("'{}'", Self::bound_display(&bounds[*i])))
                            .collect();
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "ambiguous static call '{}.{}': declared by trait {}; use an explicit trait-qualified call to choose one",
                                name, method.value, names.join(" and trait "),
                            ),
                        );
                        return self.error_call(typed_args, span);
                    }
                    if let Some((i, (ret_type, trait_type_params))) = matching.into_iter().next() {
                        let bound = bounds[i].named().expect("matched named bound");
                        return TypedExpr {
                            ty: ret_type,
                            kind: TypedExprKind::ImplFunctionCall {
                                trait_fqn: bound.trait_fqn.clone(),
                                trait_type_params,
                                for_type: ty.clone(),
                                method_name: SymbolName(method.value.clone()),
                                args: typed_args,
                                method_type_params: vec![],
                            },
                            span: span.clone(),
                        };
                    }
                    // No matching method found in trait bounds
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "no method '{}' found in trait bounds of type parameter '{}'",
                            method.value, name
                        ),
                    );
                    return TypedExpr {
                        ty: Type::Error,
                        kind: TypedExprKind::UnitLiteral,
                        span: span.clone(),
                    };
                }
                if ty.is_error() {
                    return self.error_call(typed_args, span);
                }
                let Some(type_fqn) = ty.try_to_fqn() else {
                    // Intersections have no single FQN and no static members.
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "no static method '{}' found for type '{}'",
                            method.value, ty
                        ),
                    );
                    return self.error_call(typed_args, span);
                };

                // 2a. Try named extension static methods (extensions take
                // priority over trait impls; commit only when an overload
                // matches the args — see the instance path).
                let ext_overloads = self.lookup_named_extension_methods(&type_fqn, &method_sym);
                let all_static_ext: Vec<_> = ext_overloads
                    .into_iter()
                    .filter(|(_, m)| m.params.is_empty() || m.params[0].0 != "self")
                    .collect();
                // Prefer extensions whose for_type IS the named instantiation
                // (`Wrap<Int32>.make()` with sibling-targeted extensions must
                // pick the `Wrap<Int32>` one); keep the base-FQN set as a
                // fallback for bare-name spellings — mirror of 2b below.
                let exact_static_ext: Vec<_> = all_static_ext
                    .iter()
                    .filter(|(b, _)| b.for_type == ty)
                    .cloned()
                    .collect();
                let static_ext: Vec<_> = if exact_static_ext.is_empty() {
                    all_static_ext
                } else {
                    exact_static_ext
                };
                if !static_ext.is_empty() {
                    let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
                    let any_match = static_ext.iter().any(|(_, m)| {
                        self.named_signature_allowed(&m.params)
                            && FunctionSignature::params_match_args(
                                &m.params,
                                &arg_types,
                                |p, a| self.is_assignable(p, a),
                            )
                    });
                    if any_match {
                        let display = format!("{}.{}", name, method.value);
                        return self.resolve_ext_overloads_with_intrinsics(
                            &type_fqn,
                            &method_sym,
                            static_ext,
                            &ty,
                            typed_args,
                            &display,
                            span,
                        );
                    }
                }

                // 2b. Try trait impl static methods. Prefer blocks whose
                // for_type IS the named instantiation (`Wrap<Int32>.make()`
                // must pick the `Wrap<Int32>` sibling block, not report all
                // siblings as overloads); when no block matches the exact
                // spelling (e.g. a bare-name form), keep the full base-FQN
                // set so single-impl programs resolve as before.
                let all_static_impls: Vec<_> = self
                    .registry
                    .find_impl_method(&type_fqn, &method_sym)
                    .into_iter()
                    .filter(|(b, m)| {
                        b.type_params.is_empty()
                            && m.method_type_params.is_empty()
                            && (m.params.is_empty() || m.params[0].0 != "self")
                            && (m.visibility != Visibility::Private
                                || m.span.file == self.current_file)
                    })
                    .collect();
                let exact_static_impls: Vec<_> = all_static_impls
                    .iter()
                    .filter(|(b, _)| b.for_type == ty)
                    .cloned()
                    .collect();
                let static_impl_pairs: Vec<(Fqn, Vec<Type>, FunctionSignature)> =
                    if exact_static_impls.is_empty() {
                        all_static_impls
                    } else {
                        exact_static_impls
                    }
                    .into_iter()
                    .map(|(b, m)| {
                        (
                            b.trait_fqn.clone(),
                            b.trait_type_args.clone(),
                            FunctionSignature {
                                visibility: m.visibility,
                                mangled_name: crate::typechecker::types::impl_member_mangled_name(
                                    &b.trait_fqn,
                                    &b.for_type,
                                    &b.type_params,
                                    &m.dispatch_name,
                                    &b.trait_type_args,
                                ),
                                params: m.params.clone(),
                                return_type: m.return_type.clone(),
                                source_file: b.source_file.clone(),
                                is_intrinsic: m.is_intrinsic,
                                is_property: m.is_property,
                                is_final_method: false,
                                is_abstract_method: false,
                            },
                        )
                    })
                    .collect();
                let static_impl_pairs: Vec<(Fqn, FunctionSignature)> = self
                    .prefer_origin_impl_pairs(static_impl_pairs, &typed_args, &method.value, false)
                    .into_iter()
                    .map(|(f, _, sig)| (f, sig))
                    .collect();
                if !static_impl_pairs.is_empty() {
                    let display = format!("{}.{}", name, method.value);
                    if let Some(err) = self.check_cross_trait_ambiguity(
                        &static_impl_pairs,
                        &typed_args,
                        &display,
                        &method.value,
                        span,
                    ) {
                        return err;
                    }
                    let static_trait_impl: Vec<FunctionSignature> =
                        static_impl_pairs.into_iter().map(|(_, sig)| sig).collect();
                    return self.resolve_overloads_with_intrinsics(
                        &type_fqn,
                        &method_sym,
                        static_trait_impl,
                        typed_args,
                        &display,
                        span,
                    );
                }

                // 2b-bis. Try generic trait impl static methods.
                //
                // `find_impl_method` is keyed by the bare type FQN, so impls
                // like `<T> JsonDecoder for Box<T>` are returned alongside
                // non-generic ones. The filter above rejected them because
                // `b.type_params` is non-empty. Here we bind those type params
                // from the receiver's explicit type args (e.g. `<Int32>` in
                // `Box<Int32>.fromJson(...)`), validate trait bounds, and
                // emit a substituted `ImplFunctionCall` for monomorphization
                // to pick up.
                let mut generic_static_ambiguous = false;
                let generic_match: Option<(ImplBlockSignature, ImplMethodSignature, Vec<Type>)> = {
                    let mut candidates: Vec<_> = self
                        .registry
                        .find_impl_method(&type_fqn, &method_sym)
                        .into_iter()
                        .filter(|(b, m)| {
                            !b.type_params.is_empty()
                                && m.method_type_params.is_empty()
                                && (m.params.is_empty() || m.params[0].0 != "self")
                                && (m.visibility != Visibility::Private
                                    || m.span.file == self.current_file)
                        })
                        .filter_map(|(b, m)| {
                            let mut sub = TypeParamSubstitution::new();
                            if !sub.unify(&b.for_type, &ty) {
                                return None;
                            }
                            let args = sub.resolve_type_params(&b.type_params)?;
                            Some((b, m, args))
                        })
                        .collect();
                    let arg_types: Vec<_> = typed_args.iter().map(|a| &a.ty).collect();
                    let accepting: Vec<_> = candidates
                        .iter()
                        .filter(|(b, m, args)| {
                            let sub = TypeParamSubstitution::from_pairs(&b.type_params, args);
                            let params: Vec<_> = m
                                .params
                                .iter()
                                .map(|(n, t)| (n.clone(), apply_substitution(&sub, t)))
                                .collect();
                            self.named_signature_allowed(&params)
                                && FunctionSignature::params_match_args(
                                    &params,
                                    &arg_types,
                                    |p, a| self.is_assignable(p, a),
                                )
                        })
                        .cloned()
                        .collect();
                    if !accepting.is_empty() {
                        candidates = accepting;
                    }
                    let application = |b: &ImplBlockSignature, args: &[Type]| {
                        let sub = TypeParamSubstitution::from_pairs(&b.type_params, args);
                        (
                            b.trait_fqn.clone(),
                            b.trait_type_args
                                .iter()
                                .map(|t| apply_substitution(&sub, t))
                                .collect::<Vec<_>>(),
                        )
                    };
                    let mut distinct = Vec::new();
                    for (b, _, args) in &candidates {
                        let key = application(b, args);
                        if !distinct.contains(&key) {
                            distinct.push(key);
                        }
                    }
                    if distinct.len() > 1
                        && let Some(kept) = self.dedup_traits_by_member_origin(
                            &distinct,
                            |sig, member| {
                                sig.methods
                                    .iter()
                                    .find(|m| m.name == member)
                                    .and_then(|m| m.origin.clone())
                            },
                            &method.value,
                        )
                    {
                        let keep = distinct[kept].clone();
                        candidates.retain(|(b, _, args)| application(b, args) == keep);
                        distinct = vec![keep];
                    }
                    if distinct.len() > 1 {
                        let names: Vec<_> = distinct
                            .iter()
                            .map(|(f, a)| format!("'{}'", Self::trait_application_display(f, a)))
                            .collect();
                        self.diagnostics.error(span.clone(),format!("ambiguous call to '{}.{}': implemented by trait {}; a static function of a generic implementation cannot be selected by trait name",name,method.value,names.join(" and trait ")));
                        generic_static_ambiguous = true;
                        None
                    } else {
                        candidates
                            .into_iter()
                            .next()
                            .map(|(b, m, args)| (b.clone(), m.clone(), args))
                    }
                };
                if generic_static_ambiguous {
                    return self.error_call(typed_args, span);
                }

                if let Some((block, impl_method, implementation_args)) = generic_match {
                    let sub =
                        TypeParamSubstitution::from_pairs(&block.type_params, &implementation_args);
                    // Validate trait bounds (e.g. `T: JsonDecoder`).
                    if self.check_trait_bounds(
                        &block.trait_bounds,
                        &block.type_params,
                        &implementation_args,
                        span,
                    ) {
                        let concrete_params: Vec<(String, Type)> = impl_method
                            .params
                            .iter()
                            .map(|(n, t)| (n.clone(), apply_substitution(&sub, t)))
                            .collect();
                        let concrete_ret = apply_substitution(&sub, &impl_method.return_type);
                        let concrete_for_type = apply_substitution(&sub, &block.for_type);

                        // Validate arg count and assignability.
                        if typed_args.len() == concrete_params.len() {
                            for ((_, expected), arg) in
                                concrete_params.iter().zip(typed_args.iter())
                            {
                                self.check_assignable(arg.span.clone(), expected, &arg.ty);
                            }
                            return TypedExpr {
                                ty: concrete_ret,
                                kind: TypedExprKind::ImplFunctionCall {
                                    trait_fqn: block.trait_fqn.clone(),
                                    trait_type_params: block
                                        .trait_type_args
                                        .iter()
                                        .map(|ty| apply_substitution(&sub, ty))
                                        .collect(),
                                    for_type: concrete_for_type,
                                    method_name: impl_method.dispatch_name.clone(),
                                    args: typed_args,
                                    method_type_params: vec![],
                                },
                                span: span.clone(),
                            };
                        }
                    }
                    // Bound check failed or arity mismatch — fall through to
                    // the remaining dispatch options below.
                }

                // Receiver is a type name but no static method found
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "no static method '{}' found for type '{}'",
                        method.value, ty
                    ),
                );
                return self.error_call(typed_args, span);
            }
        }

        // Static dispatch on a tuple type used as the call receiver, e.g.
        // `(Int32, String).fromJson(json)`. The parser produces an
        // `Expr::TupleLiteral` of value expressions; here we re-interpret it
        // as a tuple TYPE when every element resolves as a type, then
        // dispatch generic static trait methods registered under
        // `Tuple<arity>` (the synthetic FQN). Falls through silently when
        // any element isn't a type name (preserves the value-tuple path).
        if let Expr::TupleLiteral { elements, .. } = receiver
            && let Some(tuple_ty) = self.try_expr_as_type_for_static_dispatch(receiver)
        {
            let tuple_method_sym = SymbolName(method.value.clone());
            let type_fqn = tuple_ty.to_fqn();
            let candidates: Vec<_> = self
                .registry
                .find_impl_method(&type_fqn, &tuple_method_sym)
                .into_iter()
                .filter(|(b, m)| {
                    !b.type_params.is_empty()
                        && m.method_type_params.is_empty()
                        && (m.params.is_empty() || m.params[0].0 != "self")
                        && (m.visibility != Visibility::Private || m.span.file == self.current_file)
                })
                .map(|(b, m)| (b.clone(), m.clone()))
                .collect();
            // Only candidates that could accept this call compete (see the
            // named-type path): unify the block's for_type with the tuple to
            // bind its type params, then filter by arg assignability.
            let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
            let accepting: Vec<_> = candidates
                .iter()
                .filter(|(b, m)| {
                    let mut sub = TypeParamSubstitution::new();
                    if !sub.unify(&b.for_type, &tuple_ty) {
                        return false;
                    }
                    let params: Vec<(String, Type)> = m
                        .params
                        .iter()
                        .map(|(n, t)| (n.clone(), apply_substitution(&sub, t)))
                        .collect();
                    self.named_signature_allowed(&params)
                        && FunctionSignature::params_match_args(&params, &arg_types, |p, a| {
                            self.is_assignable(p, a)
                        })
                })
                .cloned()
                .collect();
            let mut candidates = if accepting.is_empty() {
                candidates
            } else {
                accepting
            };
            // Distinct traits' blocks providing the same tuple static are
            // ambiguous — mirror of the named-type path, not first-wins.
            let distinct_of =
                |cands: &[(ImplBlockSignature, ImplMethodSignature)]| -> Vec<(Fqn, Vec<Type>)> {
                    let mut out: Vec<(Fqn, Vec<Type>)> = Vec::new();
                    for (b, _) in cands {
                        let key = (b.trait_fqn.clone(), b.trait_type_args.clone());
                        if !out.contains(&key) {
                            out.push(key);
                        }
                    }
                    out
                };
            let mut distinct = distinct_of(&candidates);
            // ...except when they are ONE inherited declaration (a trait plus
            // a sub-trait extending it): the origin's direct impl wins.
            if distinct.len() > 1
                && let Some(kept) = self.dedup_traits_by_member_origin(
                    &distinct,
                    |sig, member| {
                        sig.methods
                            .iter()
                            .find(|m| m.name == member)
                            .and_then(|m| m.origin.clone())
                    },
                    &method.value,
                )
            {
                let (keep_fqn, keep_args) = distinct[kept].clone();
                candidates
                    .retain(|(b, _)| b.trait_fqn == keep_fqn && b.trait_type_args == keep_args);
                distinct = distinct_of(&candidates);
            }
            if distinct.len() > 1 {
                let names: Vec<String> = distinct
                    .iter()
                    .map(|(f, a)| format!("'{}'", Self::trait_application_display(f, a)))
                    .collect();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "ambiguous call to '{}': implemented by trait {}; a static function of a generic implementation cannot be selected by trait name — give the functions distinct names, or implement only one of these traits for this type",
                        method.value, names.join(" and trait "),
                    ),
                );
                return self.error_call(typed_args, span);
            }
            let generic_match = candidates.into_iter().next();

            if let Some((block, impl_method)) = generic_match {
                let mut sub = TypeParamSubstitution::new();
                if sub.unify(&block.for_type, &tuple_ty)
                    && let Some(receiver_args) = sub.resolve_type_params(&block.type_params)
                    && self.check_trait_bounds(
                        &block.trait_bounds,
                        &block.type_params,
                        &receiver_args,
                        span,
                    )
                {
                    let concrete_params: Vec<(String, Type)> = impl_method
                        .params
                        .iter()
                        .map(|(n, t)| (n.clone(), apply_substitution(&sub, t)))
                        .collect();
                    let concrete_ret = apply_substitution(&sub, &impl_method.return_type);
                    let concrete_for_type = apply_substitution(&sub, &block.for_type);

                    if typed_args.len() == concrete_params.len() {
                        for ((_, expected), arg) in concrete_params.iter().zip(typed_args.iter()) {
                            self.check_assignable(arg.span.clone(), expected, &arg.ty);
                        }
                        return TypedExpr {
                            ty: concrete_ret,
                            kind: TypedExprKind::ImplFunctionCall {
                                trait_fqn: block.trait_fqn.clone(),
                                trait_type_params: block.trait_type_args.clone(),
                                for_type: concrete_for_type,
                                method_name: impl_method.dispatch_name.clone(),
                                args: typed_args,
                                method_type_params: vec![],
                            },
                            span: span.clone(),
                        };
                    }
                }
            }
            // Silence the unused-binding warning when fall-through happens.
            let _ = elements;
        }

        // Drop arg_types borrow before potential re-inference of typed_args
        drop(arg_types);

        // 3. Infer receiver expression for instance method calls
        let typed_receiver = self.infer_expr(receiver);

        // Early check: if explicit type params are provided, verify the count matches
        // the method's type params before attempting deferred closure inference.
        // This prevents cascading errors from closure inference with unresolved type params.
        if !type_params.is_empty()
            && !typed_receiver.ty.is_error()
            && !matches!(
                typed_receiver.ty,
                Type::TypeVariable(..) | Type::GenericParam(..)
            )
            && let Some(type_fqn) = typed_receiver.ty.try_to_fqn()
            && let Some(module_info) = self.registry.lookup_module(&type_fqn).cloned()
        {
            let method_sym_early = SymbolName(method.value.clone());
            let defs: Vec<_> = module_info
                .generic_members
                .lookup_visible(&method_sym_early, &self.package_path, &self.current_file)
                .into_iter()
                .filter(|d| {
                    !d.params.is_empty()
                        && d.params[0].0 == "self"
                        && d.params.len() - 1 == args.len()
                })
                .collect();
            if !defs.is_empty()
                && defs
                    .iter()
                    .all(|d| d.method_type_params.len() != type_params.len())
            {
                let expected = defs[0].method_type_params.len();
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "method '{}' expects {} type parameter{} but {} {} provided",
                        method.value,
                        expected,
                        if expected == 1 { "" } else { "s" },
                        type_params.len(),
                        if type_params.len() == 1 {
                            "was"
                        } else {
                            "were"
                        },
                    ),
                );
                let mut all_args = vec![typed_receiver];
                all_args.extend(typed_args);
                return self.error_call(all_args, span);
            }
        }

        // Re-infer args with expected types from the method signature.
        // This is needed for:
        // - Closures with unannotated params (e.g. `v => v * 2`)
        // - Generic expressions that need expected type context (e.g. `None`, `Array.empty()`)
        // Now that we know the receiver type, we can look up the method signature.
        let mut typed_args = typed_args;
        if !typed_receiver.ty.is_error() {
            if let Some((expected, unresolved)) = self.lookup_method_expected_arg_types(
                &typed_receiver.ty,
                &method.value,
                args.len(),
                type_params,
                &typed_args,
            ) {
                // Only re-infer if there are deferred closures or if expected types
                // would help (when an arg got a different type than expected)
                let needs_reinfer = has_deferred_closures
                    || has_deferred_overloaded_refs
                    || typed_args.iter().zip(expected.iter()).any(|(arg, exp)| {
                        if let Some(exp_ty) = exp {
                            arg.ty != *exp_ty && !arg.ty.is_error()
                        } else {
                            false
                        }
                    });
                if needs_reinfer {
                    self.unresolved_method_type_params = unresolved;
                    typed_args = self.infer_args_with_expected(args, &Some(expected));
                    self.unresolved_method_type_params.clear();
                    if typed_args.iter().any(|a| a.ty.is_error()) {
                        return self.error_call(typed_args, span);
                    }
                }
            } else if has_deferred_closures || has_deferred_overloaded_refs {
                typed_args = self.infer_args_with_expected(args, &None);
                if typed_args.iter().any(|arg| arg.ty.is_error()) {
                    return self.error_call(typed_args, span);
                }
            }
        }

        // Dispatch by receiver type
        match &typed_receiver.ty {
            Type::Error => return self.error_call(typed_args, span),

            Type::TypeVariable(name, bounds) | Type::GenericParam(name, bounds, _) => {
                let name = name.clone();
                let bounds = bounds.clone();
                if let Some(expr) = self.try_resolve_method_from_class_bounds(
                    &name,
                    &bounds,
                    &typed_receiver,
                    &method.value,
                    typed_args.clone(),
                    span,
                ) {
                    return expr;
                }
                if let Some(expr) = self.try_resolve_method_from_trait_bounds(
                    &name,
                    &bounds,
                    &typed_receiver,
                    &method.value,
                    typed_args.clone(),
                    type_params,
                    span,
                ) {
                    return expr;
                }
            }

            Type::InterfaceObject { traits, .. } => {
                // Which components declare the method? Exactly one → dispatch
                // through it; several → ambiguous; none → concrete pipeline.
                let traits = traits.clone();
                let mut declaring: Vec<_> = traits
                    .iter()
                    .filter(|c| {
                        self.registry
                            .lookup_trait(&c.trait_fqn, &self.package_path)
                            .is_some_and(|sig| sig.methods.iter().any(|m| m.name == method.value))
                    })
                    .collect();
                // With `extends`, two components can declare the member via the
                // SAME origin (e.g. `A and B` where B extends A) — that is one
                // member, not an ambiguity. Prefer the component that declares
                // it as its own (the origin itself) for a stable dispatch key.
                if declaring.len() > 1 {
                    // Origin identity is the full APPLICATION — fqn AND type
                    // args, substituted through the component's own args:
                    // `Beta extends Alpha<Bool>` and `Gamma extends Alpha<Int32>`
                    // inherit DIFFERENT members and stay ambiguous.
                    let member_origin = |c: &crate::typechecker::types::InterfaceComponent| -> Option<(Fqn, Vec<Type>)> {
                        let sig = self.registry.lookup_trait(&c.trait_fqn, &self.package_path)?;
                        let m = sig.methods.iter().find(|m| m.name == method.value)?;
                        Some(match m.origin.as_ref() {
                            Some((f, raw_args)) => {
                                let sub = TypeParamSubstitution::from_pairs(
                                    &sig.type_params,
                                    &c.trait_type_args,
                                );
                                (
                                    f.clone(),
                                    raw_args
                                        .iter()
                                        .map(|t| super::generics::apply_substitution(&sub, t))
                                        .collect(),
                                )
                            }
                            None => (c.trait_fqn.clone(), c.trait_type_args.clone()),
                        })
                    };
                    let origins: Vec<Option<(Fqn, Vec<Type>)>> =
                        declaring.iter().map(|c| member_origin(c)).collect();
                    if origins.iter().all(|o| o.is_some())
                        && origins.windows(2).all(|w| w[0] == w[1])
                    {
                        let (origin_fqn, origin_args) = origins[0].clone().unwrap();
                        // Dedup only when the origin application is itself a
                        // component (`A and B` with B extends A): dispatch
                        // through the origin's own slots. Two SUB-traits
                        // inheriting the same application from a shared super
                        // carry distinct inline impls — that stays ambiguous.
                        if let Some(pos) = declaring.iter().position(|c| {
                            c.trait_fqn == origin_fqn && c.trait_type_args == origin_args
                        }) {
                            let chosen = declaring[pos];
                            declaring = vec![chosen];
                        }
                    }
                }
                if declaring.len() > 1 {
                    let names: Vec<String> = declaring
                        .iter()
                        .map(|c| format!("'{}'", c.trait_fqn.symbol))
                        .collect();
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "ambiguous method '{}': declared by {}",
                            method.value,
                            names.join(" and ")
                        ),
                    );
                    return self.error_call(typed_args, span);
                }
                if let Some(component) = declaring.first() {
                    let trait_fqn = component.trait_fqn.clone();
                    let trait_type_args = component.trait_type_args.clone();
                    if let Some(result) = self.resolve_interface_object_method_call(
                        typed_receiver.clone(),
                        &method.value,
                        &trait_fqn,
                        &trait_type_args,
                        typed_args.clone(),
                        span,
                    ) {
                        return result;
                    }
                }
                // Method not in any component — try concrete pipeline below
                if let Some(result) = self.resolve_concrete_type_instance_method(
                    &typed_receiver,
                    method,
                    &typed_args,
                    type_params,
                    span,
                ) {
                    return result;
                }
            }

            Type::Class(..) | Type::GenericClass { .. } => {
                if let Some(result) = self.try_resolve_class_instance_method(
                    &typed_receiver,
                    method,
                    typed_args.clone(),
                    type_params,
                    span,
                ) {
                    return result;
                }
                if let Some(result) = self.resolve_concrete_type_instance_method(
                    &typed_receiver,
                    method,
                    &typed_args,
                    type_params,
                    span,
                ) {
                    return result;
                }
            }

            _ => {
                if let Some(result) = self.resolve_concrete_type_instance_method(
                    &typed_receiver,
                    method,
                    &typed_args,
                    type_params,
                    span,
                ) {
                    return result;
                }
            }
        }

        // No method found — check if a generic module has an instance method with this
        // name to provide a better error about argument mismatch vs missing method.
        let method_sym = SymbolName(method.value.clone());
        let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
        let had_generic_defs = if !matches!(
            typed_receiver.ty,
            Type::TypeVariable(..) | Type::GenericParam(..)
        ) {
            // WITH a span, and this is the only instance-method site that has
            // one: every other strategy has already failed, so a candidate that
            // matched the name and the arguments and was rejected for a trait
            // bound is the reason this call does not resolve, and saying which
            // bound beats the argument-types summary below.
            let (_, had) = self.resolve_generic_module_instance_method(
                &typed_receiver.ty,
                &method_sym,
                &arg_types,
                &[],
                Some(span),
            );
            had
        } else {
            false
        };

        // Report unconditionally, including when the receiver or an argument still
        // mentions a type parameter. Deferring those to "they will resolve after
        // monomorphize substitution" does not work: `monomorphize` is a purely
        // syntactic substitution pass that never re-runs inference, so the error
        // node built below survives verbatim into every instantiation and reaches
        // codegen, which has no way to emit a call to a function that was never
        // resolved. A call the template's own types cannot justify is unresolvable,
        // and here is the only place with a span to blame it on.
        if had_generic_defs {
            let arg_type_strs: Vec<String> = arg_types.iter().map(|t| t.to_string()).collect();
            self.diagnostics.error(
                span.clone(),
                format!(
                    "no matching overload for '{}.{}' with argument types ({})",
                    typed_receiver.ty,
                    method.value,
                    arg_type_strs.join(", ")
                ),
            );
        } else {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "no method '{}' found for type '{}'",
                    method.value, typed_receiver.ty
                ),
            );
        }
        let mut all_args = vec![typed_receiver];
        all_args.extend(typed_args);
        self.error_call(all_args, span)
    }

    /// Resolve a method call on a interface object receiver.
    /// Looks up the method in the trait signature, substitutes SelfType and trait type params,
    /// and emits a `InterfaceObjectMethodCall`.
    /// Returns `None` if the method is not found in the trait (caller should try module methods).
    pub(super) fn resolve_interface_object_method_call(
        &mut self,
        typed_receiver: TypedExpr,
        method_name: &str,
        trait_fqn: &crate::common::types::Fqn,
        trait_type_args: &[Type],
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> Option<TypedExpr> {
        let trait_sig = match self.registry.lookup_trait(trait_fqn, &self.package_path) {
            Some(sig) => sig.clone(),
            None => {
                self.diagnostics.error(
                    span.clone(),
                    format!("unknown trait '{}'", trait_fqn.symbol),
                );
                return Some(self.error_call(typed_args, span));
            }
        };

        // Find the method in the trait signature. Object safety is guaranteed:
        // only interfaces reach type position, and interfaces are checked at
        // the declaration (rules/object_safety.rs).
        let substitution =
            TypeParamSubstitution::from_pairs(&trait_sig.type_params, trait_type_args)
                .with_self_type(typed_receiver.ty.clone());
        let matching: Vec<_> = trait_sig
            .methods
            .iter()
            .filter(|method| {
                method.name == method_name && self.named_signature_allowed(&method.params)
            })
            .filter(|method| {
                let parameters: Vec<_> = method
                    .params
                    .iter()
                    .filter(|(name, _)| name != "self")
                    .collect();
                parameters.len() == typed_args.len()
                    && parameters
                        .iter()
                        .zip(&typed_args)
                        .all(|((_, expected), actual)| {
                            self.is_assignable(
                                &apply_substitution(&substitution, expected),
                                &actual.ty,
                            )
                        })
            })
            .collect();
        if matching.len() > 1 {
            self.diagnostics.error(
                span.clone(),
                format!("ambiguous overload for '{method_name}'"),
            );
            return Some(self.error_call(typed_args, span));
        }
        if let Some(method_sig) = matching
            .first()
            .copied()
            .or_else(|| trait_sig.methods.iter().find(|m| m.name == method_name))
        {
            // Slot identity (member name, and the interface `Self` re-boxes
            // into) comes from the ORIGIN trait for inherited members.
            let (raw_name, raw_param_strs, origin_self_type) = {
                let (_origin_sig, raw) = self.registry.origin_method_raw(&trait_sig, method_sig);
                let raw_param_strs: Vec<String> = raw
                    .params
                    .iter()
                    .filter(|(name, _)| name != "self")
                    .map(|(_, ty)| ty.to_string())
                    .collect();
                let origin_self_type = match &method_sig.origin {
                    None => Type::interface_object(trait_fqn.clone(), trait_type_args.to_vec()),
                    Some((origin_fqn, origin_args)) => {
                        let owner_subst: std::collections::BTreeMap<TypeParamName, Type> =
                            trait_sig
                                .type_params
                                .iter()
                                .cloned()
                                .zip(trait_type_args.iter().cloned())
                                .collect();
                        let substituted: Vec<Type> = origin_args
                            .iter()
                            .map(|t| {
                                crate::typechecker::collect::substitute_trait_type_params(
                                    t,
                                    &owner_subst,
                                )
                            })
                            .collect();
                        Type::interface_object(origin_fqn.clone(), substituted)
                    }
                };
                (raw.name.clone(), raw_param_strs, origin_self_type)
            };
            // A bare `Self` return is observed as the declaring component's
            // interface type — the vtable wrapper re-boxes into it. For an
            // intersection receiver, the full set would be unsound (the
            // wrapper cannot rebuild the other components).
            let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
                &trait_sig.type_params,
                trait_type_args,
            )
            .with_self_type(origin_self_type);

            // Substitute SelfType and trait type params in method params (skip self param)
            let non_self_params: Vec<(String, Type)> = method_sig
                .params
                .iter()
                .filter(|(name, _)| name != "self")
                .map(|(name, ty)| (name.clone(), ty.clone()))
                .collect();
            let method_params: Vec<(String, Type)> = non_self_params
                .iter()
                .map(|(n, ty)| (n.clone(), apply_substitution(&sub, ty)))
                .collect();

            // Substitute in return type
            let return_type = apply_substitution(&sub, &method_sig.return_type);

            // Check argument count
            if typed_args.len() != method_params.len() {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "method '{}' on trait '{}' expects {} argument(s), found {}",
                        method_name,
                        trait_fqn.symbol,
                        method_params.len(),
                        typed_args.len()
                    ),
                );
                return Some(self.error_call(typed_args, span));
            }

            // Check argument types
            for (i, (arg, (_, expected_ty))) in
                typed_args.iter().zip(method_params.iter()).enumerate()
            {
                if !self.is_assignable(expected_ty, &arg.ty) {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "type mismatch for argument {} of '{}': expected '{}', found '{}'",
                            i + 1,
                            method_name,
                            expected_ty,
                            arg.ty
                        ),
                    );
                }
            }

            // Compute member_name from the ORIGIN trait's *raw* (unsubstituted)
            // non-self param types — interface objects share one per-trait vtable
            // whose field indices are keyed on the un-substituted signature, so
            // dispatch must use the same types (not the per-instantiation or
            // flattened-substituted ones) to resolve the field.
            let member_name = InterfaceMemberName::new(&raw_name, &raw_param_strs);

            // The node's key is the declaring COMPONENT's per-trait key (equal to
            // the set key for a single interface) — for inherited members, the
            // ORIGIN trait's key: its slot lives in the nested super vtable and
            // codegen navigates there from the receiver's component.
            let interface_mangled_name = match &method_sig.origin {
                Some((origin_fqn, _)) => {
                    MangledName::for_interface_object_per_interface(origin_fqn)
                }
                None => MangledName::for_interface_object_per_interface(trait_fqn),
            };
            return Some(TypedExpr {
                kind: TypedExprKind::InterfaceObjectMethodCall {
                    interface_mangled_name,
                    method_name: method_name.to_string(),
                    member_name,
                    receiver: Box::new(typed_receiver),
                    args: typed_args,
                },
                ty: return_type,
                span: span.clone(),
            });
        }

        // Method not in trait declaration — return None to try module methods
        None
    }

    /// Resolve a method call on a type parameter by looking up the method in its trait bounds.
    /// Used when type-checking generic function bodies (receiver is e.g. T where T : Display).
    /// Returns None if no bound provides the method.
    #[allow(clippy::too_many_arguments)]
    /// When candidate impl methods spanning more than one trait all match the
    /// argument types, the call is ambiguous per trait-design-appendix §3 —
    /// report it with the trait names and the explicit-call escape hatch.
    fn check_cross_trait_ambiguity(
        &mut self,
        impl_pairs: &[(Fqn, FunctionSignature)],
        all_args: &[TypedExpr],
        display: &str,
        method_name: &str,
        span: &Span,
    ) -> Option<TypedExpr> {
        let arg_types: Vec<&Type> = all_args.iter().map(|a| &a.ty).collect();
        let mut matching_traits: Vec<&Fqn> = Vec::new();
        for (trait_fqn, sig) in impl_pairs {
            if self.named_signature_allowed(&sig.params)
                && sig.matches_args(&arg_types, |p, a| self.is_assignable(p, a))
                && !matching_traits.contains(&trait_fqn)
            {
                matching_traits.push(trait_fqn);
            }
        }
        if matching_traits.len() > 1 {
            let names: Vec<String> = matching_traits
                .iter()
                .map(|f| format!("'{}'", f.symbol))
                .collect();
            self.diagnostics.error(
                span.clone(),
                format!(
                    "ambiguous call to '{}': implemented by trait {}; use '{}.{}(...)' to choose one",
                    display, names.join(" and trait "), matching_traits[0].symbol, method_name,
                ),
            );
            return Some(self.error_call(all_args.to_vec(), span));
        }
        None
    }

    /// Explicit extension call: `ExtName.method(receiver, args...)` selects
    /// this extension's implementation, bypassing resolution priority
    /// (trait-design-appendix §2.3).
    fn infer_explicit_extension_call(
        &mut self,
        ext_name: &str,
        ext_fqn: &Fqn,
        method: &crate::common::span::Spanned<String>,
        typed_args: Vec<TypedExpr>,
        explicit_type_params: &[TypeExpr],
        span: &Span,
    ) -> TypedExpr {
        let method_sym = SymbolName(method.value.clone());
        let blocks: Vec<ExtensionBlockSignature> = self
            .import_scope
            .extension_blocks
            .iter()
            .filter(|b| b.ext_fqn == *ext_fqn)
            .cloned()
            .collect();

        let member_named = |m: &ExtMethodSignature| m.name == method_sym;
        let is_instance = |m: &ExtMethodSignature| !m.params.is_empty() && m.params[0].0 == "self";
        let has_any = blocks.iter().any(|b| {
            b.methods
                .iter()
                .chain(b.properties.iter())
                .any(member_named)
        });
        if !has_any {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "extension '{}' has no function '{}'",
                    ext_name, method.value
                ),
            );
            return self.error_call(typed_args, span);
        }
        let has_instance = blocks.iter().any(|b| {
            b.methods
                .iter()
                .chain(b.properties.iter())
                .any(|m| member_named(m) && is_instance(m))
        });

        if has_instance && !typed_args.is_empty() {
            let receiver_ty = typed_args[0].ty.clone();
            let display = format!("{}.{}", ext_name, method.value);

            // Non-generic blocks matching the receiver's base type.
            if let Some(type_fqn) = receiver_ty.try_to_fqn() {
                let mut candidates: Vec<(ExtensionBlockSignature, ExtMethodSignature)> = Vec::new();
                for b in &blocks {
                    if !b.type_params.is_empty()
                        || !b.for_type.try_to_fqn().is_some_and(|f| f == type_fqn)
                    {
                        continue;
                    }
                    for m in b.methods.iter().chain(b.properties.iter()) {
                        if member_named(m)
                            && is_instance(m)
                            && crate::typechecker::registry::is_accessible(
                                m.visibility,
                                &b.package,
                                &self.package_path,
                                &b.source_file,
                                &self.current_file,
                            )
                        {
                            candidates.push((b.clone(), m.clone()));
                        }
                    }
                }
                if !candidates.is_empty() {
                    return self.resolve_ext_overloads_with_intrinsics(
                        &type_fqn,
                        &method_sym,
                        candidates,
                        &receiver_ty,
                        typed_args,
                        &display,
                        span,
                    );
                }
            }

            // Generic blocks, restricted to this extension.
            let rest_types: Vec<&Type> = typed_args[1..].iter().map(|a| &a.ty).collect();
            let candidates = self.resolve_generic_extension_instance_filtered(
                &receiver_ty,
                &method_sym,
                &rest_types,
                explicit_type_params,
                Some(ext_fqn),
            );
            if !candidates.is_empty() {
                return self.resolve_overload(&display, candidates, typed_args, span);
            }

            self.diagnostics.error(
                span.clone(),
                format!(
                    "extension '{}' has no function '{}' for type '{}'",
                    ext_name, method.value, receiver_ty,
                ),
            );
            return self.error_call(typed_args, span);
        }

        // Static form: candidates across the extension's non-generic blocks,
        // filtered by argument assignability.
        let arg_tys: Vec<Type> = typed_args.iter().map(|a| a.ty.clone()).collect();
        let mut matching: Vec<(ExtensionBlockSignature, ExtMethodSignature)> = Vec::new();
        for b in &blocks {
            if !b.type_params.is_empty() {
                continue;
            }
            for m in b.methods.iter().chain(b.properties.iter()) {
                if member_named(m)
                    && !is_instance(m)
                    && crate::typechecker::registry::is_accessible(
                        m.visibility,
                        &b.package,
                        &self.package_path,
                        &b.source_file,
                        &self.current_file,
                    )
                    && m.params.len() == arg_tys.len()
                {
                    let all_match = m
                        .params
                        .iter()
                        .zip(arg_tys.iter())
                        .all(|((_, p), a)| self.is_assignable(p, a));
                    if all_match {
                        matching.push((b.clone(), m.clone()));
                    }
                }
            }
        }
        let display = format!("{}.{}", ext_name, method.value);
        let distinct_for_types: Vec<&Type> = {
            let mut seen: Vec<&Type> = Vec::new();
            for (b, _) in &matching {
                if !seen.contains(&&b.for_type) {
                    seen.push(&b.for_type);
                }
            }
            seen
        };
        if matching.is_empty() {
            let has_generic_statics = blocks.iter().any(|b| {
                !b.type_params.is_empty()
                    && b.methods
                        .iter()
                        .chain(b.properties.iter())
                        .any(|m| member_named(m) && !is_instance(m))
            });
            let message = if has_generic_statics {
                format!(
                    "cannot call '{}.{}': the only matching members are on generic blocks; call it on a concrete type instead",
                    ext_name, method.value,
                )
            } else {
                format!(
                    "extension '{}' has no function '{}' matching the given arguments",
                    ext_name, method.value,
                )
            };
            self.diagnostics.error(span.clone(), message);
            return self.error_call(typed_args, span);
        }
        if distinct_for_types.len() > 1 {
            let names: Vec<String> = distinct_for_types
                .iter()
                .map(|t| format!("'{}'", t))
                .collect();
            self.diagnostics.error(
                span.clone(),
                format!(
                    "ambiguous call to '{}.{}': provided for {}",
                    ext_name,
                    method.value,
                    names.join(" and "),
                ),
            );
            return self.error_call(typed_args, span);
        }
        let for_type = matching[0].0.for_type.clone();
        let type_fqn = match for_type.try_to_fqn() {
            Some(f) => f,
            None => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "extension '{}' has no function '{}' matching the given arguments",
                        ext_name, method.value
                    ),
                );
                return self.error_call(typed_args, span);
            }
        };
        self.resolve_ext_overloads_with_intrinsics(
            &type_fqn,
            &method_sym,
            matching,
            &for_type,
            typed_args,
            &display,
            span,
        )
    }

    /// Explicit trait call: `TraitName.method(receiver, args...)` selects this
    /// trait's implementation (trait-design-appendix §2.3), or
    /// `TraitName.staticFn(args...)` selects the unique implementing type's
    /// static. Returns `None` to fall through to the interface-object
    /// type-name path (interface receiver whose member isn't declared here).
    #[allow(clippy::too_many_arguments)]
    fn infer_explicit_trait_call(
        &mut self,
        trait_name: &str,
        trait_fqn: &Fqn,
        receiver_type_args: &[TypeExpr],
        method: &crate::common::span::Spanned<String>,
        typed_args: &[TypedExpr],
        explicit_type_params: &[TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        let trait_sig = self
            .registry
            .lookup_trait(trait_fqn, &self.package_path)?
            .clone();
        // `Conv<Bool>.tag(r)` — explicit trait type args select among sibling
        // instantiations.
        let required_trait_args: Vec<Type> = if receiver_type_args.is_empty() {
            vec![]
        } else {
            self.resolve_type_args(receiver_type_args)
                .unwrap_or_default()
        };
        let member_takes_self = trait_sig
            .methods
            .iter()
            .find(|m| m.name == method.value)
            .map(|m| !m.params.is_empty() && m.params[0].0 == "self")
            .or_else(|| {
                trait_sig
                    .properties
                    .iter()
                    .find(|p| p.name == method.value)
                    .map(|p| !p.params.is_empty() && p.params[0].0 == "self")
            });

        let Some(is_instance) = member_takes_self else {
            if trait_sig.is_interface {
                // The interface-object type-name path may still resolve this
                // (e.g. statics of `implement X for TheInterface` blocks).
                return None;
            }
            self.diagnostics.error(
                span.clone(),
                format!("trait '{}' has no function '{}'", trait_name, method.value),
            );
            return Some(self.error_call(typed_args.to_vec(), span));
        };

        if is_instance {
            if typed_args.is_empty() {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "'{}.{}' expects the receiver as its first argument",
                        trait_name, method.value,
                    ),
                );
                return Some(self.error_call(vec![], span));
            }
            let receiver = typed_args[0].clone();
            let rest: Vec<TypedExpr> = typed_args[1..].to_vec();

            match receiver.ty.clone() {
                Type::TypeVariable(param_name, bounds)
                | Type::GenericParam(param_name, bounds, _) => {
                    // `T: B` also answers explicit calls through B's supers
                    // ("B satisfies A everywhere") — the member lives in B's
                    // flattened signature. Explicit trait args, when given,
                    // must match the bound's application of the trait.
                    let bound_application_matches = |b: &NamedTraitBound| -> bool {
                        if required_trait_args.is_empty() {
                            return true;
                        }
                        if b.trait_fqn == *trait_fqn {
                            return b.type_args == required_trait_args;
                        }
                        self.registry
                            .super_closure_args(&b.trait_fqn, &b.type_args, trait_fqn)
                            .is_some_and(|args| args == required_trait_args)
                    };
                    let candidate_bounds: Vec<NamedTraitBound> = bounds
                        .iter()
                        .filter_map(TraitBound::named)
                        .filter(|b| {
                            (b.trait_fqn == *trait_fqn
                                || self
                                    .registry
                                    .super_closure_args(&b.trait_fqn, &b.type_args, trait_fqn)
                                    .is_some())
                                && bound_application_matches(b)
                        })
                        .cloned()
                        .collect();
                    // Several bounds can supply the named trait at DIFFERENT
                    // applications (`T: Conv<Int32> + Conv<Bool>` under a
                    // bare `Conv.tag(v)`): first-wins would make bound order
                    // decide the callee. Distinct applications are ambiguous
                    // — the explicit `Conv<Int32>.tag(v)` form disambiguates.
                    let mut distinct_apps: Vec<Vec<Type>> = Vec::new();
                    for b in &candidate_bounds {
                        let app = if b.trait_fqn == *trait_fqn {
                            b.type_args.clone()
                        } else {
                            self.registry
                                .super_closure_args(&b.trait_fqn, &b.type_args, trait_fqn)
                                .unwrap_or_default()
                        };
                        if !distinct_apps.contains(&app) {
                            distinct_apps.push(app);
                        }
                    }
                    if distinct_apps.len() > 1 {
                        let names: Vec<String> = distinct_apps
                            .iter()
                            .map(|a| format!("'{}'", Self::trait_application_display(trait_fqn, a)))
                            .collect();
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "ambiguous call to '{}.{}' on type parameter '{}': the bounds supply {}; name the application to choose one",
                                trait_name, method.value, param_name, names.join(" and "),
                            ),
                        );
                        return Some(self.error_call(typed_args.to_vec(), span));
                    }
                    let Some(bound) = candidate_bounds.into_iter().next() else {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "cannot call '{}.{}': type parameter '{}' is not bounded by trait '{}'",
                                trait_name, method.value, param_name, trait_name,
                            ),
                        );
                        return Some(self.error_call(typed_args.to_vec(), span));
                    };
                    // The bound proves the requested application is available,
                    // but the explicit trait name determines dispatch. Keeping a
                    // sub-trait here would bypass a direct impl of the named super.
                    let bound = NamedTraitBound {
                        associated_types: bound.associated_types.clone(),
                        trait_fqn: trait_fqn.clone(),
                        type_args: distinct_apps.remove(0),
                        kind: bound.kind,
                    };
                    let receiver_ty = receiver.ty.clone();
                    let rest_is_empty = rest.is_empty();
                    match self.try_resolve_method_from_one_bound(
                        &param_name,
                        &receiver_ty,
                        &bound,
                        &receiver,
                        &method.value,
                        &rest,
                        explicit_type_params,
                        span,
                    ) {
                        BoundMethodResolution::Match(expr)
                        | BoundMethodResolution::HardError(expr) => Some(expr),
                        BoundMethodResolution::NoMatch => {
                            // Properties aren't in the method table — and the
                            // explicit form is the documented disambiguator
                            // for an ambiguous property on a type parameter,
                            // so it must dispatch the property slot too (same
                            // fallback as the interface-object arm).
                            if rest_is_empty {
                                let field = crate::common::span::Spanned::new(
                                    method.value.clone(),
                                    method.span.clone(),
                                );
                                if let Some(expr) = self.try_resolve_property_from_bound(
                                    &receiver, &bound, &field, span,
                                ) {
                                    return Some(expr);
                                }
                            }
                            self.diagnostics.error(
                                span.clone(),
                                format!(
                                    "no matching overload for '{}.{}' with the given arguments",
                                    trait_name, method.value,
                                ),
                            );
                            Some(self.error_call(typed_args.to_vec(), span))
                        }
                    }
                }
                Type::InterfaceObject { ref traits, .. } => {
                    // A component matching directly — or, with `extends`, one
                    // whose super closure contains the named trait ("B
                    // satisfies A everywhere"): its flattened signature holds
                    // the member and the origin machinery keys the slot.
                    let component_application_matches =
                        |c: &crate::typechecker::types::InterfaceComponent| -> bool {
                            if required_trait_args.is_empty() {
                                return true;
                            }
                            if c.trait_fqn == *trait_fqn {
                                return c.trait_type_args == required_trait_args;
                            }
                            self.registry
                                .super_closure_args(&c.trait_fqn, &c.trait_type_args, trait_fqn)
                                .is_some_and(|args| args == required_trait_args)
                        };
                    let candidates: Vec<_> = traits
                        .iter()
                        .filter(|c| {
                            (c.trait_fqn == *trait_fqn
                                || self
                                    .registry
                                    .super_closure_args(&c.trait_fqn, &c.trait_type_args, trait_fqn)
                                    .is_some())
                                && component_application_matches(c)
                        })
                        .collect();
                    let Some(first_component) = candidates.first() else {
                        self.diagnostics.error(
                            span.clone(),
                            format!(
                                "cannot call '{}.{}': interface object '{}' does not include '{}'",
                                trait_name, method.value, receiver.ty, trait_name,
                            ),
                        );
                        return Some(self.error_call(typed_args.to_vec(), span));
                    };
                    let application =
                        |component: &crate::typechecker::types::InterfaceComponent| {
                            if component.trait_fqn == *trait_fqn {
                                component.trait_type_args.clone()
                            } else {
                                self.registry
                                    .super_closure_args(
                                        &component.trait_fqn,
                                        &component.trait_type_args,
                                        trait_fqn,
                                    )
                                    .unwrap_or_default()
                            }
                        };
                    let first_application = application(first_component);
                    let different_applications = candidates
                        .iter()
                        .any(|component| application(component) != first_application);
                    let direct_component = candidates
                        .iter()
                        .find(|component| component.trait_fqn == *trait_fqn);
                    if different_applications
                        || (direct_component.is_none() && candidates.len() > 1)
                    {
                        let names: Vec<_> = candidates
                            .iter()
                            .map(|component| {
                                Self::trait_application_display(
                                    &component.trait_fqn,
                                    &component.trait_type_args,
                                )
                            })
                            .collect();
                        self.diagnostics.error(span.clone(), format!(
                            "ambiguous call to '{}.{}': interface components {} supply the trait; name a component or a unique trait application to disambiguate",
                            trait_name, method.value, names.join(" and "),
                        ));
                        return Some(self.error_call(typed_args.to_vec(), span));
                    }
                    let component = direct_component.copied().unwrap_or(first_component);
                    let component_fqn = component.trait_fqn.clone();
                    let component_args = component.trait_type_args.clone();
                    // Preserve the selected component when an inherited slot is
                    // keyed by its origin. Otherwise codegen could reach that
                    // origin through a different intersection component.
                    let receiver = if traits.len() > 1 {
                        TypedExpr {
                            ty: Type::interface_object(
                                component_fqn.clone(),
                                component_args.clone(),
                            ),
                            span: receiver.span.clone(),
                            kind: TypedExprKind::InterfaceObjectUpcast {
                                inner: Box::new(receiver),
                            },
                        }
                    } else {
                        receiver
                    };
                    let rest_is_empty = rest.is_empty();
                    match self.resolve_interface_object_method_call(
                        receiver.clone(),
                        &method.value,
                        &component_fqn,
                        &component_args,
                        rest,
                        span,
                    ) {
                        Some(expr) => Some(expr),
                        None => {
                            // Properties are not in the method table — the
                            // explicit form `Trait.prop(recv)` dispatches the
                            // property slot.
                            if rest_is_empty {
                                let field = crate::common::span::Spanned::new(
                                    method.value.clone(),
                                    method.span.clone(),
                                );
                                if let Some(expr) = self.try_resolve_interface_object_property(
                                    &receiver,
                                    &component_fqn,
                                    &component_args,
                                    &field,
                                    span,
                                ) {
                                    return Some(expr);
                                }
                            }
                            self.diagnostics.error(
                                span.clone(),
                                format!(
                                    "no matching overload for '{}.{}' with the given arguments",
                                    trait_name, method.value,
                                ),
                            );
                            Some(self.error_call(typed_args.to_vec(), span))
                        }
                    }
                }
                _ => {
                    let rest_types: Vec<&Type> = rest.iter().map(|a| &a.ty).collect();
                    match self.resolve_trait_impl_method_for_type_detailed(
                        &receiver.ty,
                        trait_fqn,
                        &method.value,
                        &rest_types,
                        &required_trait_args,
                    ) {
                        super::traits::ImplMethodResolution::Found {
                            resolved,
                            params,
                            return_type,
                        } => {
                            if let Some(params) = params {
                                if params.len() != rest.len() {
                                    self.diagnostics.error(
                                        span.clone(),
                                        format!(
                                            "no matching overload for '{}.{}': expected {} argument(s) after the receiver, found {}",
                                            trait_name, method.value, params.len(), rest.len(),
                                        ),
                                    );
                                    return Some(self.error_call(typed_args.to_vec(), span));
                                }
                                for ((_, expected), arg) in params.iter().zip(rest.iter()) {
                                    self.check_assignable(arg.span.clone(), expected, &arg.ty);
                                }
                            }
                            Some(TypedExpr {
                                ty: return_type,
                                kind: TypedExprKind::ImplFunctionCall {
                                    trait_fqn: resolved.trait_fqn,
                                    trait_type_params: resolved.trait_type_params,
                                    for_type: resolved.for_type,
                                    method_name: resolved.method_name,
                                    args: typed_args.to_vec(),
                                    method_type_params: resolved.method_type_params,
                                },
                                span: span.clone(),
                            })
                        }
                        super::traits::ImplMethodResolution::Ambiguous => {
                            self.diagnostics.error(
                                span.clone(),
                                format!(
                                    "ambiguous call to '{}.{}' for type '{}': multiple implementations match",
                                    trait_name, method.value, receiver.ty,
                                ),
                            );
                            Some(self.error_call(typed_args.to_vec(), span))
                        }
                        super::traits::ImplMethodResolution::NotFound => {
                            self.diagnostics.error(
                                span.clone(),
                                format!(
                                    "type '{}' does not implement trait '{}'",
                                    receiver.ty, trait_name,
                                ),
                            );
                            Some(self.error_call(typed_args.to_vec(), span))
                        }
                    }
                }
            }
        } else {
            // Static member: select the unique non-generic implementing block
            // by argument assignability.
            // Direct impls first; when none provide the member, sub-trait
            // provider blocks do ("B satisfies A everywhere" — the static is
            // implemented inline in the provider block).
            type StaticMethodCandidate = (Fqn, Type, Vec<Type>, Vec<(String, Type)>, Type);
            let collect_from = |blocks: Vec<&crate::typechecker::registry::ImplBlockSignature>| -> Vec<StaticMethodCandidate> {
                blocks
                    .iter()
                    .flat_map(|b| {
                        b.methods
                            .iter()
                            .chain(b.properties.iter())
                            .filter(|m| {
                                m.name.0 == method.value
                                    && self.named_trait_implementation_allowed(trait_fqn, &m.dispatch_name)
&& (m.params.is_empty() || m.params[0].0 != "self")
                                    && m.method_type_params.is_empty()
                                    && (m.visibility != Visibility::Private
                                        || m.span.file == self.current_file)
                            })
                            .map(|m| {
                                (
                                    b.trait_fqn.clone(),
                                    b.for_type.clone(),
                                    b.trait_type_args.clone(),
                                    m.params.clone(),
                                    m.return_type.clone(),
                                )
                            })
                            .collect::<Vec<_>>()
                    })
                    .collect()
            };
            let direct_blocks: Vec<_> = self
                .registry
                .all_implement_blocks()
                .iter()
                .filter(|b| {
                    b.trait_fqn == *trait_fqn
                        && b.type_params.is_empty()
                        && (required_trait_args.is_empty()
                            || b.trait_type_args == required_trait_args)
                })
                .collect();
            let mut candidate_blocks = collect_from(direct_blocks);
            if candidate_blocks.is_empty() {
                let provider_blocks: Vec<_> = self
                    .registry
                    .all_implement_blocks()
                    .iter()
                    .filter(|b| {
                        b.trait_fqn != *trait_fqn
                            && b.type_params.is_empty()
                            && self
                                .registry
                                .super_closure_args(&b.trait_fqn, &b.trait_type_args, trait_fqn)
                                .is_some_and(|args| {
                                    // `Maker<Bool>.make(...)` must not run a
                                    // `Maker<Int32>` provider, and explicit args
                                    // uniquely select among sibling providers.
                                    required_trait_args.is_empty() || args == required_trait_args
                                })
                    })
                    .collect();
                candidate_blocks = collect_from(provider_blocks);
            }
            let mut matching: Vec<(Fqn, Type, Vec<Type>, Type)> = Vec::new();
            for (block_trait_fqn, for_type, trait_args, params, return_type) in candidate_blocks {
                if params.len() != typed_args.len() {
                    continue;
                }
                let all_match = params
                    .iter()
                    .zip(typed_args.iter())
                    .all(|((_, p), a)| self.is_assignable(p, &a.ty));
                if all_match {
                    matching.push((block_trait_fqn, for_type, trait_args, return_type));
                }
            }
            match matching.len() {
                1 => {
                    // Target the SELECTED block's trait (a provider block's own
                    // trait when routed) — sibling providers of one super all
                    // provide the same requested fqn, so emitting the requested
                    // trait would collapse them at monomorphize.
                    let (block_trait_fqn, for_type, trait_args, return_type) =
                        matching.pop().unwrap();
                    Some(TypedExpr {
                        ty: return_type,
                        kind: TypedExprKind::ImplFunctionCall {
                            trait_fqn: block_trait_fqn,
                            trait_type_params: trait_args,
                            for_type,
                            method_name: SymbolName(method.value.clone()),
                            args: typed_args.to_vec(),
                            method_type_params: vec![],
                        },
                        span: span.clone(),
                    })
                }
                0 => {
                    let has_generic_impls = self.registry.all_implement_blocks().iter().any(|b| {
                        b.trait_fqn == *trait_fqn
                            && !b.type_params.is_empty()
                            && b.methods
                                .iter()
                                .chain(b.properties.iter())
                                .any(|m| m.name.0 == method.value)
                    });
                    let message = if has_generic_impls {
                        format!(
                            "cannot call '{}.{}': the only implementations are generic; call it on a concrete type instead (e.g. 'Box<Int32>.{}(...)')",
                            trait_name, method.value, method.value,
                        )
                    } else {
                        format!(
                            "no implementation of trait '{}' provides a static function '{}' matching the given arguments",
                            trait_name, method.value,
                        )
                    };
                    self.diagnostics.error(span.clone(), message);
                    Some(self.error_call(typed_args.to_vec(), span))
                }
                _ => {
                    // Candidates can differ by for_type, by trait application,
                    // or both — render whichever actually distinguishes them,
                    // and point the escape hatch at a usable spelling.
                    let for_types_differ = matching.windows(2).any(|w| w[0].1 != w[1].1);
                    let names: Vec<String> = if for_types_differ {
                        matching
                            .iter()
                            .map(|(_, t, _, _)| format!("'{}'", t))
                            .collect()
                    } else {
                        matching
                            .iter()
                            .map(|(f, _, args, _)| {
                                format!("'{}'", Self::trait_application_display(f, args))
                            })
                            .collect()
                    };
                    let suggestion = if for_types_differ {
                        format!("{}.{}(...)", matching[0].1, method.value)
                    } else {
                        format!(
                            "{}.{}(...)",
                            Self::trait_application_display(&matching[0].0, &matching[0].2),
                            method.value,
                        )
                    };
                    let by = if for_types_differ {
                        "implemented for"
                    } else {
                        "implemented by trait"
                    };
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "ambiguous call to '{}.{}': {} {}; use '{}' to choose one",
                            trait_name,
                            method.value,
                            by,
                            names.join(" and "),
                            suggestion,
                        ),
                    );
                    Some(self.error_call(typed_args.to_vec(), span))
                }
            }
        }
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Keep the compiler context parameters explicit at this call boundary."
    )]
    fn try_resolve_method_from_trait_bounds(
        &mut self,
        name: &TypeParamName,
        bounds: &[TraitBound],
        typed_receiver: &TypedExpr,
        method_name: &str,
        typed_args: Vec<TypedExpr>,
        explicit_type_params: &[TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        let receiver_ty = Type::TypeVariable(name.clone(), bounds.to_vec());
        // Resolve against every bound; a member declared by several bounds with
        // a matching signature is ambiguous and needs an explicit
        // `TraitName.method(...)` call.
        // Matches are keyed by BOUND INDEX, not trait fqn: `T: Conv<Int32> +
        // Conv<Bool>` has two distinct bounds of one trait, and they must
        // stay distinguishable for both dedup and the diagnostic.
        let mut matches: Vec<(usize, TypedExpr)> = Vec::new();
        for (i, bound) in bounds.iter().enumerate() {
            let Some(bound) = bound.named() else {
                continue;
            };
            match self.try_resolve_method_from_one_bound(
                name,
                &receiver_ty,
                bound,
                typed_receiver,
                method_name,
                &typed_args,
                explicit_type_params,
                span,
            ) {
                BoundMethodResolution::Match(expr) => matches.push((i, expr)),
                BoundMethodResolution::HardError(expr) => return Some(expr),
                BoundMethodResolution::NoMatch => {}
            }
        }
        if matches.len() > 1 {
            // `extends` origin dedup, mirroring the intersection path: with
            // `T: Alpha + Beta` where Beta extends Alpha, the member is one
            // inherited declaration, not an ambiguity — dispatch through the
            // ORIGIN bound when that origin application is itself a bound.
            if let Some(kept) = self.dedup_bound_matches_by_origin(
                &matches,
                bounds,
                |sig, member| {
                    sig.methods
                        .iter()
                        .find(|m| m.name == member)
                        .and_then(|m| m.origin.clone())
                },
                method_name,
            ) {
                matches = vec![matches.swap_remove(kept)];
            }
        }
        if matches.len() > 1 {
            let names: Vec<String> = matches
                .iter()
                .map(|(i, _)| format!("'{}'", Self::bound_display(&bounds[*i])))
                .collect();
            let first = Self::bound_display(&bounds[matches[0].0]);
            self.diagnostics.error(
                span.clone(),
                format!(
                    "ambiguous method '{}' on type parameter '{}': declared by trait {}; use '{}.{}(...)' to choose one",
                    method_name, name, names.join(" and trait "), first, method_name,
                ),
            );
            let mut all_args = vec![typed_receiver.clone()];
            all_args.extend(typed_args);
            return Some(self.error_call(all_args, span));
        }
        matches.into_iter().next().map(|(_, expr)| expr)
    }

    /// A bound rendered as written: `Conv<Int32>` when it has type args, else
    /// the bare trait name. Two applications of one trait must not both print
    /// as `'Conv'` in an ambiguity message.
    pub(super) fn bound_display(bound: &TraitBound) -> String {
        match bound.named() {
            Some(bound) => Self::trait_application_display(&bound.trait_fqn, &bound.type_args),
            None => "class".to_string(),
        }
    }

    /// Narrow candidate impl-method pairs to the origin's own block when the
    /// candidates are ONE inherited declaration reached through a trait and a
    /// sub-trait that extends it (`Alpha` plus `Beta extends Alpha`): the
    /// direct impl of the origin wins, exactly as on the bound and explicit
    /// paths. Returns the input unchanged when the candidates are genuinely
    /// distinct members.
    pub(super) fn prefer_origin_impl_pairs(
        &mut self,
        impl_pairs: Vec<(Fqn, Vec<Type>, FunctionSignature)>,
        all_args: &[TypedExpr],
        member_name: &str,
        is_property: bool,
    ) -> Vec<(Fqn, Vec<Type>, FunctionSignature)> {
        // Narrow only among candidates that ACTUALLY apply to this call —
        // `find_impl_method` is keyed by the bare type FQN, so blocks for a
        // sibling for_type and same-named members of unrelated traits with
        // other signatures are in the list too. Judging origins over those
        // would discard the one applicable block, or defeat the dedup.
        let arg_types: Vec<&Type> = all_args.iter().map(|a| &a.ty).collect();
        let applicable: Vec<usize> = impl_pairs
            .iter()
            .enumerate()
            .filter(|(_, (_, _, sig))| {
                self.named_signature_allowed(&sig.params)
                    && sig.matches_args(&arg_types, |p, a| self.is_assignable(p, a))
            })
            .map(|(i, _)| i)
            .collect();
        // Applications carry the block's real trait args: a generic origin
        // (`Beta extends Producer<Int32>`) only dedups against the same
        // application of that origin.
        let mut distinct: Vec<(Fqn, Vec<Type>)> = Vec::new();
        for i in &applicable {
            let key = (impl_pairs[*i].0.clone(), impl_pairs[*i].1.clone());
            if !distinct.contains(&key) {
                distinct.push(key);
            }
        }
        if distinct.len() < 2 {
            return impl_pairs;
        }
        let Some(kept) = self.dedup_traits_by_member_origin(
            &distinct,
            |sig, member| {
                if is_property {
                    sig.properties
                        .iter()
                        .find(|p| p.name == member)
                        .and_then(|p| p.origin.clone())
                } else {
                    sig.methods
                        .iter()
                        .find(|m| m.name == member)
                        .and_then(|m| m.origin.clone())
                }
            },
            member_name,
        ) else {
            return impl_pairs;
        };
        let (keep_fqn, keep_args) = distinct[kept].clone();
        // Keep the origin's own block, plus every candidate that did not
        // compete here (a different signature entirely) so their own
        // resolution is unaffected.
        impl_pairs
            .into_iter()
            .enumerate()
            .filter(|(i, (f, args, _))| {
                !applicable.contains(i) || (*f == keep_fqn && *args == keep_args)
            })
            .map(|(_, p)| p)
            .collect()
    }

    /// The `prefer_origin_impl_pairs` narrowing for already-resolved generic
    /// candidates (step 6): resolution has unified them against the receiver
    /// and args already, so every candidate here applies.
    pub(super) fn prefer_origin_resolved_candidates(
        &self,
        candidates: Vec<super::ResolvedFunction>,
        member_name: &str,
    ) -> Vec<super::ResolvedFunction> {
        let mut distinct: Vec<(Fqn, Vec<Type>)> = Vec::new();
        for c in &candidates {
            if let super::ResolvedFunction::ImplMethod { resolved, .. } = c {
                let key = (
                    resolved.trait_fqn.clone(),
                    resolved.trait_type_params.clone(),
                );
                if !distinct.contains(&key) {
                    distinct.push(key);
                }
            } else {
                return candidates; // mixed kinds — leave resolution alone
            }
        }
        if distinct.len() < 2 {
            return candidates;
        }
        let Some(kept) = self.dedup_traits_by_member_origin(
            &distinct,
            |sig, member| {
                sig.methods
                    .iter()
                    .find(|m| m.name == member)
                    .and_then(|m| m.origin.clone())
            },
            member_name,
        ) else {
            return candidates;
        };
        let (keep_fqn, keep_args) = distinct[kept].clone();
        candidates
            .into_iter()
            .filter(|c| match c {
                super::ResolvedFunction::ImplMethod { resolved, .. } => {
                    resolved.trait_fqn == keep_fqn && resolved.trait_type_params == keep_args
                }
                _ => true,
            })
            .collect()
    }

    /// When every candidate trait declares the member via the SAME origin
    /// application, and that origin application is itself one of the
    /// candidates, return its index — the member is ONE inherited
    /// declaration reached through several traits (`Alpha` plus
    /// `Beta extends Alpha`), and the direct impl of the origin wins, exactly
    /// as it does on the bound and explicit paths. `None` leaves the
    /// ambiguity to the caller (e.g. two sub-traits of a common super with no
    /// direct impl of that super).
    pub(super) fn dedup_traits_by_member_origin(
        &self,
        candidates: &[(Fqn, Vec<Type>)],
        member_origin: impl Fn(
            &crate::typechecker::registry::TraitSignature,
            &str,
        ) -> Option<(Fqn, Vec<Type>)>,
        member_name: &str,
    ) -> Option<usize> {
        let origin_of = |(f, args): &(Fqn, Vec<Type>)| -> Option<(Fqn, Vec<Type>)> {
            let sig = self.registry.lookup_trait(f, &self.package_path)?;
            Some(match member_origin(sig, member_name) {
                Some((of, raw_args)) => {
                    let sub = TypeParamSubstitution::from_pairs(&sig.type_params, args);
                    (
                        of,
                        raw_args
                            .iter()
                            .map(|t| super::generics::apply_substitution(&sub, t))
                            .collect(),
                    )
                }
                None => (f.clone(), args.clone()),
            })
        };
        let origins: Vec<Option<(Fqn, Vec<Type>)>> = candidates.iter().map(origin_of).collect();
        if !origins.iter().all(|o| o.is_some()) || !origins.windows(2).all(|w| w[0] == w[1]) {
            return None;
        }
        let (origin_fqn, origin_args) = origins[0].clone().unwrap();
        candidates
            .iter()
            .position(|(f, args)| *f == origin_fqn && *args == origin_args)
    }

    /// A trait application rendered as written: `Conv<Int32>` with type args,
    /// else the bare trait name. Ambiguity messages that distinguish
    /// applications must not print both candidates as `'Conv'`.
    pub(super) fn trait_application_display(trait_fqn: &Fqn, type_args: &[Type]) -> String {
        if type_args.is_empty() {
            trait_fqn.symbol.0.clone()
        } else {
            let args: Vec<String> = type_args.iter().map(|t| t.to_string()).collect();
            format!("{}<{}>", trait_fqn.symbol.0, args.join(", "))
        }
    }

    /// When every matched bound declares the member via the SAME origin
    /// application, and that origin application is itself one of the matched
    /// bounds, return that match's index — the member is one inherited
    /// declaration ("B satisfies A everywhere"), dispatched through its
    /// origin. `None` leaves the ambiguity to the caller.
    pub(super) fn dedup_bound_matches_by_origin<T>(
        &self,
        matches: &[(usize, T)],
        bounds: &[TraitBound],
        member_origin: impl Fn(
            &crate::typechecker::registry::TraitSignature,
            &str,
        ) -> Option<(Fqn, Vec<Type>)>,
        member_name: &str,
    ) -> Option<usize> {
        // Each match names its OWN bound (by index), so two applications of
        // one trait resolve their origins independently and stay ambiguous.
        let origin_of = |bound_idx: usize| -> Option<(Fqn, Vec<Type>)> {
            let bound = bounds.get(bound_idx)?.named()?;
            let f = &bound.trait_fqn;
            let sig = self.registry.lookup_trait(f, &self.package_path)?;
            Some(match member_origin(sig, member_name) {
                Some((of, raw_args)) => {
                    let sub = TypeParamSubstitution::from_pairs(&sig.type_params, &bound.type_args);
                    (
                        of,
                        raw_args
                            .iter()
                            .map(|t| super::generics::apply_substitution(&sub, t))
                            .collect(),
                    )
                }
                None => (f.clone(), bound.type_args.clone()),
            })
        };
        let origins: Vec<Option<(Fqn, Vec<Type>)>> =
            matches.iter().map(|(i, _)| origin_of(*i)).collect();
        if !origins.iter().all(|o| o.is_some()) || !origins.windows(2).all(|w| w[0] == w[1]) {
            return None;
        }
        let (origin_fqn, origin_args) = origins[0].clone().unwrap();
        // Keep the match whose OWN bound is the origin application.
        matches.iter().position(|(i, _)| {
            bounds[*i].named().is_some_and(|bound| {
                bound.trait_fqn == origin_fqn && bound.type_args == origin_args
            })
        })
    }

    /// Resolve a method call against one specific trait bound of a type
    /// parameter. Shared by implicit dispatch (all bounds tried) and explicit
    /// `TraitName.method(receiver, ...)` calls (single bound).
    #[allow(clippy::too_many_arguments)]
    fn try_resolve_method_from_one_bound(
        &mut self,
        _name: &TypeParamName,
        receiver_ty: &Type,
        bound: &NamedTraitBound,
        typed_receiver: &TypedExpr,
        method_name: &str,
        typed_args: &[TypedExpr],
        explicit_type_params: &[TypeExpr],
        span: &Span,
    ) -> BoundMethodResolution {
        let Some(signature) = self
            .registry
            .lookup_trait(&bound.trait_fqn, &self.package_path)
            .cloned()
        else {
            return BoundMethodResolution::NoMatch;
        };
        let mut matches = Vec::new();
        for method in signature
            .methods
            .iter()
            .filter(|method| method.name == method_name)
        {
            match self.try_resolve_bound_method(
                _name,
                receiver_ty,
                bound,
                typed_receiver,
                method_name,
                typed_args,
                explicit_type_params,
                span,
                &signature,
                method,
            ) {
                BoundMethodResolution::Match(expression) => matches.push(expression),
                BoundMethodResolution::HardError(expression) => {
                    return BoundMethodResolution::HardError(expression);
                }
                BoundMethodResolution::NoMatch => {}
            }
        }
        if matches.len() > 1 {
            self.diagnostics.error(
                span.clone(),
                format!("ambiguous overload for '{method_name}'"),
            );
            return BoundMethodResolution::HardError(self.error_call(typed_args.to_vec(), span));
        }
        matches
            .pop()
            .map(BoundMethodResolution::Match)
            .unwrap_or(BoundMethodResolution::NoMatch)
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "Keep the compiler context parameters explicit at this call boundary."
    )]
    fn try_resolve_bound_method(
        &mut self,
        _name: &TypeParamName,
        receiver_ty: &Type,
        bound: &NamedTraitBound,
        typed_receiver: &TypedExpr,
        _method_name: &str,
        typed_args: &[TypedExpr],
        explicit_type_params: &[TypeExpr],
        span: &Span,
        trait_sig: &crate::typechecker::registry::TraitSignature,
        method_sig: &crate::typechecker::registry::TraitMethodSig,
    ) -> BoundMethodResolution {
        if !self.named_signature_allowed(&method_sig.params) {
            return BoundMethodResolution::NoMatch;
        }
        {
            // Build substitution: Self → receiver type + trait type params → type args from bound
            let mut sub = TypeParamSubstitution::new().with_self_type(receiver_ty.clone());
            for (tp, arg) in trait_sig.type_params.iter().zip(bound.type_args.iter()) {
                sub.insert(tp.clone(), arg.clone());
            }

            for (name, ty) in &bound.associated_types {
                sub.insert(TypeParamName(name.clone()), self.scoped_bound_type(ty));
            }

            for associated in &trait_sig.associated_types {
                let parameters = associated
                    .type_params
                    .iter()
                    .map(|name| Type::TypeVariable(name.clone(), vec![]))
                    .collect();
                if let Some(projection) = crate::typechecker::associated_types::from_bound(
                    receiver_ty,
                    bound,
                    &associated.name,
                    parameters,
                    self.registry,
                ) {
                    sub.insert(TypeParamName(associated.name.clone()), projection);
                }
            }

            let non_self_params: Vec<(String, Type)> = method_sig
                .params
                .iter()
                .filter(|(n, _)| n != "self")
                .map(|(n, ty)| (n.clone(), ty.clone()))
                .collect();

            if typed_args.len() != non_self_params.len() {
                return BoundMethodResolution::NoMatch;
            }

            // If method has its own type params, infer them via unification
            if !method_sig.type_params.is_empty() {
                // Unify arg types against param types to infer method type params
                for ((_, param_ty), arg) in non_self_params.iter().zip(typed_args.iter()) {
                    sub.unify(param_ty, &arg.ty);
                }

                // Try to resolve method type params
                let resolved = match sub.resolve_type_params(&method_sig.type_params) {
                    Some(args) => args,
                    None => {
                        // Wrong number of explicit type params — skip this method entirely
                        if !explicit_type_params.is_empty()
                            && explicit_type_params.len() != method_sig.type_params.len()
                        {
                            return BoundMethodResolution::NoMatch;
                        }
                        // Fallback 1: explicit type params from call syntax
                        if !explicit_type_params.is_empty() {
                            match self.resolve_type_args(explicit_type_params) {
                                Some(args) => {
                                    for (tp, arg) in method_sig.type_params.iter().zip(args.iter())
                                    {
                                        sub.insert(tp.clone(), arg.clone());
                                    }
                                    args
                                }
                                None => return BoundMethodResolution::NoMatch,
                            }
                        } else if let Some(ref expected) = self.expected_type.clone() {
                            // Fallback 2: bidirectional inference via expected return type
                            sub.unify(&method_sig.return_type, expected);
                            match sub.resolve_type_params(&method_sig.type_params) {
                                Some(args) => args,
                                None => return BoundMethodResolution::NoMatch,
                            }
                        } else {
                            return BoundMethodResolution::NoMatch;
                        }
                    }
                };

                // Insert resolved method type params into substitution
                for (tp, arg) in method_sig.type_params.iter().zip(resolved.iter()) {
                    sub.insert(tp.clone(), arg.clone());
                }
            }

            let method_params: Vec<(String, Type)> = non_self_params
                .iter()
                .map(|(n, ty)| (n.clone(), apply_substitution(&sub, ty)))
                .collect();
            let return_type = apply_substitution(&sub, &method_sig.return_type);
            let all_match = typed_args
                .iter()
                .zip(method_params.iter())
                .all(|(arg, (_, expected))| self.is_assignable(expected, &arg.ty));
            if !all_match {
                return BoundMethodResolution::NoMatch;
            }
            let contract_parameters: Vec<_> = trait_sig
                .type_params
                .iter()
                .chain(&method_sig.type_params)
                .cloned()
                .collect();
            let contract_arguments = sub
                .resolve_type_params(&contract_parameters)
                .unwrap_or_default();
            if !self.check_trait_bounds(
                &method_sig.trait_bounds,
                &contract_parameters,
                &contract_arguments,
                span,
            ) {
                return BoundMethodResolution::HardError(
                    self.error_call(typed_args.to_vec(), span),
                );
            }
            let mut all_args = vec![typed_receiver.clone()];
            all_args.extend(typed_args.iter().cloned());
            BoundMethodResolution::Match(TypedExpr {
                kind: TypedExprKind::ImplFunctionCall {
                    trait_fqn: bound.trait_fqn.clone(),
                    trait_type_params: bound.type_args.clone(),
                    for_type: typed_receiver.ty.clone(),
                    method_name: trait_sig.method_dispatch_name(method_sig),
                    args: all_args,
                    method_type_params: sub
                        .resolve_type_params(&method_sig.type_params)
                        .unwrap_or_default(),
                },
                ty: return_type,
                span: span.clone(),
            })
        }
    }

    /// Try to resolve a method call on a TypeParameter via class bounds.
    /// When a type parameter has a class bound (e.g. `T: Animal`), we can call
    /// instance methods defined on that class.
    fn try_resolve_method_from_class_bounds(
        &mut self,
        _name: &TypeParamName,
        bounds: &[TraitBound],
        typed_receiver: &TypedExpr,
        method_name: &str,
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> Option<TypedExpr> {
        for bound in bounds.iter().filter_map(TraitBound::named) {
            if bound.kind != crate::typechecker::types::BoundKind::SubtypeOf {
                continue;
            }
            // Walk the class hierarchy to find a matching instance method
            let mut current_fqn = Some(bound.trait_fqn.clone());
            while let Some(fqn) = current_fqn.take() {
                let class_sig = match self.registry.lookup_class_type(&fqn, &self.package_path) {
                    Some(sig) => sig.clone(),
                    None => break,
                };
                let method_sym = SymbolName(method_name.to_string());
                if let Some(overloads) = class_sig.instance_methods.get(&method_sym) {
                    // Find a matching overload by argument count
                    for sig in overloads {
                        let non_self_params: Vec<&(String, Type)> =
                            sig.params.iter().filter(|(n, _)| n != "self").collect();
                        if non_self_params.len() != typed_args.len() {
                            continue;
                        }
                        let all_match = typed_args
                            .iter()
                            .zip(non_self_params.iter())
                            .all(|(arg, (_, expected))| self.is_assignable(expected, &arg.ty));
                        if !all_match {
                            continue;
                        }
                        let mut all_args = vec![typed_receiver.clone()];
                        all_args.extend(typed_args);
                        // Produce an ImplFunctionCall so monomorphize can resolve
                        // to the concrete class method after type substitution.
                        return Some(TypedExpr {
                            kind: TypedExprKind::ImplFunctionCall {
                                trait_fqn: bound.trait_fqn.clone(),
                                trait_type_params: vec![],
                                for_type: typed_receiver.ty.clone(),
                                method_name: SymbolName(method_name.to_string()),
                                args: all_args,
                                method_type_params: vec![],
                            },
                            ty: sig.return_type.clone(),
                            span: span.clone(),
                        });
                    }
                }
                // Walk up to parent class
                current_fqn = class_sig.parent_class;
            }
        }
        None
    }

    /// Filter overloads to those matching the given argument types, wrapping as `ResolvedFunction::Regular`.
    fn filter_overloads_resolved(
        &self,
        overloads: Vec<FunctionSignature>,
        arg_types: &[&Type],
    ) -> Vec<ResolvedFunction> {
        overloads
            .into_iter()
            .filter(|sig| {
                self.named_signature_allowed(&sig.params)
                    && sig.matches_args(arg_types, |p, a| self.is_assignable(p, a))
            })
            .map(|sig| ResolvedFunction::Regular {
                mangled_name: sig.mangled_name,
                return_type: sig.return_type,
                type_args: vec![],
            })
            .collect()
    }

    /// Check overloads for an intrinsic match first; fall back to regular overload resolution.
    /// Try to interpret an expression as a type expression for static
    /// method-call dispatch — `(Int32, String).fromJson(json)` re-parses
    /// `(Int32, String)` from a value-tuple literal to the tuple type
    /// `(Int32, String)`. Returns `None` if any element isn't a type name
    /// (in which case the original value-expression dispatch path runs).
    ///
    /// Handles:
    /// - `Expr::Identifier(name, _)` → `resolve_type_name(name)`
    /// - `Expr::TupleLiteral { elements }` → recurse and build `Type::Tuple`
    ///
    /// Other expression shapes (numeric literals, calls, etc.) return None,
    /// preserving the existing value-receiver dispatch.
    pub(super) fn try_expr_as_type_for_static_dispatch(&mut self, expr: &Expr) -> Option<Type> {
        match expr {
            Expr::Identifier(name, span) => self.resolve_type_name(name, &[], span),
            // A lowering-produced receiver: resolve the type straight from the
            // FQN, bypassing imports.
            Expr::ResolvedTypeRef(fqn, _) => self
                .registry
                .lookup_type(fqn, &self.package_path, &self.current_file)
                .cloned(),
            Expr::TupleLiteral { elements, .. } => {
                if elements.len() < 2 {
                    return None;
                }
                let mut tys = Vec::with_capacity(elements.len());
                for el in elements {
                    let t = self.try_expr_as_type_for_static_dispatch(el)?;
                    if t.is_error() {
                        return None;
                    }
                    tys.push(t);
                }
                let mn = MangledName::for_tuple(&tys);
                Some(Type::Tuple(tys, mn))
            }
            _ => None,
        }
    }

    pub(super) fn resolve_overloads_with_intrinsics(
        &mut self,
        type_fqn: &Fqn,
        method_sym: &SymbolName,
        overloads: Vec<FunctionSignature>,
        typed_args: Vec<TypedExpr>,
        display_name: &str,
        span: &Span,
    ) -> TypedExpr {
        let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
        let intrinsic_match = overloads.iter().find(|sig| {
            sig.is_intrinsic
                && self.named_signature_allowed(&sig.params)
                && sig.matches_args(&arg_types, |p, a| self.is_assignable(p, a))
        });
        if let Some(intrinsic_sig) = intrinsic_match
            && let Some(intrinsic) =
                resolve_intrinsic_kind(type_fqn, method_sym, &intrinsic_sig.return_type)
        {
            return TypedExpr {
                ty: intrinsic_sig.return_type.clone(),
                kind: TypedExprKind::IntrinsicCall {
                    intrinsic,
                    args: typed_args,
                },
                span: span.clone(),
            };
        }
        let candidates = self.filter_overloads_resolved(overloads, &arg_types);
        self.resolve_overload(display_name, candidates, typed_args, span)
    }

    /// Like `resolve_overloads_with_intrinsics`, but produces `ExtMethod` candidates for non-intrinsic
    /// extension overloads. Intrinsic overloads still produce `IntrinsicCall`.
    #[allow(clippy::too_many_arguments)]
    fn resolve_ext_overloads_with_intrinsics(
        &mut self,
        type_fqn: &Fqn,
        method_sym: &SymbolName,
        overloads: Vec<(ExtensionBlockSignature, ExtMethodSignature)>,
        for_type: &Type,
        typed_args: Vec<TypedExpr>,
        display_name: &str,
        span: &Span,
    ) -> TypedExpr {
        let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
        let intrinsic_match = overloads.iter().find(|(_, m)| {
            m.is_intrinsic
                && self.named_signature_allowed(&m.params)
                && FunctionSignature::params_match_args(&m.params, &arg_types, |p, a| {
                    self.is_assignable(p, a)
                })
        });
        if let Some((_, intrinsic_method)) = intrinsic_match
            && let Some(intrinsic) =
                resolve_intrinsic_kind(type_fqn, method_sym, &intrinsic_method.return_type)
        {
            return TypedExpr {
                ty: intrinsic_method.return_type.clone(),
                kind: TypedExprKind::IntrinsicCall {
                    intrinsic,
                    args: typed_args,
                },
                span: span.clone(),
            };
        }
        let candidates: Vec<super::ResolvedFunction> = overloads
            .into_iter()
            .filter(|(_, m)| {
                self.named_signature_allowed(&m.params)
                    && FunctionSignature::params_match_args(&m.params, &arg_types, |p, a| {
                        self.is_assignable(p, a)
                    })
            })
            .map(|(block, m)| super::ResolvedFunction::ExtMethod {
                ext_fqn: block.ext_fqn.clone(),
                for_type: for_type.clone(),
                method_name: method_sym.clone(),
                type_args: vec![],
                return_type: m.return_type.clone(),
            })
            .collect();
        self.resolve_overload(display_name, candidates, typed_args, span)
    }

    /// Try to resolve an instance method through the concrete type pipeline:
    /// module → generic module → extension → generic extension → trait impl → generic trait impl,
    /// then function-typed field closure call as a final fallback.
    /// Returns `None` if no method was found by any strategy.
    pub(super) fn resolve_concrete_type_instance_method(
        &mut self,
        typed_receiver: &TypedExpr,
        method: &crate::common::span::Spanned<String>,
        typed_args: &[TypedExpr],
        type_params: &[TypeExpr],
        span: &Span,
    ) -> Option<TypedExpr> {
        if typed_receiver.ty.is_error() {
            return None;
        }
        // Intersections have no single FQN (no module/impl fallbacks apply).
        let type_fqn = typed_receiver.ty.try_to_fqn()?;
        let method_sym = SymbolName(method.value.clone());

        // Helper: build all_args = [receiver] + args (cloning from refs)
        let build_all_args = |recv: &TypedExpr, args: &[TypedExpr]| -> Vec<TypedExpr> {
            let mut all = vec![recv.clone()];
            all.extend(args.iter().cloned());
            all
        };

        // 1. Module-for-type instance method
        if let Some(module_info) = self.registry.lookup_module(&type_fqn).cloned()
            && let Some(overloads) = module_info.functions.get(&method_sym)
        {
            let instance_overloads: Vec<_> = overloads
                .iter()
                .filter(|sig| {
                    !sig.is_property && !sig.params.is_empty() && sig.params[0].0 == "self"
                })
                .filter(|sig| {
                    sig.params.len() == typed_args.len() + 1
                        && self.is_assignable(&sig.params[0].1, &typed_receiver.ty)
                        && sig.params[1..]
                            .iter()
                            .zip(typed_args)
                            .all(|((_, p), a)| self.is_assignable(p, &a.ty))
                })
                .filter(|sig| {
                    self.is_member_visible(
                        sig.visibility,
                        &module_info.fqn.package,
                        &sig.source_file,
                    )
                })
                .cloned()
                .collect();
            if !instance_overloads.is_empty() {
                let all_args = build_all_args(typed_receiver, typed_args);
                let display = format!("{}.{}", type_fqn.symbol, method.value);
                return Some(self.resolve_overloads_with_intrinsics(
                    &type_fqn,
                    &method_sym,
                    instance_overloads,
                    all_args,
                    &display,
                    span,
                ));
            }
        }

        // 2. Generic module-for-type instance method
        {
            let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
            // No span here: trait-impl dispatch is tried after this one.
            let (candidates, _) = self.resolve_generic_module_instance_method(
                &typed_receiver.ty,
                &method_sym,
                &arg_types,
                type_params,
                None,
            );
            if !candidates.is_empty() {
                let all_args = build_all_args(typed_receiver, typed_args);
                let display = format!("{}.{}", all_args[0].ty, method.value);
                return Some(self.resolve_overload(&display, candidates, all_args, span));
            }
        }

        // 3. Named extension instance methods (extensions take priority over
        // trait impls). Per §2.1 a category wins only when it "yields a
        // matching candidate": name-matching overloads whose args don't fit
        // do NOT block fall-through to trait impls.
        {
            let ext_overloads = self.lookup_named_extension_methods(&type_fqn, &method_sym);
            let instance_overloads: Vec<_> = ext_overloads
                .into_iter()
                .filter(|(_, m)| !m.is_property && !m.params.is_empty() && m.params[0].0 == "self")
                .collect();
            if !instance_overloads.is_empty() {
                let all_args = build_all_args(typed_receiver, typed_args);
                let arg_types: Vec<&Type> = all_args.iter().map(|a| &a.ty).collect();
                let any_match = instance_overloads.iter().any(|(_, m)| {
                    self.named_signature_allowed(&m.params)
                        && FunctionSignature::params_match_args(&m.params, &arg_types, |p, a| {
                            self.is_assignable(p, a)
                        })
                });
                if any_match {
                    let display = format!("{}.{}", type_fqn.symbol, method.value);
                    return Some(self.resolve_ext_overloads_with_intrinsics(
                        &type_fqn,
                        &method_sym,
                        instance_overloads,
                        &typed_receiver.ty,
                        all_args,
                        &display,
                        span,
                    ));
                }
            }
        }

        // 4. Generic extension instance methods
        {
            let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
            let candidates = self.resolve_generic_extension_instance(
                &typed_receiver.ty,
                &method_sym,
                &arg_types,
                type_params,
            );
            if !candidates.is_empty() {
                let all_args = build_all_args(typed_receiver, typed_args);
                let display = format!("{}.{}", all_args[0].ty, method.value);
                return Some(self.resolve_overload(&display, candidates, all_args, span));
            }
        }

        // 5. Trait impl instance methods
        {
            let impl_pairs: Vec<(Fqn, Vec<Type>, FunctionSignature)> = self
                .registry
                .find_impl_method(&type_fqn, &method_sym)
                .into_iter()
                .filter(|(b, m)| {
                    b.type_params.is_empty()
                        && m.method_type_params.is_empty()
                        && !m.is_property
                        && !m.params.is_empty()
                        && m.params[0].0 == "self"
                        && (m.visibility != Visibility::Private || m.span.file == self.current_file)
                })
                .map(|(b, m)| {
                    (
                        b.trait_fqn.clone(),
                        b.trait_type_args.clone(),
                        FunctionSignature {
                            visibility: m.visibility,
                            mangled_name: crate::typechecker::types::impl_member_mangled_name(
                                &b.trait_fqn,
                                &b.for_type,
                                &b.type_params,
                                &m.dispatch_name,
                                &b.trait_type_args,
                            ),
                            params: m.params.clone(),
                            return_type: m.return_type.clone(),
                            source_file: b.source_file.clone(),
                            is_intrinsic: m.is_intrinsic,
                            is_property: m.is_property,
                            is_final_method: false,
                            is_abstract_method: false,
                        },
                    )
                })
                .collect();
            if !impl_pairs.is_empty() {
                let all_args = build_all_args(typed_receiver, typed_args);
                let impl_pairs: Vec<(Fqn, FunctionSignature)> = self
                    .prefer_origin_impl_pairs(impl_pairs, &all_args, &method.value, false)
                    .into_iter()
                    .map(|(f, _, sig)| (f, sig))
                    .collect();
                let display = format!("{}.{}", type_fqn.symbol, method.value);
                if let Some(err) = self.check_cross_trait_ambiguity(
                    &impl_pairs,
                    &all_args,
                    &display,
                    &method.value,
                    span,
                ) {
                    return Some(err);
                }
                let instance_overloads: Vec<FunctionSignature> =
                    impl_pairs.into_iter().map(|(_, sig)| sig).collect();
                return Some(self.resolve_overloads_with_intrinsics(
                    &type_fqn,
                    &method_sym,
                    instance_overloads,
                    all_args,
                    &display,
                    span,
                ));
            }
        }

        // 6. Generic trait impl instance methods
        {
            let arg_types: Vec<&Type> = typed_args.iter().map(|a| &a.ty).collect();
            let candidates = self.resolve_generic_trait_impl_instance(
                &typed_receiver.ty,
                &method_sym,
                &arg_types,
                type_params,
                None,
            );
            // Same `extends` origin dedup as the non-generic path: a trait
            // and a sub-trait that extends it surface ONE inherited
            // declaration, and the direct impl of the origin wins.
            let candidates = self.prefer_origin_resolved_candidates(candidates, &method.value);
            if !candidates.is_empty() {
                let all_args = build_all_args(typed_receiver, typed_args);
                let display = format!("{}.{}", all_args[0].ty, method.value);
                return Some(self.resolve_overload(&display, candidates, all_args, span));
            }
        }

        // 7. Function-typed field closure call
        if let Some(field_expr) = self.try_resolve_record_field(typed_receiver, method, span)
            && let Type::Function(param_types, return_type) = field_expr.ty.clone()
        {
            return Some(self.infer_field_closure_call(
                field_expr,
                &param_types,
                &return_type,
                typed_args.to_vec(),
                span,
                &method.value,
            ));
        }
        if let Some(field_expr) = self.try_resolve_class_field(typed_receiver, method, span)
            && let Type::Function(param_types, return_type) = field_expr.ty.clone()
        {
            return Some(self.infer_field_closure_call(
                field_expr,
                &param_types,
                &return_type,
                typed_args.to_vec(),
                span,
                &method.value,
            ));
        }

        None
    }

    /// Given pre-filtered matching candidates, pick the single match and produce a typed call.
    pub(super) fn resolve_overload(
        &mut self,
        display_name: &str,
        candidates: Vec<ResolvedFunction>,
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> TypedExpr {
        match candidates.len() {
            1 => match &candidates[0] {
                ResolvedFunction::Regular {
                    mangled_name,
                    return_type,
                    type_args,
                } => {
                    let rt = return_type.clone();
                    TypedExpr {
                        kind: TypedExprKind::FunctionCall {
                            name: mangled_name.clone(),
                            args: typed_args,
                            type_params: type_args.clone(),
                        },
                        ty: rt,
                        span: span.clone(),
                    }
                }
                ResolvedFunction::ImplMethod {
                    resolved,
                    return_type,
                } => {
                    let rt = return_type.clone();
                    TypedExpr {
                        kind: TypedExprKind::ImplFunctionCall {
                            trait_fqn: resolved.trait_fqn.clone(),
                            trait_type_params: resolved.trait_type_params.clone(),
                            for_type: resolved.for_type.clone(),
                            method_name: resolved.method_name.clone(),
                            args: typed_args,
                            method_type_params: resolved.method_type_params.clone(),
                        },
                        ty: rt,
                        span: span.clone(),
                    }
                }
                ResolvedFunction::Intrinsic {
                    intrinsic,
                    return_type,
                } => {
                    let rt = return_type.clone();
                    TypedExpr {
                        kind: TypedExprKind::IntrinsicCall {
                            intrinsic: intrinsic.clone(),
                            args: typed_args,
                        },
                        ty: rt,
                        span: span.clone(),
                    }
                }
                ResolvedFunction::ExtMethod {
                    ext_fqn,
                    for_type,
                    method_name,
                    type_args,
                    return_type,
                } => {
                    let rt = return_type.clone();
                    TypedExpr {
                        kind: TypedExprKind::ExtFunctionCall {
                            ext_fqn: ext_fqn.clone(),
                            for_type: for_type.clone(),
                            method_name: method_name.clone(),
                            args: typed_args,
                            type_params: type_args.clone(),
                        },
                        ty: rt,
                        span: span.clone(),
                    }
                }
            },
            0 => {
                // Suppress overload errors when TypeParameter types are involved —
                // resolution may fail due to unresolved type params.
                let involves_type_params =
                    typed_args.iter().any(|a| a.ty.contains_type_parameter());
                if !involves_type_params {
                    self.diagnostics.error(
                        span.clone(),
                        format!("no matching overload for '{}'", display_name),
                    );
                }
                self.error_call(typed_args, span)
            }
            _ => {
                // Tailor the message when the ambiguity is between distinct
                // extensions or distinct traits — the explicit-call syntax is
                // the user's escape hatch (trait-design-appendix §2.3).
                let ext_names: Vec<&Fqn> = {
                    let mut seen: Vec<&Fqn> = Vec::new();
                    for c in &candidates {
                        if let ResolvedFunction::ExtMethod { ext_fqn, .. } = c
                            && !seen.contains(&ext_fqn)
                        {
                            seen.push(ext_fqn);
                        }
                    }
                    seen
                };
                let impl_traits: Vec<&Fqn> = {
                    let mut seen: Vec<&Fqn> = Vec::new();
                    for c in &candidates {
                        if let ResolvedFunction::ImplMethod { resolved, .. } = c
                            && !seen.contains(&&resolved.trait_fqn)
                        {
                            seen.push(&resolved.trait_fqn);
                        }
                    }
                    seen
                };
                let all_ext = candidates
                    .iter()
                    .all(|c| matches!(c, ResolvedFunction::ExtMethod { .. }));
                let all_impl = candidates
                    .iter()
                    .all(|c| matches!(c, ResolvedFunction::ImplMethod { .. }));
                let member = display_name.rsplit('.').next().unwrap_or(display_name);
                let message = if all_ext && ext_names.len() > 1 {
                    let names: Vec<String> = ext_names
                        .iter()
                        .map(|f| format!("'{}'", f.symbol))
                        .collect();
                    format!(
                        "ambiguous call to '{}': provided by extension {}; use '{}.{}(...)' to choose one",
                        display_name,
                        names.join(" and extension "),
                        ext_names[0].symbol,
                        member,
                    )
                } else if all_impl && impl_traits.len() > 1 {
                    let names: Vec<String> = impl_traits
                        .iter()
                        .map(|f| format!("'{}'", f.symbol))
                        .collect();
                    format!(
                        "ambiguous call to '{}': implemented by trait {}; use '{}.{}(...)' to choose one",
                        display_name,
                        names.join(" and trait "),
                        impl_traits[0].symbol,
                        member,
                    )
                } else {
                    format!(
                        "ambiguous call to '{}': {} overloads match",
                        display_name,
                        candidates.len()
                    )
                };
                self.diagnostics.error(span.clone(), message);
                self.error_call(typed_args, span)
            }
        }
    }

    /// Try to resolve a method call as a qualified function call.
    /// Returns overloads if the receiver resolves to a package import or FQN path.
    pub(super) fn resolve_qualified_call(
        &self,
        receiver: &Expr,
        method_name: &str,
    ) -> Option<Vec<FunctionSignature>> {
        use crate::common::types::PackagePath;
        use crate::typechecker::imports::ImportTarget;

        // 1. Check if receiver is an identifier matching a package import alias
        if let Expr::Identifier(name, _) = receiver
            && let Some(resolved) = self.import_scope.lookup(name)
            && let ImportTarget::Package(ref pkg) = resolved.target
        {
            return self.registry.lookup_function_in_package(
                pkg,
                method_name,
                &self.package_path,
                &self.current_file,
            );
        }

        // 2. Try to flatten receiver into a path for FQN resolution
        if let Some(path) = Self::try_flatten_to_path(receiver) {
            // path + method_name = full FQN: path is the package, method_name is the symbol
            let mut full_path = path;
            full_path.push(method_name);

            // All-but-last = package, last = symbol
            let pkg = PackagePath(
                full_path[..full_path.len() - 1]
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            );
            let fqn = Fqn {
                package: pkg,
                symbol: SymbolName(full_path.last().unwrap().to_string()),
            };
            if let Some(sigs) =
                self.registry
                    .lookup_function(&fqn, &self.package_path, &self.current_file)
            {
                return Some(sigs);
            }
        }

        None
    }

    /// Try to flatten an expression into a dotted path (for FQN resolution).
    pub(super) fn try_flatten_to_path(expr: &Expr) -> Option<Vec<&str>> {
        match expr {
            Expr::Identifier(name, _) => Some(vec![name.as_str()]),
            Expr::FieldAccess { object, field, .. } => {
                let mut path = Self::try_flatten_to_path(object)?;
                path.push(&field.value);
                Some(path)
            }
            _ => None,
        }
    }

    /// Look up non-generic extension methods for a type + method name from import scope.
    /// Returns cloned `(block, method)` pairs for matching methods.
    pub(super) fn lookup_named_extension_methods(
        &self,
        type_fqn: &Fqn,
        method_name: &SymbolName,
    ) -> Vec<(ExtensionBlockSignature, ExtMethodSignature)> {
        self.import_scope
            .extension_blocks
            .iter()
            .filter(|b| {
                b.for_type.try_to_fqn().is_some_and(|f| f == *type_fqn) && b.type_params.is_empty()
            })
            .flat_map(|b| {
                b.methods
                    .iter()
                    .chain(b.properties.iter())
                    .filter(|m| m.name == *method_name)
                    .filter(|m| {
                        crate::typechecker::registry::is_accessible(
                            m.visibility,
                            &b.package,
                            &self.package_path,
                            &b.source_file,
                            &self.current_file,
                        )
                    })
                    .map(move |m| (b.clone(), m.clone()))
            })
            .collect()
    }

    /// Resolve `EmbeddedResource.bytes("literal")` to an embedded-resource
    /// intrinsic call. Validates the literal at compile time against the
    /// calling project's declared resources (registry-side); on success
    /// emits an `IntrinsicCall { ResourceBytes { resource_name,
    /// declaring_root }, ... }` whose codegen lowers to `array.new_data`
    /// over a passive WASM segment.
    fn resolve_resource_bytes_intrinsic(&mut self, args: &[Expr], span: &Span) -> TypedExpr {
        if args.len() != 1 {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "'EmbeddedResource.bytes' expects 1 argument, found {}",
                    args.len()
                ),
            );
            return self.error_call(vec![], span);
        }
        let literal = match &args[0] {
            Expr::StringLiteral(s, _) => s.clone(),
            _ => {
                self.diagnostics.error(
                    args[0].span(),
                    "'EmbeddedResource.bytes' requires a string literal as its argument (the resource name is resolved at compile time)".to_string(),
                );
                return self.error_call(vec![], span);
            }
        };
        match self.registry.lookup_resource(&self.package_path, &literal) {
            Some((root, _bytes)) => TypedExpr {
                ty: Type::Array(Box::new(Type::Uint8)),
                kind: TypedExprKind::IntrinsicCall {
                    intrinsic: IntrinsicKind::ResourceBytes {
                        resource_name: literal,
                        declaring_root: root.clone(),
                    },
                    args: vec![],
                },
                span: span.clone(),
            },
            None => {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "no resource named '{}' declared for package '{}'; add it to the project's `Dovetail.toml` `resources = [...]` list",
                        literal, self.package_path
                    ),
                );
                self.error_call(vec![], span)
            }
        }
    }

    /// Produce an error-typed function call expression.
    ///
    /// The `<error>` name is a placeholder that only exists to keep inference going
    /// after a failure, so that the rest of the run can accumulate its own errors.
    /// It is never callable: nothing downstream can give it a function index, and
    /// `monomorphize` will copy it into every instantiation untouched. So it is only
    /// safe to build one once the failure has been *reported* — a diagnostic is what
    /// stops the pipeline before codegen. Producing one silently means the compiler
    /// accepts the program and then traps while emitting it.
    pub(super) fn error_call(&self, typed_args: Vec<TypedExpr>, span: &Span) -> TypedExpr {
        debug_assert!(
            self.diagnostics.has_errors(),
            "error node built at {:?} without reporting a diagnostic",
            span
        );
        TypedExpr {
            kind: TypedExprKind::FunctionCall {
                name: MangledName("<error>".to_string()),
                args: typed_args,
                type_params: vec![],
            },
            ty: Type::Error,
            span: span.clone(),
        }
    }

    /// Produce an error-typed expression. Same node and same rule as `error_call`:
    /// only build one after reporting the failure it stands in for.
    pub(super) fn error_expr(&self, span: &Span) -> TypedExpr {
        self.error_call(vec![], span)
    }

    /// Try to resolve `Some(x)`, `Ok(x)`, or `Error(x)` as prelude enum variant construction.
    /// Returns `Ok(expr)` on success, `Err(typed_args)` when not a bare variant (gives args back).
    fn try_resolve_bare_variant_call(
        &mut self,
        name: &str,
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> Result<TypedExpr, Vec<TypedExpr>> {
        let enum_name = match name {
            "Some" => "Option",
            "Ok" | "Error" => "Result",
            _ => return Err(typed_args),
        };
        let enum_sig = match self.resolve_enum_type(enum_name) {
            Some(sig) => sig,
            None => return Err(typed_args),
        };
        let payload_types = match enum_sig.variants.iter().find(|(v, _)| v == name) {
            Some((_, VariantPayload::Tuple(types))) => types.clone(),
            Some((_, VariantPayload::None | VariantPayload::Record(_))) => return Err(typed_args),
            None => return Err(typed_args),
        };
        self.check_private_type_access(
            &enum_sig.fqn,
            enum_sig.construction_private,
            "enum",
            span,
            "construct",
        );

        if typed_args.len() != payload_types.len() {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "'{}' expects {} argument(s), found {}",
                    name,
                    payload_types.len(),
                    typed_args.len()
                ),
            );
            return Ok(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }
        // Unify payload types against arg types to infer type params
        let mut substitution = super::type_param_substitution::TypeParamSubstitution::new();
        for (payload_ty, arg) in payload_types.iter().zip(typed_args.iter()) {
            substitution.unify(payload_ty, &arg.ty);
        }
        let resolved = substitution.resolve_type_params(&enum_sig.type_params);
        let type_args: Vec<Type> = match resolved {
            Some(args) => args,
            _ => {
                // Fallback: try expected_type
                match &self.expected_type {
                    Some(Type::GenericEnum {
                        fqn: exp_fqn,
                        type_args: expected_args,
                        ..
                    }) if *exp_fqn == enum_sig.fqn => {
                        // Merge payload unification results with expected type args.
                        // Prefer concrete bindings from payload unification (e.g. T=Int32 from Ok(x))
                        // over the expected type args.
                        expected_args
                            .iter()
                            .zip(enum_sig.type_params.iter())
                            .map(|((_, exp_ty), tp)| match substitution.get(tp) {
                                Some(ty)
                                    if !matches!(
                                        ty,
                                        Type::TypeVariable(..) | Type::GenericParam(..)
                                    ) =>
                                {
                                    ty.clone()
                                }
                                _ => exp_ty.clone(),
                            })
                            .collect()
                    }
                    _ => {
                        // Default covariant params to Never
                        match substitution.resolve_with_variance_defaults(
                            &enum_sig.type_params,
                            &enum_sig.type_param_variances,
                        ) {
                            Some(args) => args,
                            None => {
                                self.diagnostics.error(
                                    span.clone(),
                                    format!("cannot infer type arguments for '{}'", name),
                                );
                                return Ok(TypedExpr {
                                    kind: TypedExprKind::UnitLiteral,
                                    ty: Type::Error,
                                    span: span.clone(),
                                });
                            }
                        }
                    }
                }
            }
        };
        let enum_ty = self.resolve_generic_enum_type(&enum_sig.fqn, &enum_sig, &type_args);
        let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
            &enum_sig.type_params,
            &type_args,
        );
        let concrete_payload: Vec<Type> = payload_types
            .iter()
            .map(|t| apply_substitution(&sub, t))
            .collect();
        for (arg, expected) in typed_args.iter().zip(concrete_payload.iter()) {
            self.check_assignable(arg.span.clone(), expected, &arg.ty);
        }
        Ok(TypedExpr {
            ty: enum_ty,
            kind: TypedExprKind::EnumCreate {
                fqn: enum_sig.fqn.clone(),
                variant_name: name.to_string(),
                args: typed_args,
                type_params: type_args.clone(),
            },
            span: span.clone(),
        })
    }

    /// Derive expected argument types for bare variant calls from the outer expected_type.
    /// For example, if expected_type is `Result<T, Result<Never, E>>` and name is "Error",
    /// the Error variant payload type `E` gets substituted to `Result<Never, E>`.
    fn derive_bare_variant_expected_args(
        &self,
        name: &str,
        arg_count: usize,
    ) -> Option<Vec<Option<Type>>> {
        let enum_name = match name {
            "Some" => "Option",
            "Ok" | "Error" => "Result",
            _ => return None,
        };
        let expected = self.expected_type.as_ref()?;
        let (exp_fqn, exp_type_args) = match expected {
            Type::GenericEnum { fqn, type_args, .. } => (fqn, type_args),
            _ => return None,
        };
        if exp_fqn.symbol.0 != enum_name {
            return None;
        }
        let enum_sig = self.resolve_enum_type(enum_name)?;
        let payload_types = match enum_sig.variants.iter().find(|(v, _)| v == name) {
            Some((_, VariantPayload::Tuple(types))) => types,
            _ => return None,
        };
        if payload_types.len() != arg_count {
            return None;
        }
        let concrete_type_args: Vec<Type> = exp_type_args.iter().map(|(_, t)| t.clone()).collect();
        let sub = super::type_param_substitution::TypeParamSubstitution::from_pairs(
            &enum_sig.type_params,
            &concrete_type_args,
        );
        Some(
            payload_types
                .iter()
                .map(|t| Some(super::generics::apply_substitution(&sub, t)))
                .collect(),
        )
    }

    /// Try to resolve a bare function call as a newtype constructor: `Cents(100)` or `Wrapper<Int32>(42)`.
    fn try_resolve_newtype_call(
        &mut self,
        name: &str,
        type_args: &[TypeExpr],
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> Result<TypedExpr, Vec<TypedExpr>> {
        let fqn = match self.resolve_fqn(name, super::types::SymbolKind::Newtype) {
            Some(fqn) => fqn,
            None => return Err(typed_args),
        };
        let sig =
            match self
                .registry
                .lookup_newtype_type(&fqn, &self.package_path, &self.current_file)
            {
                Some(sig) => sig.clone(),
                None => return Err(typed_args),
            };
        if !self.check_newtype_inner_access(&sig, span, "construct") {
            return Ok(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }
        if typed_args.len() != 1 {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "newtype '{}' constructor expects 1 argument, found {}",
                    name,
                    typed_args.len()
                ),
            );
            return Ok(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }

        // Handle generic newtypes
        if !sig.type_params.is_empty() {
            let resolved_type_args = if !type_args.is_empty() {
                // Explicit type args: `Wrapper<Int32>(42)`
                if type_args.len() != sig.type_params.len() {
                    self.diagnostics.error(
                        span.clone(),
                        format!(
                            "expected {} type argument(s) for '{}', found {}",
                            sig.type_params.len(),
                            name,
                            type_args.len()
                        ),
                    );
                    return Ok(TypedExpr {
                        kind: TypedExprKind::UnitLiteral,
                        ty: Type::Error,
                        span: span.clone(),
                    });
                }
                match self.resolve_type_args(type_args) {
                    Some(args) => args,
                    None => {
                        return Ok(TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        });
                    }
                }
            } else {
                // Infer type args from the argument type by unifying inner type with actual arg type
                match self.infer_newtype_type_args(&sig, &typed_args[0].ty, span) {
                    Some(args) => args,
                    None => {
                        return Ok(TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: span.clone(),
                        });
                    }
                }
            };

            let newtype_ty = self.resolve_generic_newtype(&fqn, &sig, &resolved_type_args, span);
            if let Type::GenericNewtype {
                concrete_inner_type,
                ..
            } = &newtype_ty
            {
                self.check_assignable(
                    typed_args[0].span.clone(),
                    concrete_inner_type,
                    &typed_args[0].ty,
                );
            }
            return Ok(TypedExpr {
                ty: newtype_ty,
                kind: TypedExprKind::NewtypeCreate {
                    value: Box::new(typed_args.into_iter().next().unwrap()),
                },
                span: span.clone(),
            });
        }

        // Non-generic newtype with type args → error
        if !type_args.is_empty() {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "newtype '{}' is not generic but was given type arguments",
                    name
                ),
            );
            return Ok(TypedExpr {
                kind: TypedExprKind::UnitLiteral,
                ty: Type::Error,
                span: span.clone(),
            });
        }

        self.check_assignable(
            typed_args[0].span.clone(),
            &sig.inner_type,
            &typed_args[0].ty,
        );
        let newtype_ty = Type::Newtype(fqn, Box::new(sig.inner_type));
        Ok(TypedExpr {
            ty: newtype_ty,
            kind: TypedExprKind::NewtypeCreate {
                value: Box::new(typed_args.into_iter().next().unwrap()),
            },
            span: span.clone(),
        })
    }

    /// Infer type arguments for a generic newtype from the argument type.
    /// Unifies the signature's inner type (which may contain TypeParameter) with the actual arg type.
    /// Falls back to `expected_type` for bidirectional inference when argument-based unification
    /// leaves type parameters unresolved.
    fn infer_newtype_type_args(
        &mut self,
        sig: &crate::typechecker::registry::NewtypeSignature,
        arg_ty: &Type,
        span: &Span,
    ) -> Option<Vec<Type>> {
        let mut substitution = TypeParamSubstitution::new();
        substitution.unify(&sig.inner_type, arg_ty);

        // Fallback: use expected_type for bidirectional inference
        // e.g. `let p: Parser<Int32> = Parser(...)` or return type `Parser<T>`
        if let Some(Type::GenericNewtype {
            fqn: expected_fqn,
            type_args: expected_args,
            ..
        }) = self.expected_type.as_ref()
            && *expected_fqn == sig.fqn
        {
            let placeholder_args: Vec<Type> = sig
                .type_params
                .iter()
                .map(|tp| {
                    Type::TypeVariable(
                        tp.clone(),
                        sig.trait_bounds.get(tp).cloned().unwrap_or_default(),
                    )
                })
                .collect();
            for (placeholder, (_, expected_arg)) in
                placeholder_args.iter().zip(expected_args.iter())
            {
                substitution.unify(placeholder, expected_arg);
            }
        }

        let mut resolved = Vec::with_capacity(sig.type_params.len());
        for tp in &sig.type_params {
            if let Some(ty) = substitution.get(tp) {
                resolved.push(ty.clone());
            } else {
                self.diagnostics.error(
                    span.clone(),
                    format!(
                        "cannot infer type argument '{}' for generic newtype '{}'; provide explicit type arguments",
                        tp, sig.fqn.symbol
                    ),
                );
                return None;
            }
        }
        Some(resolved)
    }

    /// Infer arguments with optional expected type hints.
    /// When the expected type is `ByName<T>`, infers against `T` (the inner type)
    /// so the argument expression is type-checked correctly. The actual wrapping
    /// is done by the `coerce_byname` pass before capture analysis.
    fn infer_args_with_expected(
        &mut self,
        args: &[Expr],
        expected_arg_types: &Option<Vec<Option<Type>>>,
    ) -> Vec<TypedExpr> {
        if args.iter().any(|arg| matches!(arg, Expr::AsyncDo { .. })) {
            let hints = expected_arg_types
                .clone()
                .unwrap_or_else(|| vec![None; args.len()]);
            let typed = args
                .iter()
                .enumerate()
                .map(|(i, arg)| {
                    if matches!(arg, Expr::AsyncDo { .. })
                        || super::async_arguments::deferred_async_sibling(arg)
                    {
                        return TypedExpr {
                            kind: TypedExprKind::UnitLiteral,
                            ty: Type::Error,
                            span: arg.span(),
                        };
                    }
                    let saved = self.expected_type.take();
                    self.expected_type = hints
                        .get(i)
                        .cloned()
                        .flatten()
                        .map(|ty| extract_byname_inner(&ty).cloned().unwrap_or(ty))
                        .filter(|ty| {
                            !ty.contains_type_parameter() || matches!(arg, Expr::Closure { .. })
                        });
                    let typed = self.infer_expr(arg);
                    self.expected_type = saved;
                    typed
                })
                .collect();
            return self.infer_deferred_async_arguments(args, typed, &hints);
        }
        args.iter()
            .enumerate()
            .map(|(i, arg)| {
                let prev = self.expected_type.take();
                if let Some(expected) = expected_arg_types {
                    let expected_ty = expected.get(i).cloned().flatten();
                    self.expected_type = match &expected_ty {
                        Some(ty) => match extract_byname_inner(ty) {
                            Some(inner) => Some(inner.clone()),
                            None => expected_ty,
                        },
                        None => None,
                    };
                }
                let typed = self.infer_expr(arg);
                self.expected_type = prev;
                typed
            })
            .collect()
    }

    /// True for argument expressions like `EnumName.method` where `EnumName`
    /// has multiple visible static `implement`-block methods of that name
    /// (e.g. several `From<T> for EnumName` impls all named `from`). Such
    /// references need an `expected_type` from the surrounding method's
    /// parameter type to disambiguate; without it, first-pass inference
    /// can't pick an overload. Deferring them lets the second pass
    /// re-infer with that context.
    fn is_ambiguous_static_method_ref(&self, arg: &Expr) -> bool {
        if let Expr::FieldAccess {
            object,
            object_type_params,
            field,
            field_type_params,
            ..
        } = arg
        {
            if !object_type_params.is_empty() || !field_type_params.is_empty() {
                return false;
            }
            if let Expr::Identifier(name, _) = object.as_ref()
                && let Some(enum_sig) = self.resolve_enum_type(name)
            {
                let member_sym = SymbolName(field.value.clone());
                let matching = self
                    .registry
                    .find_impl_method(&enum_sig.fqn, &member_sym)
                    .into_iter()
                    .filter(|(_, m)| {
                        !m.is_property && (m.params.is_empty() || m.params[0].0 != "self")
                    })
                    .count();
                return matching > 1;
            }
        }
        false
    }

    /// Look up the expected argument types for a method call on a given receiver type.
    /// Used to provide expected types for closure arguments that couldn't be inferred
    /// during the initial (eager) argument inference pass.
    ///
    /// Returns `Some(expected_types)` where each element is the expected type for the
    /// corresponding non-self argument, with unresolved method-level type parameters
    /// replaced by `Type::Error` (so closures can infer parameter types without
    /// spurious return-type errors).
    /// Returns expected argument types and the list of unresolved method-level type param names.
    fn lookup_method_expected_arg_types(
        &mut self,
        receiver_ty: &Type,
        method_name: &str,
        num_args: usize,
        explicit_method_type_params: &[TypeExpr],
        known_args: &[TypedExpr],
    ) -> Option<(Vec<Option<Type>>, Vec<TypeParamName>)> {
        if receiver_ty.is_error()
            || matches!(
                receiver_ty,
                Type::TypeVariable(_, _) | Type::GenericParam(_, _, _)
            )
        {
            return None;
        }

        // Intersections have no single FQN (and no module/impl lookups apply).
        let type_fqn = receiver_ty.try_to_fqn()?;
        let method_sym = SymbolName(method_name.to_string());
        let bind_known_args = |sub: &mut TypeParamSubstitution, params: &[(String, Type)]| {
            for ((_, param), arg) in params.iter().skip(1).zip(known_args) {
                if !arg.ty.is_error() {
                    sub.unify(param, &arg.ty);
                }
            }
        };

        if let Some(module_info) = self.registry.lookup_module(&type_fqn) {
            // Try non-generic module instance methods
            if let Some(overloads) = module_info.functions.get(&method_sym) {
                for sig in overloads {
                    if !self.named_signature_allowed(&sig.params) {
                        continue;
                    }
                    if sig.params.is_empty() || sig.params[0].0 != "self" {
                        continue;
                    }
                    if sig.params.len() - 1 != num_args {
                        continue;
                    }
                    return Some((
                        sig.params[1..]
                            .iter()
                            .map(|(_, ty)| Some(ty.clone()))
                            .collect(),
                        Vec::new(),
                    ));
                }
            }

            // Try generic module instance methods
            let defs: Vec<_> = module_info
                .generic_members
                .lookup_visible(&method_sym, &self.package_path, &self.current_file)
                .into_iter()
                .cloned()
                .collect();

            for def in defs {
                if !self.named_signature_allowed(&def.params) {
                    continue;
                }
                if def.is_property || def.params.is_empty() || def.params[0].0 != "self" {
                    continue;
                }
                if def.params.len() - 1 != num_args {
                    continue;
                }

                // Unify for_type against receiver to bind module-level type params
                let mut substitution = TypeParamSubstitution::new();
                if !substitution.unify(&def.for_type, receiver_ty) {
                    continue;
                }

                bind_known_args(&mut substitution, &def.params);

                // Try to resolve method-level type params from the expected return type
                // context (e.g. fold<U, E2> return type Async<U, E2> vs expected Async<T, E2>).
                // We reject any binding that resolves to Never, because Never can leak
                // here from the closure unresolved-type-param substitution: closures with
                // unresolved method type params get Never as a sentinel in their expected
                // return type (for enum variant resolution). If that Never propagates into
                // self.expected_type and we used it here, we'd incorrectly bind U=Never
                // in nested method calls, causing cascading type errors.
                if !def.method_type_params.is_empty() {
                    // First, try to bind from explicit type params on the call site
                    if !explicit_method_type_params.is_empty()
                        && explicit_method_type_params.len() == def.method_type_params.len()
                        && let Some(method_args) =
                            self.resolve_type_args(explicit_method_type_params)
                    {
                        for (tp, arg) in def.method_type_params.iter().zip(method_args.iter()) {
                            substitution.insert(tp.clone(), arg.clone());
                        }
                    }

                    // Then, try to resolve remaining from expected return type context
                    if let Some(ref expected) = self.expected_type {
                        let method_return_substituted =
                            apply_substitution(&substitution, &def.return_type);
                        let mut trial = substitution.clone();
                        trial.unify(&method_return_substituted, expected);
                        // Only keep bindings that don't map method type params to Never
                        for tp in &def.method_type_params {
                            if substitution.get(tp).is_none()
                                && let Some(resolved) = trial.get(tp)
                                && *resolved != Type::Never
                            {
                                substitution.insert(tp.clone(), resolved.clone());
                            }
                        }
                    }
                }

                // Collect unresolved method-level type param names
                let unresolved: Vec<TypeParamName> = def
                    .method_type_params
                    .iter()
                    .filter(|tp| substitution.get(tp).is_none())
                    .cloned()
                    .collect();

                // Apply substitution to non-self param types
                return Some((
                    def.params[1..]
                        .iter()
                        .map(|(_, ty)| Some(apply_substitution(&substitution, ty)))
                        .collect(),
                    unresolved,
                ));
            }
        }

        // Try class instance methods (classes store methods in ClassTypeSignature, not modules)
        if matches!(receiver_ty, Type::Class(..) | Type::GenericClass { .. })
            && let Some(class_sig) = self
                .registry
                .lookup_class_type(&type_fqn, &self.package_path)
        {
            // Walk the class hierarchy
            let mut current_sig = class_sig.clone();
            loop {
                if let Some(overloads) = current_sig.instance_methods.get(&method_sym) {
                    for sig in overloads {
                        if !self.named_signature_allowed(&sig.params) {
                            continue;
                        }
                        // Instance methods have self as first param
                        if sig.params.is_empty() || sig.params[0].0 != "self" {
                            continue;
                        }
                        if sig.params.len() - 1 != num_args {
                            continue;
                        }
                        return Some((
                            sig.params[1..]
                                .iter()
                                .map(|(_, ty)| Some(ty.clone()))
                                .collect(),
                            Vec::new(),
                        ));
                    }
                }
                // Check generic instance methods on the class
                if let Some(defs) = current_sig.generic_instance_methods.get(&method_sym) {
                    for def in defs {
                        if !self.named_signature_allowed(&def.params) {
                            continue;
                        }
                        if def.params.is_empty() || def.params[0].0 != "self" {
                            continue;
                        }
                        if def.params.len() - 1 != num_args {
                            continue;
                        }
                        // Unify class type params from receiver type args
                        let mut substitution = TypeParamSubstitution::new();
                        if let Type::GenericClass { type_args, .. } = receiver_ty {
                            for (tp, (_, ta)) in def.class_type_params.iter().zip(type_args.iter())
                            {
                                substitution.insert(tp.clone(), ta.clone());
                            }
                        }
                        let unresolved: Vec<TypeParamName> = def
                            .method_type_params
                            .iter()
                            .filter(|tp| substitution.get(tp).is_none())
                            .cloned()
                            .collect();
                        return Some((
                            def.params[1..]
                                .iter()
                                .map(|(_, ty)| Some(apply_substitution(&substitution, ty)))
                                .collect(),
                            unresolved,
                        ));
                    }
                }
                // Walk up to parent class
                if let Some(ref parent) = current_sig.parent_class
                    && let Some(parent_sig) =
                        self.registry.lookup_class_type(parent, &self.package_path)
                {
                    current_sig = parent_sig.clone();
                    continue;
                }
                break;
            }
        }

        // Try extension instance methods (extensions resolve before trait impls,
        // so their expected arg types must be surfaced first too).
        for (_, method) in self.lookup_named_extension_methods(&type_fqn, &method_sym) {
            if method.is_property || method.params.is_empty() || method.params[0].0 != "self" {
                continue;
            }
            if method.params.len() - 1 != num_args {
                continue;
            }
            return Some((
                method.params[1..]
                    .iter()
                    .map(|(_, ty)| Some(ty.clone()))
                    .collect(),
                method.method_type_params.clone(),
            ));
        }
        for (block, method) in self.lookup_all_generic_extension_methods(receiver_ty, &method_sym) {
            if method.is_property || method.params.is_empty() || method.params[0].0 != "self" {
                continue;
            }
            if method.params.len() - 1 != num_args {
                continue;
            }
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.unify(&block.for_type, receiver_ty) {
                continue;
            }
            bind_known_args(&mut substitution, &method.params);
            let unresolved: Vec<TypeParamName> = method
                .method_type_params
                .iter()
                .filter(|tp| substitution.get(tp).is_none())
                .cloned()
                .collect();
            return Some((
                method.params[1..]
                    .iter()
                    .map(|(_, ty)| Some(apply_substitution(&substitution, ty)))
                    .collect(),
                unresolved,
            ));
        }

        let trait_defs = self.registry.find_impl_method(&type_fqn, &method_sym);
        for (block, method) in trait_defs {
            if method.params.is_empty() || method.params[0].0 != "self" {
                continue;
            }
            if method.params.len() - 1 != num_args {
                continue;
            }

            let mut substitution = TypeParamSubstitution::new();
            if !substitution.unify(&block.for_type, receiver_ty) {
                continue;
            }

            bind_known_args(&mut substitution, &method.params);
            let unresolved: Vec<TypeParamName> = method
                .method_type_params
                .iter()
                .filter(|tp| substitution.get(tp).is_none())
                .cloned()
                .collect();

            return Some((
                method.params[1..]
                    .iter()
                    .map(|(_, ty)| Some(apply_substitution(&substitution, ty)))
                    .collect(),
                unresolved,
            ));
        }

        None
    }

    /// Infer a closure call: `f(args)` where `f` is a variable with function type.
    fn infer_closure_call(
        &mut self,
        name: &crate::common::span::Spanned<String>,
        param_types: &[Type],
        return_type: &Type,
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> TypedExpr {
        if param_types.len() != typed_args.len() {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "'{}' expects {} argument(s) but {} were provided",
                    name.value,
                    param_types.len(),
                    typed_args.len()
                ),
            );
            return self.error_call(typed_args, span);
        }
        for (i, (param_ty, arg)) in param_types.iter().zip(typed_args.iter()).enumerate() {
            if !self.is_assignable(param_ty, &arg.ty) {
                self.diagnostics.error(
                    arg.span.clone(),
                    format!(
                        "argument {} of '{}': expected '{}', got '{}'",
                        i + 1,
                        name.value,
                        param_ty,
                        arg.ty
                    ),
                );
            }
        }
        let callee = TypedExpr {
            kind: TypedExprKind::VarRef {
                name: VarName(name.value.clone()),
                boxed: false,
            },
            ty: Type::Function(param_types.to_vec(), Box::new(return_type.clone())),
            span: name.span.clone(),
        };
        TypedExpr {
            kind: TypedExprKind::ClosureCall {
                callee: Box::new(callee),
                args: typed_args,
            },
            ty: return_type.clone(),
            span: span.clone(),
        }
    }

    /// Infer a closure call where the callee is a field access expression (record/class field
    /// with function type). Similar to `infer_closure_call` but takes a pre-built callee `TypedExpr`.
    fn infer_field_closure_call(
        &mut self,
        callee: TypedExpr,
        param_types: &[Type],
        return_type: &Type,
        typed_args: Vec<TypedExpr>,
        span: &Span,
        display_name: &str,
    ) -> TypedExpr {
        if param_types.len() != typed_args.len() {
            self.diagnostics.error(
                span.clone(),
                format!(
                    "'{}' expects {} argument(s) but {} were provided",
                    display_name,
                    param_types.len(),
                    typed_args.len()
                ),
            );
            return self.error_call(typed_args, span);
        }
        for (i, (param_ty, arg)) in param_types.iter().zip(typed_args.iter()).enumerate() {
            if !self.is_assignable(param_ty, &arg.ty) {
                self.diagnostics.error(
                    arg.span.clone(),
                    format!(
                        "argument {} of '{}': expected '{}', got '{}'",
                        i + 1,
                        display_name,
                        param_ty,
                        arg.ty
                    ),
                );
            }
        }
        TypedExpr {
            kind: TypedExprKind::ClosureCall {
                callee: Box::new(callee),
                args: typed_args,
            },
            ty: return_type.clone(),
            span: span.clone(),
        }
    }

    /// Produce an "undefined function" error and return an error expression.
    fn undefined_function_error(
        &mut self,
        name: &str,
        typed_args: Vec<TypedExpr>,
        span: &Span,
    ) -> TypedExpr {
        let msg = if let Some(fqn) = self.registry.suggest_import_for_function(name) {
            format!(
                "undefined function: '{}'; try adding 'import {}'",
                name, fqn
            )
        } else {
            format!("undefined function: '{}'", name)
        };
        self.diagnostics.error(span.clone(), msg);
        self.error_call(typed_args, span)
    }
}

/// Map a type FQN and method name to an IntrinsicKind.
/// The `return_type` comes from the already-resolved function signature in the prelude.
pub(super) fn resolve_intrinsic_kind(
    type_fqn: &Fqn,
    method_name: &SymbolName,
    return_type: &Type,
) -> Option<IntrinsicKind> {
    if let Some(op) = crate::typechecker::types::primitive_binary_operator(type_fqn, &method_name.0)
    {
        return Some(IntrinsicKind::BinaryOperator(op));
    }
    match (type_fqn.symbol.0.as_str(), method_name.0.as_str()) {
        ("String", "unsafe_bytes") => Some(IntrinsicKind::StringUnsafeBytes),
        ("String", "fromBytes") => Some(IntrinsicKind::StringFromBytes),
        ("String", "fromChar") => Some(IntrinsicKind::StringFromChar),
        ("String", "length") => Some(IntrinsicKind::StringLength),
        ("String", "byteLength") => Some(IntrinsicKind::StringByteLength),
        ("String", "getChar") => Some(IntrinsicKind::StringGetChar),
        ("String", "isAscii") => Some(IntrinsicKind::StringIsAscii),
        ("ReadonlySlice", "make") => Some(IntrinsicKind::ReadonlySliceMake),
        ("ReadonlySlice", "length") => Some(IntrinsicKind::ReadonlySliceLength),
        ("ReadonlySlice", "get") => Some(IntrinsicKind::ReadonlySliceGet),
        ("ReadonlySlice", "slice") => Some(IntrinsicKind::ReadonlySliceSlice),
        ("ReadonlySlice", "copyTo") => Some(IntrinsicKind::ReadonlySliceCopyTo),
        ("Array", "get") => Some(IntrinsicKind::ArrayGet),
        ("Array", "set") => Some(IntrinsicKind::ArraySet),
        ("Array", "length") => Some(IntrinsicKind::ArrayLength),
        ("Array", "clone") => Some(IntrinsicKind::ArrayClone),
        ("Array", "fill") => Some(IntrinsicKind::ArrayFill),
        ("Array", "extend") => Some(IntrinsicKind::ArrayExtend),
        ("Array", "concat") => Some(IntrinsicKind::ArrayConcat),
        ("Array", "empty") => Some(IntrinsicKind::ArrayEmpty),
        ("Array", "copy") => Some(IntrinsicKind::ArrayCopy),
        ("Math", "floor") => Some(IntrinsicKind::MathFloor),
        ("Math", "trunc") => Some(IntrinsicKind::MathTrunc),
        ("Math", "abs") => Some(IntrinsicKind::MathAbs),
        ("Math", "fmod") => Some(IntrinsicKind::MathFmod),
        ("Math", "isNan") => Some(IntrinsicKind::MathIsNan),
        ("Math", "isInfinity") => Some(IntrinsicKind::MathIsInfinity),
        ("Uint128", "multiply") => Some(IntrinsicKind::Uint128Multiply),
        ("Uint128", "make") => Some(IntrinsicKind::Uint128Make),
        ("Uint128", "high") => Some(IntrinsicKind::Uint128High),
        (_, "toBits") if matches!(return_type, Type::Int64 | Type::Int32) => {
            Some(IntrinsicKind::FloatToBits)
        }
        (_, "bitsToFloat64") if matches!(return_type, Type::Float64) => {
            Some(IntrinsicKind::BitsToFloat)
        }
        (_, "bitsToFloat32") if matches!(return_type, Type::Float32) => {
            Some(IntrinsicKind::BitsToFloat)
        }
        (_, name) if name.starts_with("to") && return_type.is_numeric() => {
            Some(IntrinsicKind::NumericConvert(return_type.clone()))
        }
        // IO intrinsics
        // Stream intrinsics (trait modules)
        // Networking intrinsics
        // TCP intrinsics
        // Random intrinsics
        // UDP intrinsics
        // Clock intrinsics
        // ── WASI p3 core ──
        ("WaitableSet", "make") => Some(IntrinsicKind::P3WaitableSetNew),
        ("WaitableSet", "join") => Some(IntrinsicKind::P3WaitableSetJoin),
        ("WaitableSet", "remove") => Some(IntrinsicKind::P3WaitableRemove),
        ("WaitableSet", "wait") => Some(IntrinsicKind::P3WaitableSetWait),
        ("WaitableSet", "pollEvent") => Some(IntrinsicKind::P3WaitableSetPoll),
        ("WaitableSet", "yieldToHost") => Some(IntrinsicKind::P3ThreadYield),
        ("WaitableSet", "close") => Some(IntrinsicKind::P3WaitableSetDrop),
        ("SubtaskHandle", "drop") => Some(IntrinsicKind::P3SubtaskDrop),
        ("SubtaskHandle", "cancel") => Some(IntrinsicKind::P3SubtaskCancel),
        ("MonotonicClock", "now") => Some(IntrinsicKind::P3MonotonicNow),
        // The nanosecond form is the ABI one; `MonotonicClock.waitForStart` is a
        // Dovetail-side wrapper over it that takes a `Duration`.
        ("MonotonicClock", "waitForNanosStart") => Some(IntrinsicKind::P3WaitForStart),
        ("MonotonicClock", "waitUntilNanosStart") => Some(IntrinsicKind::P3WaitUntilStart),
        ("StdinStream", "open") => Some(IntrinsicKind::P3StdinOpen),
        ("StdinStream", "readStart") => Some(IntrinsicKind::P3StdinReadStart),
        ("StdinStream", "dropReadable") => Some(IntrinsicKind::P3StdinDropReadable),
        ("StdinStream", "readResult") => Some(IntrinsicKind::P3StdinReadResult),
        ("StdinStream", "dropResultFuture") => Some(IntrinsicKind::P3StdinDropResultFuture),
        ("Stream", "readFinish") => Some(IntrinsicKind::P3StreamReadFinish),
        ("Stream", "discard") => Some(IntrinsicKind::P3StreamDiscard),
        ("FileDescriptor", "openAtStart") => Some(IntrinsicKind::P3FsOpenAtStart),
        ("FileDescriptor", "openAtFinish") => Some(IntrinsicKind::P3FsOpenAtFinish),
        ("FileDescriptor", "statStart") => Some(IntrinsicKind::P3FsStatStart),
        ("FileDescriptor", "statFinish") => Some(IntrinsicKind::P3FsStatFinish),
        ("FileDescriptor", "createDirectoryAtStart") => {
            Some(IntrinsicKind::P3FsCreateDirectoryAtStart)
        }
        ("FileDescriptor", "unlinkFileAtStart") => Some(IntrinsicKind::P3FsUnlinkFileAtStart),
        ("FileDescriptor", "removeDirectoryAtStart") => {
            Some(IntrinsicKind::P3FsRemoveDirectoryAtStart)
        }
        ("FileDescriptor", "unitFinish") => Some(IntrinsicKind::P3FsUnitFinish),
        ("FileDescriptor", "statAtStart") => Some(IntrinsicKind::P3FsStatAtStart),
        ("FileDescriptor", "setSizeStart") => Some(IntrinsicKind::P3FsSetSizeStart),
        ("FileDescriptor", "syncStart") => Some(IntrinsicKind::P3FsSyncStart),
        ("FileDescriptor", "syncDataStart") => Some(IntrinsicKind::P3FsSyncDataStart),
        ("FileDescriptor", "adviseStart") => Some(IntrinsicKind::P3FsAdviseStart),
        ("FileDescriptor", "getFlagsStart") => Some(IntrinsicKind::P3FsGetFlagsStart),
        ("FileDescriptor", "getFlagsFinish") => Some(IntrinsicKind::P3FsGetFlagsFinish),
        ("FileDescriptor", "getTypeStart") => Some(IntrinsicKind::P3FsGetTypeStart),
        ("FileDescriptor", "getTypeFinish") => Some(IntrinsicKind::P3FsGetTypeFinish),
        ("FileDescriptor", "isSameObjectStart") => Some(IntrinsicKind::P3FsIsSameObjectStart),
        ("FileDescriptor", "isSameObjectFinish") => Some(IntrinsicKind::P3FsIsSameObjectFinish),
        ("FileDescriptor", "metadataHashStart") => Some(IntrinsicKind::P3FsMetadataHashStart),
        ("FileDescriptor", "metadataHashAtStart") => Some(IntrinsicKind::P3FsMetadataHashAtStart),
        ("FileDescriptor", "metadataHashFinish") => Some(IntrinsicKind::P3FsMetadataHashFinish),
        ("FileDescriptor", "linkAtStart") => Some(IntrinsicKind::P3FsLinkAtStart),
        ("FileDescriptor", "readlinkAtStart") => Some(IntrinsicKind::P3FsReadlinkAtStart),
        ("FileDescriptor", "readlinkAtFinish") => Some(IntrinsicKind::P3FsReadlinkAtFinish),
        ("FileDescriptor", "setTimesStart") => Some(IntrinsicKind::P3FsSetTimesStart),
        ("FileDescriptor", "setTimesAtStart") => Some(IntrinsicKind::P3FsSetTimesAtStart),
        ("FileDescriptor", "readDirectory") => Some(IntrinsicKind::P3FsReadDirectory),
        ("FsStreamOps", "entryReadStart") => Some(IntrinsicKind::P3FsEntryReadStart),
        ("FsStreamOps", "entryReadFinish") => Some(IntrinsicKind::P3FsEntryReadFinish),
        ("FsStreamOps", "dropEntryReadable") => Some(IntrinsicKind::P3FsDropEntryReadable),
        ("FsStreamOps", "dropEntryResult") => Some(IntrinsicKind::P3FsDropEntryResult),
        ("TcpSocket", "create") => Some(IntrinsicKind::P3TcpCreate),
        ("TcpSocket", "bind") => Some(IntrinsicKind::P3TcpBind),
        ("TcpSocket", "connectStart") => Some(IntrinsicKind::P3TcpConnectStart),
        ("TcpSocket", "connectFinish") => Some(IntrinsicKind::P3TcpConnectFinish),
        ("TcpSocket", "listen") => Some(IntrinsicKind::P3TcpListen),
        ("TcpSocket", "acceptStart") => Some(IntrinsicKind::P3TcpAcceptStart),
        ("TcpSocket", "acceptFinish") => Some(IntrinsicKind::P3TcpAcceptFinish),
        ("TcpSocket", "dropAcceptStream") => Some(IntrinsicKind::P3TcpDropAcceptStream),
        ("TcpSocket", "send") => Some(IntrinsicKind::P3TcpSend),
        ("TcpSocket", "receive") => Some(IntrinsicKind::P3TcpReceive),
        ("TcpSocket", "sendWriteStart") => Some(IntrinsicKind::P3TcpSendWriteStart),
        ("TcpSocket", "receiveReadStart") => Some(IntrinsicKind::P3TcpReceiveReadStart),
        ("TcpSocket", "dropSendWritable") => Some(IntrinsicKind::P3TcpDropSendWritable),
        ("TcpSocket", "dropReceiveReadable") => Some(IntrinsicKind::P3TcpDropReceiveReadable),
        ("TcpSocket", "dropSendResult") => Some(IntrinsicKind::P3TcpDropSendResult),
        ("TcpSocket", "dropReceiveResult") => Some(IntrinsicKind::P3TcpDropReceiveResult),
        ("TcpSocket", "cancelAcceptRead") => Some(IntrinsicKind::P3TcpCancelAcceptRead),
        ("TcpSocket", "cancelSendWrite") => Some(IntrinsicKind::P3TcpCancelSendWrite),
        ("TcpSocket", "cancelReceiveRead") => Some(IntrinsicKind::P3TcpCancelReceiveRead),
        ("FsStreamOps", "cancelRead") => Some(IntrinsicKind::P3FsCancelRead),
        ("FsStreamOps", "cancelWrite") => Some(IntrinsicKind::P3FsCancelWrite),
        ("FsStreamOps", "cancelEntryRead") => Some(IntrinsicKind::P3FsCancelEntryRead),
        ("StdinStream", "cancelRead") => Some(IntrinsicKind::P3StdinCancelRead),
        ("StdoutStream", "open") => Some(IntrinsicKind::P3StdoutOpen),
        ("StderrStream", "open") => Some(IntrinsicKind::P3StderrOpen),
        ("StdoutStream", "writeStart") => Some(IntrinsicKind::P3StdoutWriteStart),
        ("StderrStream", "writeStart") => Some(IntrinsicKind::P3StderrWriteStart),
        ("StdoutStream", "cancelWrite") => Some(IntrinsicKind::P3StdoutCancelWrite),
        ("StderrStream", "cancelWrite") => Some(IntrinsicKind::P3StderrCancelWrite),
        ("StdoutStream", "readResult") => Some(IntrinsicKind::P3StdoutReadResult),
        ("StderrStream", "readResult") => Some(IntrinsicKind::P3StderrReadResult),
        ("TcpSocket", "readReceiveResult") => Some(IntrinsicKind::P3TcpReadReceiveResult),
        ("TcpSocket", "readSendResult") => Some(IntrinsicKind::P3TcpReadSendResult),
        ("FsStreamOps", "readReadResult") => Some(IntrinsicKind::P3FsReadReadResult),
        ("FsStreamOps", "readWriteResult") => Some(IntrinsicKind::P3FsReadWriteResult),
        ("FsStreamOps", "readEntryResult") => Some(IntrinsicKind::P3FsReadEntryResult),
        ("FsStreamOps", "readAppendResult") => Some(IntrinsicKind::P3FsReadAppendResult),
        ("AsyncCall", "discard") => Some(IntrinsicKind::P3AsyncCallDiscard),
        ("NameLookup", "resolveStart") => Some(IntrinsicKind::P3DnsResolveStart),
        ("NameLookup", "resolveFinish") => Some(IntrinsicKind::P3DnsResolveFinish),
        ("UdpSocket", "create") => Some(IntrinsicKind::P3UdpCreate),
        ("UdpSocket", "bind") => Some(IntrinsicKind::P3UdpBind),
        ("UdpSocket", "connect") => Some(IntrinsicKind::P3UdpConnect),
        ("UdpSocket", "disconnect") => Some(IntrinsicKind::P3UdpDisconnect),
        ("UdpSocket", "sendStart") => Some(IntrinsicKind::P3UdpSendStart),
        ("UdpSocket", "sendFinish") => Some(IntrinsicKind::P3UdpSendFinish),
        ("UdpSocket", "receiveStart") => Some(IntrinsicKind::P3UdpReceiveStart),
        ("UdpSocket", "receiveFinish") => Some(IntrinsicKind::P3UdpReceiveFinish),
        ("UdpSocket", "localAddress") => Some(IntrinsicKind::P3UdpLocalAddress),
        ("UdpSocket", "remoteAddress") => Some(IntrinsicKind::P3UdpRemoteAddress),
        ("UdpSocket", "close") => Some(IntrinsicKind::P3UdpClose),
        ("TcpSocket", "localAddress") => Some(IntrinsicKind::P3TcpLocalAddress),
        ("TcpSocket", "remoteAddress") => Some(IntrinsicKind::P3TcpRemoteAddress),
        ("TcpSocket", "setListenBacklogSize") => Some(IntrinsicKind::P3TcpSetListenBacklogSize),
        ("TcpSocket", "isListening") => Some(IntrinsicKind::P3TcpIsListening),
        ("TcpSocket", "addressFamily") => Some(IntrinsicKind::P3TcpAddressFamily),
        ("UdpSocket", "addressFamily") => Some(IntrinsicKind::P3UdpAddressFamily),
        ("TcpSocket", "keepAliveEnabled") => Some(IntrinsicKind::P3TcpKeepAliveEnabled),
        ("TcpSocket", "setKeepAliveEnabled") => Some(IntrinsicKind::P3TcpSetKeepAliveEnabled),
        ("TcpSocket", "keepAliveIdleTime") => Some(IntrinsicKind::P3TcpKeepAliveIdleTime),
        ("TcpSocket", "setKeepAliveIdleTime") => Some(IntrinsicKind::P3TcpSetKeepAliveIdleTime),
        ("TcpSocket", "keepAliveInterval") => Some(IntrinsicKind::P3TcpKeepAliveInterval),
        ("TcpSocket", "setKeepAliveInterval") => Some(IntrinsicKind::P3TcpSetKeepAliveInterval),
        ("TcpSocket", "keepAliveCount") => Some(IntrinsicKind::P3TcpKeepAliveCount),
        ("TcpSocket", "setKeepAliveCount") => Some(IntrinsicKind::P3TcpSetKeepAliveCount),
        ("TcpSocket", "hopLimit") => Some(IntrinsicKind::P3TcpHopLimit),
        ("UdpSocket", "unicastHopLimit") => Some(IntrinsicKind::P3UdpUnicastHopLimit),
        ("UdpSocket", "setUnicastHopLimit") => Some(IntrinsicKind::P3UdpSetUnicastHopLimit),
        ("UdpSocket", "receiveBufferSize") => Some(IntrinsicKind::P3UdpReceiveBufferSize),
        ("UdpSocket", "setReceiveBufferSize") => Some(IntrinsicKind::P3UdpSetReceiveBufferSize),
        ("UdpSocket", "sendBufferSize") => Some(IntrinsicKind::P3UdpSendBufferSize),
        ("UdpSocket", "setSendBufferSize") => Some(IntrinsicKind::P3UdpSetSendBufferSize),
        ("TcpSocket", "setHopLimit") => Some(IntrinsicKind::P3TcpSetHopLimit),
        ("TcpSocket", "receiveBufferSize") => Some(IntrinsicKind::P3TcpReceiveBufferSize),
        ("TcpSocket", "setReceiveBufferSize") => Some(IntrinsicKind::P3TcpSetReceiveBufferSize),
        ("TcpSocket", "sendBufferSize") => Some(IntrinsicKind::P3TcpSendBufferSize),
        ("TcpSocket", "setSendBufferSize") => Some(IntrinsicKind::P3TcpSetSendBufferSize),
        ("TcpSocket", "close") => Some(IntrinsicKind::P3TcpClose),
        ("FileDescriptor", "symlinkAtStart") => Some(IntrinsicKind::P3FsSymlinkAtStart),
        ("FileDescriptor", "renameAtStart") => Some(IntrinsicKind::P3FsRenameAtStart),
        ("FileDescriptor", "readViaStream") => Some(IntrinsicKind::P3FsReadViaStream),
        ("FileDescriptor", "writeViaStream") => Some(IntrinsicKind::P3FsWriteViaStream),
        ("FileDescriptor", "appendViaStream") => Some(IntrinsicKind::P3FsAppendViaStream),
        ("FileDescriptor", "close") => Some(IntrinsicKind::P3FsClose),
        ("FileDescriptor", "preopens") => Some(IntrinsicKind::FsPreopensGetDirectories),
        ("FsStreamOps", "readStart") => Some(IntrinsicKind::P3FsStreamReadStart),
        ("FsStreamOps", "writeStart") => Some(IntrinsicKind::P3FsStreamWriteStart),
        ("FsStreamOps", "appendWriteStart") => Some(IntrinsicKind::P3FsStreamAppendWriteStart),
        ("FsStreamOps", "dropReadable") => Some(IntrinsicKind::P3FsDropReadable),
        ("FsStreamOps", "dropWritable") => Some(IntrinsicKind::P3FsDropWritable),
        ("FsStreamOps", "dropReadResult") => Some(IntrinsicKind::P3FsDropReadResult),
        ("FsStreamOps", "dropWriteResult") => Some(IntrinsicKind::P3FsDropWriteResult),
        ("FsStreamOps", "dropAppendResult") => Some(IntrinsicKind::P3FsDropAppendResult),
        ("WallClock", "now") => Some(IntrinsicKind::WallClockNow),
        ("Random", "bytes") => Some(IntrinsicKind::RandomBytes),
        ("Random", "int64") => Some(IntrinsicKind::RandomInt64),
        // Filesystem intrinsics
        ("FileSystem", "preopens") => Some(IntrinsicKind::FsPreopensGetDirectories),
        // CLI intrinsics
        ("Terminal", "stdin") => Some(IntrinsicKind::CliTerminalStdin),
        ("Terminal", "stdout") => Some(IntrinsicKind::CliTerminalStdout),
        ("Terminal", "stderr") => Some(IntrinsicKind::CliTerminalStderr),
        ("Environment", "variables") => Some(IntrinsicKind::CliGetEnvironment),
        ("Environment", "arguments") => Some(IntrinsicKind::CliGetArguments),
        ("Environment", "initialCwd") => Some(IntrinsicKind::CliInitialCwd),
        ("Process", "exit") => Some(IntrinsicKind::CliExit),
        // Console intrinsics (blocking stdout/stderr output)
        ("Console", "print") => Some(IntrinsicKind::ConsolePrint),
        ("Console", "println") => Some(IntrinsicKind::ConsolePrintln),
        ("Console", "eprint") => Some(IntrinsicKind::ConsoleEprint),
        ("Console", "eprintln") => Some(IntrinsicKind::ConsoleEprintln),
        _ => None,
    }
}

/// Map a freestanding function FQN to an IntrinsicKind (for non-method intrinsics like `debug`).
pub(super) fn resolve_freestanding_intrinsic(fqn: &Fqn) -> Option<IntrinsicKind> {
    match fqn.symbol.0.as_str() {
        "debug" => Some(IntrinsicKind::DebugPrint),
        _ => None,
    }
}

/// If `ty` is `ByName<T>` (i.e. `GenericNewtype { fqn: standard.prelude::ByName, ... }`),
/// return the inner type `T`. Otherwise return `None`.
pub(super) fn extract_byname_inner(ty: &Type) -> Option<&Type> {
    if let Type::GenericNewtype { fqn, type_args, .. } = ty
        && super::types::is_byname_fqn(fqn)
        && type_args.len() == 1
    {
        return Some(&type_args[0].1);
    }
    None
}
