//! Preserve written evaluation order while lowering labelled calls to positional calls.
use crate::common::types::VarName;
use crate::typechecker::types::{Type, TypedExpr, TypedExprKind};

pub(crate) fn arguments(call: &TypedExpr) -> Option<&[TypedExpr]> {
    match &call.kind {
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassVirtualCall { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. }
        | TypedExprKind::InterfaceObjectMethodCall { args, .. } => Some(args),
        _ => None,
    }
}

/// Interface calls keep their receiver separately, including when the source
/// uses the explicit `Interface.method(receiver, ...)` spelling.
pub(crate) fn explicit_arguments(call: &TypedExpr, count: usize) -> Option<Vec<&TypedExpr>> {
    let args = arguments(call)?;
    if args.len() >= count {
        return Some(args[args.len() - count..].iter().collect());
    }
    match &call.kind {
        TypedExprKind::InterfaceObjectMethodCall { receiver, .. } if count == args.len() + 1 => {
            Some(std::iter::once(receiver.as_ref()).chain(args).collect())
        }
        _ => None,
    }
}

fn operand_order(call: &TypedExpr, order: &[usize]) -> Vec<usize> {
    if matches!(call.kind, TypedExprKind::InterfaceObjectMethodCall { .. })
        && arguments(call).is_some_and(|args| args.len() + 1 == order.len())
    {
        debug_assert_eq!(order.first(), Some(&0));
        order[1..].iter().map(|slot| slot - 1).collect()
    } else {
        order.to_vec()
    }
}

pub(crate) fn arguments_mut(call: &mut TypedExpr) -> Option<&mut Vec<TypedExpr>> {
    match &mut call.kind {
        TypedExprKind::FunctionCall { args, .. }
        | TypedExprKind::IntrinsicCall { args, .. }
        | TypedExprKind::ClassNew { args, .. }
        | TypedExprKind::ClassVirtualCall { args, .. }
        | TypedExprKind::ClassSuperCall { args, .. }
        | TypedExprKind::ImplFunctionCall { args, .. }
        | TypedExprKind::ExtFunctionCall { args, .. }
        | TypedExprKind::InterfaceObjectMethodCall { args, .. } => Some(args),
        _ => None,
    }
}

/// Called after lazy arguments have been wrapped, before capture analysis.
fn lower(mut call: TypedExpr, order: &[usize]) -> TypedExpr {
    let order = operand_order(&call, order);
    let mut bindings = Vec::new();
    let span = call.span.clone();
    // Virtual/interface receivers live outside the explicit argument vector.
    match &mut call.kind {
        TypedExprKind::ClassVirtualCall { object, args, .. } => {
            materialize(object, &mut bindings, "receiver");
            if !args.is_empty() {
                args[0] = (**object).clone();
            }
        }
        TypedExprKind::InterfaceObjectMethodCall { receiver, .. } => {
            materialize(receiver, &mut bindings, "receiver");
        }
        _ => {}
    }
    let virtual_receiver = matches!(call.kind, TypedExprKind::ClassVirtualCall { .. });
    if let Some(args) = arguments_mut(&mut call) {
        let offset = args.len() - order.len();
        if !virtual_receiver {
            for argument in &mut args[..offset] {
                materialize(argument, &mut bindings, "receiver");
            }
        }
        for &parameter in &order {
            materialize(&mut args[offset + parameter], &mut bindings, "argument");
        }
    }
    let ty = call.ty.clone();
    bindings.push(call);
    TypedExpr {
        kind: TypedExprKind::Block(bindings),
        ty,
        span,
    }
}

/// Wrap deferred arguments before creating their written-order bindings.
/// This also runs before await extraction so eager prefixes stay before awaits.
pub(crate) fn lower_with_parameter_types(
    mut call: TypedExpr,
    order: &[usize],
    parameter_types: &[Type],
) -> TypedExpr {
    if let Some(args) = arguments_mut(&mut call) {
        let explicit_count = args.len().min(parameter_types.len());
        let argument_offset = args.len() - explicit_count;
        let parameter_offset = parameter_types.len() - explicit_count;
        for (argument, expected) in args[argument_offset..]
            .iter_mut()
            .zip(&parameter_types[parameter_offset..])
        {
            *argument = super::coerce_byname::coerce_argument(argument.clone(), expected);
        }
    }
    lower(call, order)
}

fn materialize(value: &mut TypedExpr, bindings: &mut Vec<TypedExpr>, role: &str) {
    let span = value.span.clone();
    let ty = value.ty.clone();
    let name = VarName(format!(
        "$named${role}${}${}${}",
        span.line,
        span.column,
        bindings.len()
    ));
    let reference = TypedExpr {
        kind: TypedExprKind::VarRef {
            name: name.clone(),
            boxed: false,
        },
        ty: ty.clone(),
        span: span.clone(),
    };
    let original = std::mem::replace(value, reference);
    bindings.push(TypedExpr {
        kind: TypedExprKind::Let {
            name,
            mutable: false,
            boxed: false,
            var_ty: ty,
            value: Box::new(original),
        },
        ty: Type::Unit,
        span,
    });
}
