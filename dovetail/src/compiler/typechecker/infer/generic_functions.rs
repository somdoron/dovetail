use crate::common::types::{Fqn, MangledName, PackagePath, SymbolName, TypeParamName};
use crate::parser::ast::TypeExpr;
use crate::typechecker::registry::{
    ExtMethodSignature, ExtensionBlockSignature, GenericFunctionDef,
};
use crate::typechecker::types::Type;

use crate::typechecker::types::IntrinsicKind;

use super::function_expressions::resolve_intrinsic_kind;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;
use super::{Inference, ResolvedFunction};

/// Describes the kind of method being instantiated, so the correct
/// mangled-name constructor is used.
pub(super) enum MethodKind<'a> {
    /// Free function or unnamed extension method — use MangledName::for_function.
    FreeFunction,
    /// Module function — same mangling as FreeFunction (FQN symbol = `Mod.func`),
    /// but a separate variant for clarity.
    ModuleFunction,
    /// Class methods retain their own generic arity in their template identity.
    ClassMethod { method_parameter_count: usize },
    /// Trait impl method — use the impl-block-aware member mangling.
    ImplMethod {
        trait_fqn: &'a Fqn,
        /// Precomputed via `Type::impl_segment` for non-generic blocks;
        /// the bare type FQN rendering for generic blocks.
        type_segment: String,
        method_name: &'a SymbolName,
        trait_type_args: &'a [Type],
    },
}

impl Inference<'_> {
    /// Infer type arguments for a generic function call.
    /// If explicit type args are provided, resolves and validates count.
    /// Otherwise, unifies arg types against param types.
    pub(super) fn infer_type_args(
        &mut self,
        def: &GenericFunctionDef,
        arg_types: &[&Type],
        explicit_type_args: &[TypeExpr],
    ) -> Option<Vec<Type>> {
        if !explicit_type_args.is_empty() {
            // Explicit type args
            if explicit_type_args.len() != def.type_params.len() {
                return None;
            }
            self.resolve_type_args(explicit_type_args)
        } else {
            // Infer from argument types via unification
            if def.params.len() != arg_types.len() {
                return None;
            }
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.infer_from_arguments(
                def.params
                    .iter()
                    .zip(arg_types)
                    .map(|((_, param), arg)| (param, *arg)),
            ) {
                return None;
            }
            self.infer_associated_bound_types(&def.trait_bounds, &mut substitution);
            // Check all type params are bound
            substitution.resolve_type_params(&def.type_params)
        }
    }

    /// Resolve a generic function call: compute the template mangled name and
    /// substituted return type. The caller stores type_args on the FunctionCall/FunctionRef
    /// node; monomorphize discovers and instantiates concrete copies by walking the AST.
    pub(super) fn resolve_generic_function_template(
        &mut self,
        fqn: &Fqn,
        def: &GenericFunctionDef,
        type_args: &[Type],
        method_kind: MethodKind<'_>,
    ) -> (MangledName, Type) {
        debug_assert!(
            def.type_params.len() == type_args.len(),
            "resolve_generic_function_template: type_params/type_args count mismatch for {}: {} type_params vs {} type_args",
            fqn,
            def.type_params.len(),
            type_args.len()
        );
        let substitution = TypeParamSubstitution::from_pairs(&def.type_params, type_args);
        let return_type = apply_substitution(&substitution, &def.return_type);
        // Compute template name using generic param types (matching the template stored
        // in typed_functions by infer_function_typecheck_only / infer_module_function_template)
        let generic_param_types: Vec<&Type> = def.params.iter().map(|(_, ty)| ty).collect();
        let template_mn = match method_kind {
            MethodKind::FreeFunction | MethodKind::ModuleFunction => {
                MangledName::for_function(fqn, &generic_param_types)
            }
            MethodKind::ClassMethod {
                method_parameter_count,
            } => crate::typechecker::class_trait_methods::template_name(
                fqn,
                &generic_param_types,
                method_parameter_count,
            ),
            MethodKind::ImplMethod {
                trait_fqn,
                type_segment,
                method_name,
                trait_type_args,
            } => MangledName::for_impl_block_method(
                trait_fqn,
                &type_segment,
                method_name,
                trait_type_args,
            ),
        };
        (template_mn, return_type)
    }

    /// Resolve generic function candidates for a call.
    /// Looks up generic overloads, infers type arguments, checks assignability,
    /// instantiates matching ones, and returns them as `ResolvedFunction`s.
    pub(super) fn resolve_generic_function(
        &mut self,
        fqn: &Fqn,
        arg_types: &[&Type],
        explicit_type_args: &[TypeExpr],
    ) -> Vec<ResolvedFunction> {
        let generic = self
            .registry
            .lookup_generic_function(fqn, &self.package_path)
            .map(|s| s.to_vec())
            .unwrap_or_default();

        let mut results = Vec::new();
        let mut extension_inference_span = None;
        for def in &generic {
            if !self.named_signature_allowed(&def.params) {
                continue;
            }
            let resolved_type_args = self
                .infer_type_args(def, arg_types, explicit_type_args)
                .or_else(|| {
                    // Fallback: try unifying arg types + return type with expected_type
                    if explicit_type_args.is_empty() && def.params.len() == arg_types.len() {
                        let mut sub = TypeParamSubstitution::new();
                        sub.infer_from_arguments(
                            def.params
                                .iter()
                                .zip(arg_types)
                                .map(|((_, param), arg)| (param, *arg)),
                        );
                        if let Some(ref expected) = self.expected_type {
                            sub.unify(&def.return_type, expected);
                        }
                        sub.resolve_type_params(&def.type_params)
                    } else {
                        None
                    }
                });
            if resolved_type_args.is_none()
                && explicit_type_args.is_empty()
                && def.params.len() == arg_types.len()
                && (def.return_type.contains_tuple_extension()
                    || def
                        .params
                        .iter()
                        .any(|(_, ty)| ty.contains_tuple_extension()))
            {
                extension_inference_span = Some(
                    self.current_expr_span
                        .clone()
                        .unwrap_or_else(|| def.span.clone()),
                );
            }
            if let Some(resolved_type_args) = resolved_type_args
                && def.params.len() == arg_types.len()
            {
                let substitution =
                    TypeParamSubstitution::from_pairs(&def.type_params, &resolved_type_args);

                let all_match =
                    def.params
                        .iter()
                        .zip(arg_types.iter())
                        .all(|((_, param_ty), arg_ty)| {
                            let concrete = apply_substitution(&substitution, param_ty);
                            self.is_assignable(&concrete, arg_ty)
                        });

                if all_match {
                    // Validate trait bounds
                    let bound_span = self
                        .current_expr_span
                        .clone()
                        .unwrap_or_else(|| def.span.clone());
                    if !self.check_trait_bounds(
                        &def.trait_bounds,
                        &def.type_params,
                        &resolved_type_args,
                        &bound_span,
                    ) {
                        continue;
                    }

                    // Intercept freestanding intrinsic generic functions (e.g. debug<T>)
                    if def.is_intrinsic && fqn.symbol.0 == "debug" {
                        let substitution = TypeParamSubstitution::from_pairs(
                            &def.type_params,
                            &resolved_type_args,
                        );
                        let arg_ty = apply_substitution(&substitution, &def.params[0].1);
                        let display_fqn = Fqn {
                            package: PackagePath(vec!["standard".into(), "prelude".into()]),
                            symbol: SymbolName("Display".to_string()),
                        };
                        if let Some((resolved_impl, _)) = self.resolve_trait_impl_method_for_type(
                            &arg_ty,
                            &display_fqn,
                            "format",
                            &[],
                        ) {
                            let empty_type_args: &[Type] = &[];
                            let format_mangled = MangledName::for_impl_block_method(
                                &resolved_impl.trait_fqn,
                                &resolved_impl.for_type.impl_segment(),
                                &resolved_impl.method_name,
                                empty_type_args,
                            );
                            results.push(ResolvedFunction::Intrinsic {
                                intrinsic: IntrinsicKind::DebugPrintDisplay {
                                    format_method: format_mangled,
                                },
                                return_type: Type::Unit,
                            });
                            continue;
                        }
                    }

                    let (mangled, return_type) = self.resolve_generic_function_template(
                        fqn,
                        def,
                        &resolved_type_args,
                        MethodKind::FreeFunction,
                    );
                    results.push(ResolvedFunction::Regular {
                        mangled_name: mangled,
                        return_type,
                        type_args: resolved_type_args.clone(),
                    });
                }
            }
        }
        if results.is_empty()
            && let Some(span) = extension_inference_span
        {
            self.diagnostics.error(span, format!(
                    "cannot infer tuple extension operands for '{}'; provide explicit type arguments", fqn.symbol
                ));
        }
        results
    }

    /// Resolve generic extension instance method candidates.
    /// Unifies receiver type against each def's for_type to determine type args,
    /// then instantiates matching methods.
    /// `arg_types` are the non-self argument types for method-level type param inference.
    /// `explicit_method_type_params` are explicit type params on the method call (e.g. `.map<String>(...)`).
    pub(super) fn resolve_generic_extension_instance(
        &mut self,
        receiver_ty: &Type,
        method_name: &SymbolName,
        arg_types: &[&Type],
        explicit_method_type_params: &[TypeExpr],
    ) -> Vec<ResolvedFunction> {
        self.resolve_generic_extension_instance_filtered(
            receiver_ty,
            method_name,
            arg_types,
            explicit_method_type_params,
            None,
        )
    }

    /// As `resolve_generic_extension_instance`, optionally restricted to one
    /// named extension (for explicit `ExtName.method(...)` calls).
    pub(super) fn resolve_generic_extension_instance_filtered(
        &mut self,
        receiver_ty: &Type,
        method_name: &SymbolName,
        arg_types: &[&Type],
        explicit_method_type_params: &[TypeExpr],
        ext_filter: Option<&Fqn>,
    ) -> Vec<ResolvedFunction> {
        if receiver_ty.is_error() {
            return vec![];
        }
        let defs =
            self.lookup_generic_extension_methods_filtered(receiver_ty, method_name, ext_filter);
        if defs.is_empty() {
            return vec![];
        }

        let mut results = Vec::new();
        for (block, method) in defs {
            if !self.named_signature_allowed(&method.params) {
                continue;
            }
            // Skip properties on the IMPLICIT path — they are resolved via
            // infer_field_access. Explicit `ExtName.prop(recv)` calls pass an
            // ext filter and dispatch the property like a call — with no
            // arguments beyond the receiver.
            if method.is_property && (ext_filter.is_none() || !arg_types.is_empty()) {
                continue;
            }

            // Unify receiver type against block.for_type to infer extension-level type args
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.unify(&block.for_type, receiver_ty) {
                continue;
            }

            // Must be an instance method (first param = "self")
            if method.params.is_empty() || method.params[0].0 != "self" {
                continue;
            }

            if method.params.len() - 1 != arg_types.len() {
                continue;
            }

            // If method has its own type params, unify non-self args to bind them
            if !method.method_type_params.is_empty() {
                let non_self_params = &method.params[1..];
                if non_self_params.len() != arg_types.len() {
                    continue;
                }
                if !substitution.infer_from_arguments(
                    non_self_params
                        .iter()
                        .zip(arg_types)
                        .map(|((_, param), arg)| (param, *arg)),
                ) {
                    continue;
                }
            } else {
                // Also unify method param types (excluding self) with arg types
                // to help infer any remaining type params
                let method_params = &method.params[1..];
                if method_params.len() == arg_types.len() {
                    substitution.infer_from_arguments(
                        method_params
                            .iter()
                            .zip(arg_types)
                            .map(|((_, param), arg)| (param, *arg)),
                    );
                }
            }

            // Build combined type params: extension-level + method-level
            let all_type_params: Vec<TypeParamName> = block
                .type_params
                .iter()
                .chain(method.method_type_params.iter())
                .cloned()
                .collect();

            // Merge trait bounds from block + method
            let mut trait_bounds = block.trait_bounds.clone();
            trait_bounds.merge(&method.trait_bounds);

            // Resolve combined params; fall back to explicit method type params, then expected_type
            self.infer_associated_bound_types(&trait_bounds, &mut substitution);
            let type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    // Try explicit method type params for unresolved method-level params
                    if !explicit_method_type_params.is_empty()
                        && explicit_method_type_params.len() == method.method_type_params.len()
                    {
                        // Extension-level already bound from receiver
                        let ext_args = match substitution.resolve_type_params(&block.type_params) {
                            Some(a) => a,
                            None => continue,
                        };
                        let method_args = match self.resolve_type_args(explicit_method_type_params)
                        {
                            Some(a) => a,
                            None => continue,
                        };
                        [ext_args, method_args].concat()
                    } else if let Some(ref expected) = self.expected_type {
                        // Try to infer remaining type params from expected return type
                        substitution.unify(&method.return_type, expected);
                        match substitution.resolve_type_params(&all_type_params) {
                            Some(args) => args,
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                }
            };

            let type_args = if explicit_method_type_params.is_empty() {
                type_args
            } else {
                if explicit_method_type_params.len() != method.method_type_params.len() {
                    continue;
                }
                let Some(method_args) = self.resolve_type_args(explicit_method_type_params) else {
                    continue;
                };
                [type_args[..block.type_params.len()].to_vec(), method_args].concat()
            };

            let concrete_sub = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
            if !method.params[1..]
                .iter()
                .zip(arg_types)
                .all(|((_, param), arg)| {
                    self.is_assignable(&apply_substitution(&concrete_sub, param), arg)
                })
            {
                continue;
            }

            // Validate trait bounds
            let bound_span = self
                .current_expr_span
                .clone()
                .unwrap_or_else(|| method.span.clone());
            let failures =
                self.unsatisfied_trait_bounds(&trait_bounds, &all_type_params, &type_args);
            if !failures.is_empty() {
                if ext_filter.is_some() {
                    for failure in failures {
                        self.diagnostics.error(bound_span.clone(), failure);
                    }
                }
                continue;
            }

            // If the method is intrinsic, resolve to IntrinsicCall directly
            if method.is_intrinsic {
                let substitution = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
                let concrete_return = apply_substitution(&substitution, &method.return_type);
                let Some(base_type_fqn) = receiver_ty.try_to_fqn() else {
                    continue;
                };
                if let Some(intrinsic) =
                    resolve_intrinsic_kind(&base_type_fqn, method_name, &concrete_return)
                {
                    results.push(ResolvedFunction::Intrinsic {
                        intrinsic,
                        return_type: concrete_return,
                    });
                    continue;
                }
            }

            // Compute concrete return type via substitution
            let substitution = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
            let concrete_return = apply_substitution(&substitution, &method.return_type);

            // Named extension — return ExtMethod for monomorphize to handle
            results.push(ResolvedFunction::ExtMethod {
                ext_fqn: block.ext_fqn.clone(),
                for_type: receiver_ty.clone(),
                method_name: method_name.clone(),
                type_args: type_args.clone(),
                return_type: concrete_return,
            });
        }
        results
    }

    /// Resolve generic trait impl instance method candidates.
    /// Looks up generic trait impl methods from registry by (type_fqn, method_name),
    /// unifies non-self arg types against param types to infer method type params,
    /// then instantiates matching methods.
    pub(super) fn resolve_generic_trait_impl_instance(
        &mut self,
        receiver_ty: &Type,
        method_name: &SymbolName,
        arg_types: &[&Type],
        explicit_type_args: &[TypeExpr],
        trait_filter: Option<(&Fqn, &[Type])>,
    ) -> Vec<ResolvedFunction> {
        self.resolve_generic_trait_impl_member(
            receiver_ty,
            method_name,
            arg_types,
            explicit_type_args,
            trait_filter,
            None,
        )
    }

    pub(super) fn resolve_generic_trait_impl_member(
        &mut self,
        receiver_ty: &Type,
        method_name: &SymbolName,
        arg_types: &[&Type],
        explicit_type_args: &[TypeExpr],
        trait_filter: Option<(&Fqn, &[Type])>,
        property_only: Option<bool>,
    ) -> Vec<ResolvedFunction> {
        if receiver_ty.is_error() {
            return vec![];
        }
        let Some(type_fqn) = receiver_ty.try_to_fqn() else {
            return vec![];
        };
        let defs = self.registry.find_impl_method(&type_fqn, method_name);

        if defs.is_empty() {
            return vec![];
        }

        let mut results = Vec::new();
        let mut bound_errors = Vec::new();
        for (block, method) in &defs {
            let names_match = match trait_filter {
                Some((trait_fqn, _)) => {
                    self.named_trait_implementation_allowed(trait_fqn, &method.dispatch_name)
                }
                None => self.named_signature_allowed(&method.params),
            };
            if !names_match {
                continue;
            }
            if property_only.is_some_and(|is_property| method.is_property != is_property) {
                continue;
            }
            // An explicit trait call must not validate unrelated same-named
            // candidates: their bounds cannot reject the selected trait.
            if trait_filter.is_some_and(|(fqn, _)| block.trait_fqn != *fqn) {
                continue;
            }
            if block.type_params.is_empty() && method.method_type_params.is_empty() {
                continue;
            }
            let non_self_params = if !method.params.is_empty() && method.params[0].0 == "self" {
                &method.params[1..]
            } else {
                &method.params[..]
            };

            if non_self_params.len() != arg_types.len() {
                continue;
            }

            let mut substitution = TypeParamSubstitution::new();

            if !substitution.unify(&block.for_type, receiver_ty) {
                continue;
            }

            if !substitution.infer_from_arguments(
                non_self_params
                    .iter()
                    .zip(arg_types)
                    .map(|((_, param), arg)| (param, *arg)),
            ) {
                continue;
            }

            let all_type_params: Vec<TypeParamName> = block
                .type_params
                .iter()
                .chain(method.method_type_params.iter())
                .cloned()
                .collect();

            let mut combined_bounds = block.trait_bounds.clone();
            combined_bounds.merge(&method.trait_bounds);
            self.infer_associated_bound_types(&combined_bounds, &mut substitution);

            let type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    if !explicit_type_args.is_empty()
                        && explicit_type_args.len() == method.method_type_params.len()
                    {
                        let block_args = match substitution.resolve_type_params(&block.type_params)
                        {
                            Some(a) => a,
                            None => continue,
                        };
                        let method_args = match self.resolve_type_args(explicit_type_args) {
                            Some(a) => a,
                            None => continue,
                        };
                        [block_args, method_args].concat()
                    } else if let Some(ref expected) = self.expected_type {
                        substitution.unify(&method.return_type, expected);
                        match substitution.resolve_type_params(&all_type_params) {
                            Some(args) => args,
                            None => continue,
                        }
                    } else {
                        continue;
                    }
                }
            };

            let type_args = if explicit_type_args.is_empty() {
                type_args
            } else {
                if explicit_type_args.len() != method.method_type_params.len() {
                    continue;
                }
                let Some(method_args) = self.resolve_type_args(explicit_type_args) else {
                    continue;
                };
                [type_args[..block.type_params.len()].to_vec(), method_args].concat()
            };

            // Sibling applications can have different bounds. Filter the
            // substituted application before diagnosing a candidate's bounds.
            if let Some((_, required_args)) = trait_filter {
                let sub = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
                let provided_args: Vec<_> = block
                    .trait_type_args
                    .iter()
                    .map(|arg| apply_substitution(&sub, arg))
                    .collect();
                if !required_args.is_empty() && provided_args != required_args {
                    continue;
                }
            }

            // Verify non-self arg types are assignable to substituted param
            // types. `unify_at` is variance-aware (accepts a Never arg in a
            // covariant slot whose binding is wider), so we need a second
            // pass through `is_assignable` to enforce true assignability.
            {
                let sub = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
                let mut args_match = true;
                for ((_, param_ty), arg_ty) in non_self_params.iter().zip(arg_types.iter()) {
                    let concrete = apply_substitution(&sub, param_ty);
                    if !self.is_assignable(&concrete, arg_ty) {
                        args_match = false;
                        break;
                    }
                }
                if !args_match {
                    continue;
                }
            }

            // Failed candidate bounds matter only when no candidate applies.
            // Keep their diagnostics for that case without rejecting a valid
            // sibling implementation encountered later in the search.
            let failures =
                self.unsatisfied_trait_bounds(&combined_bounds, &all_type_params, &type_args);
            if !failures.is_empty() {
                bound_errors.extend(failures);
                continue;
            }

            // Look up GenericFunctionDef registered during collect (ImplMethodSignature no longer stores body)
            let impl_method_fqn = Fqn {
                package: block.package.clone(),
                symbol: SymbolName(format!(
                    "{}${}.{}",
                    block.trait_fqn.symbol, block.type_fqn.symbol, method_name
                )),
            };
            // A synthesized default member has no GenericFunctionDef — its
            // return type comes straight off the (substituted) signature and
            // its body is materialized from the trait's default template at
            // monomorphize.
            if method.is_default {
                let sub = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
                let return_type = apply_substitution(&sub, &method.return_type);
                let concrete_trait_type_args: Vec<Type> = block
                    .trait_type_args
                    .iter()
                    .map(|t| super::generics::apply_substitution(&sub, t))
                    .collect();
                let resolved = crate::typechecker::types::ResolvedImplMethod {
                    trait_fqn: block.trait_fqn.clone(),
                    trait_type_params: concrete_trait_type_args,
                    for_type: receiver_ty.clone(),
                    method_name: method.dispatch_name.clone(),
                    method_type_params: type_args.clone(),
                };
                results.push(ResolvedFunction::ImplMethod {
                    resolved,
                    return_type,
                });
                continue;
            }
            let generic_defs = match self
                .registry
                .lookup_generic_function(&impl_method_fqn, &self.package_path)
            {
                Some(defs) => defs,
                None => continue,
            };
            // Different applications of one trait share this FQN. Preserve the
            // method selected from the operands when retrieving its definition.
            let generic_def = match generic_defs.iter().find(|def| def.span == method.span) {
                Some(d) => d,
                None => continue,
            };

            // For methods with method-level type params (non-generic impl block with generic methods),
            // the template TypedFunction was created by typecheck_generic_impl_method. Monomorphize
            // discovers and creates concrete copies by walking the AST.
            // For methods without method-level type params (generic impl block with non-generic methods),
            // the body lives in TypedImplementBlock and resolve_impl_calls handles instantiation.
            let return_type = if !method.method_type_params.is_empty() {
                let method_kind = MethodKind::ImplMethod {
                    trait_fqn: &block.trait_fqn,
                    type_segment: crate::typechecker::types::impl_block_segment(
                        &block.for_type,
                        &block.type_params,
                    ),
                    method_name: &method.dispatch_name,
                    trait_type_args: &block.trait_type_args,
                };
                let (_mangled, return_type) = self.resolve_generic_function_template(
                    &impl_method_fqn,
                    generic_def,
                    &type_args,
                    method_kind,
                );
                return_type
            } else {
                let sub_for_return =
                    TypeParamSubstitution::from_pairs(&generic_def.type_params, &type_args);
                apply_substitution(&sub_for_return, &generic_def.return_type)
            };

            // Substitute block type params in trait_type_args to get concrete trait type params.
            // E.g., `implement <A, B, T> FlatZip<T> for Parser<(A, B)>` has
            // block.trait_type_args = [TypeParameter(T)]; after resolving type_args = [Char, Char, Char],
            // we substitute to get concrete_trait_type_args = [Char].
            let sub = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
            let concrete_trait_type_args: Vec<Type> = block
                .trait_type_args
                .iter()
                .map(|t| super::generics::apply_substitution(&sub, t))
                .collect();

            let resolved = crate::typechecker::types::ResolvedImplMethod {
                trait_fqn: block.trait_fqn.clone(),
                trait_type_params: concrete_trait_type_args,
                for_type: receiver_ty.clone(),
                method_name: method.dispatch_name.clone(),
                method_type_params: type_args.clone(),
            };
            results.push(ResolvedFunction::ImplMethod {
                resolved,
                return_type,
            });
        }
        if results.is_empty() && trait_filter.is_none() {
            let bound_span = self
                .current_expr_span
                .clone()
                .unwrap_or_else(|| defs[0].1.span.clone());
            for message in bound_errors {
                self.diagnostics.error(bound_span.clone(), message);
            }
        }
        results
    }

    /// Resolve generic extension properties for a type + property name.
    /// Similar to `resolve_generic_extension_instance` but filters to `is_property == true`.
    pub(super) fn resolve_generic_extension_property(
        &mut self,
        receiver_ty: &Type,
        prop_name: &SymbolName,
    ) -> Vec<ResolvedFunction> {
        if receiver_ty.is_error() {
            return vec![];
        }
        let defs = self.lookup_all_generic_extension_methods(receiver_ty, prop_name);
        if defs.is_empty() {
            return vec![];
        }

        let mut results = Vec::new();
        for (block, method) in defs {
            if !self.named_signature_allowed(&method.params) {
                continue;
            }
            // Only consider properties
            if !method.is_property {
                continue;
            }

            // Unify receiver type against block.for_type to infer type args
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.unify(&block.for_type, receiver_ty) {
                continue;
            }

            let type_args = match substitution.resolve_type_params(&block.type_params) {
                Some(args) => args,
                None => continue,
            };

            // Validate trait bounds
            let bound_span = self
                .current_expr_span
                .clone()
                .unwrap_or_else(|| method.span.clone());
            if !self.check_trait_bounds(
                &block.trait_bounds,
                &block.type_params,
                &type_args,
                &bound_span,
            ) {
                continue;
            }

            // Must have self param
            if method.params.is_empty() || method.params[0].0 != "self" {
                continue;
            }

            // If intrinsic, resolve directly
            if method.is_intrinsic {
                let substitution =
                    TypeParamSubstitution::from_pairs(&block.type_params, &type_args);
                let concrete_return = apply_substitution(&substitution, &method.return_type);
                let Some(base_type_fqn) = receiver_ty.try_to_fqn() else {
                    continue;
                };
                if let Some(intrinsic) =
                    resolve_intrinsic_kind(&base_type_fqn, prop_name, &concrete_return)
                {
                    results.push(ResolvedFunction::Intrinsic {
                        intrinsic,
                        return_type: concrete_return,
                    });
                    continue;
                }
            }

            // Compute concrete return type via substitution
            let substitution = TypeParamSubstitution::from_pairs(&block.type_params, &type_args);
            let concrete_return = apply_substitution(&substitution, &method.return_type);

            // Named extension — return ExtMethod for monomorphize to handle
            results.push(ResolvedFunction::ExtMethod {
                ext_fqn: block.ext_fqn.clone(),
                for_type: receiver_ty.clone(),
                method_name: prop_name.clone(),
                type_args: type_args.clone(),
                return_type: concrete_return,
            });
        }
        results
    }

    /// Collect all visible generic extension methods/properties for a type + method name.
    /// Returns cloned `(block, method)` pairs from imported named generic extensions.
    pub(super) fn lookup_all_generic_extension_methods(
        &self,
        receiver_ty: &Type,
        method_name: &SymbolName,
    ) -> Vec<(ExtensionBlockSignature, ExtMethodSignature)> {
        self.lookup_generic_extension_methods_filtered(receiver_ty, method_name, None)
    }

    /// As `lookup_all_generic_extension_methods`, optionally restricted to one
    /// named extension (for explicit `ExtName.method(...)` calls).
    pub(super) fn lookup_generic_extension_methods_filtered(
        &self,
        receiver_ty: &Type,
        method_name: &SymbolName,
        ext_filter: Option<&Fqn>,
    ) -> Vec<(ExtensionBlockSignature, ExtMethodSignature)> {
        if receiver_ty.is_error() {
            return vec![];
        }
        let Some(base_type_fqn) = receiver_ty.try_to_fqn() else {
            return vec![];
        };
        self.import_scope
            .extension_blocks
            .iter()
            .filter(|b| ext_filter.is_none_or(|f| b.ext_fqn == *f))
            .filter(|b| {
                b.for_type.try_to_fqn().is_some_and(|f| f == base_type_fqn)
                    && !b.type_params.is_empty()
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

    /// Resolve generic static extension method candidates for a type name.
    /// Used for calls like `Array.fill(size, value)` or `Array<Int32>.empty()`.
    /// `explicit_receiver_type_args` provides type args from the receiver (e.g. `<Int32>` in `Array<Int32>`).
    /// `explicit_method_type_params` provides type params on the method call (e.g. `<String>` in `.transform<String>(...)`).
    /// Returns (matched_candidates, has_any_static_defs) so caller can distinguish
    /// "no defs at all" from "defs found but none matched".
    pub(super) fn resolve_generic_static_extension(
        &mut self,
        type_name: &str,
        method_name: &SymbolName,
        arg_types: &[&Type],
        explicit_receiver_type_args: &[TypeExpr],
        explicit_method_type_params: &[TypeExpr],
    ) -> (Vec<ResolvedFunction>, bool) {
        // Map type name to base FQN
        let base_type_fqn = if type_name == "Array" {
            Fqn {
                package: crate::common::types::PackagePath(vec![
                    "standard".into(),
                    "prelude".into(),
                ]),
                symbol: SymbolName("Array".to_string()),
            }
        } else {
            return (vec![], false);
        };

        // Look up generic extension methods from named imports
        // Use a dummy Array type — to_fqn() returns the same prelude Array FQN
        let dummy_type = Type::Array(Box::new(Type::Error));
        let defs = self.lookup_all_generic_extension_methods(&dummy_type, method_name);

        // Check if there are any static method defs for this type+method
        let has_static_defs = defs
            .iter()
            .any(|(_, m)| m.params.is_empty() || m.params[0].0 != "self");

        let mut results = Vec::new();
        for (block, method) in defs {
            if !self.named_signature_allowed(&method.params) {
                continue;
            }
            // Filter to static methods (no `self` first param)
            if !method.params.is_empty() && method.params[0].0 == "self" {
                continue;
            }

            // Check arg count matches param count
            if method.params.len() != arg_types.len() {
                continue;
            }

            // Unify argument types against param types to infer type params
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.infer_from_arguments(
                method
                    .params
                    .iter()
                    .zip(arg_types)
                    .map(|((_, param), arg)| (param, *arg)),
            ) {
                continue;
            }

            // Build combined type params: extension-level + method-level
            let all_type_params: Vec<TypeParamName> = block
                .type_params
                .iter()
                .chain(method.method_type_params.iter())
                .cloned()
                .collect();

            // Merge trait bounds from block + method
            let mut trait_bounds = block.trait_bounds.clone();
            trait_bounds.merge(&method.trait_bounds);

            // Check all type params are bound; fall back to explicit type params or expected_type
            self.infer_associated_bound_types(&trait_bounds, &mut substitution);
            let type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    // Try resolving extension-level from explicit receiver type params + method-level from explicit method type params
                    if !method.method_type_params.is_empty()
                        && !explicit_method_type_params.is_empty()
                        && explicit_method_type_params.len() == method.method_type_params.len()
                    {
                        let ext_args = match substitution.resolve_type_params(&block.type_params) {
                            Some(a) => a,
                            None => {
                                if !explicit_receiver_type_args.is_empty()
                                    && explicit_receiver_type_args.len() == block.type_params.len()
                                {
                                    match self.resolve_type_args(explicit_receiver_type_args) {
                                        Some(a) => a,
                                        None => continue,
                                    }
                                } else {
                                    continue;
                                }
                            }
                        };
                        let method_args = match self.resolve_type_args(explicit_method_type_params)
                        {
                            Some(a) => a,
                            None => continue,
                        };
                        [ext_args, method_args].concat()
                    } else if !explicit_receiver_type_args.is_empty()
                        && explicit_receiver_type_args.len() == block.type_params.len()
                        && method.method_type_params.is_empty()
                    {
                        match self.resolve_type_args(explicit_receiver_type_args) {
                            Some(args) => args,
                            None => continue,
                        }
                    } else if let Some(ref expected) = self.expected_type {
                        // Try to infer type args from expected type by unifying return type
                        let mut ret_sub = TypeParamSubstitution::new();
                        if ret_sub.unify(&method.return_type, expected) {
                            match ret_sub.resolve_type_params(&all_type_params) {
                                Some(args) => args,
                                None => continue,
                            }
                        } else {
                            continue;
                        }
                    } else {
                        continue;
                    }
                }
            };

            let substitution = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
            if !method
                .params
                .iter()
                .zip(arg_types)
                .all(|((_, param), arg)| {
                    self.is_assignable(&apply_substitution(&substitution, param), arg)
                })
            {
                continue;
            }

            // Validate trait bounds
            let bound_span = self
                .current_expr_span
                .clone()
                .unwrap_or_else(|| method.span.clone());
            if !self.check_trait_bounds(&trait_bounds, &all_type_params, &type_args, &bound_span) {
                continue;
            }

            let concrete_return = apply_substitution(&substitution, &method.return_type);

            // If intrinsic, resolve directly
            if method.is_intrinsic
                && let Some(intrinsic) =
                    resolve_intrinsic_kind(&base_type_fqn, method_name, &concrete_return)
            {
                results.push(ResolvedFunction::Intrinsic {
                    intrinsic,
                    return_type: concrete_return,
                });
                continue;
            }

            // Named extension — return ExtMethod for monomorphize to handle
            let concrete_for_type = apply_substitution(&substitution, &block.for_type);
            results.push(ResolvedFunction::ExtMethod {
                ext_fqn: block.ext_fqn.clone(),
                for_type: concrete_for_type,
                method_name: method_name.clone(),
                type_args: type_args.clone(),
                return_type: concrete_return,
            });
        }
        (results, has_static_defs)
    }
}
