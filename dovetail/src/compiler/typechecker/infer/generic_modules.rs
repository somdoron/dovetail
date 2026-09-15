use crate::common::span::Span;
use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::parser::ast::TypeExpr;
use crate::typechecker::registry::{
    GenericFunctionDef, GenericModuleGlobalDef, GenericModuleMemberDef, ModuleInfo,
};
use crate::typechecker::types::Type;

use super::function_expressions::resolve_intrinsic_kind;
use super::generic_functions::MethodKind;
use super::generics::apply_substitution;
use super::type_param_substitution::TypeParamSubstitution;
use super::{Inference, ResolvedFunction};

impl Inference<'_> {
    /// Resolve generic module instance method candidates.
    /// Unifies receiver type against each def's for_type to determine type args,
    /// then instantiates matching methods.
    /// `arg_types` are the non-self argument types for method-level type param inference.
    /// `explicit_method_type_params` are explicit type params on the method call.
    /// Returns `(candidates, had_instance_defs)` where `had_instance_defs` is true
    /// when at least one visible instance method with this name exists (even if
    /// none matched the argument types).
    /// `call_span` is where an unsatisfied trait bound is reported, and `None`
    /// means "do not report": callers that resolve candidates in order to make a
    /// choice pass their call's span, callers that are only asking whether any
    /// definition exists pass nothing, so a speculative lookup cannot put an
    /// error in the file.
    pub(super) fn resolve_generic_module_instance_method(
        &mut self,
        receiver_ty: &Type,
        method_name: &SymbolName,
        arg_types: &[&Type],
        explicit_method_type_params: &[TypeExpr],
        call_span: Option<&Span>,
    ) -> (Vec<ResolvedFunction>, bool) {
        if receiver_ty.is_error() {
            return (vec![], false);
        }
        // Intersections have no single FQN (and no module of their own).
        let Some(type_fqn) = receiver_ty.try_to_fqn() else {
            return (vec![], false);
        };
        let (defs, module_trait_bounds): (Vec<GenericModuleMemberDef>, _) =
            match self.registry.lookup_module(&type_fqn) {
                Some(info) => (
                    info.generic_members
                        .lookup_visible(method_name, &self.package_path, &self.current_file)
                        .into_iter()
                        .cloned()
                        .collect(),
                    info.trait_bounds.clone(),
                ),
                None => return (vec![], false),
            };
        let has_instance_defs = defs
            .iter()
            .any(|d| !d.params.is_empty() && d.params[0].0 == "self");

        let mut results = Vec::new();
        let mut unsatisfied_bounds: Vec<String> = Vec::new();
        for def in defs {
            // Unify receiver type against def.for_type to infer module-level type args
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.unify(&def.for_type, receiver_ty) {
                continue;
            }

            // Must be an instance method (first param = "self")
            if def.params.is_empty() || def.params[0].0 != "self" {
                continue;
            }

            // Check non-self arg count matches
            let non_self_params = &def.params[1..];
            if non_self_params.len() != arg_types.len() {
                continue;
            }

            // If method has its own type params, unify non-self args to bind them.
            // Use `unify_arg` which accepts a `Never` arg against a pre-bound
            // TypeVariable (covariant-slot widening). The `is_assignable`
            // recheck below enforces real assignability rules.
            if !def.method_type_params.is_empty() {
                let mut all_unified = true;
                for ((_, param_ty), arg_ty) in non_self_params.iter().zip(arg_types.iter()) {
                    if !substitution.unify_arg(param_ty, arg_ty) {
                        all_unified = false;
                        break;
                    }
                }
                if !all_unified {
                    continue;
                }
            }

            // Build combined type params: module-level + method-level
            let all_type_params: Vec<TypeParamName> = def
                .type_params
                .iter()
                .chain(def.method_type_params.iter())
                .cloned()
                .collect();

            // Resolve combined params; fall back to explicit method type params
            let mut inference_bounds = module_trait_bounds.clone();
            inference_bounds.merge(&def.trait_bounds);
            self.infer_associated_bound_types(&inference_bounds, &mut substitution);
            let type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    if !explicit_method_type_params.is_empty()
                        && explicit_method_type_params.len() == def.method_type_params.len()
                    {
                        let module_args = match substitution.resolve_type_params(&def.type_params) {
                            Some(a) => a,
                            None => continue,
                        };
                        let method_args = match self.resolve_type_args(explicit_method_type_params)
                        {
                            Some(a) => a,
                            None => continue,
                        };
                        [module_args, method_args].concat()
                    } else {
                        continue;
                    }
                }
            };

            // Verify non-self arg types are assignable to the substituted param
            // types. `unify` validated structural shape and bound type params
            // during inference; `is_assignable` enforces variance (e.g. accepts
            // an arg `Resource<X, Never>` for a slot expecting `Resource<X, E>`
            // when `E` was widely bound by the receiver, since the slot's E is
            // covariant). Runs even for methods with their own type params —
            // the variance-aware widening in `unify_at` makes unification
            // sometimes more permissive than the actual assignability rules.
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

            // If the method is intrinsic, resolve to IntrinsicCall directly
            if def.is_intrinsic {
                let substitution = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
                let concrete_return = apply_substitution(&substitution, &def.return_type);
                if let Some(intrinsic) =
                    resolve_intrinsic_kind(&type_fqn, method_name, &concrete_return)
                {
                    results.push(ResolvedFunction::Intrinsic {
                        intrinsic,
                        return_type: concrete_return,
                    });
                    continue;
                }
            }

            // Validate trait bounds: the module's, which apply to every member,
            // AND this member's own `where` clause, which may constrain a module
            // type param (`function run(self) where E: Display` in
            // `module Async<T, E>`). Checking only the module's left the second
            // kind unenforced anywhere, and an unsatisfied bound then reached
            // monomorphization as a missing impl and killed codegen.
            //
            // A failed bound drops the candidate rather than resolving it, so an
            // overload set with a satisfiable member still resolves; if none is
            // left the caller reports the unresolved call, next to the specific
            // bound diagnostic raised here.
            let mut combined_bounds = module_trait_bounds.clone();
            combined_bounds.merge(&def.trait_bounds);
            let failures =
                self.unsatisfied_trait_bounds(&combined_bounds, &all_type_params, &type_args);
            if !failures.is_empty() {
                unsatisfied_bounds.extend(failures);
                continue;
            }

            // Build a GenericFunctionDef from the module member def with ALL type params
            let generic_def = GenericFunctionDef {
                visibility: def.visibility,
                type_params: all_type_params,
                params: def.params.clone(),
                return_type: def.return_type.clone(),
                body: def.body.clone(),
                span: Span::point(def.source_file.clone(), 1, 1),
                container_name: Some(type_fqn.symbol.0.clone()),
                trait_bounds: combined_bounds,
                is_async: def.is_async,
                is_intrinsic: false,
            };

            let effective_fqn = Fqn {
                package: def.package.clone(),
                symbol: SymbolName(format!("{}.{}", type_fqn.symbol, method_name)),
            };

            let (mangled, return_type) = self.resolve_generic_function_template(
                &effective_fqn,
                &generic_def,
                &type_args,
                MethodKind::ModuleFunction,
            );
            results.push(ResolvedFunction::Regular {
                mangled_name: mangled,
                return_type,
                type_args: type_args.clone(),
            });
        }
        // Only once nothing resolved: a bound that ruled out one overload while
        // another took the call is not a diagnosis, it is a detail of how the
        // choice was made.
        if results.is_empty()
            && let Some(span) = call_span
        {
            for message in dedup_messages(unsatisfied_bounds) {
                self.diagnostics.error(span.clone(), message);
            }
        }
        (results, has_instance_defs)
    }

    /// Resolve generic module instance property candidates.
    /// Similar to instance method but filters to `is_property == true`.
    /// `explicit_property_type_args` are explicit type params on the property access.
    pub(super) fn resolve_generic_module_instance_property(
        &mut self,
        receiver_ty: &Type,
        prop_name: &SymbolName,
        explicit_property_type_args: &[TypeExpr],
    ) -> Vec<ResolvedFunction> {
        if receiver_ty.is_error() {
            return vec![];
        }
        let Some(type_fqn) = receiver_ty.try_to_fqn() else {
            return vec![];
        };
        let (defs, module_trait_bounds): (Vec<GenericModuleMemberDef>, _) =
            match self.registry.lookup_module(&type_fqn) {
                Some(info) => (
                    info.generic_members
                        .lookup_visible(prop_name, &self.package_path, &self.current_file)
                        .into_iter()
                        .cloned()
                        .collect(),
                    info.trait_bounds.clone(),
                ),
                None => return vec![],
            };

        let mut results = Vec::new();
        for def in defs {
            // Only consider properties
            if !def.is_property {
                continue;
            }

            // Unify receiver type against def.for_type to infer module-level type args
            let mut substitution = TypeParamSubstitution::new();
            if !substitution.unify(&def.for_type, receiver_ty) {
                continue;
            }

            // Must have self param
            if def.params.is_empty() || def.params[0].0 != "self" {
                continue;
            }

            // Build combined type params: module-level + property-level
            let all_type_params: Vec<TypeParamName> = def
                .type_params
                .iter()
                .chain(def.method_type_params.iter())
                .cloned()
                .collect();

            // Resolve combined params; fall back to explicit property type args
            let mut inference_bounds = module_trait_bounds.clone();
            inference_bounds.merge(&def.trait_bounds);
            self.infer_associated_bound_types(&inference_bounds, &mut substitution);
            let type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    if !explicit_property_type_args.is_empty()
                        && explicit_property_type_args.len() == def.method_type_params.len()
                    {
                        let module_args = match substitution.resolve_type_params(&def.type_params) {
                            Some(a) => a,
                            None => continue,
                        };
                        let property_args =
                            match self.resolve_type_args(explicit_property_type_args) {
                                Some(a) => a,
                                None => continue,
                            };
                        [module_args, property_args].concat()
                    } else if let Some(ref expected) = self.expected_type {
                        // Try to infer property type args from expected type
                        let mut ret_sub = TypeParamSubstitution::new();
                        for tp in &def.type_params {
                            if let Some(bound) = substitution.get(tp) {
                                ret_sub.unify(&Type::TypeVariable(tp.clone(), vec![]), bound);
                            }
                        }
                        if ret_sub.unify(&def.return_type, expected) {
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

            // If the property is intrinsic, resolve to IntrinsicCall directly
            if def.is_intrinsic {
                let substitution = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
                let concrete_return = apply_substitution(&substitution, &def.return_type);
                if let Some(intrinsic) =
                    resolve_intrinsic_kind(&type_fqn, prop_name, &concrete_return)
                {
                    results.push(ResolvedFunction::Intrinsic {
                        intrinsic,
                        return_type: concrete_return,
                    });
                    continue;
                }
            }

            let generic_def = GenericFunctionDef {
                visibility: def.visibility,
                type_params: all_type_params,
                params: def.params.clone(),
                return_type: def.return_type.clone(),
                body: def.body.clone(),
                span: Span::point(def.source_file.clone(), 1, 1),
                container_name: Some(type_fqn.symbol.0.clone()),
                trait_bounds: module_trait_bounds.clone(),
                is_async: def.is_async,
                is_intrinsic: false,
            };

            let effective_fqn = Fqn {
                package: def.package.clone(),
                symbol: SymbolName(format!("{}.{}", type_fqn.symbol, prop_name)),
            };

            let (mangled, return_type) = self.resolve_generic_function_template(
                &effective_fqn,
                &generic_def,
                &type_args,
                MethodKind::ModuleFunction,
            );
            results.push(ResolvedFunction::Regular {
                mangled_name: mangled,
                return_type,
                type_args: type_args.clone(),
            });
        }
        results
    }

    /// Resolve generic module static method candidates.
    /// Used for calls like `Box.wrap(42)` or `Box<Int32>.wrap(42)`.
    /// `call_span`: see `resolve_generic_module_instance_method`.
    pub(super) fn resolve_generic_module_static_method(
        &mut self,
        module_info: &crate::typechecker::registry::ModuleInfo,
        method_name: &SymbolName,
        arg_types: &[&Type],
        explicit_type_args: &[TypeExpr],
        explicit_method_type_params: &[TypeExpr],
        call_span: Option<&Span>,
    ) -> Vec<ResolvedFunction> {
        let defs: Vec<GenericModuleMemberDef> = module_info
            .generic_members
            .lookup_visible(method_name, &self.package_path, &self.current_file)
            .into_iter()
            .cloned()
            .collect();

        let mut results = Vec::new();
        let mut unsatisfied_bounds: Vec<String> = Vec::new();
        for def in defs {
            // Filter to static members (no `self` first param)
            if !def.params.is_empty() && def.params[0].0 == "self" {
                continue;
            }

            // Check arg count matches param count
            if def.params.len() != arg_types.len() {
                continue;
            }

            // Unify argument types against param types to infer type params
            let mut substitution = TypeParamSubstitution::new();
            // Explicit arguments constrain inference, rather than being used
            // only as a fallback after argument unification has failed.
            let mut valid_explicit = true;
            for (params, arguments) in [
                (&def.type_params, explicit_type_args),
                (&def.method_type_params, explicit_method_type_params),
            ] {
                if arguments.is_empty() {
                    continue;
                }
                if params.len() != arguments.len() {
                    valid_explicit = false;
                    break;
                }
                let Some(types) = self.resolve_type_args(arguments) else {
                    valid_explicit = false;
                    break;
                };
                for (param, ty) in params.iter().zip(types) {
                    substitution.insert(param.clone(), ty);
                }
            }
            if !valid_explicit {
                continue;
            }
            let mut all_unified = true;
            let has_explicit =
                !explicit_type_args.is_empty() || !explicit_method_type_params.is_empty();
            for ((_, param_ty), arg_ty) in def.params.iter().zip(arg_types.iter()) {
                let substituted = apply_substitution(&substitution, param_ty);
                if !substitution.unify(param_ty, arg_ty)
                    && !(has_explicit && self.is_assignable(&substituted, arg_ty))
                {
                    all_unified = false;
                    break;
                }
            }
            if !all_unified {
                continue;
            }

            // Build combined type params: module-level + method-level
            let all_type_params: Vec<TypeParamName> = def
                .type_params
                .iter()
                .chain(def.method_type_params.iter())
                .cloned()
                .collect();

            // For static methods without explicit type args, module-level type params
            // that don't appear in the method's parameter types or return type can be
            // defaulted to Never since they don't affect the method's behavior.
            if explicit_type_args.is_empty() {
                for tp in &def.type_params {
                    if substitution.get(tp).is_none() {
                        let referenced = def
                            .params
                            .iter()
                            .any(|(_, ty)| ty.contains_type_parameter_named(tp))
                            || def.return_type.contains_type_parameter_named(tp);
                        if !referenced {
                            substitution.insert(tp.clone(), Type::Never);
                        }
                    }
                }
            }

            // Check all type params are bound; fall back to explicit type args or expected_type
            let mut inference_bounds = module_info.trait_bounds.clone();
            inference_bounds.merge(&def.trait_bounds);
            self.infer_associated_bound_types(&inference_bounds, &mut substitution);
            let type_args = match substitution.resolve_type_params(&all_type_params) {
                Some(args) => args,
                None => {
                    // Try explicit method type params for method-level params
                    if !def.method_type_params.is_empty()
                        && !explicit_method_type_params.is_empty()
                        && explicit_method_type_params.len() == def.method_type_params.len()
                    {
                        let module_args = match substitution.resolve_type_params(&def.type_params) {
                            Some(a) => a,
                            None => {
                                if !explicit_type_args.is_empty()
                                    && explicit_type_args.len() == def.type_params.len()
                                {
                                    match self.resolve_type_args(explicit_type_args) {
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
                        [module_args, method_args].concat()
                    } else if !explicit_type_args.is_empty()
                        && explicit_type_args.len() == def.type_params.len()
                        && def.method_type_params.is_empty()
                    {
                        match self.resolve_type_args(explicit_type_args) {
                            Some(args) => args,
                            None => continue,
                        }
                    } else if !explicit_type_args.is_empty()
                        && explicit_type_args.len() == def.type_params.len()
                        && !def.method_type_params.is_empty()
                    {
                        // Explicit module args + bidirectional inference for method params
                        let module_args = match self.resolve_type_args(explicit_type_args) {
                            Some(a) => a,
                            None => continue,
                        };
                        if let Some(ref expected) = self.expected_type {
                            let mut ret_sub = TypeParamSubstitution::new();
                            if ret_sub.unify(&def.return_type, expected) {
                                match ret_sub.resolve_type_params(&def.method_type_params) {
                                    Some(method_args) => [module_args, method_args].concat(),
                                    None => continue,
                                }
                            } else {
                                continue;
                            }
                        } else {
                            continue;
                        }
                    } else {
                        // The expected type is a HINT, not a gate. Use it when the
                        // return type actually unifies with it, and otherwise fall
                        // through to variance defaulting — an ambient expectation
                        // that has nothing to do with this member (the `Bool` of an
                        // enclosing `assert`, say) must not defeat an inference that
                        // would succeed on its own.
                        let from_expected = self.expected_type.clone().and_then(|expected| {
                            let mut ret_sub = TypeParamSubstitution::new();
                            if ret_sub.unify(&def.return_type, &expected) {
                                ret_sub.resolve_type_params(&all_type_params)
                            } else {
                                None
                            }
                        });
                        match from_expected {
                            Some(args) => args,
                            None if !module_info.type_param_variances.is_empty() => {
                                // Variance-based defaulting for module type params,
                                // the same fallback an enum variant already gets.
                                match substitution.resolve_with_variance_defaults(
                                    &all_type_params,
                                    &module_info.type_param_variances,
                                ) {
                                    Some(args) => args,
                                    None => continue,
                                }
                            }
                            None => continue,
                        }
                    }
                }
            };

            // If the method is intrinsic, resolve to IntrinsicCall directly
            if def.is_intrinsic {
                let substitution = TypeParamSubstitution::from_pairs(&all_type_params, &type_args);
                let concrete_return = apply_substitution(&substitution, &def.return_type);
                if let Some(intrinsic) =
                    resolve_intrinsic_kind(&module_info.fqn, method_name, &concrete_return)
                {
                    results.push(ResolvedFunction::Intrinsic {
                        intrinsic,
                        return_type: concrete_return,
                    });
                    continue;
                }
            }

            // Validate trait bounds: module-level AND the member's own `where`
            // clause. The second was collected and then dropped here, so a
            // constraint like `where E: Display` on a member of a generic module
            // bound nothing at all — see the note in the instance-method path.
            let mut combined_bounds = module_info.trait_bounds.clone();
            combined_bounds.merge(&def.trait_bounds);
            let failures =
                self.unsatisfied_trait_bounds(&combined_bounds, &all_type_params, &type_args);
            if !failures.is_empty() {
                unsatisfied_bounds.extend(failures);
                continue;
            }

            // Build GenericFunctionDef with ALL type params and instantiate
            let generic_def = GenericFunctionDef {
                visibility: def.visibility,
                type_params: all_type_params,
                params: def.params.clone(),
                return_type: def.return_type.clone(),
                body: def.body.clone(),
                span: Span::point(def.source_file.clone(), 1, 1),
                container_name: Some(module_info.fqn.symbol.0.clone()),
                trait_bounds: combined_bounds,
                is_async: def.is_async,
                is_intrinsic: false,
            };

            let effective_fqn = Fqn {
                package: def.package.clone(),
                symbol: SymbolName(format!("{}.{}", module_info.fqn.symbol, method_name)),
            };

            let (mangled, return_type) = self.resolve_generic_function_template(
                &effective_fqn,
                &generic_def,
                &type_args,
                MethodKind::ModuleFunction,
            );
            results.push(ResolvedFunction::Regular {
                mangled_name: mangled,
                return_type,
                type_args: type_args.clone(),
            });
        }
        if results.is_empty()
            && let Some(span) = call_span
        {
            for message in dedup_messages(unsatisfied_bounds) {
                self.diagnostics.error(span.clone(), message);
            }
        }
        results
    }

    /// Resolve and instantiate a generic module global.
    /// Returns (mangled_name, type, mutable, type_args) if successful.
    pub(super) fn resolve_generic_module_global(
        &mut self,
        module_info: &ModuleInfo,
        global_name: &SymbolName,
        explicit_type_args: &[TypeExpr],
    ) -> Option<(MangledName, Type, bool, Vec<Type>)> {
        let def = module_info.generic_globals.get(global_name)?.clone();

        // Determine type args: explicit or from expected_type via bidirectional inference
        let type_args = if !explicit_type_args.is_empty() {
            if explicit_type_args.len() != def.type_params.len() {
                self.diagnostics.error(
                    Span::point(def.source_file.clone(), 1, 1),
                    format!(
                        "expected {} type argument(s) for module global '{}', found {}",
                        def.type_params.len(),
                        global_name,
                        explicit_type_args.len(),
                    ),
                );
                return None;
            }
            self.resolve_type_args(explicit_type_args)?
        } else {
            let expected = self.expected_type.as_ref()?;
            // Bidirectional: unify def.ty against expected to infer type args
            let mut substitution = TypeParamSubstitution::new();
            if substitution.unify(&def.ty, expected) {
                substitution.resolve_type_params(&def.type_params)?
            } else {
                return None;
            }
        };

        self.instantiate_and_resolve_generic_global(module_info, global_name, &def, type_args)
    }

    /// Common tail for generic module global resolution: substitute, mangle, instantiate.
    /// Returns (concrete_mangled_name, concrete_type, mutable, type_args).
    pub(super) fn instantiate_and_resolve_generic_global(
        &mut self,
        module_info: &ModuleInfo,
        global_name: &SymbolName,
        def: &GenericModuleGlobalDef,
        type_args: Vec<Type>,
    ) -> Option<(MangledName, Type, bool, Vec<Type>)> {
        // Substitute type params to get concrete type
        let substitution = TypeParamSubstitution::from_pairs(&def.type_params, &type_args);
        let concrete_ty = apply_substitution(&substitution, &def.ty);

        // Build effective FQN and mangled name
        let effective_fqn = Fqn {
            package: def.package.clone(),
            symbol: SymbolName(format!("{}.{}", module_info.fqn.symbol, global_name)),
        };

        // Globals on generic modules share one storage across all instantiations
        // (the rules pass forbids the declared type from referencing the module's
        // type parameters, so every `Foo<T>.global` resolves to the same global).
        let concrete_mangled = MangledName::for_global(&effective_fqn);
        let _ = type_args;

        Some((concrete_mangled, concrete_ty, def.mutable, vec![]))
    }
}

/// Distinct messages, in first-seen order: one call can produce the same
/// unsatisfied bound from several candidates (a module method and its property
/// twin, or two overloads sharing a constraint), and a caller wants to be told
/// once.
fn dedup_messages(messages: Vec<String>) -> Vec<String> {
    let mut seen = std::collections::BTreeSet::new();
    messages
        .into_iter()
        .filter(|m| seen.insert(m.clone()))
        .collect()
}
