//! Lower a shape-resolved extension to ordinary tuple operations.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::types::{IntrinsicKind, TupleProjection, Type, TypedExpr, TypedExprKind};
use crate::common::span::Span;
use crate::common::types::VarName;
use crate::parser::ast::BinOp;

static TEMPORARY_COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Generic templates retain the binary node until substitution reveals its shape.
pub(crate) fn lower(expr: TypedExpr) -> TypedExpr {
    if matches!(
        &expr.kind,
        TypedExprKind::IntrinsicCall {
            intrinsic: IntrinsicKind::TupleProjection(_),
            ..
        }
    ) {
        return lower_projection(expr);
    }
    let TypedExprKind::BinaryOp {
        op: BinOp::TupleExtend,
        left,
        right,
    } = &expr.kind
    else {
        return expr;
    };
    let ty = Type::tuple_extend(left.ty.clone(), right.ty.clone());
    if !matches!(ty, Type::Tuple(..)) {
        return expr;
    }
    let TypedExprKind::BinaryOp { left, right, .. } = expr.kind else {
        unreachable!()
    };
    let span = expr.span;
    let Type::Tuple(element_types, _) = &left.ty else {
        return TypedExpr {
            kind: TypedExprKind::TupleLiteral {
                elements: vec![*left, *right],
            },
            ty,
            span,
        };
    };
    let element_types = element_types.clone();
    let (binding, receiver) = bind_receiver(*left, &span);
    let mut elements = tuple_fields(&receiver, &element_types);
    elements.push(*right);
    let tuple = TypedExpr {
        kind: TypedExprKind::TupleLiteral { elements },
        ty: ty.clone(),
        span: span.clone(),
    };
    TypedExpr {
        kind: TypedExprKind::Block(vec![binding, tuple]),
        ty,
        span,
    }
}

fn tuple_fields(receiver: &TypedExpr, element_types: &[Type]) -> Vec<TypedExpr> {
    element_types
        .iter()
        .enumerate()
        .map(|(index, ty)| TypedExpr {
            kind: TypedExprKind::FieldAccess {
                object: Box::new(receiver.clone()),
                field_name: format!("_{index}"),
                field_index: index as u32,
                boxed: false,
            },
            ty: ty.clone(),
            span: receiver.span.clone(),
        })
        .collect()
}

fn bind_receiver(value: TypedExpr, span: &Span) -> (TypedExpr, TypedExpr) {
    let name = VarName(format!(
        "$tupleExtension{}",
        TEMPORARY_COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    let receiver = TypedExpr {
        kind: TypedExprKind::VarRef {
            name: name.clone(),
            boxed: false,
        },
        ty: value.ty.clone(),
        span: span.clone(),
    };
    let binding = TypedExpr {
        kind: TypedExprKind::Let {
            name,
            mutable: false,
            boxed: false,
            var_ty: value.ty.clone(),
            value: Box::new(value),
        },
        ty: Type::Unit,
        span: span.clone(),
    };
    (binding, receiver)
}

fn lower_projection(expr: TypedExpr) -> TypedExpr {
    let TypedExprKind::IntrinsicCall {
        intrinsic: IntrinsicKind::TupleProjection(kind),
        args,
    } = &expr.kind
    else {
        return expr;
    };
    let Some(receiver) = args.first() else {
        return expr;
    };
    let Type::Tuple(elements, _) = &receiver.ty else {
        return expr;
    };
    let ty = Type::tuple_projection(receiver.ty.clone(), *kind);
    if *kind == TupleProjection::Last || elements.len() == 2 {
        let index = if *kind == TupleProjection::Last {
            elements.len() - 1
        } else {
            0
        };
        return TypedExpr {
            kind: TypedExprKind::FieldAccess {
                object: Box::new(receiver.clone()),
                field_name: format!("_{index}"),
                field_index: index as u32,
                boxed: false,
            },
            ty,
            span: expr.span,
        };
    }
    let prefix = elements[..elements.len() - 1].to_vec();
    let (binding, receiver) = bind_receiver(receiver.clone(), &expr.span);
    let tuple = TypedExpr {
        kind: TypedExprKind::TupleLiteral {
            elements: tuple_fields(&receiver, &prefix),
        },
        ty: ty.clone(),
        span: expr.span.clone(),
    };
    TypedExpr {
        kind: TypedExprKind::Block(vec![binding, tuple]),
        ty,
        span: expr.span,
    }
}
