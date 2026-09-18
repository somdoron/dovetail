use crate::typechecker::types::VtableMethodGroup;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::common::span::Span;
use crate::common::types::{
    Fqn, InterfaceMemberName, MangledName, SymbolName, TypeParamName, VarName,
};

use crate::typechecker::registry::Registry;
use crate::typechecker::types::{
    InterfaceObjectTypeDef, Type, TypeDef, TypedExpr, TypedExprKind, TypedMatchArm, TypedModule,
    TypedPattern,
};

/// Walk the entire typed module and elaborate the coercion AST nodes that the typechecker
/// proved are required but didn't emit inline. Two kinds:
///
/// 1. **`InterfaceObjectCoerce` / `TemplateInterfaceObjectCoerce`** — at each assignment point where
///    a concrete value flows into a `Type::InterfaceObject` slot, wrap the value in a coercion
///    node so codegen can build the interface object struct (data + vtable).
/// 2. **`BoxToAny`** — at each assignment point where a primitive or flattened value
///    flows into `Type::Any`, wrap in `BoxToAny` so codegen supplies its boxed form.
///
/// Generic data-type variance and closure variance both fall out of full type erasure (one
/// WASM struct per generic def, one `Closure_N` struct per arity) and do not need
/// elaboration here. This pass also registers `InterfaceObjectTypeDef` entries into
/// `module.types` for every distinct (trait, type_args) coerced.
pub fn elaborate_coercions(module: &mut TypedModule, registry: &Registry) {
    SYNTHETIC_DIRECT.with(|cell| *cell.borrow_mut() = (Vec::new(), Default::default()));
    DIRECT_REBOX_AUTH.with(|cell| *cell.borrow_mut() = (Vec::new(), Default::default()));
    // Clone the type defs for lookup during rewriting (we need shared immutable
    // access while mutating the function bodies).
    let types = module.types.clone();

    // Collect function param types for FunctionCall coercion.
    let function_params: BTreeMap<MangledName, Vec<Type>> = module
        .functions
        .iter()
        .chain(module.function_templates.iter())
        .map(|(mn, f)| (mn.clone(), f.params.iter().map(|p| p.ty.clone()).collect()))
        .collect();

    // Accumulate InterfaceObjectTypeDef entries created during coercion.
    let interface_object_types: RefCell<BTreeMap<MangledName, TypeDef>> =
        RefCell::new(BTreeMap::new());

    let globals_map: BTreeMap<MangledName, Type> = module
        .globals
        .iter()
        .map(|(mn, g)| (mn.clone(), g.ty.clone()))
        .collect();

    // Pre-pass: register interface-object TypeDefs for every type reachable
    // from a SIGNATURE (function params/returns, globals, typedef fields).
    // Codegen lowers these types whether or not any coercion in this module
    // ever constructs them — e.g. a library function taking `A and B` whose
    // callers live in another package, compiled standalone by `dovetail test`.
    {
        for func in module.functions.values() {
            for p in &func.params {
                register_interface_types_in(&p.ty, registry, &interface_object_types);
            }
            register_interface_types_in(&func.return_type, registry, &interface_object_types);
            if let Some(t) = &func.vtable_self_type {
                register_interface_types_in(t, registry, &interface_object_types);
            }
        }
        for global in module.globals.values() {
            register_interface_types_in(&global.ty, registry, &interface_object_types);
        }
        for td in module.types.values() {
            match td {
                TypeDef::Record(rec) => {
                    for (_, ty) in &rec.fields {
                        register_interface_types_in(ty, registry, &interface_object_types);
                    }
                }
                TypeDef::Class(cls) => {
                    for f in &cls.fields {
                        register_interface_types_in(&f.ty, registry, &interface_object_types);
                    }
                    for p in &cls.constructor_params {
                        register_interface_types_in(&p.ty, registry, &interface_object_types);
                    }
                }
                TypeDef::Enum(e) => {
                    for variant in &e.variants {
                        for ty in &variant.payload_types {
                            register_interface_types_in(ty, registry, &interface_object_types);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    {
        let ctx = WalkContext {
            types: &types,
            function_params: &function_params,
            globals: &globals_map,
            registry,
            interface_object_types: &interface_object_types,
        };

        // Walk function bodies and apply return-type coercion.
        for func in module
            .functions
            .values_mut()
            .chain(module.function_templates.values_mut())
        {
            func.body = walk_expr(std::mem::replace(&mut func.body, placeholder_expr()), &ctx);
            apply_return_coercion(
                &mut func.body,
                &func.return_type,
                registry,
                &interface_object_types,
            );
        }

        // Walk global initializers and apply declared-type coercion.
        for global in module.globals.values_mut() {
            global.initializer = walk_expr(
                std::mem::replace(&mut global.initializer, placeholder_expr()),
                &ctx,
            );
            apply_return_coercion(
                &mut global.initializer,
                &global.ty,
                registry,
                &interface_object_types,
            );
        }

        for ty in module.types.values_mut() {
            if let TypeDef::Class(class) = ty {
                for initializer in &mut class.initializer {
                    *initializer =
                        walk_expr(std::mem::replace(initializer, placeholder_expr()), &ctx);
                }
                if let Some(args) = &mut class.extends_args {
                    let parent_params = class
                        .parent_mangled_name
                        .as_ref()
                        .and_then(|name| ctx.types.get(name))
                        .and_then(|ty| match ty {
                            TypeDef::Class(parent) => Some(&parent.constructor_params),
                            _ => None,
                        });
                    for (index, arg) in args.iter_mut().enumerate() {
                        *arg = walk_expr(std::mem::replace(arg, placeholder_expr()), &ctx);
                        if let Some(param) = parent_params.and_then(|params| params.get(index)) {
                            apply_return_coercion(
                                arg,
                                &param.ty,
                                registry,
                                &interface_object_types,
                            );
                        }
                    }
                }
            }
        }

        // Walk test bodies.
        for test in module.tests.iter_mut() {
            test.body = walk_expr(std::mem::replace(&mut test.body, placeholder_expr()), &ctx);
        }
        for block in &mut module.implement_blocks {
            for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
                method.body = walk_expr(
                    std::mem::replace(&mut method.body, placeholder_expr()),
                    &ctx,
                );
            }
        }
        for block in &mut module.extension_blocks {
            for method in block.methods.iter_mut().chain(block.properties.iter_mut()) {
                method.body = walk_expr(
                    std::mem::replace(&mut method.body, placeholder_expr()),
                    &ctx,
                );
            }
        }
    } // drop ctx so we can consume interface_object_types

    // Merge accumulated interface object type defs into the module's types.
    module.types.extend(interface_object_types.into_inner());
    module
        .synthetic_interface_coercions
        .extend(SYNTHETIC_DIRECT.with(|cell| std::mem::take(&mut cell.borrow_mut().0)));
    module
        .direct_rebox_authorizations
        .extend(DIRECT_REBOX_AUTH.with(|cell| std::mem::take(&mut cell.borrow_mut().0)));
}

/// Apply the interface-object-coercion or Any-boxing rule to a body expression's value type vs
/// its expected (return / declared) type.
fn apply_return_coercion(
    body: &mut TypedExpr,
    expected: &Type,
    registry: &Registry,
    interface_object_types: &RefCell<BTreeMap<MangledName, TypeDef>>,
) {
    if needs_param_coercion(&body.ty, expected) {
        *body = coerce_arg_to_param(
            std::mem::replace(body, placeholder_expr()),
            expected,
            registry,
            interface_object_types,
        );
    } else if needs_any_boxing(&body.ty, expected) {
        *body = box_to_any(std::mem::replace(body, placeholder_expr()));
    }
}

/// Context needed during the walk to look up expected types.
struct WalkContext<'a> {
    types: &'a BTreeMap<MangledName, TypeDef>,
    function_params: &'a BTreeMap<MangledName, Vec<Type>>,
    globals: &'a BTreeMap<MangledName, Type>,
    registry: &'a Registry,
    interface_object_types: &'a RefCell<BTreeMap<MangledName, TypeDef>>,
}

/// Bottom-up rewrite: recurse into children first, then check coercion at this level.
fn walk_expr(expr: TypedExpr, ctx: &WalkContext) -> TypedExpr {
    let expr = crate::typechecker::tuple_extension::lower(expr);
    let span = expr.span;
    let ty = expr.ty;

    // Every interface-object type that reaches codegen (locals, intermediate
    // values, closure types carrying interface params) needs its TypeDef,
    // whether or not a coercion constructs it in this module.
    if ty.contains_interface_object() {
        register_interface_types_in(&ty, ctx.registry, ctx.interface_object_types);
    }

    let kind = match expr.kind {
        // === Coercion points ===
        TypedExprKind::Let {
            name,
            mutable,
            boxed,
            var_ty,
            value,
        } => {
            let mut value = walk_expr(*value, ctx);
            if needs_param_coercion(&value.ty, &var_ty) {
                value =
                    coerce_arg_to_param(value, &var_ty, ctx.registry, ctx.interface_object_types);
            } else if needs_any_boxing(&value.ty, &var_ty) {
                value = box_to_any(value);
            }
            TypedExprKind::Let {
                name,
                mutable,
                boxed,
                var_ty,
                value: Box::new(value),
            }
        }

        TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value,
        } => {
            let mut value = walk_expr(*value, ctx);
            if needs_param_coercion(&value.ty, &target_ty) {
                value = coerce_arg_to_param(
                    value,
                    &target_ty,
                    ctx.registry,
                    ctx.interface_object_types,
                );
            } else if needs_any_boxing(&value.ty, &target_ty) {
                value = box_to_any(value);
            }
            TypedExprKind::Assign {
                name,
                target_ty,
                boxed,
                value: Box::new(value),
            }
        }

        TypedExprKind::GlobalAssign {
            name,
            type_params,
            value,
        } => {
            let mut value = walk_expr(*value, ctx);
            if let Some(global_ty) = ctx.globals.get(&name) {
                if needs_param_coercion(&value.ty, global_ty) {
                    value = coerce_arg_to_param(
                        value,
                        global_ty,
                        ctx.registry,
                        ctx.interface_object_types,
                    );
                } else if needs_any_boxing(&value.ty, global_ty) {
                    value = box_to_any(value);
                }
            }
            TypedExprKind::GlobalAssign {
                name,
                type_params,
                value: Box::new(value),
            }
        }

        TypedExprKind::FunctionCall {
            name,
            args,
            type_params,
        } => {
            let args: Vec<TypedExpr> = args.into_iter().map(|a| walk_expr(a, ctx)).collect();
            let args = if let Some(param_types) = ctx.function_params.get(&name) {
                coerce_args(args, param_types, ctx.registry, ctx.interface_object_types)
            } else {
                args
            };
            TypedExprKind::FunctionCall {
                name,
                args,
                type_params,
            }
        }

        TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => {
            let args: Vec<TypedExpr> = args.into_iter().map(|a| walk_expr(a, ctx)).collect();
            // Variant payload types come from the canonical enum TypeDef and persist
            // TypeVariables through monomorphize, so use the persistent-slot coercion.
            // Fall back to the registry for enums defined in another package
            // (e.g. prelude `Option`/`Result`), and substitute the enum's type
            // args so interface-typed payload positions coerce their values.
            let payload_types =
                lookup_enum_variant_payload(&ty, &variant_name, ctx.types).or_else(|| {
                    lookup_enum_variant_payload_from_registry(&fqn, &variant_name, ctx.registry)
                });
            let args = if let Some(payload_types) = payload_types {
                let payload_types =
                    substitute_enum_payload_types(&payload_types, &ty, ctx.registry);
                coerce_args(
                    args,
                    &payload_types,
                    ctx.registry,
                    ctx.interface_object_types,
                )
            } else {
                args
            };
            TypedExprKind::EnumCreate {
                fqn,
                variant_name,
                args,
                type_params,
            }
        }

        TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => {
            let args: Vec<TypedExpr> = args.into_iter().map(|a| walk_expr(a, ctx)).collect();
            // Same treatment as EnumCreate: registry fallback for cross-package
            // enums + type-arg substitution so interface-typed payload
            // positions reify their coercions.
            let payload_types =
                lookup_enum_variant_payload(&ty, &variant_name, ctx.types).or_else(|| {
                    lookup_enum_variant_payload_from_registry(&fqn, &variant_name, ctx.registry)
                });
            let args = if let Some(payload_types) = payload_types {
                let payload_types =
                    substitute_enum_payload_types(&payload_types, &ty, ctx.registry);
                coerce_args(
                    args,
                    &payload_types,
                    ctx.registry,
                    ctx.interface_object_types,
                )
            } else {
                args
            };
            TypedExprKind::EnumVariantRecordCreate {
                fqn,
                variant_name,
                args,
                type_params,
            }
        }

        TypedExprKind::RecordCreate {
            fqn,
            fields,
            type_params,
        } => {
            let fields: Vec<(String, TypedExpr)> = fields
                .into_iter()
                .map(|(name, expr)| (name, walk_expr(expr, ctx)))
                .collect();
            let fields = if let Some(field_types) = lookup_record_field_types(&ty, ctx.types) {
                fields
                    .into_iter()
                    .map(|(name, mut value)| {
                        if let Some(expected) = field_types.get(&name) {
                            if needs_param_coercion(&value.ty, expected) {
                                value = coerce_arg_to_param(
                                    value,
                                    expected,
                                    ctx.registry,
                                    ctx.interface_object_types,
                                );
                            } else if needs_any_boxing(&value.ty, expected) {
                                value = box_to_any(value);
                            }
                        }
                        (name, value)
                    })
                    .collect()
            } else {
                fields
            };
            TypedExprKind::RecordCreate {
                fqn,
                fields,
                type_params,
            }
        }

        TypedExprKind::TupleLiteral { elements } => {
            let elem_types: Vec<Type> = match &ty {
                Type::Tuple(elems, _) => elems.clone(),
                _ => Vec::new(),
            };
            let elements = elements
                .into_iter()
                .enumerate()
                .map(|(i, value)| {
                    let mut value = walk_expr(value, ctx);
                    if let Some(expected) = elem_types.get(i) {
                        if needs_param_coercion(&value.ty, expected) {
                            value = coerce_arg_to_param(
                                value,
                                expected,
                                ctx.registry,
                                ctx.interface_object_types,
                            );
                        } else if needs_any_boxing(&value.ty, expected) {
                            value = box_to_any(value);
                        }
                    }
                    value
                })
                .collect();
            TypedExprKind::TupleLiteral { elements }
        }

        TypedExprKind::RecordWith {
            object,
            fqn,
            overrides,
            type_params,
        } => {
            let object = Box::new(walk_expr(*object, ctx));
            let overrides: Vec<(String, u32, TypedExpr)> = overrides
                .into_iter()
                .map(|(name, idx, expr)| (name, idx, walk_expr(expr, ctx)))
                .collect();
            let overrides =
                if let Some(field_types) = lookup_record_field_types(&object.ty, ctx.types) {
                    overrides
                        .into_iter()
                        .map(|(name, idx, mut value)| {
                            if let Some(expected) = field_types.get(&name) {
                                if needs_param_coercion(&value.ty, expected) {
                                    value = coerce_arg_to_param(
                                        value,
                                        expected,
                                        ctx.registry,
                                        ctx.interface_object_types,
                                    );
                                } else if needs_any_boxing(&value.ty, expected) {
                                    value = box_to_any(value);
                                }
                            }
                            (name, idx, value)
                        })
                        .collect()
                } else {
                    overrides
                };
            TypedExprKind::RecordWith {
                object,
                fqn,
                overrides,
                type_params,
            }
        }

        // === Recursive-only (no coercion at this level) ===
        TypedExprKind::Block(exprs) => {
            let mut exprs: Vec<_> = exprs.into_iter().map(|e| walk_expr(e, ctx)).collect();
            if let Some(mut last) = exprs.pop() {
                if needs_param_coercion(&last.ty, &ty) {
                    last = coerce_arg_to_param(last, &ty, ctx.registry, ctx.interface_object_types);
                } else if needs_any_boxing(&last.ty, &ty) {
                    last = box_to_any(last);
                }
                exprs.push(last);
            }
            TypedExprKind::Block(exprs)
        }

        TypedExprKind::NamedCall {
            call,
            argument_order,
            parameter_names,
            parameter_types,
            named_parameters,
            source_name,
        } => TypedExprKind::NamedCall {
            call: Box::new(walk_expr(*call, ctx)),
            argument_order,
            parameter_names,
            parameter_types,
            named_parameters,
            source_name,
        },
        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: Box::new(walk_expr(*message, ctx)),
        },

        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: Box::new(walk_expr(*condition, ctx)),
            message: message.map(|m| Box::new(walk_expr(*m, ctx))),
        },

        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: Box::new(walk_expr(*left, ctx)),
            right: Box::new(walk_expr(*right, ctx)),
        },

        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: Box::new(walk_expr(*operand, ctx)),
        },

        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => {
            // Branch unification may pick a subset/interface type for the whole
            // `if` while a branch carries a superset intersection (or a concrete
            // type) — reify the conversion per branch.
            let then_branch = coerce_branch(walk_expr(*then_branch, ctx), &ty, ctx);
            let else_branch =
                else_branch.map(|e| Box::new(coerce_branch(walk_expr(*e, ctx), &ty, ctx)));
            TypedExprKind::If {
                condition: Box::new(walk_expr(*condition, ctx)),
                then_branch: Box::new(then_branch),
                else_branch,
            }
        }

        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: Box::new(walk_expr(*condition, ctx)),
            body: Box::new(walk_expr(*body, ctx)),
        },

        TypedExprKind::Match { subject, arms } => TypedExprKind::Match {
            subject: Box::new(walk_expr(*subject, ctx)),
            arms: arms
                .into_iter()
                .map(|arm| TypedMatchArm {
                    body: Box::new(coerce_branch(walk_expr(*arm.body, ctx), &ty, ctx)),
                    ..arm
                })
                .collect(),
        },

        TypedExprKind::FieldAccess {
            object,
            field_name,
            field_index,
            boxed,
        } => TypedExprKind::FieldAccess {
            object: Box::new(walk_expr(*object, ctx)),
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
        } => {
            let object = walk_expr(*object, ctx);
            let mut value = walk_expr(*value, ctx);
            if let Some(expected_ty) = lookup_field_type(&object.ty, &field_name, ctx.types) {
                if needs_param_coercion(&value.ty, &expected_ty) {
                    value = coerce_arg_to_param(
                        value,
                        &expected_ty,
                        ctx.registry,
                        ctx.interface_object_types,
                    );
                } else if needs_any_boxing(&value.ty, &expected_ty) {
                    value = box_to_any(value);
                }
            }
            TypedExprKind::FieldAssign {
                object: Box::new(object),
                field_name,
                field_index,
                value: Box::new(value),
                boxed,
            }
        }

        TypedExprKind::IntrinsicCall { intrinsic, args } => {
            let mut args: Vec<TypedExpr> = args.into_iter().map(|a| walk_expr(a, ctx)).collect();
            // Array-storing intrinsics: coerce the stored value toward the
            // array's element type (interface-typed elements need reification).
            use crate::typechecker::types::IntrinsicKind;
            let value_slot = match intrinsic {
                IntrinsicKind::ArraySet => Some(2usize),
                IntrinsicKind::ArrayFill => Some(1),
                IntrinsicKind::ArrayExtend => Some(1),
                _ => None,
            };
            if let Some(i) = value_slot {
                // ArrayFill's element type comes from the RESULT; the others
                // from the receiver.
                let elem = match (&intrinsic, args.first().map(|a| &a.ty), &ty) {
                    (IntrinsicKind::ArrayFill, _, Type::Array(elem)) => Some(elem.as_ref().clone()),
                    (_, Some(Type::Array(elem)), _) => Some(elem.as_ref().clone()),
                    _ => None,
                };
                if let (Some(elem), true) = (elem.as_ref(), i < args.len())
                    && needs_param_coercion(&args[i].ty, elem)
                {
                    let a = args.remove(i);
                    let a = coerce_arg_to_param(a, elem, ctx.registry, ctx.interface_object_types);
                    args.insert(i, a);
                }
            }
            TypedExprKind::IntrinsicCall { intrinsic, args }
        }

        TypedExprKind::ArrayLiteral { elements } => {
            // Coerce elements toward the literal's element type (interface-
            // typed elements need reification like any other sink).
            let elem_ty = match &ty {
                Type::Array(elem) => Some(elem.as_ref().clone()),
                _ => None,
            };
            let elements: Vec<TypedExpr> = elements
                .into_iter()
                .map(|e| {
                    let e = walk_expr(e, ctx);
                    match &elem_ty {
                        Some(elem) if needs_param_coercion(&e.ty, elem) => {
                            coerce_arg_to_param(e, elem, ctx.registry, ctx.interface_object_types)
                        }
                        _ => e,
                    }
                })
                .collect();
            TypedExprKind::ArrayLiteral { elements }
        }

        TypedExprKind::MethodRef {
            object,
            method_name,
            type_params,
        } => TypedExprKind::MethodRef {
            object: Box::new(walk_expr(*object, ctx)),
            method_name,
            type_params,
        },

        // === Leaf nodes (no children to walk) ===
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

        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: Box::new(walk_expr(*value, ctx)),
            target_type,
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: Box::new(walk_expr(*value, ctx)),
            target_type,
        },
        TypedExprKind::LetDestructure {
            pattern,
            var_ty,
            value,
        } => {
            let mut value = walk_expr(*value, ctx);
            if needs_param_coercion(&value.ty, &var_ty) {
                value =
                    coerce_arg_to_param(value, &var_ty, ctx.registry, ctx.interface_object_types);
            } else if needs_any_boxing(&value.ty, &var_ty) {
                value = box_to_any(value);
            }
            TypedExprKind::LetDestructure {
                pattern,
                var_ty,
                value: Box::new(value),
            }
        }
        TypedExprKind::NewtypeCreate { value } => {
            let mut value = walk_expr(*value, ctx);
            // Coerce toward the newtype's declared inner type (an interface-
            // typed inner needs the conversion reified like any other sink).
            if let Type::Newtype(_, inner) = &ty
                && needs_param_coercion(&value.ty, inner)
            {
                value = coerce_arg_to_param(value, inner, ctx.registry, ctx.interface_object_types);
            }
            TypedExprKind::NewtypeCreate {
                value: Box::new(value),
            }
        }
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: Box::new(walk_expr(*value, ctx)),
        },
        TypedExprKind::InterfaceObjectCoerce {
            inner,
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        } => {
            // Register TypeDefs for inherited coercions (e.g. from monomorphized generics)
            if let Type::InterfaceObject {
                traits,
                mangled_name,
            } = &ty
            {
                register_interface_object_set(
                    traits,
                    mangled_name,
                    ctx.registry,
                    ctx.interface_object_types,
                );
                synthesize_direct_super_coercions(
                    ctx.registry,
                    &concrete_type,
                    traits,
                    &vtable_methods,
                );
            }
            TypedExprKind::InterfaceObjectCoerce {
                inner: Box::new(walk_expr(*inner, ctx)),
                interface_mangled_name,
                concrete_type,
                vtable_methods,
            }
        }
        TypedExprKind::InterfaceObjectUpcast { inner } => {
            // Register TypeDefs for inherited upcasts; the source set's defs are
            // registered when the upcast is created, but a monomorphized copy may
            // land here first.
            if let Type::InterfaceObject {
                traits,
                mangled_name,
            } = &ty
            {
                register_interface_object_set(
                    traits,
                    mangled_name,
                    ctx.registry,
                    ctx.interface_object_types,
                );
            }
            let inner = walk_expr(*inner, ctx);
            if let Type::InterfaceObject {
                traits,
                mangled_name,
            } = &inner.ty
            {
                register_interface_object_set(
                    traits,
                    mangled_name,
                    ctx.registry,
                    ctx.interface_object_types,
                );
            }
            TypedExprKind::InterfaceObjectUpcast {
                inner: Box::new(inner),
            }
        }
        TypedExprKind::TemplateInterfaceObjectCoerce {
            inner,
            traits,
            concrete_type,
        } => {
            let walked_inner = walk_expr(*inner, ctx);
            // A substituted source that is itself an interface object needs an
            // upcast, not a vtable-building coercion (and has no single FQN).
            if matches!(&concrete_type, Type::InterfaceObject { .. }) {
                let target = Type::interface_intersection(traits);
                if let Type::InterfaceObject {
                    traits: target_traits,
                    mangled_name: set_mn,
                } = &target
                {
                    register_interface_object_set(
                        target_traits,
                        set_mn,
                        ctx.registry,
                        ctx.interface_object_types,
                    );
                }
                if let Type::InterfaceObject {
                    traits: src_traits,
                    mangled_name: src_mn,
                } = &concrete_type
                {
                    register_interface_object_set(
                        src_traits,
                        src_mn,
                        ctx.registry,
                        ctx.interface_object_types,
                    );
                }
                return TypedExpr {
                    kind: TypedExprKind::InterfaceObjectUpcast {
                        inner: Box::new(walked_inner),
                    },
                    ty,
                    span,
                };
            }
            // After monomorphize substitution, concrete_type may be fully resolved.
            // Resolve to a real InterfaceObjectCoerce with computed vtable methods.
            if !concrete_type.contains_type_parameter() {
                let type_fqn = concrete_type.to_fqn();
                // Rebuild the target set to get its components (sorted) and set key.
                let target = Type::interface_intersection(traits);
                let (target_traits, set_mn) = match &target {
                    Type::InterfaceObject {
                        traits,
                        mangled_name,
                    } => (traits.clone(), mangled_name.clone()),
                    _ => unreachable!("interface_intersection returned non-InterfaceObject"),
                };
                let vtable_methods = compute_grouped_vtable_methods(
                    ctx.registry,
                    &concrete_type,
                    &type_fqn,
                    &target_traits,
                );
                register_interface_object_set(
                    &target_traits,
                    &set_mn,
                    ctx.registry,
                    ctx.interface_object_types,
                );
                synthesize_direct_super_coercions(
                    ctx.registry,
                    &concrete_type,
                    &target_traits,
                    &vtable_methods,
                );
                TypedExprKind::InterfaceObjectCoerce {
                    inner: Box::new(walked_inner),
                    interface_mangled_name: set_mn,
                    concrete_type,
                    vtable_methods,
                }
            } else {
                TypedExprKind::TemplateInterfaceObjectCoerce {
                    inner: Box::new(walked_inner),
                    traits,
                    concrete_type,
                }
            }
        }
        TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name,
            member_name,
            receiver,
            args,
        } => {
            // Walk the receiver first — its walk registers the set + component
            // TypeDefs this call relies on.
            let receiver = Box::new(walk_expr(*receiver, ctx));
            // Coerce args against the declaring component's SUBSTITUTED param
            // types (raw trait signature + the component's type args, straight
            // from the registry — order-independent). An interface-object param
            // needs a concrete→interface coercion or an intersection→subset
            // upcast reified here: the emitter's value coercion cannot rebuild
            // fat pointers, and an erased slot's wrapper casts back to the
            // impl's concrete param type, so the caller must hand it the right
            // object.
            let target_params: Vec<Type> = substituted_member_params(
                &receiver.ty,
                &interface_mangled_name,
                &member_name,
                ctx.registry,
            );
            let args: Vec<TypedExpr> = args
                .into_iter()
                .enumerate()
                .map(|(i, a)| {
                    let a = walk_expr(a, ctx);
                    match target_params.get(i) {
                        Some(param_ty) => coerce_arg_to_param(
                            a,
                            param_ty,
                            ctx.registry,
                            ctx.interface_object_types,
                        ),
                        _ => a,
                    }
                })
                .collect();
            TypedExprKind::InterfaceObjectMethodCall {
                interface_mangled_name,
                method_name,
                member_name,
                receiver,
                args,
            }
        }
        TypedExprKind::ClassNew {
            mangled_name,
            args,
            type_params,
        } => {
            let args: Vec<TypedExpr> = args.into_iter().map(|a| walk_expr(a, ctx)).collect();
            // Coerce constructor args against the class's declared param types
            // (interface-object params need reified conversions).
            let ctor_params: Vec<Type> = match ctx.types.get(&mangled_name) {
                Some(TypeDef::Class(cls)) => cls
                    .constructor_params
                    .iter()
                    .map(|p| p.ty.clone())
                    .collect(),
                _ => Vec::new(),
            };
            let args = if !ctor_params.is_empty() {
                coerce_args(args, &ctor_params, ctx.registry, ctx.interface_object_types)
            } else {
                args
            };
            TypedExprKind::ClassNew {
                mangled_name,
                type_params,
                args,
            }
        }
        TypedExprKind::ClassStructCreate {
            target_mangled_name,
            fields,
            type_params,
        } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params,
            fields: fields.into_iter().map(|f| walk_expr(f, ctx)).collect(),
        },
        TypedExprKind::Return { value, return_type } => {
            let mut value = walk_expr(*value, ctx);
            if needs_param_coercion(&value.ty, &return_type) {
                value = coerce_arg_to_param(
                    value,
                    &return_type,
                    ctx.registry,
                    ctx.interface_object_types,
                );
            } else if needs_any_boxing(&value.ty, &return_type) {
                value = box_to_any(value);
            }
            TypedExprKind::Return {
                value: Box::new(value),
                return_type,
            }
        }
        TypedExprKind::ForLoop { .. } => {
            unreachable!("ForLoop nodes should be desugared before variance cast pass")
        }
        TypedExprKind::Try { .. } => {
            unreachable!("Try nodes should be desugared before variance cast pass")
        }
        TypedExprKind::Use { .. } => {
            unreachable!("Use nodes should be desugared before variance cast pass")
        }
        TypedExprKind::Await {
            operand,
            return_type,
            and_then_method,
            map_method,
            source_location_mn,
        } => TypedExprKind::Await {
            operand: Box::new(walk_expr(*operand, ctx)),
            return_type,
            and_then_method,
            map_method,
            source_location_mn,
        },
        TypedExprKind::AsyncBlock {
            body,
            succeed_method,
        } => TypedExprKind::AsyncBlock {
            body: Box::new(walk_expr(*body, ctx)),
            succeed_method,
        },
        TypedExprKind::ClassVirtualCall {
            object,
            vtable_slot,
            args,
        } => {
            let object = Box::new(walk_expr(*object, ctx));
            // Coerce args against the vtable slot's declared param types
            // (index 0 is self — left untouched).
            let slot_params: Vec<Type> = match &object.ty {
                Type::Class(_, mn)
                | Type::GenericClass {
                    mangled_name: mn, ..
                } => match ctx.types.get(mn) {
                    Some(TypeDef::Class(cls)) => cls
                        .vtable_methods
                        .get(vtable_slot as usize)
                        .map(|slot| slot.param_types.clone())
                        .unwrap_or_default(),
                    _ => Vec::new(),
                },
                _ => Vec::new(),
            };
            let args: Vec<TypedExpr> = args
                .into_iter()
                .enumerate()
                .map(|(i, a)| {
                    let a = walk_expr(a, ctx);
                    if i == 0 {
                        return a;
                    }
                    match slot_params.get(i) {
                        Some(param_ty) if needs_param_coercion(&a.ty, param_ty) => {
                            coerce_arg_to_param(
                                a,
                                param_ty,
                                ctx.registry,
                                ctx.interface_object_types,
                            )
                        }
                        _ => a,
                    }
                })
                .collect();
            TypedExprKind::ClassVirtualCall {
                object,
                vtable_slot,
                args,
            }
        }
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(|a| walk_expr(a, ctx)).collect(),
        },
        TypedExprKind::Closure {
            params,
            body,
            captures,
        } => {
            let mut body = walk_expr(*body, ctx);
            // Coerce the closure body to the closure's declared return type, just
            // like a named function body (see `apply_return_coercion` above). The
            // closure's type is `Function(params, ret)`; without this, a closure
            // whose body yields a class with a interface-object return type (or a
            // primitive with an `Any` return type) would keep its narrower type
            // and produce a WASM struct-type mismatch at the call boundary.
            if let Type::Function(_, ret) = &ty {
                apply_return_coercion(&mut body, ret, ctx.registry, ctx.interface_object_types);
            }
            TypedExprKind::Closure {
                params,
                body: Box::new(body),
                captures,
            }
        }
        TypedExprKind::ClosureCall { callee, args } => {
            let callee = Box::new(walk_expr(*callee, ctx));
            // Coerce args against the callee's declared param types — closure
            // prologues cast anyref params to the declared interface-object
            // struct, so an interface conversion must be reified at the call.
            let param_tys: Vec<Type> = match &callee.ty {
                Type::Function(params, _) => params.clone(),
                _ => Vec::new(),
            };
            let args: Vec<TypedExpr> = args
                .into_iter()
                .enumerate()
                .map(|(i, a)| {
                    let a = walk_expr(a, ctx);
                    match param_tys.get(i) {
                        Some(param_ty) => coerce_arg_to_param(
                            a,
                            param_ty,
                            ctx.registry,
                            ctx.interface_object_types,
                        ),
                        _ => a,
                    }
                })
                .collect();
            TypedExprKind::ClosureCall { callee, args }
        }
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
            args: args.into_iter().map(|a| walk_expr(a, ctx)).collect(),
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
            args: args.into_iter().map(|a| walk_expr(a, ctx)).collect(),
            type_params,
        },
        kind @ TypedExprKind::ExtFunctionRef { .. } => kind,
    };

    TypedExpr { kind, ty, span }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Values with a flattened representation need an explicit box at Any assignment
/// points, including tuples and transparent newtypes wrapping tuples. The boxed
/// expression has type Any, so downstream slot coercion does not box it again.
fn needs_any_boxing(actual: &Type, expected: &Type) -> bool {
    expected.is_any() && !actual.is_any() && (!actual.is_reference_type() || actual.is_never())
}

/// Wrap a scalar or flattened expression in BoxToAny.
fn box_to_any(expr: TypedExpr) -> TypedExpr {
    let span = expr.span.clone();
    TypedExpr {
        kind: TypedExprKind::BoxToAny {
            inner: Box::new(expr),
        },
        ty: Type::Any,
        span,
    }
}

/// Check if a concrete value needs interface object coercion.
fn needs_interface_object_coercion(actual: &Type, expected: &Type) -> bool {
    match (actual, expected) {
        (Type::Error | Type::Never, _) => false,
        // Interface object → interface object: whenever the component
        // fqn-sets differ (a subset upcast, or an extends-upcast reached via
        // the super closure). Identical sets need no coercion; unreachable
        // pairs were rejected by is_assignable before coercion runs.
        (
            Type::InterfaceObject {
                traits: actual_traits,
                ..
            },
            Type::InterfaceObject {
                traits: expected_traits,
                ..
            },
        ) => {
            let same_set = expected_traits.len() == actual_traits.len()
                && expected_traits
                    .iter()
                    .zip(actual_traits.iter())
                    .all(|(ce, ca)| ce.trait_fqn == ca.trait_fqn);
            !same_set
        }
        (_, Type::InterfaceObject { .. }) => true,
        _ => false,
    }
}

/// Coerce a branch/arm tail to the enclosing `if`/`match` node's type when it
/// needs an interface-object conversion (concrete → interface, or intersection
/// → subset upcast). Branch unification only checks assignability; the actual
/// conversion must be reified per branch or the branches carry different WASM
/// struct types.
fn coerce_branch(branch: TypedExpr, node_ty: &Type, ctx: &WalkContext) -> TypedExpr {
    if needs_param_coercion(&branch.ty, node_ty) {
        coerce_arg_to_param(branch, node_ty, ctx.registry, ctx.interface_object_types)
    } else {
        branch
    }
}

/// Wrap an expression in a InterfaceObjectCoerce node.
/// Computes vtable methods on-the-fly from the registry by looking up the trait
/// signature and computing mangled names for each concrete impl method.
/// Also registers a InterfaceObjectTypeDef (deduplicated by trait mangled name).
fn coerce_to_interface_object(
    expr: TypedExpr,
    target: &Type,
    registry: &Registry,
    interface_object_types: &RefCell<BTreeMap<MangledName, TypeDef>>,
) -> TypedExpr {
    if let Type::InterfaceObject {
        traits,
        mangled_name,
    } = target
    {
        // Register the target set's TypeDefs up front — *before* the template early-return.
        // Interface objects are de-monomorphized, so the WASM type and its erased vtable signatures are
        // instantiation-independent; registering here (even for a generic `as` in a template body,
        // e.g. `ArrayIterator<T> as Iterator<T>`) covers traits that are *only* coerced in generic
        // contexts — the work the now-deleted monomorphize walk used to do post-monomorphization.
        register_interface_object_set(traits, mangled_name, registry, interface_object_types);

        // Interface object → interface object: static upcast. The source set's
        // TypeDefs must also exist for codegen to extract from.
        if let Type::InterfaceObject {
            traits: source_traits,
            mangled_name: source_mn,
        } = &expr.ty
        {
            register_interface_object_set(
                source_traits,
                source_mn,
                registry,
                interface_object_types,
            );
            let span = expr.span.clone();
            return TypedExpr {
                kind: TypedExprKind::InterfaceObjectUpcast {
                    inner: Box::new(expr),
                },
                ty: target.clone(),
                span,
            };
        }

        let concrete_type = expr.ty.clone();

        // If the concrete type contains a TypeParameter, emit a template version
        // that monomorphize will resolve after substitution.
        if concrete_type.contains_type_parameter() {
            let span = expr.span.clone();
            return TypedExpr {
                kind: TypedExprKind::TemplateInterfaceObjectCoerce {
                    inner: Box::new(expr),
                    traits: traits
                        .iter()
                        .map(|c| (c.trait_fqn.clone(), c.trait_type_args.clone()))
                        .collect(),
                    concrete_type,
                },
                ty: target.clone(),
                span,
            };
        }

        let type_fqn = concrete_type.to_fqn();
        let vtable_methods =
            compute_grouped_vtable_methods(registry, &concrete_type, &type_fqn, traits);
        synthesize_direct_super_coercions(registry, &concrete_type, traits, &vtable_methods);

        let span = expr.span.clone();
        TypedExpr {
            kind: TypedExprKind::InterfaceObjectCoerce {
                inner: Box::new(expr),
                interface_mangled_name: mangled_name.clone(),
                concrete_type,
                vtable_methods,
            },
            ty: target.clone(),
            span,
        }
    } else {
        expr
    }
}

/// Fresh-name counter for tuple-spill locals introduced by argument coercion.
static TUPLE_SPILL_COUNTER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Does `arg_ty` need interface-object conversion or Any boxing, at the
/// top level or recursively inside a tuple, to flow into `param_ty`?
fn needs_param_coercion(arg_ty: &Type, param_ty: &Type) -> bool {
    if needs_interface_object_coercion(arg_ty, param_ty) || needs_any_boxing(arg_ty, param_ty) {
        return true;
    }
    if let (Type::Tuple(arg_elems, _), Type::Tuple(param_elems, _)) = (arg_ty, param_ty) {
        return arg_elems.len() == param_elems.len()
            && arg_elems
                .iter()
                .zip(param_elems.iter())
                .any(|(a, p)| needs_param_coercion(a, p));
    }
    false
}

/// Coerce an argument value toward a param type, reifying interface-object
/// conversions and Any boxing at any depth inside tuples. A tuple literal is rebuilt
/// element-wise; a non-literal tuple is spilled through a destructuring let
/// so its elements can be coerced individually.
fn coerce_arg_to_param(
    a: TypedExpr,
    param_ty: &Type,
    registry: &Registry,
    interface_object_types: &RefCell<BTreeMap<MangledName, TypeDef>>,
) -> TypedExpr {
    if needs_interface_object_coercion(&a.ty, param_ty) {
        return coerce_to_interface_object(a, param_ty, registry, interface_object_types);
    }
    if needs_any_boxing(&a.ty, param_ty) {
        return box_to_any(a);
    }
    if !needs_param_coercion(&a.ty, param_ty) {
        return a;
    }
    // Tuple with element-level coercions.
    let Type::Tuple(param_elems, _) = param_ty else {
        return a;
    };
    let span = a.span.clone();
    match a.kind {
        TypedExprKind::TupleLiteral { elements } => {
            let new_elements: Vec<TypedExpr> = elements
                .into_iter()
                .zip(param_elems.iter())
                .map(|(e, pt)| coerce_arg_to_param(e, pt, registry, interface_object_types))
                .collect();
            let new_types: Vec<Type> = new_elements.iter().map(|e| e.ty.clone()).collect();
            let mn = MangledName::for_tuple(&new_types);
            TypedExpr {
                kind: TypedExprKind::TupleLiteral {
                    elements: new_elements,
                },
                ty: Type::Tuple(new_types, mn),
                span,
            }
        }
        _ => {
            // Spill: `{ let ($t0, $t1, …) = arg; (coerce($t0), coerce($t1), …) }`
            let Type::Tuple(arg_elems, _) = a.ty.clone() else {
                return a;
            };
            let id = TUPLE_SPILL_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            let names: Vec<VarName> = (0..arg_elems.len())
                .map(|i| VarName(format!("$argspill{id}${i}")))
                .collect();
            let pattern = TypedPattern::Tuple {
                element_patterns: names
                    .iter()
                    .zip(arg_elems.iter())
                    .map(|(n, t)| TypedPattern::Variable(n.clone(), t.clone()))
                    .collect(),
                tuple_type: a.ty.clone(),
            };
            let destructure = TypedExpr {
                kind: TypedExprKind::LetDestructure {
                    pattern,
                    var_ty: a.ty.clone(),
                    value: Box::new(a),
                },
                ty: Type::Unit,
                span: span.clone(),
            };
            let new_elements: Vec<TypedExpr> = names
                .iter()
                .zip(arg_elems.iter())
                .zip(param_elems.iter())
                .map(|((n, at), pt)| {
                    let var = TypedExpr {
                        kind: TypedExprKind::VarRef {
                            name: n.clone(),
                            boxed: false,
                        },
                        ty: at.clone(),
                        span: span.clone(),
                    };
                    coerce_arg_to_param(var, pt, registry, interface_object_types)
                })
                .collect();
            let new_types: Vec<Type> = new_elements.iter().map(|e| e.ty.clone()).collect();
            let mn = MangledName::for_tuple(&new_types);
            let tuple_ty = Type::Tuple(new_types, mn);
            let literal = TypedExpr {
                kind: TypedExprKind::TupleLiteral {
                    elements: new_elements,
                },
                ty: tuple_ty.clone(),
                span: span.clone(),
            };
            TypedExpr {
                kind: TypedExprKind::Block(vec![destructure, literal]),
                ty: tuple_ty,
                span,
            }
        }
    }
}

/// The substituted non-self param types of the vtable member a
/// `InterfaceObjectMethodCall` dispatches to: find the declaring component in the
/// receiver's set (its per-trait key equals the node's `interface_mangled_name`),
/// take the member's raw signature params from the registry, and substitute
/// the trait's type params with the component's type args. Empty when
/// anything is unresolvable (no coercion is then attempted).
fn substituted_member_params(
    receiver_ty: &Type,
    component_key: &MangledName,
    member_name: &InterfaceMemberName,
    registry: &Registry,
) -> Vec<Type> {
    let Type::InterfaceObject { traits, .. } = receiver_ty else {
        return Vec::new();
    };
    let Some(component) = traits.iter().find(|c| {
        MangledName::for_interface_object_per_interface(&c.trait_fqn) == *component_key
                // Inherited member: the node is keyed by the ORIGIN trait,
                // which lives in this component's super closure.
                || registry
                    .lookup_trait(&c.trait_fqn, &c.trait_fqn.package)
                    .is_some_and(|sig| {
                        sig.super_closure.iter().any(|(f, _)| {
                            MangledName::for_interface_object_per_interface(f) == *component_key
                        })
                    })
    }) else {
        return Vec::new();
    };
    let Some(sig) = registry.lookup_trait(&component.trait_fqn, &component.trait_fqn.package)
    else {
        return Vec::new();
    };
    for method in &sig.methods {
        // Slot identity comes from the ORIGIN trait's raw member.
        let (origin_sig, raw_method) = registry.origin_method_raw(sig, method);
        let raw: Vec<(String, Type)> = raw_method
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .cloned()
            .collect();
        let raw_strs: Vec<String> = raw.iter().map(|(_, t)| t.to_string()).collect();
        if InterfaceMemberName::new(&raw_method.name, &raw_strs) == *member_name {
            // Substitute the origin trait's params with the origin args as
            // seen from this component's application: for own members that is
            // the component's own args; for inherited members the flattened
            // origin args substituted with the component's args.
            let owner_subst: BTreeMap<crate::common::types::TypeParamName, Type> = sig
                .type_params
                .iter()
                .cloned()
                .zip(component.trait_type_args.iter().cloned())
                .collect();
            let origin_args: Vec<Type> = match &method.origin {
                None => component.trait_type_args.to_vec(),
                Some((_, args)) => args
                    .iter()
                    .map(|t| {
                        crate::typechecker::collect::substitute_trait_type_params(t, &owner_subst)
                    })
                    .collect(),
            };
            let subst: BTreeMap<&str, &Type> = origin_sig
                .type_params
                .iter()
                .map(|tp| tp.0.as_str())
                .zip(origin_args.iter())
                .collect();
            return raw
                .iter()
                .map(|(_, t)| substitute_named_type_params(t, &subst))
                .collect();
        }
    }
    Vec::new()
}

/// Substitute named type variables/params by trait-param name.
fn substitute_named_type_params(ty: &Type, subst: &BTreeMap<&str, &Type>) -> Type {
    match ty {
        Type::TypeVariable(name, _) | Type::GenericParam(name, _, _) => subst
            .get(name.0.as_str())
            .map(|t| (*t).clone())
            .unwrap_or_else(|| ty.clone()),
        Type::Array(elem) => Type::Array(Box::new(substitute_named_type_params(elem, subst))),
        Type::Function(params, ret) => Type::Function(
            params
                .iter()
                .map(|t| substitute_named_type_params(t, subst))
                .collect(),
            Box::new(substitute_named_type_params(ret, subst)),
        ),
        Type::InterfaceObject { traits, .. } => Type::interface_intersection(
            traits
                .iter()
                .map(|c| {
                    (
                        c.trait_fqn.clone(),
                        c.trait_type_args
                            .iter()
                            .map(|t| substitute_named_type_params(t, subst))
                            .collect(),
                    )
                })
                .collect(),
        ),
        _ => ty.clone(),
    }
}

/// Deep-walk a type and register the interface-object TypeDefs for every
/// `Type::InterfaceObject` it contains (components + intersection struct).
fn register_interface_types_in(
    ty: &Type,
    registry: &Registry,
    interface_object_types: &RefCell<BTreeMap<MangledName, TypeDef>>,
) {
    match ty {
        Type::InterfaceObject {
            traits,
            mangled_name,
        } => {
            register_interface_object_set(traits, mangled_name, registry, interface_object_types);
            for c in traits {
                for t in &c.trait_type_args {
                    register_interface_types_in(t, registry, interface_object_types);
                }
            }
        }
        Type::Array(elem) | Type::Newtype(_, elem) => {
            register_interface_types_in(elem, registry, interface_object_types);
        }
        Type::Tuple(types, _) => {
            for t in types {
                register_interface_types_in(t, registry, interface_object_types);
            }
        }
        Type::Function(params, ret) => {
            for t in params {
                register_interface_types_in(t, registry, interface_object_types);
            }
            register_interface_types_in(ret, registry, interface_object_types);
        }
        Type::GenericRecord { type_args, .. }
        | Type::GenericEnum { type_args, .. }
        | Type::GenericClass { type_args, .. }
        | Type::GenericNewtype { type_args, .. } => {
            for (_, t) in type_args {
                register_interface_types_in(t, registry, interface_object_types);
            }
        }
        _ => {}
    }
}

/// Register the TypeDefs for an interface-object set: every component's
/// per-trait `InterfaceObjectTypeDef`, plus (for an intersection) the
/// `InterfaceIntersectionTypeDef` tying them together.
fn register_interface_object_set(
    traits: &[crate::typechecker::types::InterfaceComponent],
    set_mangled_name: &MangledName,
    registry: &Registry,
    interface_object_types: &RefCell<BTreeMap<MangledName, TypeDef>>,
) {
    for component in traits {
        register_interface_object_type_def(&component.trait_fqn, registry, interface_object_types);
    }
    if traits.len() > 1
        && !interface_object_types
            .borrow()
            .contains_key(set_mangled_name)
    {
        let components: Vec<(Fqn, MangledName)> = traits
            .iter()
            .map(|c| {
                (
                    c.trait_fqn.clone(),
                    MangledName::for_interface_object_per_interface(&c.trait_fqn),
                )
            })
            .collect();
        interface_object_types.borrow_mut().insert(
            set_mangled_name.clone(),
            TypeDef::InterfaceIntersection(
                crate::typechecker::types::InterfaceIntersectionTypeDef {
                    mangled_name: set_mangled_name.clone(),
                    components,
                },
            ),
        );
    }
}

/// Compute the grouped per-component vtable methods for a coercion to an
/// interface-object set: one `(component per-trait key, entries)` group per
/// component, in the set's (sorted) order.
fn compute_grouped_vtable_methods(
    registry: &Registry,
    concrete_type: &Type,
    type_fqn: &Fqn,
    traits: &[crate::typechecker::types::InterfaceComponent],
) -> Vec<VtableMethodGroup> {
    // One group per component — and, for extended interfaces, one group per
    // (transitive) super in DFS post-order BEFORE its extender, so the vtable
    // globals' const-expr builds nested super vtables bottom-up on the wasm
    // operand stack. Diamonds repeat the shared super (each parent consumes
    // its own nested instance). All groups of a component tree share ONE
    // provider (the block that provides the root component for this type):
    // its inline members back every inherited slot. A group whose provider
    // trait differs from the group's own trait gets a `$via$`-tagged key so
    // wrapper names never collide with a direct coercion's wrappers; codegen
    // splits the tag off for slot/type lookups.
    let mut groups = Vec::new();
    for c in traits {
        let provider = resolve_vtable_provider(
            registry,
            &c.trait_fqn,
            &c.trait_type_args,
            type_fqn,
            concrete_type,
        );
        push_component_groups(
            registry,
            concrete_type,
            type_fqn,
            &c.trait_fqn,
            &c.trait_type_args,
            &provider,
            &mut groups,
        );
    }
    groups
}

type DeduplicatedCoercions<T> = (Vec<T>, std::collections::BTreeSet<(String, MangledName)>);

thread_local! {
    /// Synthetic direct super coercions accumulated while one
    /// `elaborate_coercions` run computes/visits vtable groups; drained into
    /// `TypedModule.synthetic_interface_coercions` at the end of the run.
    /// (Same per-compile-thread pattern as monomorphize's DEFAULT_RETARGET.)
    static SYNTHETIC_DIRECT: RefCell<DeduplicatedCoercions<crate::typechecker::types::SyntheticInterfaceCoercion>> = const { RefCell::new((Vec::new(), std::collections::BTreeSet::new())) };
    /// (concrete type, full via group key, direct group key) re-box mappings (see
    /// `TypedModule.direct_rebox_authorizations`), with a dedup set.
    static DIRECT_REBOX_AUTH: RefCell<DeduplicatedCoercions<(Type, MangledName, MangledName)>> = const { RefCell::new((Vec::new(), std::collections::BTreeSet::new())) };
}

/// "The direct impl owns the (type, super) re-box global": when a coercion's
/// component tree contains a `$via$`-backed super with a bare-`Self`-returning
/// member and the type ALSO implements that super directly, the direct
/// (type, super) vtable global must exist regardless of whether any expression
/// coerces to the super — otherwise the re-box would fall back to the
/// provider's standalone whenever no unrelated direct coercion happens to be
/// in the program, making dispatch depend on distant code. Synthesize the
/// direct coercion here (globals/wrappers only; no expression).
fn synthesize_direct_super_coercions(
    registry: &Registry,
    concrete_type: &Type,
    traits: &[crate::typechecker::types::InterfaceComponent],
    groups: &[VtableMethodGroup],
) {
    if concrete_type.contains_type_parameter()
        || matches!(concrete_type, Type::InterfaceObject { .. })
    {
        return;
    }
    let Some(type_fqn) = concrete_type.try_to_fqn() else {
        return;
    };
    for c in traits {
        let Some(sig) = registry.lookup_trait(&c.trait_fqn, &c.trait_fqn.package) else {
            continue;
        };
        if sig.supers.is_empty() {
            continue;
        }
        let Some(root_provider) = resolve_vtable_provider(
            registry,
            &c.trait_fqn,
            &c.trait_type_args,
            &type_fqn,
            concrete_type,
        ) else {
            continue;
        };
        for (super_fqn, _) in &sig.super_closure {
            // This super's group in the component tree is `$via$`-tagged only
            // when the tree's provider is a different trait.
            if root_provider.trait_fqn == *super_fqn {
                continue;
            }
            let Some(super_args) =
                registry.super_closure_args(&c.trait_fqn, &c.trait_type_args, super_fqn)
            else {
                continue;
            };
            let Some(super_sig) = registry.lookup_trait(super_fqn, &super_fqn.package) else {
                continue;
            };
            // Only members the super declares itself re-box into ITS object
            // type; inherited copies re-box into their origin's, which this
            // loop reaches as its own closure entry.
            let has_self_return = super_sig
                .methods
                .iter()
                .filter(|m| m.origin.is_none())
                .any(|m| m.return_type.contains_self_type())
                || super_sig
                    .properties
                    .iter()
                    .filter(|p| p.origin.is_none())
                    .any(|p| p.return_type.contains_self_type());
            if !has_self_return {
                continue;
            }
            let super_provider =
                resolve_vtable_provider(registry, super_fqn, &super_args, &type_fqn, concrete_type);
            if !class_directly_implements(registry, concrete_type, super_fqn, &super_args)
                && !super_provider
                    .as_ref()
                    .is_some_and(|provider| provider.trait_fqn == *super_fqn)
            {
                continue; // no direct impl of the super — the via standalone stays authoritative
            }
            let mn = MangledName::for_interface_object_per_interface(super_fqn);
            let mut direct_groups = Vec::new();
            push_component_groups(
                registry,
                concrete_type,
                &type_fqn,
                super_fqn,
                &super_args,
                &super_provider,
                &mut direct_groups,
            );
            let direct_key = direct_groups.last().expect("super group exists").0.clone();
            // The type directly implements THIS application of the super, so
            // its via-backed groups in this coercion may re-box through the
            // direct global. Authorize each matching full group key (the
            // wrapper's `self_return_group_key`): base + `$via$<provider>`
            // (+ optional `$inst$…`).
            let via_prefix = format!("{}$via${}", mn, root_provider.trait_fqn);
            for (group_key, _) in groups {
                let matches_super = group_key.0 == via_prefix
                    || group_key
                        .0
                        .strip_prefix(&via_prefix)
                        .is_some_and(|rest| rest.starts_with("$inst$"));
                if matches_super {
                    let auth_key = (format!("{}", concrete_type), group_key.clone());
                    DIRECT_REBOX_AUTH.with(|cell| {
                        let mut st = cell.borrow_mut();
                        if st.1.insert(auth_key) {
                            st.0.push((
                                concrete_type.clone(),
                                group_key.clone(),
                                direct_key.clone(),
                            ));
                        }
                    });
                }
            }
            let seen_key = (format!("{}", concrete_type), direct_key);
            let already = SYNTHETIC_DIRECT.with(|cell| !cell.borrow_mut().1.insert(seen_key));
            if already {
                continue;
            }
            SYNTHETIC_DIRECT.with(|cell| {
                cell.borrow_mut()
                    .0
                    .push(crate::typechecker::types::SyntheticInterfaceCoercion {
                        concrete_type: concrete_type.clone(),
                        interface_mangled_name: mn,
                        vtable_methods: direct_groups,
                    })
            });
        }
    }
}

fn push_component_groups(
    registry: &Registry,
    concrete_type: &Type,
    type_fqn: &Fqn,
    component_fqn: &Fqn,
    component_args: &[Type],
    provider: &Option<VtableProvider>,
    groups: &mut Vec<VtableMethodGroup>,
) {
    if let Some(sig) = registry.lookup_trait(component_fqn, &component_fqn.package) {
        let supers: Vec<(Fqn, Vec<Type>)> = sig
            .supers
            .iter()
            .map(|r| {
                let sub: BTreeMap<TypeParamName, Type> = sig
                    .type_params
                    .iter()
                    .cloned()
                    .zip(component_args.iter().cloned())
                    .collect();
                let args: Vec<Type> = r
                    .type_args
                    .iter()
                    .map(|t| crate::typechecker::collect::substitute_trait_type_params(t, &sub))
                    .collect();
                (r.fqn.clone(), args)
            })
            .collect();
        for (super_fqn, super_args) in &supers {
            push_component_groups(
                registry,
                concrete_type,
                type_fqn,
                super_fqn,
                super_args,
                provider,
                groups,
            );
        }
    }

    let entries = compute_vtable_methods(
        registry,
        concrete_type,
        type_fqn,
        component_fqn,
        component_args,
        provider,
    );
    let base_key = MangledName::for_interface_object_per_interface(component_fqn);
    let mut key = match provider {
        Some(p) if p.trait_fqn != *component_fqn => {
            MangledName(format!("{}$via${}", base_key, p.trait_fqn))
        }
        _ => base_key,
    };
    // Sibling instantiations (`implement A<Int32> for Rec` AND
    // `implement A<String> for Rec`): the de-monomorphized wrapper/global
    // keys can't tell them apart, so the group key carries the provider's
    // substituted trait args. Include generic blocks, whose parameter names
    // need not match across sibling declarations. A trait with only one
    // implementation can keep its historical untagged key.
    if let Some(p) = provider
        && !p.trait_type_args.is_empty()
    {
        let sibling_count = registry
            .all_implement_blocks()
            .iter()
            .filter(|b| b.trait_fqn == p.trait_fqn)
            .count();
        if sibling_count > 1 {
            let args: Vec<String> = p.trait_type_args.iter().map(|t| t.to_string()).collect();
            key = MangledName(format!("{}$inst${}", key, args.join(",")));
        }
    }
    groups.push((key, entries));
}

/// Register a InterfaceObjectTypeDef for the given trait (deduplicated by mangled name).
/// Uses the registry's trait signature to compute vtable member types. The trait's type args do
/// not affect the (de-monomorphized, per-trait) result, so they are not a parameter.
fn register_interface_object_type_def(
    trait_fqn: &Fqn,
    registry: &Registry,
    interface_object_types: &RefCell<BTreeMap<MangledName, TypeDef>>,
) {
    // Per-trait key (type args dropped) — one TypeDef per trait, shared across instantiations.
    // Registration is instantiation-independent (the vtable members below use the trait's *raw*
    // signatures), so a generic coercion like `… as Iterator<T>` registers the same canonical
    // TypeDef as a concrete `… as Iterator<Int32>`. We therefore do NOT skip type-parameter args.
    let mn = MangledName::for_interface_object_per_interface(trait_fqn);

    // Skip if already registered
    if interface_object_types.borrow().contains_key(&mn) {
        return;
    }

    let trait_sig = match registry.lookup_trait(trait_fqn, &trait_fqn.package) {
        Some(sig) => sig,
        None => return,
    };

    // Vtable members use the trait method's *raw* (unsubstituted) param/return types: the trait's
    // generic params stay as type parameters so codegen erases them to `anyref`, while genuinely
    // concrete types stay concrete. This makes the slot signatures (and member names) identical
    // across all instantiations of the trait — the basis for de-monomorphizing interface objects.
    // Register the direct supers' TypeDefs first (an extended interface's
    // vtable nests one ref per direct super, and upcasts/dispatch reach into
    // them), then collect this interface's OWN slots — inherited members live
    // in the nested super vtables, keyed by their origin's raw signature.
    let supers: Vec<MangledName> = trait_sig
        .supers
        .iter()
        .map(|super_ref| MangledName::for_interface_object_per_interface(&super_ref.fqn))
        .collect();
    let super_fqns: Vec<Fqn> = trait_sig.supers.iter().map(|r| r.fqn.clone()).collect();
    for super_fqn in &super_fqns {
        register_interface_object_type_def(super_fqn, registry, interface_object_types);
    }
    let trait_sig = match registry.lookup_trait(trait_fqn, &trait_fqn.package) {
        Some(sig) => sig,
        None => return,
    };

    let mut vtable_members = Vec::new();

    for method_sig in &trait_sig.methods {
        if method_sig.origin.is_some() {
            continue;
        }
        if !method_sig.type_params.is_empty() {
            continue;
        }
        if !method_sig.params.iter().any(|(name, _)| name == "self") {
            continue;
        }
        if method_sig
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .any(|(_, ty)| ty.contains_self_type())
        {
            continue;
        }

        let non_self_params: Vec<Type> = method_sig
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .map(|(_, ty)| ty.clone())
            .collect();
        let non_self_param_strs: Vec<String> =
            non_self_params.iter().map(|t| t.to_string()).collect();
        let member_name = InterfaceMemberName::new(&method_sig.name, &non_self_param_strs);
        let return_type =
            slot_return_type(&method_sig.return_type, trait_fqn, &trait_sig.type_params);
        vtable_members.push((member_name, non_self_params, return_type));
    }

    for prop_sig in &trait_sig.properties {
        if prop_sig.origin.is_some() {
            continue;
        }
        if !prop_sig.params.iter().any(|(name, _)| name == "self") {
            continue;
        }
        let member_name = InterfaceMemberName::new(&prop_sig.name, &[]);
        let return_type =
            slot_return_type(&prop_sig.return_type, trait_fqn, &trait_sig.type_params);
        vtable_members.push((member_name, vec![], return_type));
    }

    interface_object_types.borrow_mut().insert(
        mn.clone(),
        TypeDef::InterfaceObject(InterfaceObjectTypeDef {
            trait_fqn: trait_fqn.clone(),
            mangled_name: mn,
            supers,
            vtable_members,
        }),
    );
}

/// The vtable slot's return type for a trait member. A *bare* `Self` return
/// becomes the interface-object type of the declaring trait (the wrapper
/// re-boxes the concrete return into the interface at the boundary — "Self is
/// observed as the interface type"). Other returns keep the raw signature type
/// (nested `Self` is rejected at the interface declaration).
fn slot_return_type(
    raw_return: &Type,
    trait_fqn: &Fqn,
    trait_type_params: &[crate::common::types::TypeParamName],
) -> Type {
    if matches!(raw_return, Type::SelfType) {
        let raw_args: Vec<Type> = trait_type_params
            .iter()
            .map(|tp| Type::TypeVariable(tp.clone(), vec![]))
            .collect();
        Type::interface_object(trait_fqn.clone(), raw_args)
    } else {
        raw_return.clone()
    }
}

/// Compute vtable method entries for a (concrete_type, trait) pair — the
/// trait's OWN members only (inherited members live in the nested super
/// vtables, produced as their own groups). `provider` is the impl block that
/// backs the whole component tree (resolved for the root component).
fn compute_vtable_methods(
    registry: &Registry,
    concrete_type: &Type,
    type_fqn: &Fqn,
    trait_fqn: &Fqn,
    trait_type_args: &[Type],
    provider: &Option<VtableProvider>,
) -> Vec<(InterfaceMemberName, MangledName, Vec<Type>)> {
    let trait_sig = match registry.lookup_trait(trait_fqn, &trait_fqn.package) {
        Some(sig) => sig,
        None => return Vec::new(),
    };

    let class_sig = registry.get_class_type(type_fqn);

    let mut methods = Vec::new();

    // A closure computing the impl-function name for one member via the
    // provider block (or the historical concrete-type naming when no block
    // matched — e.g. signature-only compiles).
    let member_entry = |member_sym: &SymbolName,
                        member_name: &InterfaceMemberName|
     -> (InterfaceMemberName, MangledName, Vec<Type>) {
        match provider {
            Some(p) => {
                // Always the PROVIDER block's trait args — its functions are
                // mangled with them. (For a super group under a direct
                // provider, the group's own `trait_type_args` are the super's
                // substituted args, which may differ — e.g.
                // `IntProducer extends Producer<Int32>` has group args
                // `[Int32]` while the provider block's are `[]`.)
                let dispatch_name = registry
                    .get_trait(&p.trait_fqn)
                    .and_then(|signature| {
                        signature
                            .methods
                            .iter()
                            .find(|method| {
                                method.name == member_sym.0
                                    && method
                                        .origin
                                        .as_ref()
                                        .map(|(origin, _)| origin)
                                        .unwrap_or(&signature.fqn)
                                        == trait_fqn
                            })
                            .map(|method| signature.method_dispatch_name(method))
                    })
                    .unwrap_or_else(|| member_sym.clone());
                let mut mangled = crate::typechecker::types::impl_member_mangled_name(
                    &p.trait_fqn,
                    &p.for_type,
                    &p.type_params,
                    &dispatch_name,
                    // Always the PROVIDER block's trait args, EXACTLY as
                    // written (raw) — instantiated impl functions are mangled
                    // with them (resolve_impl_method_mangled); the
                    // per-instantiation discriminator is with_type_args below.
                    &p.raw_trait_type_args,
                );
                if !p.inst_args.is_empty() {
                    mangled = mangled.with_type_args(&p.inst_args);
                }
                (member_name.clone(), mangled, p.inst_args.clone())
            }
            None => {
                let mangled = crate::typechecker::types::impl_member_mangled_name(
                    trait_fqn,
                    concrete_type,
                    &[],
                    member_sym,
                    trait_type_args,
                );
                (member_name.clone(), mangled, vec![])
            }
        }
    };

    for method_sig in &trait_sig.methods {
        if method_sig.origin.is_some() {
            continue;
        }
        if !method_sig.type_params.is_empty() {
            continue;
        }
        if !method_sig.params.iter().any(|(name, _)| name == "self") {
            continue;
        }
        if method_sig
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .any(|(_, ty)| ty.contains_self_type())
        {
            continue;
        }

        // Member name from the *raw* (unsubstituted) param types — matches the per-trait vtable
        // registered in `register_interface_object_type_def`, so the field index resolves identically
        // for every instantiation.
        let non_self_params: Vec<String> = method_sig
            .params
            .iter()
            .filter(|(name, _)| name != "self")
            .map(|(_, ty)| ty.to_string())
            .collect();
        let member_name = InterfaceMemberName::new(&method_sig.name, &non_self_params);

        let method_sym = SymbolName(method_sig.name.clone());

        if class_sig.is_some() && provider.is_none() {
            if let Some((mangled, class_type_args)) = resolve_class_vtable_mangled(
                registry,
                concrete_type,
                trait_fqn,
                trait_type_args,
                &trait_sig.method_dispatch_name(method_sig),
            ) {
                methods.push((member_name, mangled, class_type_args));
            }
        } else {
            methods.push(member_entry(&method_sym, &member_name));
        }
    }

    for prop_sig in &trait_sig.properties {
        if prop_sig.origin.is_some() {
            continue;
        }
        if !prop_sig.params.iter().any(|(name, _)| name == "self") {
            continue;
        }

        let member_name = InterfaceMemberName::new(&prop_sig.name, &[]);
        let method_sym = SymbolName(prop_sig.name.clone());

        if class_sig.is_some() && provider.is_none() {
            if let Some((mangled, class_type_args)) = resolve_class_vtable_mangled(
                registry,
                concrete_type,
                trait_fqn,
                trait_type_args,
                &method_sym,
            ) {
                methods.push((member_name, mangled, class_type_args));
            }
        } else {
            methods.push(member_entry(&method_sym, &member_name));
        }
    }

    methods
}

/// The impl block whose members back a (concrete_type, trait) vtable, with
/// everything needed to name its functions. Direct impls win over sub-trait
/// providers (ambiguity among providers is rejected during inference).
struct VtableProvider {
    trait_fqn: Fqn,
    for_type: Type,
    type_params: Vec<TypeParamName>,
    /// The block's trait args with instantiation bindings applied (used for
    /// matching against the requested component's args).
    trait_type_args: Vec<Type>,
    /// The block's trait args EXACTLY as written (may mention block type
    /// params). Instantiated impl functions are mangled with these — the
    /// per-instantiation discriminator is `with_type_args(inst_args)`.
    raw_trait_type_args: Vec<Type>,
    /// Generic-block instantiation args (empty for non-generic blocks).
    inst_args: Vec<Type>,
}

fn class_directly_implements(
    registry: &Registry,
    concrete_type: &Type,
    trait_fqn: &Fqn,
    trait_type_args: &[Type],
) -> bool {
    let Some(class) = concrete_type
        .try_to_fqn()
        .and_then(|fqn| registry.get_class_type(&fqn))
    else {
        return false;
    };
    let bindings: BTreeMap<_, _> = match concrete_type {
        Type::GenericClass { type_args, .. } => class
            .type_params
            .iter()
            .cloned()
            .zip(type_args.iter().map(|(_, ty)| ty.clone()))
            .collect(),
        _ => BTreeMap::new(),
    };
    class.trait_impls.iter().any(|(fqn, args)| {
        fqn == trait_fqn
            && args.len() == trait_type_args.len()
            && args.iter().zip(trait_type_args).all(|(arg, expected)| {
                crate::typechecker::collect::substitute_trait_type_params(arg, &bindings)
                    == *expected
            })
    })
}

fn resolve_vtable_provider(
    registry: &Registry,
    trait_fqn: &Fqn,
    trait_type_args: &[Type],
    type_fqn: &Fqn,
    concrete_type: &Type,
) -> Option<VtableProvider> {
    if class_directly_implements(registry, concrete_type, trait_fqn, trait_type_args) {
        return None;
    }
    // Requested-args matching: sibling blocks (`A<Int32> for Rec` vs
    // `A<String> for Rec`) and via-providers of different super
    // instantiations share a base FQN — the requested component's args
    // pick among them.
    let args_match =
        |candidate: &[Type]| -> bool { trait_type_args.is_empty() || candidate == trait_type_args };
    for (block, via) in registry.find_providing_impl_blocks(trait_fqn, type_fqn) {
        if block.type_params.is_empty() {
            // Sibling instantiations share a base FQN — the block must match
            // the concrete type exactly (plain types always do).
            if block.for_type != *concrete_type {
                continue;
            }
            let provided_args: &[Type] = match &via {
                None => &block.trait_type_args,
                Some((_, closure_args)) => closure_args,
            };
            if !args_match(provided_args) {
                continue;
            }
            return Some(VtableProvider {
                trait_fqn: block.trait_fqn.clone(),
                for_type: block.for_type.clone(),
                type_params: vec![],
                trait_type_args: block.trait_type_args.clone(),
                raw_trait_type_args: block.trait_type_args.clone(),
                inst_args: vec![],
            });
        }
        let mut substitution =
            crate::typechecker::infer::type_param_substitution::TypeParamSubstitution::new();
        if substitution.unify(&block.for_type, concrete_type) {
            let provided_args = via
                .as_ref()
                .map_or(&block.trait_type_args, |(_, args)| args);
            if !trait_type_args.is_empty()
                && (provided_args.len() != trait_type_args.len()
                    || !provided_args
                        .iter()
                        .zip(trait_type_args)
                        .all(|(provided, required)| substitution.unify(provided, required)))
            {
                continue;
            }
            // Associated-type equalities can determine block parameters that
            // do not occur in the receiver or the requested interface.
            let Some(substitution) = crate::typechecker::infer::complete_impl_substitution(
                registry,
                block,
                substitution,
            ) else {
                continue;
            };
            if let Some(inst_args) = substitution.resolve_type_params(&block.type_params) {
                let sub: BTreeMap<TypeParamName, Type> = block
                    .type_params
                    .iter()
                    .cloned()
                    .zip(inst_args.iter().cloned())
                    .collect();
                let substituted_trait_args: Vec<Type> = block
                    .trait_type_args
                    .iter()
                    .map(|t| crate::typechecker::collect::substitute_trait_type_params(t, &sub))
                    .collect();
                let provided_args: Vec<Type> = match &via {
                    None => substituted_trait_args.clone(),
                    Some((_, closure_args)) => closure_args
                        .iter()
                        .map(|t| crate::typechecker::collect::substitute_trait_type_params(t, &sub))
                        .collect(),
                };
                if !args_match(&provided_args) {
                    continue;
                }
                return Some(VtableProvider {
                    trait_fqn: block.trait_fqn.clone(),
                    for_type: block.for_type.clone(),
                    type_params: block.type_params.clone(),
                    trait_type_args: substituted_trait_args,
                    raw_trait_type_args: block.trait_type_args.clone(),
                    inst_args,
                });
            }
        }
    }
    None
}

fn resolve_class_vtable_mangled(
    registry: &Registry,
    concrete_type: &Type,
    trait_fqn: &Fqn,
    trait_parameters: &[Type],
    method_name: &SymbolName,
) -> Option<(MangledName, Vec<Type>)> {
    let (template, parameters) = crate::typechecker::class_trait_methods::resolve_template(
        registry,
        trait_fqn,
        trait_parameters,
        concrete_type,
        method_name,
        &[],
    )?;
    let name = if parameters.is_empty() {
        template
    } else {
        template.with_type_args(&parameters)
    };
    Some((name, parameters))
}

/// Coerce a list of arguments against expected parameter types — interface-object coercion
/// for ref-type values into InterfaceObject slots, primitive boxing for non-ref values into
/// `Type::Any` slots, identity otherwise.
fn coerce_args(
    args: Vec<TypedExpr>,
    param_types: &[Type],
    registry: &Registry,
    interface_object_types: &RefCell<BTreeMap<MangledName, TypeDef>>,
) -> Vec<TypedExpr> {
    args.into_iter()
        .zip(param_types.iter())
        .map(|(arg, expected)| {
            if needs_param_coercion(&arg.ty, expected) {
                coerce_arg_to_param(arg, expected, registry, interface_object_types)
            } else if needs_any_boxing(&arg.ty, expected) {
                box_to_any(arg)
            } else {
                arg
            }
        })
        .collect()
}

/// Look up variant payload types for an enum type.
fn lookup_enum_variant_payload(
    enum_type: &Type,
    variant_name: &str,
    types: &BTreeMap<MangledName, TypeDef>,
) -> Option<Vec<Type>> {
    let mn = match enum_type {
        Type::GenericEnum { mangled_name, .. } | Type::Enum(_, mangled_name) => mangled_name,
        _ => return None,
    };
    let enum_def = match types.get(mn) {
        Some(TypeDef::Enum(e)) => e,
        _ => return None,
    };
    enum_def
        .variants
        .iter()
        .find(|v| v.name == variant_name)
        .map(|v| v.payload_types.clone())
}

/// Registry fallback for enum variant payload types — for enums declared in
/// another package (their TypeDef lives in that package's module, not here).
fn lookup_enum_variant_payload_from_registry(
    fqn: &Fqn,
    variant_name: &str,
    registry: &Registry,
) -> Option<Vec<Type>> {
    let sig = registry.get_enum_type(fqn)?;
    sig.variants
        .iter()
        .find(|(name, _)| name == variant_name)
        .map(|(_, payload)| match payload {
            crate::typechecker::registry::VariantPayload::None => Vec::new(),
            crate::typechecker::registry::VariantPayload::Tuple(types) => types.clone(),
            crate::typechecker::registry::VariantPayload::Record(fields) => {
                fields.iter().map(|(_, t)| t.clone()).collect()
            }
        })
}

/// Substitute an enum's declared type params with the instantiation's type
/// args in raw payload types, so an interface-typed payload position (e.g.
/// `Option<Alpha>`'s `Some(T)` → `Alpha`) coerces its value instead of
/// passing the raw concrete struct through the erased slot.
fn substitute_enum_payload_types(
    payload_types: &[Type],
    enum_ty: &Type,
    registry: &Registry,
) -> Vec<Type> {
    let (fqn, type_args): (&Fqn, Vec<&Type>) = match enum_ty {
        Type::GenericEnum { fqn, type_args, .. } => {
            (fqn, type_args.iter().map(|(_, t)| t).collect())
        }
        _ => return payload_types.to_vec(),
    };
    let Some(sig) = registry.get_enum_type(fqn) else {
        return payload_types.to_vec();
    };
    if sig.type_params.len() != type_args.len() {
        return payload_types.to_vec();
    }
    let subst: BTreeMap<&str, &Type> = sig
        .type_params
        .iter()
        .map(|tp| tp.0.as_str())
        .zip(type_args)
        .collect();
    payload_types
        .iter()
        .map(|t| substitute_named_type_params(t, &subst))
        .collect()
}

/// Look up field types for a record type. Returns name → Type mapping.
fn lookup_record_field_types(
    record_type: &Type,
    types: &BTreeMap<MangledName, TypeDef>,
) -> Option<BTreeMap<String, Type>> {
    let mn = match record_type {
        Type::GenericRecord { mangled_name, .. } | Type::Record(_, mangled_name) => mangled_name,
        _ => return None,
    };
    let record_def = match types.get(mn) {
        Some(TypeDef::Record(r)) => r,
        _ => return None,
    };
    Some(
        record_def
            .fields
            .iter()
            .map(|(name, ty)| (name.clone(), ty.clone()))
            .collect(),
    )
}

/// Look up the type of a field by name from a class or record type.
fn lookup_field_type(
    object_type: &Type,
    field_name: &str,
    types: &BTreeMap<MangledName, TypeDef>,
) -> Option<Type> {
    let mn = match object_type {
        Type::GenericRecord { mangled_name, .. } | Type::Record(_, mangled_name) => mangled_name,
        Type::GenericClass { mangled_name, .. } | Type::Class(_, mangled_name) => mangled_name,
        _ => return None,
    };
    match types.get(mn) {
        Some(TypeDef::Record(r)) => r
            .fields
            .iter()
            .find(|(n, _)| n == field_name)
            .map(|(_, ty)| ty.clone()),
        Some(TypeDef::Class(c)) => c
            .fields
            .iter()
            .find(|f| f.name == field_name)
            .map(|f| f.ty.clone()),
        _ => None,
    }
}

/// Placeholder expression used during `std::mem::replace` when we need to take ownership
/// of an expression before rewriting it. Never appears in the final AST.
fn placeholder_expr() -> TypedExpr {
    TypedExpr {
        kind: TypedExprKind::UnitLiteral,
        ty: Type::Unit,
        span: Span::point(Arc::from(""), 0, 0),
    }
}
