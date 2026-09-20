//! Type spelling for declaration views. Resolved nominal names stay qualified.
use crate::common::span::Spanned;
use crate::common::types::Variance;
use crate::parser::ast::{self, TypeExpr};
use crate::typechecker::types::Type;

pub(super) fn arguments(items: impl IntoIterator<Item = String>) -> String {
    let items: Vec<_> = items.into_iter().collect();
    if items.is_empty() {
        String::new()
    } else {
        format!("<{}>", items.join(", "))
    }
}

pub(super) fn parameters(items: &[Spanned<String>]) -> String {
    arguments(items.iter().map(|p| p.value.clone()))
}

pub(super) fn variant_parameters(items: &[ast::VariantTypeParam]) -> String {
    arguments(items.iter().map(|p| {
        format!(
            "{}{}",
            match p.variance {
                Variance::Invariant => "",
                Variance::Covariant => "out ",
                Variance::Contravariant => "in ",
            },
            p.name.value
        )
    }))
}

fn named(ty: &ast::NamedType) -> String {
    format!(
        "{}{}",
        ty.name.value,
        arguments(ty.type_args.iter().map(source_type))
    )
}

pub(super) fn source_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named(ty) => named(ty),
        TypeExpr::Tuple(items, _) => format!(
            "({})",
            items.iter().map(source_type).collect::<Vec<_>>().join(", ")
        ),
        TypeExpr::TupleExtend(a, b, _) => format!("({} ~ {})", source_type(a), source_type(b)),
        TypeExpr::Intersection(items) => items.iter().map(named).collect::<Vec<_>>().join(" and "),
        TypeExpr::Function(params, result, _) => format!(
            "({}) => {}",
            params
                .iter()
                .map(source_type)
                .collect::<Vec<_>>()
                .join(", "),
            source_type(result)
        ),
    }
}

pub(super) fn bounds(items: &[ast::TraitConstraint]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let constraints = items
        .iter()
        .map(|c| {
            let bounds = c
                .trait_bounds
                .iter()
                .map(|b| match b {
                    ast::TypeBound::Class(_) => "class".to_owned(),
                    ast::TypeBound::Named(b) => {
                        format!(
                            "{}{}",
                            b.name.value,
                            arguments(b.type_args.iter().map(source_type).chain(
                                b.associated_types.iter().map(|(name, ty)| format!(
                                    "{} = {}",
                                    name.value,
                                    source_type(ty)
                                ))
                            ))
                        )
                    }
                })
                .collect::<Vec<_>>()
                .join(" + ");
            format!("{}: {bounds}", c.type_param.value)
        })
        .collect::<Vec<_>>()
        .join(", ");
    format!(" where {constraints}")
}

pub(super) fn params(items: &[ast::Param]) -> String {
    items
        .iter()
        .map(|p| format!("{}: {}", p.name.value, source_type(&p.type_annotation)))
        .collect::<Vec<_>>()
        .join(", ")
}

pub(super) fn resolved_type(ty: &Type) -> String {
    match ty {
        Type::Record(fqn, _) | Type::Enum(fqn, _) | Type::Class(fqn, _) | Type::Newtype(fqn, _) => {
            fqn.to_string()
        }
        Type::GenericRecord { fqn, type_args, .. }
        | Type::GenericEnum { fqn, type_args, .. }
        | Type::GenericClass { fqn, type_args, .. }
        | Type::GenericNewtype { fqn, type_args, .. } => format!(
            "{fqn}{}",
            arguments(type_args.iter().map(|(_, t)| resolved_type(t)))
        ),
        Type::Array(t) => format!("Array<{}>", resolved_type(t)),
        Type::Tuple(items, _) => format!(
            "({})",
            items
                .iter()
                .map(resolved_type)
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Type::TupleExtend(a, b) => format!("({} ~ {})", resolved_type(a), resolved_type(b)),
        Type::Function(params, result) => format!(
            "({}) => {}",
            params
                .iter()
                .map(resolved_type)
                .collect::<Vec<_>>()
                .join(", "),
            resolved_type(result)
        ),
        Type::InterfaceObject { traits, .. } => traits
            .iter()
            .map(|t| {
                format!(
                    "{}{}",
                    t.trait_fqn,
                    arguments(t.trait_type_args.iter().map(resolved_type))
                )
            })
            .collect::<Vec<_>>()
            .join(" and "),
        Type::TypeConstructor { name, type_args } => {
            format!("{name}{}", arguments(type_args.iter().map(resolved_type)))
        }
        Type::AssociatedProjection(p) => format!(
            "{}.{}{}",
            resolved_type(&p.receiver),
            p.member,
            arguments(p.parameters.iter().map(resolved_type))
        ),
        Type::TupleProjection(receiver, kind) => format!("{}.{kind:?}", resolved_type(receiver)),
        Type::Error => "/* unresolved */".to_owned(),
        Type::Unit
        | Type::Bool
        | Type::String
        | Type::Char
        | Type::Int8
        | Type::Int16
        | Type::Int32
        | Type::Int64
        | Type::Uint8
        | Type::Uint16
        | Type::Uint32
        | Type::Uint64
        | Type::Uint128
        | Type::Float32
        | Type::Float64
        | Type::Never
        | Type::Any
        | Type::TypeVariable(..)
        | Type::GenericParam(..)
        | Type::SelfType => ty.to_string(),
    }
}

pub(super) fn resolved_bounds(bounds: &crate::typechecker::types::TraitBounds) -> String {
    use crate::typechecker::types::TraitBound;
    bounds
        .iter()
        .map(|(parameter, constraints)| {
            let constraints = constraints
                .iter()
                .map(|constraint| match constraint {
                    TraitBound::IsClass => "class".to_owned(),
                    TraitBound::Named(bound) => format!(
                        "{}{}",
                        bound.trait_fqn,
                        arguments(
                            bound.type_args.iter().map(resolved_type).chain(
                                bound
                                    .associated_types
                                    .iter()
                                    .map(|(name, ty)| format!("{name} = {}", resolved_type(ty)))
                            )
                        ),
                    ),
                })
                .collect::<Vec<_>>()
                .join(" + ");
            format!("{parameter}: {constraints}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}
