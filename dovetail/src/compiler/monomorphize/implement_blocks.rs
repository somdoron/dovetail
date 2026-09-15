use std::collections::BTreeMap;

use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::typechecker::infer::make_display_name;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{
    Type, TypeDef, TypedExpr, TypedExprKind, TypedFunction, TypedImplementBlock, TypedModule,
    TypedParam,
};

use super::substitute::{apply_type_substitution, substitute_types_in_expr, unify_type};
use super::{SpecializationError, SpecializationRounds};

/// Inject defaulted trait members into typed implement blocks: for every
/// member the impl omitted (synthesized at collect with `is_default`), a
/// `TypedImplMethod` is created from the trait's default template with `Self`
/// and the trait's type params substituted for this block. Generic blocks
/// keep their own type params in the injected body — exactly like inline
/// members — so every downstream pass (expansion, per-instantiation
/// materialization, vtables, wrappers) works unchanged.
pub(super) fn inject_default_members(module: &mut TypedModule, registry: &Registry) {
    let templates = module.default_templates.clone();
    for block in &mut module.implement_blocks {
        let Some(trait_sig) = registry.lookup_trait(&block.trait_fqn, &block.trait_fqn.package)
        else {
            continue;
        };
        // The template for a member is written in the DEFAULT-SOURCE trait's
        // type params (the declaring trait, or — with extends — the nearest
        // overriding trait). Its application args as seen from this block:
        // the block's own trait args when the source IS the block's trait,
        // else the source's entry in the super closure, substituted.
        let source_bindings = |default_source: &Fqn| -> Option<BTreeMap<TypeParamName, Type>> {
            let mut bindings: BTreeMap<TypeParamName, Type> = BTreeMap::new();
            bindings.insert(TypeParamName("Self".to_string()), block.for_type.clone());
            let (source_params, source_args): (Vec<TypeParamName>, Vec<Type>) = if *default_source
                == block.trait_fqn
            {
                (trait_sig.type_params.clone(), block.trait_type_args.clone())
            } else {
                let source_sig = registry.lookup_trait(default_source, &default_source.package)?;
                let args = registry.super_closure_args(
                    &block.trait_fqn,
                    &block.trait_type_args,
                    default_source,
                )?;
                (source_sig.type_params.clone(), args)
            };
            for (tp, arg) in source_params.iter().zip(source_args.iter()) {
                bindings.insert(tp.clone(), arg.clone());
            }
            Some(bindings)
        };

        let inject =
            |name: &str,
             dispatch_name: SymbolName,
             default_source: &Fqn,
             method_parameters: &[TypeParamName],
             out: &mut Vec<crate::typechecker::types::TypedImplMethod>| {
                let template_mn =
                    MangledName::for_trait_default(default_source, &SymbolName(name.to_string()));
                let Some(template) = templates.get(&template_mn) else {
                    return;
                };
                let Some(bindings) = source_bindings(default_source) else {
                    return;
                };
                let params: Vec<TypedParam> = template
                    .params
                    .iter()
                    .map(|p| TypedParam {
                        name: p.name.clone(),
                        ty: apply_type_substitution(&p.ty, &bindings),
                        span: p.span.clone(),
                    })
                    .collect();
                let return_type = apply_type_substitution(&template.return_type, &bindings);
                let retarget = crate::compiler::monomorphize::substitute::DefaultRetarget {
                    from_traits: trait_sig
                        .super_closure
                        .iter()
                        .map(|(f, _)| f.clone())
                        .chain(std::iter::once(block.trait_fqn.clone()))
                        .collect(),
                    to_trait: block.trait_fqn.clone(),
                    to_trait_args: block.trait_type_args.clone(),
                    self_ty: block.for_type.clone(),
                };
                let _guard =
                    crate::compiler::monomorphize::substitute::DefaultRetargetGuard::install(
                        retarget,
                    );
                let body = substitute_types_in_expr(template.body.clone(), &bindings);
                drop(_guard);
                let method_parameters = template
                    .type_params
                    .iter()
                    .rev()
                    .take(method_parameters.len())
                    .cloned()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                out.push(crate::typechecker::types::TypedImplMethod {
                    name: dispatch_name,
                    method_type_params: method_parameters,
                    params,
                    return_type,
                    body,
                    span: template.span.clone(),
                    is_async: false,
                    visibility: template.visibility,
                });
            };

        for m in &trait_sig.methods {
            let Some(src) = &m.default_source else {
                continue;
            };
            if block
                .methods
                .iter()
                .any(|bm| bm.name == trait_sig.method_dispatch_name(m))
            {
                continue;
            }
            let mut injected = Vec::new();
            inject(
                &m.name,
                trait_sig.method_dispatch_name(m),
                src,
                &m.type_params,
                &mut injected,
            );
            if block.type_params.is_empty() {
                for method in &injected {
                    if method.method_type_params.is_empty() {
                        continue;
                    }
                    let name = crate::typechecker::types::impl_member_mangled_name(
                        &block.trait_fqn,
                        &block.for_type,
                        &[],
                        &method.name,
                        &block.trait_type_args,
                    );
                    module.functions.insert(
                        name.clone(),
                        TypedFunction {
                            visibility: method.visibility,
                            name,
                            source_name: method.name.0.clone(),
                            type_params: method.method_type_params.clone(),
                            params: method.params.clone(),
                            return_type: method.return_type.clone(),
                            body: method.body.clone(),
                            span: method.span.clone(),
                            vtable_self_type: None,
                            is_async: method.is_async,
                            display_name: make_display_name(&method.name.0, &method.params),
                        },
                    );
                }
            }
            block.methods.extend(injected);
        }
        for p in &trait_sig.properties {
            let Some(src) = &p.default_source else {
                continue;
            };
            if block.properties.iter().any(|bp| bp.name.0 == p.name) {
                continue;
            }
            let mut injected = Vec::new();
            inject(&p.name, SymbolName(p.name.clone()), src, &[], &mut injected);
            block.properties.extend(injected);
        }
    }
}

/// Materialize defaulted trait members that non-generic CLASSES omit: the
/// synthesized class-method signature was registered at collect
/// (`add_class_instance_method`); here the function body is created from the
/// trait's default template with `Self` bound to the class type.
pub(super) fn materialize_class_default_members(module: &mut TypedModule, registry: &Registry) {
    let templates = module.default_templates.clone();
    let mut new_functions: Vec<(MangledName, TypedFunction)> = Vec::new();
    for (class_fqn, class_sig) in registry.all_class_types() {
        if class_sig.trait_impls.is_empty() {
            continue;
        }
        let class_type = if class_sig.type_params.is_empty() {
            Type::Class(class_fqn.clone(), MangledName::for_type(class_fqn))
        } else {
            Type::GenericClass {
                fqn: class_fqn.clone(),
                mangled_name: MangledName::for_type(class_fqn),
                type_args: class_sig
                    .type_param_variances
                    .iter()
                    .copied()
                    .zip(class_sig.type_params.iter().map(|name| {
                        Type::TypeVariable(
                            name.clone(),
                            class_sig
                                .trait_bounds
                                .get(name)
                                .cloned()
                                .unwrap_or_default(),
                        )
                    }))
                    .collect(),
            }
        };
        for (trait_fqn, trait_args) in &class_sig.trait_impls {
            let Some(trait_sig) = registry.lookup_trait(trait_fqn, &trait_fqn.package) else {
                continue;
            };
            let source_bindings = |default_source: &Fqn| -> Option<BTreeMap<TypeParamName, Type>> {
                let mut bindings: BTreeMap<TypeParamName, Type> = BTreeMap::new();
                bindings.insert(TypeParamName("Self".to_string()), class_type.clone());
                let (source_params, source_args): (Vec<TypeParamName>, Vec<Type>) =
                    if *default_source == *trait_fqn {
                        (trait_sig.type_params.clone(), trait_args.clone())
                    } else {
                        let source_sig =
                            registry.lookup_trait(default_source, &default_source.package)?;
                        let args =
                            registry.super_closure_args(trait_fqn, trait_args, default_source)?;
                        (source_sig.type_params.clone(), args)
                    };
                for (tp, arg) in source_params.iter().zip(source_args.iter()) {
                    bindings.insert(tp.clone(), arg.clone());
                }
                Some(bindings)
            };
            let member_names = trait_sig
                .methods
                .iter()
                .filter_map(|m| {
                    m.default_source
                        .as_ref()
                        .map(|src| (m.name.clone(), src.clone(), false))
                })
                .chain(trait_sig.properties.iter().filter_map(|p| {
                    p.default_source
                        .as_ref()
                        .map(|src| (p.name.clone(), src.clone(), true))
                }));
            for (member_name, default_source, is_property) in member_names {
                let sym = SymbolName(member_name.clone());
                let mut sigs: Vec<_> = class_sig
                    .instance_methods
                    .get(&sym)
                    .into_iter()
                    .flatten()
                    .cloned()
                    .map(|signature| (signature, Vec::<TypeParamName>::new()))
                    .collect();
                if let Some(definitions) = class_sig.generic_instance_methods.get(&sym) {
                    for definition in definitions {
                        let parameters: Vec<_> =
                            definition.params.iter().map(|(_, ty)| ty).collect();
                        let fqn = Fqn {
                            package: class_fqn.package.clone(),
                            symbol: SymbolName(format!("{}.{}", class_fqn.symbol, sym)),
                        };
                        sigs.push((
                            crate::typechecker::registry::FunctionSignature {
                                visibility: definition.visibility,
                                mangled_name:
                                    crate::typechecker::class_trait_methods::template_name(
                                        &fqn,
                                        &parameters,
                                        definition.method_type_params.len(),
                                    ),
                                params: definition.params.clone(),
                                return_type: definition.return_type.clone(),
                                source_file: class_sig.span.file.clone(),
                                is_intrinsic: false,
                                is_property: definition.is_property,
                                is_final_method: definition.is_final_method,
                                is_abstract_method: definition.is_abstract_method,
                            },
                            definition.method_type_params.clone(),
                        ));
                    }
                }
                let Some(bindings) = source_bindings(&default_source) else {
                    continue;
                };
                let template_mn = MangledName::for_trait_default(&default_source, &sym);
                let Some(template) = templates.get(&template_mn) else {
                    continue;
                };
                let source_method_parameters: Vec<_> = template
                    .type_params
                    .iter()
                    .filter(|name| name.0.starts_with("$method$"))
                    .cloned()
                    .collect();
                let Some((sig, method_parameters, bindings)) =
                    sigs.iter().find_map(|(sig, method_parameters)| {
                        if method_parameters.len() != source_method_parameters.len()
                            || sig.is_property != is_property
                        {
                            return None;
                        }
                        let mut bindings = bindings.clone();
                        bindings.extend(
                            source_method_parameters.iter().cloned().zip(
                                method_parameters
                                    .iter()
                                    .map(|name| Type::TypeVariable(name.clone(), vec![])),
                            ),
                        );
                        let expected_params: Vec<_> = template
                            .params
                            .iter()
                            .map(|param| apply_type_substitution(&param.ty, &bindings))
                            .collect();
                        let expected_return =
                            apply_type_substitution(&template.return_type, &bindings);
                        (sig.params
                            .iter()
                            .map(|(_, ty)| ty)
                            .eq(expected_params.iter())
                            && sig.return_type == expected_return)
                            .then_some((sig, method_parameters, bindings))
                    })
                else {
                    continue;
                };
                if module.functions.contains_key(&sig.mangled_name)
                    || new_functions.iter().any(|(mn, _)| *mn == sig.mangled_name)
                {
                    continue;
                }
                let params: Vec<TypedParam> = template
                    .params
                    .iter()
                    .map(|p| TypedParam {
                        name: p.name.clone(),
                        ty: apply_type_substitution(&p.ty, &bindings),
                        span: p.span.clone(),
                    })
                    .collect();
                let return_type = apply_type_substitution(&template.return_type, &bindings);
                let retarget = crate::compiler::monomorphize::substitute::DefaultRetarget {
                    from_traits: trait_sig
                        .super_closure
                        .iter()
                        .map(|(f, _)| f.clone())
                        .chain(std::iter::once(trait_fqn.clone()))
                        .collect(),
                    to_trait: trait_fqn.clone(),
                    to_trait_args: trait_args.clone(),
                    self_ty: class_type.clone(),
                };
                // A default body materialized for a class must dispatch its
                // `self.member()` calls VIRTUALLY: the same body is inherited
                // by subclasses, and binding it statically to this class would
                // ignore their overrides (and could not resolve an abstract
                // member at all). The class TypeDef carries the vtable layout;
                // the substitution walk below rewrites the calls.
                let vtable_slots =
                    class_default_virtual_slots(module, registry, class_fqn, trait_sig, trait_args);
                let _guard =
                    crate::compiler::monomorphize::substitute::DefaultRetargetGuard::install(
                        retarget,
                    );
                let _vguard =
                    crate::compiler::monomorphize::substitute::ClassVirtualizeGuard::install(
                        crate::compiler::monomorphize::substitute::ClassVirtualize {
                            class_type: class_type.clone(),
                            trait_fqn: trait_fqn.clone(),
                            vtable_slots,
                        },
                    );
                let body = substitute_types_in_expr(template.body.clone(), &bindings);
                drop(_vguard);
                drop(_guard);
                let display_name =
                    make_display_name(&format!("{}.{}", class_fqn.symbol, member_name), &params);
                new_functions.push((
                    sig.mangled_name.clone(),
                    TypedFunction {
                        visibility: sig.visibility,
                        name: sig.mangled_name.clone(),
                        type_params: class_sig
                            .type_params
                            .iter()
                            .cloned()
                            .chain(method_parameters.iter().cloned())
                            .collect(),
                        params,
                        return_type,
                        body,
                        span: template.span.clone(),
                        vtable_self_type: class_default_vtable_self_type(
                            module,
                            class_fqn,
                            &sig.mangled_name,
                        ),
                        is_async: false,
                        source_name: member_name,
                        display_name,
                    },
                ));
            }
        }
    }
    for (mn, func) in new_functions {
        module.functions.insert(mn, func);
    }
}

fn class_default_vtable_self_type(
    module: &TypedModule,
    class_fqn: &Fqn,
    method_name: &MangledName,
) -> Option<Type> {
    let class_name = MangledName::for_type(class_fqn);
    let TypeDef::Class(class) = module.types.get(&class_name)? else {
        return None;
    };
    if !class
        .vtable_methods
        .iter()
        .any(|slot| MangledName::for_function(&slot.impl_fqn, &slot.param_types) == *method_name)
    {
        return None;
    }
    let TypeDef::Class(root) = module.types.get(&class.hierarchy_root_mangled)? else {
        return None;
    };
    Some(Type::Class(
        root.fqn.clone(),
        class.hierarchy_root_mangled.clone(),
    ))
}

fn class_default_virtual_slots(
    module: &TypedModule,
    registry: &Registry,
    class_fqn: &Fqn,
    trait_signature: &crate::typechecker::registry::TraitSignature,
    trait_parameters: &[Type],
) -> Vec<super::substitute::ClassVirtualSlot> {
    let class_name = MangledName::for_type(class_fqn);
    let Some(TypeDef::Class(class)) = module.types.get(&class_name) else {
        return Vec::new();
    };
    let mut substitution: BTreeMap<_, _> = trait_signature
        .type_params
        .iter()
        .cloned()
        .zip(trait_parameters.iter().cloned())
        .collect();
    substitution.insert(
        TypeParamName("Self".to_string()),
        Type::Class(class_fqn.clone(), class_name),
    );
    let signatures: Vec<_> = trait_signature
        .methods
        .iter()
        .map(|method| {
            (
                &method.name,
                trait_signature.method_dispatch_name(method),
                &method.params,
                false,
            )
        })
        .chain(trait_signature.properties.iter().map(|property| {
            (
                &property.name,
                SymbolName(property.name.clone()),
                &property.params,
                true,
            )
        }))
        .map(|(name, dispatch, parameters, is_property)| {
            (
                name,
                dispatch,
                parameters
                    .iter()
                    .map(|(_, ty)| apply_type_substitution(ty, &substitution))
                    .collect::<Vec<_>>(),
                is_property,
            )
        })
        .collect();
    class
        .vtable_methods
        .iter()
        .enumerate()
        .filter_map(|(index, slot)| {
            let parameters = class_slot_parameters(registry, slot)?;
            let (_, dispatch, _, _) =
                signatures.iter().find(|(name, _, expected, is_property)| {
                    **name == slot.method_name.0
                        && *is_property == slot.is_property
                        && expected.len() == parameters.len()
                        && expected.iter().skip(1).zip(parameters.iter().skip(1)).all(
                            |(expected, actual)| {
                                crate::typechecker::subtyping::identical(expected, actual)
                            },
                        )
                })?;
            Some(super::substitute::ClassVirtualSlot {
                method_name: dispatch.clone(),
                arity: parameters.len(),
                index: index as u32,
            })
        })
        .collect()
}

fn class_slot_parameters(
    registry: &Registry,
    slot: &crate::typechecker::types::VtableSlot,
) -> Option<Vec<Type>> {
    let (owner, _) = slot.impl_fqn.symbol.0.rsplit_once('.')?;
    let owner = Fqn {
        package: slot.impl_fqn.package.clone(),
        symbol: SymbolName(owner.to_string()),
    };
    let class = registry.get_class_type(&owner)?;
    let substitution = class
        .type_params
        .iter()
        .cloned()
        .zip(slot.impl_type_params.iter().cloned())
        .collect();
    Some(
        slot.param_types
            .iter()
            .map(|ty| apply_type_substitution(ty, &substitution))
            .collect(),
    )
}

pub(super) fn expand_non_generic_impl_blocks(module: &mut TypedModule) {
    for block in &module.implement_blocks {
        if !block.type_params.is_empty() {
            continue;
        }

        for method in block.methods.iter().chain(block.properties.iter()) {
            if !method.method_type_params.is_empty() {
                continue;
            }

            let mangled = crate::typechecker::types::impl_member_mangled_name(
                &block.trait_fqn,
                &block.for_type,
                &block.type_params,
                &method.name,
                &block.trait_type_args,
            );

            if module.functions.contains_key(&mangled) {
                continue;
            }

            let display_name = make_display_name(
                &format!("{}.{}", block.type_fqn, method.name),
                &method.params,
            );
            module.functions.insert(
                mangled.clone(),
                TypedFunction {
                    visibility: method.visibility,
                    name: mangled,
                    type_params: vec![],
                    params: method.params.clone(),
                    return_type: method.return_type.clone(),
                    body: method.body.clone(),
                    span: method.span.clone(),
                    vtable_self_type: None,
                    is_async: method.is_async,
                    source_name: method.name.0.clone(),
                    display_name,
                },
            );
        }
    }
}

pub(super) fn resolve_impl_calls(
    module: &mut TypedModule,
    registry: &Registry,
) -> Result<(), SpecializationError> {
    let impl_blocks = module.implement_blocks.clone();
    let existing_keys: std::collections::BTreeSet<MangledName> =
        module.functions.keys().cloned().collect();
    let mut new_functions: BTreeMap<MangledName, TypedFunction> = BTreeMap::new();
    for func in module.functions.values_mut() {
        func.body = resolve_impl_calls_in_expr(
            func.body.clone(),
            &impl_blocks,
            &mut new_functions,
            &existing_keys,
            registry,
        );
    }
    for global in module.globals.values_mut() {
        global.initializer = resolve_impl_calls_in_expr(
            global.initializer.clone(),
            &impl_blocks,
            &mut new_functions,
            &existing_keys,
            registry,
        );
    }
    for test in &mut module.tests {
        test.body = resolve_impl_calls_in_expr(
            test.body.clone(),
            &impl_blocks,
            &mut new_functions,
            &existing_keys,
            registry,
        );
    }
    for type_def in module.types.values_mut() {
        if let TypeDef::Class(class) = type_def {
            for expr in class
                .initializer
                .iter_mut()
                .chain(class.extends_args.iter_mut().flatten())
            {
                *expr = resolve_impl_calls_in_expr(
                    expr.clone(),
                    &impl_blocks,
                    &mut new_functions,
                    &existing_keys,
                    registry,
                );
            }
        }
    }
    let mut rounds = SpecializationRounds::default();
    loop {
        let keys: Vec<MangledName> = new_functions
            .keys()
            .filter(|k| !module.functions.contains_key(*k))
            .cloned()
            .collect();
        let Some(first_key) = keys.first() else {
            return Ok(());
        };
        rounds.advance(&new_functions[first_key])?;
        for key in &keys {
            module
                .functions
                .insert(key.clone(), new_functions[key].clone());
        }
        let mut more_functions: BTreeMap<MangledName, TypedFunction> = BTreeMap::new();
        for key in keys {
            let func = module.functions.get_mut(&key).unwrap();
            func.body = resolve_impl_calls_in_expr(
                func.body.clone(),
                &impl_blocks,
                &mut more_functions,
                &existing_keys,
                registry,
            );
        }
        new_functions = more_functions;
    }
}

/// When a bound-dispatched call's receiver type is an interface object (a
/// generic `T: I` instantiated with `I` itself, or an intersection containing
/// `I`), rewrite it to dynamic dispatch through the object's vtable — no impl
/// block exists for the object type. Returns `None` for non-object receivers.
fn try_dynamic_dispatch_for_interface_receiver(
    trait_fqn: &Fqn,
    trait_type_args: &[Type],
    for_type: &Type,
    method_name: &SymbolName,
    args: &[TypedExpr],
    registry: &Registry,
) -> Option<TypedExprKind> {
    let Type::InterfaceObject { traits, .. } = for_type else {
        return None;
    };
    // Preserve the bound's actual component/application before lowering an
    // inherited member to its origin slot. Several intersection components
    // may contain that origin with different implementations or arguments.
    let applies = |component: &&crate::typechecker::types::InterfaceComponent| {
        let args = if component.trait_fqn == *trait_fqn {
            Some(component.trait_type_args.clone())
        } else {
            registry.super_closure_args(&component.trait_fqn, &component.trait_type_args, trait_fqn)
        };
        args.is_some_and(|args| trait_type_args.is_empty() || args == trait_type_args)
    };
    let component = traits
        .iter()
        .filter(applies)
        .min_by_key(|component| component.trait_fqn != *trait_fqn)?;
    let trait_sig = registry.lookup_trait(trait_fqn, &trait_fqn.package)?;
    let receiver = args.first()?.clone();
    let receiver = if traits.len() > 1 {
        TypedExpr {
            ty: Type::interface_object(
                component.trait_fqn.clone(),
                component.trait_type_args.clone(),
            ),
            span: receiver.span.clone(),
            kind: TypedExprKind::InterfaceObjectUpcast {
                inner: Box::new(receiver),
            },
        }
    } else {
        receiver
    };
    let rest: Vec<TypedExpr> = args[1..].to_vec();
    // Member/slot identity from the ORIGIN trait's raw member (the bound's
    // trait may itself have inherited it).
    let (origin_key, member_name) = if let Some(method_sig) = trait_sig
        .methods
        .iter()
        .find(|m| trait_sig.method_dispatch_name(m) == *method_name)
    {
        let (_origin_sig, raw) = registry.origin_method_raw(trait_sig, method_sig);
        let param_strs: Vec<String> = raw
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .map(|(_, ty)| ty.to_string())
            .collect();
        let key = match &method_sig.origin {
            Some((f, _)) => MangledName::for_interface_object_per_interface(f),
            None => MangledName::for_interface_object_per_interface(trait_fqn),
        };
        (
            key,
            crate::common::types::InterfaceMemberName::new(&raw.name, &param_strs),
        )
    } else {
        let prop_sig = trait_sig
            .properties
            .iter()
            .find(|p| p.name == method_name.0)?;
        let key = match &prop_sig.origin {
            Some((f, _)) => MangledName::for_interface_object_per_interface(f),
            None => MangledName::for_interface_object_per_interface(trait_fqn),
        };
        (
            key,
            crate::common::types::InterfaceMemberName::new(&method_name.0, &[]),
        )
    };
    Some(TypedExprKind::InterfaceObjectMethodCall {
        interface_mangled_name: origin_key,
        method_name: method_name.0.clone(),
        member_name,
        receiver: Box::new(receiver),
        args: rest,
    })
}

fn find_impl_block_for_call<'a>(
    impl_blocks: &'a [TypedImplementBlock],
    trait_fqn: &Fqn,
    for_type: &Type,
    trait_type_params: &[Type],
    registry: &Registry,
) -> Option<&'a TypedImplementBlock> {
    if for_type.contains_type_parameter() || matches!(for_type, Type::Error) {
        return None;
    }
    // Interface objects have no single FQN and no impl blocks of their own.
    let type_fqn = for_type.try_to_fqn()?;
    // A type may implement the same trait multiple times with different trait
    // type-args (e.g. `Div<String> for Path` and `Div<Path> for Path`).
    // Disambiguate by also matching `trait_type_args` exactly when the call
    // site supplies them.
    let matches = |block: &&TypedImplementBlock| {
        if !crate::typechecker::registry::impl_receiver_family_matches(&block.type_fqn, &type_fqn) {
            return false;
        }
        let provided_args = if block.trait_fqn == *trait_fqn {
            block.trait_type_args.clone()
        } else {
            let Some(args) =
                registry.super_closure_args(&block.trait_fqn, &block.trait_type_args, trait_fqn)
            else {
                return false;
            };
            args
        };
        let mut bindings = BTreeMap::new();
        unify_type(&block.for_type, for_type, &mut bindings);
        if !trait_type_params.is_empty() {
            if provided_args.len() != trait_type_params.len() {
                return false;
            }
            for (provided, required) in provided_args.iter().zip(trait_type_params) {
                unify_type(provided, required, &mut bindings);
            }
            if provided_args
                .iter()
                .zip(trait_type_params)
                .any(|(provided, required)| {
                    apply_type_substitution(provided, &bindings) != *required
                })
            {
                return false;
            }
        }
        let receiver_matches = apply_type_substitution(&block.for_type, &bindings) == *for_type;
        if !receiver_matches || block.trait_bounds.iter().next().is_none() {
            return receiver_matches;
        }
        let Some(implementation) = registry
            .find_impl_blocks(&block.trait_fqn, &block.type_fqn)
            .into_iter()
            .find(|implementation| implementation.span == block.span)
        else {
            return false;
        };
        let mut substitution =
            crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::new();
        for (parameter, argument) in bindings {
            substitution.insert(parameter, argument);
        }
        crate::typechecker::infer::complete_impl_substitution(
            registry,
            implementation,
            substitution,
        )
        .is_some()
    };
    // A direct implementation of the resolved application wins. Check its
    // fully substituted arguments and bounds before considering providers;
    // a base-FQN fallback could select a sibling or an inapplicable block.
    impl_blocks
        .iter()
        .filter(matches)
        .min_by_key(|block| block.trait_fqn != *trait_fqn)
}

pub(super) fn ensure_vtable_method_function(
    impl_mangled: &MangledName,
    concrete_type: &Type,
    type_args: &[Type],
    impl_blocks: &[TypedImplementBlock],
    new_functions: &mut BTreeMap<MangledName, TypedFunction>,
    existing_functions: &std::collections::BTreeSet<MangledName>,
) {
    if concrete_type.contains_type_parameter() || matches!(concrete_type, Type::Error) {
        return;
    }
    let type_fqn = concrete_type.to_fqn();
    for block in impl_blocks {
        if !crate::typechecker::registry::impl_receiver_family_matches(&block.type_fqn, &type_fqn) {
            continue;
        }
        let mut bindings: BTreeMap<TypeParamName, Type> = BTreeMap::new();
        if !block.type_params.is_empty() {
            unify_type(&block.for_type, concrete_type, &mut bindings);
            // Coercion already resolved the full impl application, including
            // parameters inferred only from associated-type equalities.
            for (parameter, argument) in block.type_params.iter().zip(type_args) {
                bindings
                    .entry(parameter.clone())
                    .or_insert_with(|| argument.clone());
            }
        }
        for method in block.methods.iter().chain(block.properties.iter()) {
            let method_sym = SymbolName(method.name.0.clone());

            let candidate = if block.type_params.is_empty() {
                crate::typechecker::types::impl_member_mangled_name(
                    &block.trait_fqn,
                    &block.for_type,
                    &block.type_params,
                    &method_sym,
                    &block.trait_type_args,
                )
            } else {
                let impl_type_args: Vec<Type> = block
                    .type_params
                    .iter()
                    .map(|tp| bindings.get(tp).cloned().unwrap_or(Type::Error))
                    .collect();
                if impl_type_args.iter().any(|t| {
                    matches!(
                        t,
                        Type::Error | Type::TypeVariable(_, _) | Type::GenericParam(_, _, _)
                    )
                }) {
                    continue;
                }
                // RAW block args base + instantiation suffix — matching both
                // resolve_impl_method_mangled (the function creator) and the
                // coercion entries (member_entry uses raw_trait_type_args).
                crate::typechecker::types::impl_member_mangled_name(
                    &block.trait_fqn,
                    &block.for_type,
                    &block.type_params,
                    &method_sym,
                    &block.trait_type_args,
                )
                .with_type_args(&impl_type_args)
            };
            if candidate == *impl_mangled {
                let concrete_tta: Vec<Type> = block
                    .trait_type_args
                    .iter()
                    .map(|t| apply_type_substitution(t, &bindings))
                    .collect();
                let _ = resolve_impl_method_mangled(
                    block,
                    &method_sym,
                    concrete_type,
                    type_args,
                    &concrete_tta,
                    None,
                    new_functions,
                    existing_functions,
                    false,
                );
                return;
            }
        }
    }
}

fn try_resolve_class_impl_method(
    trait_fqn: &Fqn,
    for_type: &Type,
    method_name: &SymbolName,
    type_args: &[Type],
    existing_functions: &std::collections::BTreeSet<MangledName>,
    new_functions: &BTreeMap<MangledName, TypedFunction>,
    registry: &Registry,
) -> Option<MangledName> {
    if for_type.contains_type_parameter() || matches!(for_type, Type::Error) {
        return None;
    }
    // Interface objects have no single FQN and no impl blocks of their own.
    let type_fqn = for_type.try_to_fqn()?;

    if let Some(class_sig) = registry.get_class_type(&type_fqn) {
        if class_sig.type_params.is_empty() {
            // Which table can satisfy this member is decided by the TRAIT
            // member's self-ness: an instance member is never satisfied by a
            // same-named static (and vice versa). Falling through between the
            // tables would hand one the other's calling convention.
            let member_is_static = registry
                .lookup_trait(trait_fqn, &trait_fqn.package)
                .map(|sig| {
                    let takes_self = |params: &[(String, Type)]| {
                        params.first().is_some_and(|(n, _)| n == "self")
                    };
                    if let Some(m) = sig.methods.iter().find(|m| m.name == method_name.0) {
                        !takes_self(&m.params)
                    } else if let Some(p) = sig.properties.iter().find(|p| p.name == method_name.0)
                    {
                        !takes_self(&p.params)
                    } else {
                        false
                    }
                })
                .unwrap_or(false);
            let find_in_sig = |sig: &crate::typechecker::registry::ClassTypeSignature,
                               mn: &SymbolName|
             -> Option<MangledName> {
                if member_is_static {
                    // A class can satisfy a STATIC trait member with its own
                    // static function; those live in a separate table.
                    if let Some(overloads) = sig.static_methods.get(mn)
                        && let Some(s) = overloads.first()
                    {
                        return Some(s.mangled_name.clone());
                    }
                    return None;
                }
                if let Some(overloads) = sig.instance_methods.get(mn)
                    && let Some(s) = overloads.first()
                    && !s.is_abstract_method
                {
                    return Some(s.mangled_name.clone());
                }
                if let Some(defs) = sig.generic_instance_methods.get(mn)
                    && let Some(def) = defs.first()
                    && !def.is_abstract_method
                {
                    let param_types: Vec<&Type> = def.params.iter().map(|(_, t)| t).collect();
                    let method_fqn = Fqn {
                        package: sig.fqn.package.clone(),
                        symbol: SymbolName(format!("{}.{}", sig.fqn.symbol, mn)),
                    };
                    return Some(crate::typechecker::class_trait_methods::template_name(
                        &method_fqn,
                        &param_types,
                        def.method_type_params.len(),
                    ));
                }
                None
            };
            let mut base_mangled = find_in_sig(class_sig, method_name);
            if base_mangled.is_none() {
                let mut current_parent = class_sig.parent_class.clone();
                while let Some(ref parent_fqn) = current_parent {
                    if let Some(parent_sig) = registry.get_class_type(parent_fqn) {
                        base_mangled = find_in_sig(parent_sig, method_name);
                        if base_mangled.is_some() {
                            break;
                        }
                        current_parent = parent_sig.parent_class.clone();
                    } else {
                        break;
                    }
                }
            }

            let base_mangled = base_mangled?;
            let mangled = if !type_args.is_empty() {
                base_mangled.with_type_args(type_args)
            } else {
                base_mangled
            };
            if existing_functions.contains(&mangled) || new_functions.contains_key(&mangled) {
                return Some(mangled);
            }
            return None;
        }

        let class_type_args: Vec<Type> = match for_type {
            Type::GenericClass { type_args: cta, .. } => {
                cta.iter().map(|(_, t)| t.clone()).collect()
            }
            _ => vec![],
        };

        let _bindings: BTreeMap<TypeParamName, Type> = class_sig
            .type_params
            .iter()
            .zip(class_type_args.iter())
            .map(|(p, a)| (p.clone(), a.clone()))
            .collect();

        let find_class_method = |sig: &crate::typechecker::registry::ClassTypeSignature,
                                 mn: &SymbolName|
         -> Option<MangledName> {
            let generic_params: Option<(Vec<(String, Type)>, usize)> = sig
                .instance_methods
                .get(mn)
                .and_then(|o| o.first().filter(|s| !s.is_abstract_method))
                .map(|s| (s.params.clone(), 0))
                .or_else(|| {
                    sig.generic_instance_methods
                        .get(mn)
                        .and_then(|d| d.first().filter(|def| !def.is_abstract_method))
                        .map(|def| (def.params.clone(), def.method_type_params.len()))
                })
                .or_else(|| {
                    sig.static_methods
                        .get(mn)
                        .and_then(|o| o.first())
                        .map(|s| (s.params.clone(), 0))
                });
            let (params, method_parameter_count) = generic_params?;
            // Use template-based naming: generic param types (from registry) + type_args.
            // This matches how instantiate_generic_classes names functions.
            let param_types: Vec<&Type> = params.iter().map(|(_, t)| t).collect();
            let method_fqn = Fqn {
                package: sig.fqn.package.clone(),
                symbol: SymbolName(format!("{}.{}", sig.fqn.symbol, mn)),
            };
            if class_type_args.is_empty() {
                return Some(crate::typechecker::class_trait_methods::template_name(
                    &method_fqn,
                    &param_types,
                    method_parameter_count,
                ));
            }
            Some(
                crate::typechecker::class_trait_methods::template_name(
                    &method_fqn,
                    &param_types,
                    method_parameter_count,
                )
                .with_type_args(&class_type_args),
            )
        };

        if let Some(mangled) = find_class_method(class_sig, method_name)
            && (existing_functions.contains(&mangled) || new_functions.contains_key(&mangled))
        {
            return Some(mangled);
        }

        let mut current_parent = class_sig.parent_class.clone();
        while let Some(ref parent_fqn) = current_parent {
            if let Some(parent_sig) = registry.get_class_type(parent_fqn) {
                if let Some(mangled) = find_class_method(parent_sig, method_name) {
                    let mangled = if class_type_args.is_empty() {
                        mangled
                    } else {
                        mangled.with_type_args(&class_type_args)
                    };
                    if existing_functions.contains(&mangled) || new_functions.contains_key(&mangled)
                    {
                        return Some(mangled);
                    }
                }
                current_parent = parent_sig.parent_class.clone();
            } else {
                break;
            }
        }

        return None;
    }

    let trait_impls = registry.find_impl_blocks(trait_fqn, &type_fqn);
    for impl_info in &trait_impls {
        let mut concrete_trait_type_args = impl_info.trait_type_args.clone();
        let mut impl_type_args_for_mangling: Vec<Type> = Vec::new();

        if !impl_info.type_params.is_empty() {
            let mut bindings: BTreeMap<TypeParamName, Type> = BTreeMap::new();
            unify_type(&impl_info.for_type, for_type, &mut bindings);
            concrete_trait_type_args = impl_info
                .trait_type_args
                .iter()
                .map(|t| apply_type_substitution(t, &bindings))
                .collect();
            impl_type_args_for_mangling = impl_info
                .type_params
                .iter()
                .map(|tp| bindings.get(tp).cloned().unwrap_or(Type::Error))
                .collect();
        }

        let mut mangled = crate::typechecker::types::impl_member_mangled_name(
            trait_fqn,
            &impl_info.for_type,
            &impl_info.type_params,
            method_name,
            &concrete_trait_type_args,
        );
        if !impl_type_args_for_mangling.is_empty() {
            mangled = mangled.with_type_args(&impl_type_args_for_mangling);
        }
        if !type_args.is_empty() {
            mangled = mangled.with_type_args(type_args);
        }
        if existing_functions.contains(&mangled) || new_functions.contains_key(&mangled) {
            return Some(mangled);
        }
    }

    let empty_trait_args: Vec<Type> = vec![];
    let mut mangled =
        MangledName::for_impl_method(trait_fqn, &type_fqn, method_name, &empty_trait_args);
    if !type_args.is_empty() {
        mangled = mangled.with_type_args(type_args);
    }
    if existing_functions.contains(&mangled) || new_functions.contains_key(&mangled) {
        return Some(mangled);
    }
    None
}

/// Returns (mangled_name, type_params for FunctionCall node).
/// When the concrete function body is created here, type_params is empty.
/// When deferred to resolve_template_function_calls (non-generic impl block
/// with method-level type params), returns the template name + method type args.
#[allow(
    clippy::too_many_arguments,
    reason = "Keep the compiler context parameters explicit at this call boundary."
)]
fn resolve_impl_method_mangled(
    block: &TypedImplementBlock,
    method_name: &SymbolName,
    for_type: &Type,
    type_args: &[Type],
    trait_type_params: &[Type],
    result_type: Option<&Type>,
    new_functions: &mut BTreeMap<MangledName, TypedFunction>,
    existing_functions: &std::collections::BTreeSet<MangledName>,
    via_routed: bool,
) -> (MangledName, Vec<Type>) {
    let mut mangled = crate::typechecker::types::impl_member_mangled_name(
        &block.trait_fqn,
        &block.for_type,
        &block.type_params,
        method_name,
        &block.trait_type_args,
    );

    let method = block
        .methods
        .iter()
        .chain(block.properties.iter())
        .find(|m| m.name == *method_name);

    let has_block_type_params = !block.type_params.is_empty();
    let has_method_type_params = method
        .as_ref()
        .is_some_and(|m| !m.method_type_params.is_empty());
    if !has_block_type_params && !has_method_type_params {
        if method.is_none() && !type_args.is_empty() {
            return (mangled.with_type_args(type_args), vec![]);
        }
        return (mangled, vec![]);
    }

    let mut bindings: BTreeMap<TypeParamName, Type> = BTreeMap::new();
    let mut all_mangled_type_args: Vec<Type> = Vec::new();

    if has_block_type_params {
        unify_type(&block.for_type, for_type, &mut bindings);
        // Also unify trait type args to bind type params that only appear in the trait
        // (e.g., `implement <A, B, T> FlatZip<T> for Parser<(A, B)>` — T is only in FlatZip<T>).
        // Skipped for `extends`-routed calls (the caller's trait args belong to
        // a super trait, not this block's trait) — `via_routed` marks those.
        if !via_routed {
            for (generic_arg, concrete_arg) in
                block.trait_type_args.iter().zip(trait_type_params.iter())
            {
                unify_type(generic_arg, concrete_arg, &mut bindings);
            }
        }
        for (parameter, argument) in block.type_params.iter().zip(type_args) {
            bindings
                .entry(parameter.clone())
                .or_insert_with(|| argument.clone());
        }
        // The operands already selected this block. Its resolved result can
        // complete output-only parameters for calls through a generic bound.
        if let (Some(method), Some(result_type)) = (method, result_type) {
            unify_type(&method.return_type, result_type, &mut bindings);
        }
        let impl_type_args: Vec<Type> = block
            .type_params
            .iter()
            .map(|tp| bindings.get(tp).cloned().unwrap_or(Type::Error))
            .collect();
        all_mangled_type_args.extend(impl_type_args);
    }

    if has_method_type_params {
        let method = method.as_ref().unwrap();
        let method_type_args = &type_args[block.type_params.len()..];
        for (tp, ty) in method
            .method_type_params
            .iter()
            .zip(method_type_args.iter())
        {
            bindings.insert(tp.clone(), ty.clone());
        }
        all_mangled_type_args.extend_from_slice(method_type_args);
    }

    // For non-generic impl blocks with method-level type params, the real body
    // lives in a template TypedFunction (created by typecheck_generic_impl_method).
    // Emit a FunctionCall with the template name + method type args so that
    // resolve_template_function_calls creates the concrete copy.
    if !has_block_type_params && has_method_type_params {
        let method_type_args: Vec<Type> = all_mangled_type_args;
        // mangled is the template name (no type args appended)
        return (mangled, method_type_args);
    }

    mangled = mangled.with_type_args(&all_mangled_type_args);

    if !new_functions.contains_key(&mangled)
        && !existing_functions.contains(&mangled)
        && let Some(method) = method
    {
        let params: Vec<TypedParam> = method
            .params
            .iter()
            .map(|p| TypedParam {
                name: p.name.clone(),
                ty: apply_type_substitution(&p.ty, &bindings),
                span: p.span.clone(),
            })
            .collect();
        let return_type = apply_type_substitution(&method.return_type, &bindings);
        let body = substitute_types_in_expr(method.body.clone(), &bindings);
        let display_name =
            make_display_name(&format!("{}.{}", block.type_fqn, method_name), &params);
        new_functions.insert(
            mangled.clone(),
            TypedFunction {
                visibility: method.visibility,
                name: mangled.clone(),
                type_params: vec![],
                params,
                return_type,
                body,
                span: method.span.clone(),
                vtable_self_type: None,
                is_async: method.is_async,
                source_name: method.name.0.clone(),
                display_name,
            },
        );
    }

    (mangled, vec![])
}

fn complete_bound_impl_type_args(
    block: &TypedImplementBlock,
    method_name: &SymbolName,
    requested_trait: &Fqn,
    for_type: &Type,
    trait_type_args: &[Type],
    supplied: &[Type],
    registry: &Registry,
) -> Vec<Type> {
    let method_param_count = block
        .methods
        .iter()
        .chain(&block.properties)
        .find(|method| method.name == *method_name)
        .map_or(0, |method| method.method_type_params.len());
    // Sugar may know only the receiver's arguments, or only the method's
    // arguments for a bound receiver. Recover the method suffix and infer
    // the implementation prefix from the selected block's actual signature.
    let method_args = &supplied[supplied.len().saturating_sub(method_param_count)..];
    if block.type_params.is_empty() {
        return method_args.to_vec();
    }
    use crate::typechecker::infer::type_param_substitution::TypeParamSubstitution;
    let Some(implementation) = registry
        .find_impl_blocks(&block.trait_fqn, &block.type_fqn)
        .into_iter()
        .find(|implementation| implementation.span == block.span)
    else {
        return supplied.to_vec();
    };
    // This unifier consumes registry TypeVariable patterns. Typed block
    // arguments contain inference GenericParams and are not binding patterns.
    let provided_args = if implementation.trait_fqn == *requested_trait {
        implementation.trait_type_args.clone()
    } else {
        let Some(args) = registry.super_closure_args(
            &implementation.trait_fqn,
            &implementation.trait_type_args,
            requested_trait,
        ) else {
            return supplied.to_vec();
        };
        args
    };
    let mut substitution = TypeParamSubstitution::new();
    if !substitution.unify(&implementation.for_type, for_type)
        || !provided_args
            .iter()
            .zip(trait_type_args)
            .all(|(pattern, actual)| substitution.unify(pattern, actual))
    {
        return supplied.to_vec();
    }
    let Some(mut args) = crate::typechecker::infer::complete_impl_substitution(
        registry,
        implementation,
        substitution,
    )
    .and_then(|substitution| substitution.resolve_type_params(&block.type_params)) else {
        return supplied.to_vec();
    };
    args.extend_from_slice(method_args);
    args
}

fn resolve_impl_calls_in_expr(
    expr: TypedExpr,
    impl_blocks: &[TypedImplementBlock],
    new_functions: &mut BTreeMap<MangledName, TypedFunction>,
    existing_functions: &std::collections::BTreeSet<MangledName>,
    registry: &Registry,
) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;
    let kind = match expr.kind {
        TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args,
            method_type_params,
        } => {
            let args: Vec<TypedExpr> = args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect();
            // Skip resolution if method_type_params contain TypeParameter types —
            // this is a template body that will be substituted later.
            if method_type_params
                .iter()
                .any(|t| t.contains_type_parameter())
            {
                TypedExprKind::ImplFunctionCall {
                    trait_fqn,
                    trait_type_params,
                    for_type,
                    method_name,
                    args,
                    method_type_params,
                }
            } else if let Some(kind) = try_dynamic_dispatch_for_interface_receiver(
                &trait_fqn,
                &trait_type_params,
                &for_type,
                &method_name,
                &args,
                registry,
            ) {
                // A generic bound instantiated with an interface-object type:
                // there is no impl block for the object type itself — dispatch
                // dynamically through the object's vtable instead.
                kind
            } else if let Some(op) = for_type
                .try_to_fqn()
                .and_then(|fqn| {
                    crate::typechecker::types::primitive_binary_operator(&fqn, &method_name.0)
                })
                .filter(|_| {
                    trait_fqn.package.to_string() == "standard.prelude"
                        && matches!(
                            trait_fqn.symbol.0.as_str(),
                            "Add" | "Sub" | "Mul" | "Div" | "Concat"
                        )
                })
            {
                TypedExprKind::BinaryOp {
                    op,
                    left: Box::new(args[0].clone()),
                    right: Box::new(args[1].clone()),
                }
            } else if let Some(block) = find_impl_block_for_call(
                impl_blocks,
                &trait_fqn,
                &for_type,
                &trait_type_params,
                registry,
            ) {
                let mut receiver_substitution =
                    crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::new(
                    );
                receiver_substitution.unify(&block.for_type, &for_type);
                let provider_parameters: Vec<_> = block
                    .trait_type_args
                    .iter()
                    .map(|ty| {
                        crate::typechecker::infer::generics::apply_substitution(
                            &receiver_substitution,
                            ty,
                        )
                    })
                    .collect();
                let method_name = registry.route_trait_method(
                    &block.trait_fqn,
                    &provider_parameters,
                    &trait_fqn,
                    &trait_type_params,
                    &method_name,
                );
                let method_type_params = complete_bound_impl_type_args(
                    block,
                    &method_name,
                    &trait_fqn,
                    &for_type,
                    &trait_type_params,
                    &method_type_params,
                    registry,
                );
                let (mangled, call_type_params) = resolve_impl_method_mangled(
                    block,
                    &method_name,
                    &for_type,
                    &method_type_params,
                    &trait_type_params,
                    Some(&ty),
                    new_functions,
                    existing_functions,
                    block.trait_fqn != trait_fqn,
                );
                TypedExprKind::FunctionCall {
                    name: mangled,
                    args,
                    type_params: call_type_params,
                }
            } else if let Some(slot) = crate::typechecker::infer::class_trait_virtual_slot(
                registry,
                &for_type,
                &trait_fqn,
                &trait_type_params,
                &method_name,
            ) {
                TypedExprKind::ClassVirtualCall {
                    object: Box::new(args[0].clone()),
                    vtable_slot: slot,
                    args,
                }
            } else if let Some((template, arguments)) =
                crate::typechecker::class_trait_methods::resolve_template(
                    registry,
                    &trait_fqn,
                    &trait_type_params,
                    &for_type,
                    &method_name,
                    &method_type_params,
                )
            {
                TypedExprKind::FunctionCall {
                    name: template,
                    args,
                    type_params: arguments,
                }
            } else if let Some(mangled) = try_resolve_class_impl_method(
                &trait_fqn,
                &for_type,
                &method_name,
                &method_type_params,
                existing_functions,
                new_functions,
                registry,
            ) {
                TypedExprKind::FunctionCall {
                    name: mangled,
                    args,
                    type_params: vec![],
                }
            } else {
                TypedExprKind::ImplFunctionCall {
                    trait_fqn,
                    trait_type_params,
                    for_type,
                    method_name,
                    args,
                    method_type_params,
                }
            }
        }
        TypedExprKind::ImplFunctionRef {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            method_type_params,
        } => {
            if method_type_params
                .iter()
                .any(|t| t.contains_type_parameter())
            {
                TypedExprKind::ImplFunctionRef {
                    trait_fqn,
                    trait_type_params,
                    for_type,
                    method_name,
                    method_type_params,
                }
            } else if let Some(block) = find_impl_block_for_call(
                impl_blocks,
                &trait_fqn,
                &for_type,
                &trait_type_params,
                registry,
            ) {
                let mut receiver_substitution =
                    crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::new(
                    );
                receiver_substitution.unify(&block.for_type, &for_type);
                let provider_parameters: Vec<_> = block
                    .trait_type_args
                    .iter()
                    .map(|ty| {
                        crate::typechecker::infer::generics::apply_substitution(
                            &receiver_substitution,
                            ty,
                        )
                    })
                    .collect();
                let method_name = registry.route_trait_method(
                    &block.trait_fqn,
                    &provider_parameters,
                    &trait_fqn,
                    &trait_type_params,
                    &method_name,
                );
                let method_type_params = complete_bound_impl_type_args(
                    block,
                    &method_name,
                    &trait_fqn,
                    &for_type,
                    &trait_type_params,
                    &method_type_params,
                    registry,
                );
                let result_type = match &ty {
                    Type::Function(_, result) => Some(result.as_ref()),
                    _ => None,
                };
                let (mangled, call_type_params) = resolve_impl_method_mangled(
                    block,
                    &method_name,
                    &for_type,
                    &method_type_params,
                    &trait_type_params,
                    result_type,
                    new_functions,
                    existing_functions,
                    block.trait_fqn != trait_fqn,
                );
                TypedExprKind::FunctionRef {
                    name: mangled,
                    type_params: call_type_params,
                }
            } else if let Some((template, arguments)) =
                crate::typechecker::class_trait_methods::resolve_template(
                    registry,
                    &trait_fqn,
                    &trait_type_params,
                    &for_type,
                    &method_name,
                    &method_type_params,
                )
            {
                TypedExprKind::FunctionRef {
                    name: template,
                    type_params: arguments,
                }
            } else if let Some(mangled) = try_resolve_class_impl_method(
                &trait_fqn,
                &for_type,
                &method_name,
                &method_type_params,
                existing_functions,
                new_functions,
                registry,
            ) {
                TypedExprKind::FunctionRef {
                    name: mangled,
                    type_params: vec![],
                }
            } else {
                TypedExprKind::ImplFunctionRef {
                    trait_fqn,
                    trait_type_params,
                    for_type,
                    method_name,
                    method_type_params,
                }
            }
        }
        TypedExprKind::Block(exprs) => TypedExprKind::Block(
            exprs
                .into_iter()
                .map(|e| {
                    resolve_impl_calls_in_expr(
                        e,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        ),
        TypedExprKind::FunctionCall {
            name,
            args,
            type_params,
        } => TypedExprKind::FunctionCall {
            name,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        },
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => TypedExprKind::If {
            condition: Box::new(resolve_impl_calls_in_expr(
                *condition,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            then_branch: Box::new(resolve_impl_calls_in_expr(
                *then_branch,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            else_branch: else_branch.map(|e| {
                Box::new(resolve_impl_calls_in_expr(
                    *e,
                    impl_blocks,
                    new_functions,
                    existing_functions,
                    registry,
                ))
            }),
        },
        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(resolve_impl_calls_in_expr(
                *condition,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            body: Box::new(resolve_impl_calls_in_expr(
                *body,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
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
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::LetDestructure {
            pattern,
            value,
            var_ty,
        } => TypedExprKind::LetDestructure {
            pattern,
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            var_ty,
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
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::FieldAssign {
            object,
            field_name,
            field_index,
            value,
            boxed,
        } => TypedExprKind::FieldAssign {
            object: Box::new(resolve_impl_calls_in_expr(
                *object,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            field_name,
            field_index,
            boxed,
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(resolve_impl_calls_in_expr(
                *left,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            right: Box::new(resolve_impl_calls_in_expr(
                *right,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(resolve_impl_calls_in_expr(
                *operand,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(resolve_impl_calls_in_expr(
                *subject,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            arms: arms
                .into_iter()
                .map(|arm| crate::typechecker::types::TypedMatchArm {
                    pattern: arm.pattern,
                    guard: arm.guard.map(|g| {
                        Box::new(resolve_impl_calls_in_expr(
                            *g,
                            impl_blocks,
                            new_functions,
                            existing_functions,
                            registry,
                        ))
                    }),
                    body: Box::new(resolve_impl_calls_in_expr(
                        *arm.body,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )),
                    span: arm.span,
                })
                .collect(),
        },
        TypedExprKind::RecordCreate {
            fqn,
            fields,
            type_params,
        } => TypedExprKind::RecordCreate {
            fqn,
            fields: fields
                .into_iter()
                .map(|(n, e)| {
                    (
                        n,
                        resolve_impl_calls_in_expr(
                            e,
                            impl_blocks,
                            new_functions,
                            existing_functions,
                            registry,
                        ),
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements
                .into_iter()
                .map(|e| {
                    resolve_impl_calls_in_expr(
                        e,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
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
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::FieldAccess {
            object,
            field_name,
            field_index,
            boxed,
        } => TypedExprKind::FieldAccess {
            object: Box::new(resolve_impl_calls_in_expr(
                *object,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            field_name,
            field_index,
            boxed,
        },
        TypedExprKind::Closure {
            params,
            body,
            captures,
        } => TypedExprKind::Closure {
            params,
            body: Box::new(resolve_impl_calls_in_expr(
                *body,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            captures,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(resolve_impl_calls_in_expr(
                *callee,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        },
        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(resolve_impl_calls_in_expr(
                *message,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(resolve_impl_calls_in_expr(
                *condition,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            message: message.map(|m| {
                Box::new(resolve_impl_calls_in_expr(
                    *m,
                    impl_blocks,
                    new_functions,
                    existing_functions,
                    registry,
                ))
            }),
        },
        TypedExprKind::RecordWith {
            object,
            fqn,
            overrides,
            type_params,
        } => TypedExprKind::RecordWith {
            object: Box::new(resolve_impl_calls_in_expr(
                *object,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            fqn,
            overrides: overrides
                .into_iter()
                .map(|(n, idx, e)| {
                    (
                        n,
                        idx,
                        resolve_impl_calls_in_expr(
                            e,
                            impl_blocks,
                            new_functions,
                            existing_functions,
                            registry,
                        ),
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            target_type,
        },
        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            target_type,
        },
        TypedExprKind::InterfaceObjectCoerce {
            inner,
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        } => {
            for (_component_mn, entries) in &vtable_methods {
                for (_member_name, vtable_impl_mangled, vtable_type_args) in entries {
                    if !existing_functions.contains(vtable_impl_mangled)
                        && !new_functions.contains_key(vtable_impl_mangled)
                    {
                        ensure_vtable_method_function(
                            vtable_impl_mangled,
                            &concrete_type,
                            vtable_type_args,
                            impl_blocks,
                            new_functions,
                            existing_functions,
                        );
                    }
                }
            }
            TypedExprKind::InterfaceObjectCoerce {
                inner: Box::new(resolve_impl_calls_in_expr(
                    *inner,
                    impl_blocks,
                    new_functions,
                    existing_functions,
                    registry,
                )),
                interface_mangled_name,
                concrete_type,
                vtable_methods,
            }
        }
        TypedExprKind::TemplateInterfaceObjectCoerce {
            inner,
            traits,
            concrete_type,
        } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: Box::new(resolve_impl_calls_in_expr(
                *inner,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            traits,
            concrete_type,
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(resolve_impl_calls_in_expr(
                *inner,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
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
            receiver: Box::new(resolve_impl_calls_in_expr(
                *receiver,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        },
        TypedExprKind::MethodRef {
            object,
            method_name,
            type_params,
        } => TypedExprKind::MethodRef {
            object: Box::new(resolve_impl_calls_in_expr(
                *object,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            method_name,
            type_params,
        },
        TypedExprKind::ClassVirtualCall {
            object,
            vtable_slot,
            args,
        } => TypedExprKind::ClassVirtualCall {
            object: Box::new(resolve_impl_calls_in_expr(
                *object,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            vtable_slot,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        },
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        },
        TypedExprKind::ClassStructCreate {
            target_mangled_name,
            fields,
            type_params,
        } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        },
        TypedExprKind::ClassNew {
            mangled_name,
            args,
            type_params,
        } => TypedExprKind::ClassNew {
            mangled_name,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
            return_type,
        },
        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements
                .into_iter()
                .map(|e| {
                    resolve_impl_calls_in_expr(
                        e,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect(),
        },
        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: Box::new(resolve_impl_calls_in_expr(
                *inner,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::GlobalAssign {
            name,
            type_params,
            value,
        } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(resolve_impl_calls_in_expr(
                *value,
                impl_blocks,
                new_functions,
                existing_functions,
                registry,
            )),
        },
        TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args,
            type_params,
        } => {
            let args: Vec<TypedExpr> = args
                .into_iter()
                .map(|a| {
                    resolve_impl_calls_in_expr(
                        a,
                        impl_blocks,
                        new_functions,
                        existing_functions,
                        registry,
                    )
                })
                .collect();
            TypedExprKind::ExtFunctionCall {
                ext_fqn,
                for_type,
                method_name,
                args,
                type_params,
            }
        }
        kind @ TypedExprKind::ExtFunctionRef { .. } => kind,
        other @ (TypedExprKind::UnitLiteral
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
        | TypedExprKind::FunctionRef { .. }
        | TypedExprKind::GlobalRef { .. }
        | TypedExprKind::Break
        | TypedExprKind::Continue
        | TypedExprKind::Await { .. }
        | TypedExprKind::ForLoop { .. }
        | TypedExprKind::AsyncBlock { .. }
        | TypedExprKind::Try { .. }
        | TypedExprKind::Use { .. }) => other,
    };
    TypedExpr { kind, ty, span }
}
