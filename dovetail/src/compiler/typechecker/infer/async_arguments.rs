use crate::common::span::{Span, Spanned};
use crate::common::types::{SymbolName, TypeParamName};
use crate::parser::ast::{Expr, TypeExpr};
use crate::typechecker::registry::VariantPayload;
use crate::typechecker::types::{Type, TypedExpr};

use super::Inference;
use super::function_expressions::extract_byname_inner;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;

impl Inference<'_> {
    /// Ordinary arguments constrain an async argument even when they follow it.
    /// Preserve their typed expressions so discovery never evaluates an argument twice.
    pub(super) fn infer_deferred_async_arguments(
        &mut self,
        args: &[Expr],
        mut typed: Vec<TypedExpr>,
        expected: &[Option<Type>],
    ) -> Vec<TypedExpr> {
        let mut substitution = TypeParamSubstitution::new();
        for ((arg, typed), hint) in args.iter().zip(&typed).zip(expected) {
            if !matches!(arg, Expr::AsyncDo { .. })
                && !typed.ty.is_error()
                && let Some(hint) = hint
            {
                let parameter = if extract_byname_inner(&typed.ty).is_some() {
                    hint
                } else {
                    extract_byname_inner(hint).unwrap_or(hint)
                };
                substitution.unify(parameter, &typed.ty);
            }
        }
        // Async bodies establish result parameters before sibling closures need
        // their parameter types. Both passes finish before static dispatch can
        // consume the temporary error-typed placeholders.
        for infer_async in [true, false] {
            for (index, arg) in args.iter().enumerate() {
                let is_async = matches!(arg, Expr::AsyncDo { .. });
                if is_async != infer_async || (!is_async && !deferred_async_sibling(arg)) {
                    continue;
                }
                let hint = expected
                    .get(index)
                    .cloned()
                    .flatten()
                    .map(|ty| extract_byname_inner(&ty).cloned().unwrap_or(ty));
                let saved = self.expected_type.take();
                self.expected_type = hint
                    .as_ref()
                    .map(|ty| apply_substitution(&substitution, ty));
                typed[index] = self.infer_expr(arg);
                self.expected_type = saved;
                if let Some(hint) = hint {
                    substitution.unify(&hint, &typed[index].ty);
                }
            }
        }
        typed
    }

    /// Static dispatch returns before instance-method argument inference. Resolve
    /// its signature hints here so an async expression cannot escape as a placeholder.
    pub(super) fn static_async_argument_context(
        &mut self,
        receiver: &Expr,
        method: &Spanned<String>,
        receiver_args: &[TypeExpr],
        method_args: &[TypeExpr],
        count: usize,
        span: &Span,
    ) -> Option<Vec<Option<Type>>> {
        let module = match receiver {
            Expr::Identifier(name, _) => self.resolve_module_name(name).cloned(),
            Expr::ResolvedTypeRef(fqn, _) => self.registry.lookup_module(fqn).cloned(),
            _ => None,
        };
        let method_name = SymbolName(method.value.clone());
        if let Some(module) = module {
            let mut candidates = Vec::new();
            if let Some(overloads) = module.functions.get(&method_name) {
                for sig in overloads {
                    if sig.params.first().is_some_and(|(name, _)| name == "self")
                        || !self.is_member_visible(
                            sig.visibility,
                            &module.fqn.package,
                            &sig.source_file,
                        )
                    {
                        continue;
                    }
                    candidates.push(sig.params.iter().map(|(_, ty)| ty.clone()).collect());
                }
            }
            for def in module.generic_members.lookup_visible(
                &method_name,
                &self.package_path,
                &self.current_file,
            ) {
                if def.params.first().is_some_and(|(name, _)| name == "self") {
                    continue;
                }
                candidates.push(self.async_signature_context(
                    &def.params,
                    &def.return_type,
                    &[
                        (&def.type_params, receiver_args),
                        (&def.method_type_params, method_args),
                    ],
                ));
            }
            if candidates
                .iter()
                .any(|args: &Vec<Type>| args.len() == count)
            {
                return Some(common_context(candidates, count));
            }
        }
        if let Expr::Identifier(name, _) = receiver {
            if let Some(sig) = self.resolve_enum_type(name)
                && let Some((_, VariantPayload::Tuple(payload))) = sig
                    .variants
                    .iter()
                    .find(|(variant, _)| variant == &method.value)
            {
                let mut substitution = TypeParamSubstitution::new();
                if let Some(Type::GenericEnum { fqn, type_args, .. }) = &self.expected_type
                    && *fqn == sig.fqn
                {
                    for (name, (_, ty)) in sig.type_params.iter().zip(type_args) {
                        substitution.insert(name.clone(), ty.clone());
                    }
                }
                self.bind_async_explicit_args(&mut substitution, &sig.type_params, receiver_args);
                return Some(
                    payload
                        .iter()
                        .map(|ty| Some(apply_substitution(&substitution, ty)))
                        .collect(),
                );
            }
            if let Some(ty) = self.resolve_type_name(name, receiver_args, span) {
                return Some(self.async_type_static_context(
                    &ty,
                    &method_name,
                    receiver_args,
                    method_args,
                    count,
                ));
            }
        }
        self.resolve_qualified_call(receiver, &method.value)
            .map(|overloads| {
                common_context(
                    overloads
                        .iter()
                        .map(|sig| sig.params.iter().map(|(_, ty)| ty.clone()).collect())
                        .collect(),
                    count,
                )
            })
    }

    fn async_type_static_context(
        &mut self,
        ty: &Type,
        method: &SymbolName,
        receiver_args: &[TypeExpr],
        method_args: &[TypeExpr],
        count: usize,
    ) -> Vec<Option<Type>> {
        let Some(fqn) = ty.try_to_fqn() else {
            return vec![None; count];
        };
        let mut candidates = Vec::new();
        if let Some(class) = self.registry.lookup_class_type(&fqn, &self.package_path) {
            if let Some(overloads) = class.static_methods.get(method) {
                candidates.extend(
                    overloads
                        .iter()
                        .map(|sig| sig.params.iter().map(|(_, ty)| ty.clone()).collect()),
                );
            }
            if let Some(defs) = class.generic_static_methods.get(method) {
                for def in defs {
                    candidates.push(self.async_signature_context(
                        &def.params,
                        &def.return_type,
                        &[
                            (&def.class_type_params, receiver_args),
                            (&def.method_type_params, method_args),
                        ],
                    ));
                }
            }
        }
        for (block, sig) in self.registry.find_impl_method(&fqn, method) {
            if sig.params.first().is_some_and(|(name, _)| name == "self") {
                continue;
            }
            let mut substitution = TypeParamSubstitution::new().with_self_type(ty.clone());
            substitution.unify(&block.for_type, ty);
            self.bind_async_explicit_args(&mut substitution, &sig.method_type_params, method_args);
            if let Some(expected) = &self.expected_type {
                substitution.unify(&sig.return_type, expected);
            }
            candidates.push(
                sig.params
                    .iter()
                    .map(|(_, ty)| apply_substitution(&substitution, ty))
                    .collect(),
            );
        }
        for (_, sig) in self.lookup_named_extension_methods(&fqn, method) {
            if sig.params.first().is_none_or(|(name, _)| name != "self") {
                candidates.push(sig.params.iter().map(|(_, ty)| ty.clone()).collect());
            }
        }
        common_context(candidates, count)
    }

    fn async_signature_context(
        &mut self,
        params: &[(String, Type)],
        result: &Type,
        explicit_groups: &[(&[TypeParamName], &[TypeExpr])],
    ) -> Vec<Type> {
        let mut substitution = TypeParamSubstitution::new();
        for (names, arguments) in explicit_groups {
            self.bind_async_explicit_args(&mut substitution, names, arguments);
        }
        if let Some(expected) = &self.expected_type {
            substitution.unify(result, expected);
        }
        params
            .iter()
            .map(|(_, ty)| apply_substitution(&substitution, ty))
            .collect()
    }

    fn bind_async_explicit_args(
        &mut self,
        substitution: &mut TypeParamSubstitution,
        names: &[TypeParamName],
        arguments: &[TypeExpr],
    ) {
        if names.len() == arguments.len() {
            for (name, argument) in names.iter().zip(arguments) {
                substitution.insert(name.clone(), self.resolve_type_expr(argument));
            }
        }
    }
}

fn common_context(candidates: Vec<Vec<Type>>, count: usize) -> Vec<Option<Type>> {
    let matching: Vec<_> = candidates
        .iter()
        .filter(|params| params.len() == count)
        .collect();
    let Some(first) = matching.first() else {
        return vec![None; count];
    };
    first
        .iter()
        .enumerate()
        .map(|(index, ty)| {
            matching
                .iter()
                .all(|params| params[index] == *ty)
                .then(|| ty.clone())
        })
        .collect()
}

/// Async closures need their Awaitable result context; unannotated synchronous
/// closures need their parameter context. Both can depend on async siblings.
pub(super) fn deferred_async_sibling(arg: &Expr) -> bool {
    matches!(arg, Expr::Closure { is_async, params, .. } if *is_async || params.iter().any(|param| param.type_annotation.is_none()))
}
