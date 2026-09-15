use crate::typechecker::types::VtableMethodGroup;
use std::collections::BTreeMap;

use crate::common::types::{Fqn, MangledName, TypeParamName};
use crate::typechecker::types::{
    ResolvedImplMethod, Type, TypedExpr, TypedExprKind, TypedFunction, TypedParam,
};

/// Active while materializing a trait default into an impl block or class:
/// deferred self-member `ImplFunctionCall`s in the default body target the
/// DECLARING trait, but §6's object-coherence requires them to dispatch to
/// the materializing provider's own inline members (a direct impl of the
/// declaring trait must not hijack them).
pub(super) struct DefaultRetarget {
    /// The declaring traits whose calls retarget (the provider's super closure
    /// plus itself).
    pub from_traits: std::collections::BTreeSet<Fqn>,
    pub to_trait: Fqn,
    pub to_trait_args: Vec<Type>,
    /// The substituted `Self` — only calls whose receiver IS this object
    /// retarget.
    pub self_ty: Type,
}

thread_local! {
    pub(super) static DEFAULT_RETARGET: std::cell::RefCell<Option<DefaultRetarget>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard: installs a retarget and clears it on drop — panic-safe (the
/// LSP catches panics and keeps serving on the same thread, so a leaked
/// retarget would silently rewrite calls in the next rebuild).
pub(super) struct DefaultRetargetGuard;

impl DefaultRetargetGuard {
    pub(super) fn install(retarget: DefaultRetarget) -> Self {
        DEFAULT_RETARGET.with(|slot| *slot.borrow_mut() = Some(retarget));
        DefaultRetargetGuard
    }
}

impl Drop for DefaultRetargetGuard {
    fn drop(&mut self) {
        DEFAULT_RETARGET.with(|slot| *slot.borrow_mut() = None);
    }
}

/// While materializing a trait default body FOR A CLASS, `self.member()`
/// calls must dispatch through the class vtable: the same body is inherited
/// by subclasses, so binding it statically would ignore their overrides (and
/// an abstract member has no static body at all).
pub(super) struct ClassVirtualize {
    /// The class the body is being materialized for (the substituted `Self`).
    pub class_type: Type,
    pub trait_fqn: Fqn,
    /// Only slots matching this trait's instantiated member signatures.
    pub vtable_slots: Vec<ClassVirtualSlot>,
}

pub(super) struct ClassVirtualSlot {
    pub method_name: crate::common::types::SymbolName,
    pub arity: usize,
    pub index: u32,
}

thread_local! {
    static CLASS_VIRTUALIZE: std::cell::RefCell<Option<ClassVirtualize>> =
        const { std::cell::RefCell::new(None) };
}

pub(super) struct ClassVirtualizeGuard;

impl ClassVirtualizeGuard {
    pub(super) fn install(ctx: ClassVirtualize) -> Self {
        CLASS_VIRTUALIZE.with(|slot| *slot.borrow_mut() = Some(ctx));
        ClassVirtualizeGuard
    }
}

impl Drop for ClassVirtualizeGuard {
    fn drop(&mut self) {
        CLASS_VIRTUALIZE.with(|slot| *slot.borrow_mut() = None);
    }
}

pub(super) fn unify_type(
    pattern: &Type,
    concrete: &Type,
    bindings: &mut BTreeMap<TypeParamName, Type>,
) {
    if let Type::TupleExtend(left, right) = pattern {
        if let Some((prefix, last)) = pattern.split_tuple_extension(concrete) {
            unify_type(left, &prefix, bindings);
            unify_type(right, &last, bindings);
        }
        return;
    }
    match pattern {
        Type::TypeVariable(name, _) | Type::GenericParam(name, _, _) => {
            bindings
                .entry(name.clone())
                .or_insert_with(|| concrete.clone());
        }
        Type::GenericRecord {
            type_args: p_args, ..
        } => {
            if let Type::GenericRecord {
                type_args: c_args, ..
            } = concrete
            {
                for ((_, p), (_, c)) in p_args.iter().zip(c_args.iter()) {
                    unify_type(p, c, bindings);
                }
            }
        }
        Type::GenericEnum {
            type_args: p_args, ..
        } => {
            if let Type::GenericEnum {
                type_args: c_args, ..
            } = concrete
            {
                for ((_, p), (_, c)) in p_args.iter().zip(c_args.iter()) {
                    unify_type(p, c, bindings);
                }
            }
        }
        Type::GenericNewtype {
            type_args: p_args, ..
        } => {
            if let Type::GenericNewtype {
                type_args: c_args, ..
            } = concrete
            {
                for ((_, p), (_, c)) in p_args.iter().zip(c_args.iter()) {
                    unify_type(p, c, bindings);
                }
            }
        }
        Type::GenericClass {
            type_args: p_args, ..
        } => {
            if let Type::GenericClass {
                type_args: c_args, ..
            } = concrete
            {
                for ((_, p), (_, c)) in p_args.iter().zip(c_args.iter()) {
                    unify_type(p, c, bindings);
                }
            }
        }
        Type::Array(p_elem) => {
            if let Type::Array(c_elem) = concrete {
                unify_type(p_elem, c_elem, bindings);
            }
        }
        Type::Function(p_params, p_ret) => {
            if let Type::Function(c_params, c_ret) = concrete {
                for (p, c) in p_params.iter().zip(c_params.iter()) {
                    unify_type(p, c, bindings);
                }
                unify_type(p_ret, c_ret, bindings);
            }
        }
        Type::Tuple(p_elems, _) => {
            if let Type::Tuple(c_elems, _) = concrete {
                for (p, c) in p_elems.iter().zip(c_elems.iter()) {
                    unify_type(p, c, bindings);
                }
            }
        }
        Type::InterfaceObject {
            traits: p_traits, ..
        } => {
            if let Type::InterfaceObject {
                traits: c_traits, ..
            } = concrete
            {
                for (p, c) in p_traits.iter().zip(c_traits) {
                    if p.trait_fqn == c.trait_fqn {
                        for (p_arg, c_arg) in p.trait_type_args.iter().zip(&c.trait_type_args) {
                            unify_type(p_arg, c_arg, bindings);
                        }
                    }
                }
            }
        }
        _ => {}
    }
}

pub(crate) fn apply_type_substitution(ty: &Type, bindings: &BTreeMap<TypeParamName, Type>) -> Type {
    match ty {
        Type::TypeVariable(name, _) | Type::GenericParam(name, _, _) => {
            bindings.get(name).cloned().unwrap_or_else(|| ty.clone())
        }
        Type::GenericRecord { fqn, type_args, .. } => {
            let new_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, apply_type_substitution(t, bindings)))
                .collect();
            Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: new_args,
            }
        }
        Type::GenericEnum { fqn, type_args, .. } => {
            let new_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, apply_type_substitution(t, bindings)))
                .collect();
            Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: new_args,
            }
        }
        Type::GenericNewtype {
            fqn,
            type_args,
            concrete_inner_type,
        } => Type::GenericNewtype {
            fqn: fqn.clone(),
            type_args: type_args
                .iter()
                .map(|(v, t)| (*v, apply_type_substitution(t, bindings)))
                .collect(),
            concrete_inner_type: Box::new(apply_type_substitution(concrete_inner_type, bindings)),
        },
        Type::GenericClass { fqn, type_args, .. } => {
            let new_args: Vec<(crate::common::types::Variance, Type)> = type_args
                .iter()
                .map(|(v, t)| (*v, apply_type_substitution(t, bindings)))
                .collect();
            Type::GenericClass {
                fqn: fqn.clone(),
                mangled_name: MangledName::for_type(fqn),
                type_args: new_args,
            }
        }
        Type::Array(elem) => Type::Array(Box::new(apply_type_substitution(elem, bindings))),
        Type::TupleExtend(left, right) => Type::tuple_extend(
            apply_type_substitution(left, bindings),
            apply_type_substitution(right, bindings),
        ),
        Type::AssociatedProjection(projection) => projection
            .map(|ty| apply_type_substitution(ty, bindings))
            .into_type(),
        Type::TupleProjection(receiver, kind) => {
            Type::tuple_projection(apply_type_substitution(receiver, bindings), *kind)
        }
        Type::Tuple(types, _mn) => {
            let new_types: Vec<Type> = types
                .iter()
                .map(|t| apply_type_substitution(t, bindings))
                .collect();
            let new_mn = MangledName::for_tuple(&new_types);
            Type::Tuple(new_types, new_mn)
        }
        Type::Function(params, ret) => Type::Function(
            params
                .iter()
                .map(|t| apply_type_substitution(t, bindings))
                .collect(),
            Box::new(apply_type_substitution(ret, bindings)),
        ),
        Type::InterfaceObject { traits, .. } => Type::interface_intersection(
            traits
                .iter()
                .map(|c| {
                    (
                        c.trait_fqn.clone(),
                        c.trait_type_args
                            .iter()
                            .map(|t| apply_type_substitution(t, bindings))
                            .collect(),
                    )
                })
                .collect(),
        ),
        Type::TypeConstructor { name, type_args } => Type::TypeConstructor {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|ty| apply_type_substitution(ty, bindings))
                .collect(),
        },
        _ => ty.clone(),
    }
}

pub(super) fn substitute_types_in_expr(
    expr: TypedExpr,
    sub: &BTreeMap<TypeParamName, Type>,
) -> TypedExpr {
    use crate::typechecker::types::TypedPattern;
    type SubMap = BTreeMap<TypeParamName, Type>;
    fn sub_expr(e: TypedExpr, s: &SubMap) -> TypedExpr {
        substitute_types_in_expr(e, s)
    }
    fn sub_boxed(e: Box<TypedExpr>, s: &SubMap) -> Box<TypedExpr> {
        Box::new(substitute_types_in_expr(*e, s))
    }
    fn sub_pat(p: TypedPattern, s: &SubMap) -> TypedPattern {
        substitute_types_in_pattern(p, s)
    }
    fn sub_resolved_impl(m: &ResolvedImplMethod, s: &SubMap) -> ResolvedImplMethod {
        ResolvedImplMethod {
            trait_fqn: m.trait_fqn.clone(),
            trait_type_params: m
                .trait_type_params
                .iter()
                .map(|t| apply_type_substitution(t, s))
                .collect(),
            for_type: apply_type_substitution(&m.for_type, s),
            method_name: m.method_name.clone(),
            method_type_params: m
                .method_type_params
                .iter()
                .map(|t| apply_type_substitution(t, s))
                .collect(),
        }
    }
    let ty = apply_type_substitution(&expr.ty, sub);
    let span = expr.span;
    let kind = match expr.kind {
        TypedExprKind::Block(exprs) => {
            TypedExprKind::Block(exprs.into_iter().map(|e| sub_expr(e, sub)).collect())
        }
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
            var_ty: apply_type_substitution(&var_ty, sub),
            value: sub_boxed(value, sub),
        },
        TypedExprKind::FunctionCall {
            name,
            args,
            type_params,
        } => TypedExprKind::FunctionCall {
            name,
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::IntrinsicCall { intrinsic, args } => TypedExprKind::IntrinsicCall {
            intrinsic,
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
        },
        TypedExprKind::If {
            condition,
            then_branch,
            else_branch,
        } => TypedExprKind::If {
            condition: sub_boxed(condition, sub),
            then_branch: sub_boxed(then_branch, sub),
            else_branch: else_branch.map(|e| sub_boxed(e, sub)),
        },
        TypedExprKind::While { condition, body } => TypedExprKind::While {
            condition: sub_boxed(condition, sub),
            body: sub_boxed(body, sub),
        },
        TypedExprKind::BinaryOp { op, left, right } => TypedExprKind::BinaryOp {
            op,
            left: sub_boxed(left, sub),
            right: sub_boxed(right, sub),
        },
        TypedExprKind::UnaryOp { op, operand } => TypedExprKind::UnaryOp {
            op,
            operand: sub_boxed(operand, sub),
        },
        TypedExprKind::VarRef { name, boxed } => TypedExprKind::VarRef { name, boxed },
        TypedExprKind::Assign {
            name,
            target_ty,
            boxed,
            value,
        } => TypedExprKind::Assign {
            name,
            target_ty: apply_type_substitution(&target_ty, sub),
            boxed,
            value: sub_boxed(value, sub),
        },
        TypedExprKind::Match { subject, arms } => {
            let subject = sub_boxed(subject, sub);
            let subject_ty = &subject.ty;
            // Only static type-parameter matches can be folded after substitution.
            // Dynamic subjects retain runtime tests even with concrete type arguments.
            // - If subject type == arm type → convert to Variable binding (always matches)
            // - If subject type != arm type → remove arm (unreachable)
            let mut simplified_arms = Vec::new();
            let mut found_always_match = false;
            for arm in arms {
                if found_always_match {
                    break;
                }
                let pattern = sub_pat(arm.pattern, sub);
                let guard = arm.guard.map(|g| sub_boxed(g, sub));
                let body = sub_boxed(arm.body, sub);
                match &pattern {
                    TypedPattern::TypeAnnotated { binding, ty }
                        if !subject_ty.contains_type_parameter()
                            && !subject_ty.is_any()
                            && !subject_ty.is_class_type()
                            && !matches!(
                                subject_ty,
                                Type::InterfaceObject { .. }
                                    | Type::GenericRecord { .. }
                                    | Type::GenericEnum { .. }
                            ) =>
                    {
                        if subject_ty == ty {
                            // Subject always matches this type → convert to variable binding
                            let is_unconditional = guard.is_none();
                            simplified_arms.push(crate::typechecker::types::TypedMatchArm {
                                pattern: TypedPattern::Variable(binding.clone(), ty.clone()),
                                guard,
                                body,
                                span: arm.span,
                            });
                            if is_unconditional {
                                found_always_match = true;
                            }
                        }
                        // else: subject can never match → skip arm
                    }
                    _ => {
                        if guard.is_none()
                            && matches!(
                                pattern,
                                TypedPattern::Wildcard | TypedPattern::Variable(..)
                            )
                        {
                            found_always_match = true;
                        }
                        simplified_arms.push(crate::typechecker::types::TypedMatchArm {
                            pattern,
                            guard,
                            body,
                            span: arm.span,
                        });
                    }
                }
            }
            TypedExprKind::Match {
                subject,
                arms: simplified_arms,
            }
        }
        TypedExprKind::Closure {
            params,
            body,
            captures,
        } => TypedExprKind::Closure {
            params: params
                .into_iter()
                .map(|p| crate::typechecker::types::TypedClosureParam {
                    name: p.name,
                    ty: apply_type_substitution(&p.ty, sub),
                    span: p.span,
                })
                .collect(),
            body: sub_boxed(body, sub),
            captures: captures
                .into_iter()
                .map(|mut capture| {
                    capture.ty = apply_type_substitution(&capture.ty, sub);
                    capture
                })
                .collect(),
        },
        TypedExprKind::ClosureCall { callee, args } => TypedExprKind::ClosureCall {
            callee: sub_boxed(callee, sub),
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
        },
        TypedExprKind::Return { value, return_type } => TypedExprKind::Return {
            value: sub_boxed(value, sub),
            return_type: apply_type_substitution(&return_type, sub),
        },
        TypedExprKind::ImplFunctionCall {
            trait_fqn,
            trait_type_params,
            for_type,
            method_name,
            args,
            method_type_params,
        } => {
            // Only calls the template resolved AGAINST `Self` (its for_type is
            // the `Self` type variable pre-substitution) retarget to the
            // provider block — a call on some OTHER value whose concrete type
            // merely equals the substituted Self (e.g. an explicit
            // `Alpha.alpha(localRec)` disambiguation in a default body) keeps
            // the resolution typecheck fixed.
            let receiver_was_self = matches!(&for_type, Type::TypeVariable(n, _) if n.0 == "Self");
            let for_type = apply_type_substitution(&for_type, sub);
            let mut method_name = method_name;
            let mut trait_fqn = trait_fqn;
            let mut trait_type_params: Vec<Type> = trait_type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect();
            DEFAULT_RETARGET.with(|slot| {
                if let Some(retarget) = slot.borrow().as_ref()
                    && receiver_was_self
                    && trait_fqn != retarget.to_trait
                    && retarget.from_traits.contains(&trait_fqn)
                    && for_type == retarget.self_ty
                {
                    method_name = crate::typechecker::associated_types::with_registry(|registry| {
                        registry.route_trait_method(
                            &retarget.to_trait,
                            &retarget.to_trait_args,
                            &trait_fqn,
                            &trait_type_params,
                            &method_name,
                        )
                    })
                    .unwrap_or_else(|| method_name.clone());
                    trait_fqn = retarget.to_trait.clone();
                    trait_type_params = retarget.to_trait_args.clone();
                }
            });
            let args: Vec<TypedExpr> = args.into_iter().map(|a| sub_expr(a, sub)).collect();
            // Class default materialization: a call on this class's own value
            // becomes a virtual call so subclass overrides win.
            let virtual_slot = CLASS_VIRTUALIZE.with(|slot| {
                let borrowed = slot.borrow();
                let ctx = borrowed.as_ref()?;
                if for_type != ctx.class_type || trait_fqn != ctx.trait_fqn {
                    return None;
                }
                if !args.first().is_some_and(|a| a.ty == ctx.class_type) {
                    return None;
                }
                ctx.vtable_slots
                    .iter()
                    .find(|slot| slot.method_name == method_name && slot.arity == args.len())
                    .map(|slot| slot.index)
            });
            if let Some(vtable_slot) = virtual_slot {
                return TypedExpr {
                    kind: TypedExprKind::ClassVirtualCall {
                        object: Box::new(args[0].clone()),
                        vtable_slot,
                        args,
                    },
                    ty,
                    span,
                };
            }
            TypedExprKind::ImplFunctionCall {
                trait_fqn,
                trait_type_params,
                for_type,
                method_name,
                args,
                method_type_params: method_type_params
                    .into_iter()
                    .map(|t| apply_type_substitution(&t, sub))
                    .collect(),
            }
        }
        TypedExprKind::RecordCreate {
            fqn,
            fields,
            type_params,
        } => TypedExprKind::RecordCreate {
            fqn,
            fields: fields
                .into_iter()
                .map(|(n, e)| (n, sub_expr(e, sub)))
                .collect(),
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::TupleLiteral { elements } => TypedExprKind::TupleLiteral {
            elements: elements.into_iter().map(|e| sub_expr(e, sub)).collect(),
        },
        TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => TypedExprKind::EnumCreate {
            fqn,
            variant_name,
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::FieldAccess {
            object,
            field_name,
            field_index,
            boxed,
        } => TypedExprKind::FieldAccess {
            object: sub_boxed(object, sub),
            field_name,
            field_index,
            boxed,
        },
        TypedExprKind::Panic { message } => TypedExprKind::Panic {
            message: sub_boxed(message, sub),
        },
        TypedExprKind::Assert { condition, message } => TypedExprKind::Assert {
            condition: sub_boxed(condition, sub),
            message: message.map(|m| sub_boxed(m, sub)),
        },
        TypedExprKind::LetDestructure {
            pattern,
            value,
            var_ty,
        } => TypedExprKind::LetDestructure {
            pattern: sub_pat(pattern, sub),
            value: sub_boxed(value, sub),
            var_ty: apply_type_substitution(&var_ty, sub),
        },
        TypedExprKind::FieldAssign {
            object,
            field_name,
            field_index,
            value,
            boxed,
        } => TypedExprKind::FieldAssign {
            object: sub_boxed(object, sub),
            field_name,
            field_index,
            boxed,
            value: sub_boxed(value, sub),
        },
        TypedExprKind::RecordWith {
            object,
            fqn,
            overrides,
            type_params,
        } => TypedExprKind::RecordWith {
            object: sub_boxed(object, sub),
            fqn,
            overrides: overrides
                .into_iter()
                .map(|(n, idx, e)| (n, idx, sub_expr(e, sub)))
                .collect(),
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::ArrayLiteral { elements } => TypedExprKind::ArrayLiteral {
            elements: elements.into_iter().map(|e| sub_expr(e, sub)).collect(),
        },
        TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            args,
            type_params,
        } => TypedExprKind::EnumVariantRecordCreate {
            fqn,
            variant_name,
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::InterfaceObjectCoerce {
            inner,
            interface_mangled_name,
            concrete_type,
            vtable_methods,
        } => {
            let new_concrete_type = apply_type_substitution(&concrete_type, sub);
            // Substitute type args in each component's vtable entries; the set key
            // is re-derived from the substituted expression type below.
            let new_vtable_methods: Vec<VtableMethodGroup> = vtable_methods
                .into_iter()
                .map(|(component_mn, entries)| {
                    let new_entries = entries
                        .into_iter()
                        .map(|(member, impl_mn, type_args)| {
                            let new_type_args: Vec<Type> = type_args
                                .iter()
                                .map(|t| apply_type_substitution(t, sub))
                                .collect();
                            (member, impl_mn, new_type_args)
                        })
                        .collect();
                    (component_mn, new_entries)
                })
                .collect();
            // Recompute interface_mangled_name from the new expression type
            let new_trait_mn = if let Type::InterfaceObject { mangled_name, .. } = &ty {
                mangled_name.clone()
            } else {
                interface_mangled_name
            };
            TypedExprKind::InterfaceObjectCoerce {
                inner: sub_boxed(inner, sub),
                interface_mangled_name: new_trait_mn,
                concrete_type: new_concrete_type,
                vtable_methods: new_vtable_methods,
            }
        }
        TypedExprKind::TemplateInterfaceObjectCoerce {
            inner,
            traits,
            concrete_type,
        } => TypedExprKind::TemplateInterfaceObjectCoerce {
            inner: sub_boxed(inner, sub),
            traits: traits
                .into_iter()
                .map(|(fqn, args)| {
                    (
                        fqn,
                        args.into_iter()
                            .map(|t| apply_type_substitution(&t, sub))
                            .collect(),
                    )
                })
                .collect(),
            concrete_type: apply_type_substitution(&concrete_type, sub),
        },
        TypedExprKind::InterfaceObjectUpcast { inner } => TypedExprKind::InterfaceObjectUpcast {
            inner: sub_boxed(inner, sub),
        },
        TypedExprKind::InterfaceObjectMethodCall {
            interface_mangled_name,
            method_name,
            member_name,
            receiver,
            args,
        } => {
            // Recompute interface_mangled_name from the substituted receiver type —
            // but ONLY when the node key tracked the receiver's set key before
            // substitution (the plain single-interface case). For an
            // intersection receiver the node key is the declaring COMPONENT's
            // per-trait key, and with `extends` an inherited member's node key
            // is the ORIGIN trait's key — in both cases the set key here would
            // corrupt slot lookup.
            let receiver_key_before = match &receiver.ty {
                Type::InterfaceObject {
                    traits,
                    mangled_name,
                } if traits.len() == 1 => Some(mangled_name.clone()),
                _ => None,
            };
            let new_receiver = sub_boxed(receiver, sub);
            let new_trait_mn = match (&new_receiver.ty, receiver_key_before) {
                (
                    Type::InterfaceObject {
                        traits,
                        mangled_name,
                    },
                    Some(old_key),
                ) if traits.len() == 1 && old_key == interface_mangled_name => mangled_name.clone(),
                _ => interface_mangled_name,
            };
            TypedExprKind::InterfaceObjectMethodCall {
                interface_mangled_name: new_trait_mn,
                method_name,
                member_name,
                receiver: new_receiver,
                args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
            }
        }
        TypedExprKind::MethodRef {
            object,
            method_name,
            type_params,
        } => TypedExprKind::MethodRef {
            object: sub_boxed(object, sub),
            method_name,
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::ClassNew {
            mangled_name,
            args,
            type_params,
        } => {
            let new_type_params: Vec<Type> = type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect();
            // Recompute mangled_name from the substituted expression type
            let new_mangled = match &ty {
                Type::GenericClass {
                    mangled_name: mn, ..
                } => mn.clone(),
                Type::Class(_, mn) => mn.clone(),
                _ => mangled_name,
            };
            TypedExprKind::ClassNew {
                mangled_name: new_mangled,
                args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
                type_params: new_type_params,
            }
        }
        TypedExprKind::ClassStructCreate {
            target_mangled_name,
            fields,
            type_params,
        } => TypedExprKind::ClassStructCreate {
            target_mangled_name,
            type_params: type_params
                .iter()
                .map(|t| apply_type_substitution(t, sub))
                .collect(),
            fields: fields.into_iter().map(|a| sub_expr(a, sub)).collect(),
        },
        TypedExprKind::ClassVirtualCall {
            object,
            vtable_slot,
            args,
        } => TypedExprKind::ClassVirtualCall {
            object: sub_boxed(object, sub),
            vtable_slot,
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
        },
        TypedExprKind::ClassSuperCall {
            method_mangled,
            args,
        } => TypedExprKind::ClassSuperCall {
            method_mangled,
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
        },
        TypedExprKind::NewtypeCreate { value } => TypedExprKind::NewtypeCreate {
            value: sub_boxed(value, sub),
        },
        TypedExprKind::NewtypeValue { value } => TypedExprKind::NewtypeValue {
            value: sub_boxed(value, sub),
        },
        TypedExprKind::TypeCast { value, target_type } => TypedExprKind::TypeCast {
            value: sub_boxed(value, sub),
            target_type: apply_type_substitution(&target_type, sub),
        },
        TypedExprKind::TypeTest { value, target_type } => TypedExprKind::TypeTest {
            value: sub_boxed(value, sub),
            target_type: apply_type_substitution(&target_type, sub),
        },
        TypedExprKind::BoxToAny { inner } => TypedExprKind::BoxToAny {
            inner: sub_boxed(inner, sub),
        },
        TypedExprKind::GlobalRef { name, type_params } => {
            // Globals (including those on generic classes/modules) have canonical storage —
            // the name is independent of the type_params. Substitute the type_params for
            // diagnostic/codegen accuracy, but leave the name alone.
            let new_type_params: Vec<Type> = type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect();
            TypedExprKind::GlobalRef {
                name,
                type_params: new_type_params,
            }
        }
        TypedExprKind::GlobalAssign {
            name,
            type_params,
            value,
        } => {
            let new_type_params: Vec<Type> = type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect();
            TypedExprKind::GlobalAssign {
                name,
                type_params: new_type_params,
                value: sub_boxed(value, sub),
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
            trait_type_params: trait_type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
            for_type: apply_type_substitution(&for_type, sub),
            method_name,
            method_type_params: method_type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type,
            method_name,
            args,
            type_params,
        } => TypedExprKind::ExtFunctionCall {
            ext_fqn,
            for_type: apply_type_substitution(&for_type, sub),
            method_name,
            args: args.into_iter().map(|a| sub_expr(a, sub)).collect(),
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::ExtFunctionRef {
            ext_fqn,
            for_type,
            method_name,
            type_params,
        } => TypedExprKind::ExtFunctionRef {
            ext_fqn,
            for_type: apply_type_substitution(&for_type, sub),
            method_name,
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::FunctionRef { name, type_params } => TypedExprKind::FunctionRef {
            name,
            type_params: type_params
                .into_iter()
                .map(|t| apply_type_substitution(&t, sub))
                .collect(),
        },
        TypedExprKind::AsyncBlock {
            body,
            succeed_method,
        } => TypedExprKind::AsyncBlock {
            body: sub_boxed(body, sub),
            succeed_method: sub_resolved_impl(&succeed_method, sub),
        },
        TypedExprKind::ForLoop {
            pattern,
            iterable,
            iterator_method,
            iterator_type,
            element_type,
            body,
        } => TypedExprKind::ForLoop {
            pattern: sub_pat(pattern, sub),
            iterable: sub_boxed(iterable, sub),
            iterator_method: sub_resolved_impl(&iterator_method, sub),
            iterator_type: apply_type_substitution(&iterator_type, sub),
            element_type: apply_type_substitution(&element_type, sub),
            body: sub_boxed(body, sub),
        },
        TypedExprKind::Try {
            operand,
            unwrap_method,
            unwrap_return_type,
            return_type,
            from_method,
        } => TypedExprKind::Try {
            operand: sub_boxed(operand, sub),
            unwrap_method: sub_resolved_impl(&unwrap_method, sub),
            unwrap_return_type: apply_type_substitution(&unwrap_return_type, sub),
            return_type: apply_type_substitution(&return_type, sub),
            from_method: from_method.as_ref().map(|m| sub_resolved_impl(m, sub)),
        },
        TypedExprKind::Await {
            operand,
            return_type,
            and_then_method,
            map_method,
            source_location_mn,
        } => TypedExprKind::Await {
            operand: sub_boxed(operand, sub),
            return_type: apply_type_substitution(&return_type, sub),
            and_then_method: sub_resolved_impl(&and_then_method, sub),
            map_method: sub_resolved_impl(&map_method, sub),
            source_location_mn,
        },
        other => other,
    };
    crate::typechecker::tuple_extension::lower(TypedExpr { kind, ty, span })
}

pub(super) fn substitute_types_in_pattern(
    pattern: crate::typechecker::types::TypedPattern,
    sub: &BTreeMap<TypeParamName, Type>,
) -> crate::typechecker::types::TypedPattern {
    use crate::typechecker::types::{TypedFieldPattern, TypedPattern};
    match pattern {
        TypedPattern::Wildcard => TypedPattern::Wildcard,
        TypedPattern::Variable(name, ty) => {
            TypedPattern::Variable(name, apply_type_substitution(&ty, sub))
        }
        TypedPattern::Literal(lit) => TypedPattern::Literal(lit),
        TypedPattern::TypeAnnotated { binding, ty } => TypedPattern::TypeAnnotated {
            binding,
            ty: apply_type_substitution(&ty, sub),
        },
        TypedPattern::EnumVariant {
            enum_type,
            variant_name,
            variant_index,
            payload_patterns,
        } => TypedPattern::EnumVariant {
            enum_type: apply_type_substitution(&enum_type, sub),
            variant_name,
            variant_index,
            payload_patterns: payload_patterns
                .into_iter()
                .map(|p| substitute_types_in_pattern(p, sub))
                .collect(),
        },
        TypedPattern::EnumVariantRecord {
            enum_type,
            variant_name,
            variant_index,
            field_patterns,
        } => TypedPattern::EnumVariantRecord {
            enum_type: apply_type_substitution(&enum_type, sub),
            variant_name,
            variant_index,
            field_patterns: field_patterns
                .into_iter()
                .map(|fp| TypedFieldPattern {
                    field_name: fp.field_name,
                    field_index: fp.field_index,
                    pattern: substitute_types_in_pattern(fp.pattern, sub),
                })
                .collect(),
        },
        TypedPattern::Record { ty, fields } => TypedPattern::Record {
            ty: apply_type_substitution(&ty, sub),
            fields: fields
                .into_iter()
                .map(|fp| TypedFieldPattern {
                    field_name: fp.field_name,
                    field_index: fp.field_index,
                    pattern: substitute_types_in_pattern(fp.pattern, sub),
                })
                .collect(),
        },
        TypedPattern::Tuple {
            element_patterns,
            tuple_type,
        } => TypedPattern::Tuple {
            element_patterns: element_patterns
                .into_iter()
                .map(|p| substitute_types_in_pattern(p, sub))
                .collect(),
            tuple_type: apply_type_substitution(&tuple_type, sub),
        },
        TypedPattern::Newtype {
            inner_pattern,
            newtype_ty,
        } => TypedPattern::Newtype {
            inner_pattern: Box::new(substitute_types_in_pattern(*inner_pattern, sub)),
            newtype_ty: apply_type_substitution(&newtype_ty, sub),
        },
    }
}

/// Create a concrete TypedFunction from a generic template by substituting type parameters.
pub(super) fn substitute_types_in_function(
    template: &TypedFunction,
    sub: &BTreeMap<TypeParamName, Type>,
    name: MangledName,
) -> TypedFunction {
    let params = template
        .params
        .iter()
        .map(|p| TypedParam {
            name: p.name.clone(),
            ty: apply_type_substitution(&p.ty, sub),
            span: p.span.clone(),
        })
        .collect();
    let return_type = apply_type_substitution(&template.return_type, sub);
    let body = substitute_types_in_expr(template.body.clone(), sub);
    let vtable_self_type = template
        .vtable_self_type
        .as_ref()
        .map(|t| apply_type_substitution(t, sub));
    TypedFunction {
        visibility: template.visibility,
        name,
        type_params: vec![], // concrete — no type params
        params,
        return_type,
        body,
        span: template.span.clone(),
        vtable_self_type,
        is_async: template.is_async,
        source_name: template.source_name.clone(),
        display_name: template.display_name.clone(),
    }
}
