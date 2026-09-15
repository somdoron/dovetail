use std::collections::{BTreeMap, BTreeSet};

use crate::common::types::{Fqn, MangledName, SymbolName, TypeParamName};
use crate::typechecker::infer::make_display_name;
use crate::typechecker::types::{
    Type, TypedExpr, TypedExprKind, TypedExtensionBlock, TypedFunction, TypedModule, TypedParam,
};

use super::substitute::{apply_type_substitution, substitute_types_in_expr, unify_type};
use super::{SpecializationError, SpecializationRounds};

pub(super) fn expand_non_generic_ext_blocks(module: &mut TypedModule) {
    for block in &module.extension_blocks {
        if !block.type_params.is_empty() {
            continue;
        }

        for method in block.methods.iter().chain(block.properties.iter()) {
            if !method.method_type_params.is_empty() {
                continue;
            }

            let param_types: Vec<&Type> = method.params.iter().map(|p| &p.ty).collect();
            let mangled = MangledName::for_named_extension_method(
                &block.ext_fqn.package,
                &block.ext_fqn.symbol,
                &method.name,
                &block.for_type,
                &param_types,
            );

            if module.functions.contains_key(&mangled) {
                continue;
            }

            let display_name = make_display_name(
                &format!("{}.{}", block.ext_fqn, method.name),
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

pub(super) fn resolve_ext_calls(module: &mut TypedModule) -> Result<(), SpecializationError> {
    let ext_blocks = module.extension_blocks.clone();
    let existing_keys: BTreeSet<MangledName> = module.functions.keys().cloned().collect();
    let mut new_functions: BTreeMap<MangledName, TypedFunction> = BTreeMap::new();
    for func in module.functions.values_mut() {
        func.body = resolve_ext_calls_in_expr(
            func.body.clone(),
            &ext_blocks,
            &mut new_functions,
            &existing_keys,
        );
    }
    for global in module.globals.values_mut() {
        global.initializer = resolve_ext_calls_in_expr(
            global.initializer.clone(),
            &ext_blocks,
            &mut new_functions,
            &existing_keys,
        );
    }
    for test in &mut module.tests {
        test.body = resolve_ext_calls_in_expr(
            test.body.clone(),
            &ext_blocks,
            &mut new_functions,
            &existing_keys,
        );
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
            func.body = resolve_ext_calls_in_expr(
                func.body.clone(),
                &ext_blocks,
                &mut more_functions,
                &existing_keys,
            );
        }
        new_functions = more_functions;
    }
}

fn find_ext_block_for_call<'a>(
    ext_blocks: &'a [TypedExtensionBlock],
    ext_fqn: &Fqn,
    for_type: &Type,
) -> Option<&'a TypedExtensionBlock> {
    // Multiple blocks may share an `ext_fqn` (one named extension targeting several
    // different `for_type`s). Disambiguate by `for_type`: non-generic blocks need an
    // exact match; generic blocks match if the base FQNs agree (the call site will
    // have a concrete instantiation like `Box<Int32>` and the block has `Box<T>`).
    ext_blocks.iter().find(|block| {
        if block.ext_fqn != *ext_fqn {
            return false;
        }
        if block.type_params.is_empty() {
            block.for_type == *for_type
        } else {
            block.for_type.to_fqn() == for_type.to_fqn()
        }
    })
}

fn resolve_ext_method_mangled(
    block: &TypedExtensionBlock,
    method_name: &SymbolName,
    for_type: &Type,
    type_args: &[Type],
    arg_types: &[Type],
    new_functions: &mut BTreeMap<MangledName, TypedFunction>,
    existing_functions: &BTreeSet<MangledName>,
) -> MangledName {
    // Find the matching method by name and argument types (for overload disambiguation)
    let method = block
        .methods
        .iter()
        .chain(block.properties.iter())
        .find(|m| {
            m.name == *method_name
                && m.params.len() == arg_types.len()
                && m.params.iter().zip(arg_types.iter()).all(|(p, a)| {
                    p.ty == *a || a.contains_type_parameter() || p.ty.contains_type_parameter()
                })
        })
        .or_else(|| {
            // Fallback: match by name only (for cases where type params prevent exact match)
            block
                .methods
                .iter()
                .chain(block.properties.iter())
                .find(|m| m.name == *method_name)
        });

    let has_block_type_params = !block.type_params.is_empty();
    let has_method_type_params = method
        .as_ref()
        .is_some_and(|m| !m.method_type_params.is_empty());

    if !has_block_type_params && !has_method_type_params {
        let param_types: Vec<&Type> = method
            .map(|m| m.params.iter().map(|p| &p.ty).collect())
            .unwrap_or_default();
        let mangled = MangledName::for_named_extension_method(
            &block.ext_fqn.package,
            &block.ext_fqn.symbol,
            method_name,
            for_type,
            &param_types,
        );
        if method.is_none() && !type_args.is_empty() {
            return mangled.with_type_args(type_args);
        }
        return mangled;
    }

    let mut bindings: BTreeMap<TypeParamName, Type> = BTreeMap::new();
    let mut all_mangled_type_args: Vec<Type> = Vec::new();

    if has_block_type_params {
        unify_type(&block.for_type, for_type, &mut bindings);
        for (param, argument) in block.type_params.iter().zip(type_args) {
            bindings.insert(param.clone(), argument.clone());
        }
        let block_type_args: Vec<Type> = block
            .type_params
            .iter()
            .map(|tp| bindings.get(tp).cloned().unwrap_or(Type::Error))
            .collect();
        all_mangled_type_args.extend(block_type_args);
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

    // Compute concrete param types after substitution
    let concrete_param_types: Vec<Type> = method
        .as_ref()
        .map(|m| {
            m.params
                .iter()
                .map(|p| apply_type_substitution(&p.ty, &bindings))
                .collect()
        })
        .unwrap_or_default();
    let concrete_param_refs: Vec<&Type> = concrete_param_types.iter().collect();

    let mut mangled = MangledName::for_named_extension_method(
        &block.ext_fqn.package,
        &block.ext_fqn.symbol,
        method_name,
        for_type,
        &concrete_param_refs,
    );
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
            make_display_name(&format!("{}.{}", block.ext_fqn, method_name), &params);
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

    mangled
}

fn resolve_ext_calls_in_expr(
    expr: TypedExpr,
    ext_blocks: &[TypedExtensionBlock],
    new_functions: &mut BTreeMap<MangledName, TypedFunction>,
    existing_functions: &BTreeSet<MangledName>,
) -> TypedExpr {
    let span = expr.span;
    let ty = expr.ty;
    let kind = match expr.kind {
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
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect();
            // Skip resolution if type_params or for_type contain TypeParameter types —
            // this is a template body that will be substituted later.
            if for_type.contains_type_parameter()
                || type_params.iter().any(|t| t.contains_type_parameter())
            {
                TypedExprKind::ExtFunctionCall {
                    ext_fqn,
                    for_type,
                    method_name,
                    args,
                    type_params,
                }
            } else if let Some(block) = find_ext_block_for_call(ext_blocks, &ext_fqn, &for_type) {
                let arg_types: Vec<Type> = args.iter().map(|a| a.ty.clone()).collect();
                let mangled = resolve_ext_method_mangled(
                    block,
                    &method_name,
                    &for_type,
                    &type_params,
                    &arg_types,
                    new_functions,
                    existing_functions,
                );
                TypedExprKind::FunctionCall {
                    name: mangled,
                    args,
                    type_params: vec![],
                }
            } else {
                TypedExprKind::ExtFunctionCall {
                    ext_fqn,
                    for_type,
                    method_name,
                    args,
                    type_params,
                }
            }
        }
        TypedExprKind::ExtFunctionRef {
            ext_fqn,
            for_type,
            method_name,
            type_params,
        } => {
            if for_type.contains_type_parameter()
                || type_params.iter().any(|t| t.contains_type_parameter())
            {
                TypedExprKind::ExtFunctionRef {
                    ext_fqn,
                    for_type,
                    method_name,
                    type_params,
                }
            } else if let Some(block) = find_ext_block_for_call(ext_blocks, &ext_fqn, &for_type) {
                let mangled = resolve_ext_method_mangled(
                    block,
                    &method_name,
                    &for_type,
                    &type_params,
                    &[], // No arg types for function refs
                    new_functions,
                    existing_functions,
                );
                TypedExprKind::FunctionRef {
                    name: mangled,
                    type_params: vec![],
                }
            } else {
                TypedExprKind::ExtFunctionRef {
                    ext_fqn,
                    for_type,
                    method_name,
                    type_params,
                }
            }
        }
        TypedExprKind::Block(exprs) => TypedExprKind::Block(
            exprs
                .into_iter()
                .map(|e| {
                    resolve_ext_calls_in_expr(e, ext_blocks, new_functions, existing_functions)
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
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect(),
            type_params,
        },
        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect(),
        },
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => TypedExprKind::If {
            condition: Box::new(resolve_ext_calls_in_expr(
                *condition,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            then_branch: Box::new(resolve_ext_calls_in_expr(
                *then_branch,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            else_branch: else_branch.map(|e| {
                Box::new(resolve_ext_calls_in_expr(
                    *e,
                    ext_blocks,
                    new_functions,
                    existing_functions,
                ))
            }),
        },
        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(resolve_ext_calls_in_expr(
                *condition,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            body: Box::new(resolve_ext_calls_in_expr(
                *body,
                ext_blocks,
                new_functions,
                existing_functions,
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
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::LetDestructure {
            pattern,
            value,
            var_ty,
        } => TypedExprKind::LetDestructure {
            pattern,
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
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
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::FieldAssign {
            object,
            field_name,
            field_index,
            value,
            boxed,
        } => TypedExprKind::FieldAssign {
            object: Box::new(resolve_ext_calls_in_expr(
                *object,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            field_name,
            field_index,
            boxed,
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(resolve_ext_calls_in_expr(
                *left,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            right: Box::new(resolve_ext_calls_in_expr(
                *right,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(resolve_ext_calls_in_expr(
                *operand,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(resolve_ext_calls_in_expr(
                *subject,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            arms: arms
                .into_iter()
                .map(|arm| crate::typechecker::types::TypedMatchArm {
                    pattern: arm.pattern,
                    guard: arm.guard.map(|g| {
                        Box::new(resolve_ext_calls_in_expr(
                            *g,
                            ext_blocks,
                            new_functions,
                            existing_functions,
                        ))
                    }),
                    body: Box::new(resolve_ext_calls_in_expr(
                        *arm.body,
                        ext_blocks,
                        new_functions,
                        existing_functions,
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
                        resolve_ext_calls_in_expr(e, ext_blocks, new_functions, existing_functions),
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements
                .into_iter()
                .map(|e| {
                    resolve_ext_calls_in_expr(e, ext_blocks, new_functions, existing_functions)
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
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
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
            object: Box::new(resolve_ext_calls_in_expr(
                *object,
                ext_blocks,
                new_functions,
                existing_functions,
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
            body: Box::new(resolve_ext_calls_in_expr(
                *body,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            captures,
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: Box::new(resolve_ext_calls_in_expr(
                *callee,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            args: args
                .into_iter()
                .map(|a| {
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect(),
        },
        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(resolve_ext_calls_in_expr(
                *message,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(resolve_ext_calls_in_expr(
                *condition,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            message: message.map(|m| {
                Box::new(resolve_ext_calls_in_expr(
                    *m,
                    ext_blocks,
                    new_functions,
                    existing_functions,
                ))
            }),
        },
        TypedExprKind::RecordWith {
            object,
            fqn,
            overrides,
            type_params,
        } => TypedExprKind::RecordWith {
            object: Box::new(resolve_ext_calls_in_expr(
                *object,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            fqn,
            overrides: overrides
                .into_iter()
                .map(|(n, idx, e)| {
                    (
                        n,
                        idx,
                        resolve_ext_calls_in_expr(e, ext_blocks, new_functions, existing_functions),
                    )
                })
                .collect(),
            type_params,
        },
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            target_type,
        },
        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            target_type,
        },
        TypedExprKind::InterfaceObjectCoerce {
            inner,
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        } => TypedExprKind::InterfaceObjectCoerce {
            inner: Box::new(resolve_ext_calls_in_expr(
                *inner,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        },
        TypedExprKind::TemplateInterfaceObjectCoerce {
            inner,
            traits,
            concrete_type,
        } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: Box::new(resolve_ext_calls_in_expr(
                *inner,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            traits,
            concrete_type,
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: Box::new(resolve_ext_calls_in_expr(
                *inner,
                ext_blocks,
                new_functions,
                existing_functions,
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
            receiver: Box::new(resolve_ext_calls_in_expr(
                *receiver,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            args: args
                .into_iter()
                .map(|a| {
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect(),
        },
        TypedExprKind::MethodRef {
            object,
            method_name,
            type_params,
        } => TypedExprKind::MethodRef {
            object: Box::new(resolve_ext_calls_in_expr(
                *object,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            method_name,
            type_params,
        },
        TypedExprKind::ClassVirtualCall {
            object,
            vtable_slot,
            args,
        } => TypedExprKind::ClassVirtualCall {
            object: Box::new(resolve_ext_calls_in_expr(
                *object,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            vtable_slot,
            args: args
                .into_iter()
                .map(|a| {
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
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
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect(),
        },
        TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields,
        } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields
                .into_iter()
                .map(|a| {
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
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
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
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
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect(),
            type_params,
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
            return_type,
        },
        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements
                .into_iter()
                .map(|e| {
                    resolve_ext_calls_in_expr(e, ext_blocks, new_functions, existing_functions)
                })
                .collect(),
        },
        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: Box::new(resolve_ext_calls_in_expr(
                *inner,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
        TypedExprKind::GlobalAssign {
            name,
            type_params,
            value,
        } => TypedExprKind::GlobalAssign {
            name,
            type_params,
            value: Box::new(resolve_ext_calls_in_expr(
                *value,
                ext_blocks,
                new_functions,
                existing_functions,
            )),
        },
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
                    resolve_ext_calls_in_expr(a, ext_blocks, new_functions, existing_functions)
                })
                .collect();
            TypedExprKind::ImplFunctionCall {
                trait_fqn,
                trait_type_params,
                for_type,
                method_name,
                args,
                method_type_params,
            }
        }
        TypedExprKind::ImplFunctionRef {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            method_type_params,
        } => TypedExprKind::ImplFunctionRef {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            method_type_params,
        },
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
