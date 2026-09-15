//! Match exhaustiveness checking.
//!
//! Builds a pattern matrix from the unguarded arms and asks whether the
//! all-wildcard row is still useful (Maranget). Unlike a variant-name tally
//! this sees *through* payloads, so `case [] / case [a, b]` is correctly
//! rejected — `[x]` escapes both.
//!
//! When metadata is missing the check bails silently: a failed lookup must
//! never surface as a false "non-exhaustive".

mod ctor;
mod class_regions;
mod matrix;
mod witness;

use std::collections::BTreeMap;

use crate::common::diagnostics::Diagnostics;
use crate::common::types::MangledName;
use crate::typechecker::registry::Registry;
use crate::typechecker::types::{
    Type, TypeDef, TypedExpr, TypedExprKind, TypedMatchArm, TypedModule,
};

use super::Rule;
use super::visitor::{self, TypedExprVisitor};

use ctor::PatCx;
use matrix::{Matrix, missing_witnesses};

pub(super) struct ExhaustivenessRule<'a> {
    types: &'a BTreeMap<MangledName, TypeDef>,
    registry: &'a Registry,
}

impl<'a> ExhaustivenessRule<'a> {
    pub(super) fn new(types: &'a BTreeMap<MangledName, TypeDef>, registry: &'a Registry) -> Self {
        Self { types, registry }
    }
}

impl Rule for ExhaustivenessRule<'_> {
    fn check(&mut self, typed_module: &TypedModule, diagnostics: &mut Diagnostics) {
        visitor::visit_all_functions(self, typed_module, diagnostics);
    }
}

impl TypedExprVisitor for ExhaustivenessRule<'_> {
    fn visit_match(&mut self, expr: &TypedExpr, diagnostics: &mut Diagnostics) {
        // Depth-first, so inner matches report before the outer one.
        visitor::walk_match(self, expr, diagnostics);
        if let TypedExprKind::Match { subject, arms } = &expr.kind {
            if !subject.ty.is_error() {
                if let Some(msg) =
                    check_match_exhaustiveness(arms, &subject.ty, self.types, self.registry)
                {
                    diagnostics.error(expr.span.clone(), msg);
                }
            }
        }
    }
}

/// Returns an error message when the match is not exhaustive, else `None`.
fn check_match_exhaustiveness(
    arms: &[TypedMatchArm],
    scrutinee_ty: &Type,
    types: &BTreeMap<MangledName, TypeDef>,
    registry: &Registry,
) -> Option<String> {
    let cx = PatCx::new(types, registry);
    let needs_reified_proof = arms.iter().any(|a| reified_pattern(&a.pattern));

    // A guarded arm proves nothing about coverage.
    let mut rows: Matrix = Vec::new();
    for arm in arms.iter().filter(|a| a.guard.is_none()) {
        match cx.lower(&arm.pattern, scrutinee_ty) {
            Ok(p) => rows.push(vec![p]),
            Err(_) => return needs_reified_proof.then(|| "cannot prove generic match coverage; add a fallback case".to_string()),
        }
    }

    let witnesses = match missing_witnesses(&cx, &rows, std::slice::from_ref(scrutinee_ty), 0) {
        Ok(witnesses) => witnesses,
        Err(_) => return needs_reified_proof.then(|| "cannot prove generic match coverage; add a fallback case".to_string()),
    };
    if witnesses.is_empty() {
        return None;
    }

    let rendered: Vec<String> = witnesses
        .iter()
        .filter_map(|w| w.first())
        .map(|w| witness::render(&cx, w, scrutinee_ty))
        .collect();
    if rendered.is_empty() {
        return None;
    }

    Some(format!(
        "non-exhaustive match: missing case for {}{}",
        rendered.join(", "),
        if needs_reified_proof { "; add the remaining cases or a fallback case" } else { "" }
    ))
}

fn reified_pattern(pattern: &crate::typechecker::types::TypedPattern) -> bool {
    use crate::typechecker::types::TypedPattern as P;
    match pattern {
        P::TypeAnnotated { ty: Type::GenericClass { .. } | Type::GenericRecord { .. } | Type::GenericEnum { .. }, .. } => true,
        P::Tuple { element_patterns, .. } | P::EnumVariant { payload_patterns: element_patterns, .. } => element_patterns.iter().any(reified_pattern),
        P::Record { fields, .. } | P::EnumVariantRecord { field_patterns: fields, .. } => fields.iter().any(|f|reified_pattern(&f.pattern)),
        P::Newtype { inner_pattern, .. } => reified_pattern(inner_pattern),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::Span;
    use crate::common::types::{Fqn, MangledName, VarName};
    use crate::typechecker::types::{TypedExpr, TypedExprKind, TypedPattern};

    fn span() -> Span {
        Span::new("test".into(), 1, 1, 1, 1)
    }

    fn body() -> Box<TypedExpr> {
        Box::new(TypedExpr {
            kind: TypedExprKind::UnitLiteral,
            ty: Type::Unit,
            span: span(),
        })
    }

    fn arm(pattern: TypedPattern) -> TypedMatchArm {
        TypedMatchArm {
            pattern,
            guard: None,
            body: body(),
            span: span(),
        }
    }

    fn guarded(pattern: TypedPattern) -> TypedMatchArm {
        TypedMatchArm {
            pattern,
            guard: Some(Box::new(TypedExpr {
                kind: TypedExprKind::BoolLiteral(true),
                ty: Type::Bool,
                span: span(),
            })),
            body: body(),
            span: span(),
        }
    }

    fn bool_pat(v: bool) -> TypedPattern {
        TypedPattern::Literal(Box::new(TypedExpr {
            kind: TypedExprKind::BoolLiteral(v),
            ty: Type::Bool,
            span: span(),
        }))
    }

    fn int_pat(v: i32) -> TypedPattern {
        TypedPattern::Literal(Box::new(TypedExpr {
            kind: TypedExprKind::Int32Literal(v),
            ty: Type::Int32,
            span: span(),
        }))
    }

    fn tuple_pat(elements: Vec<TypedPattern>, tys: Vec<Type>) -> TypedPattern {
        TypedPattern::Tuple {
            element_patterns: elements,
            tuple_type: Type::Tuple(tys, MangledName::for_type(&fqn("a.T"))),
        }
    }

    fn fqn(s: &str) -> Fqn {
        Fqn::from_dotted(s).unwrap()
    }

    fn bool_pair() -> Type {
        Type::Tuple(
            vec![Type::Bool, Type::Bool],
            MangledName::for_type(&fqn("a.T")),
        )
    }

    fn check(arms: &[TypedMatchArm], ty: &Type) -> Option<String> {
        check_match_exhaustiveness(arms, ty, &BTreeMap::new(), &Registry::new())
    }

    #[test]
    fn wildcard_is_exhaustive() {
        assert_eq!(check(&[arm(TypedPattern::Wildcard)], &Type::Int32), None);
    }

    #[test]
    fn variable_binding_is_exhaustive() {
        let p = TypedPattern::Variable(VarName("x".to_string()), Type::Int32);
        assert_eq!(check(&[arm(p)], &Type::Int32), None);
    }

    #[test]
    fn bool_both_values_is_exhaustive() {
        assert_eq!(
            check(&[arm(bool_pat(true)), arm(bool_pat(false))], &Type::Bool),
            None
        );
    }

    #[test]
    fn bool_missing_a_value_is_reported() {
        let msg = check(&[arm(bool_pat(true))], &Type::Bool).expect("non-exhaustive");
        assert!(msg.contains("false"), "{msg}");
    }

    #[test]
    fn bool_missing_both_values_is_reported() {
        let msg = check(&[], &Type::Bool).expect("non-exhaustive");
        assert!(msg.contains("true") && msg.contains("false"), "{msg}");
    }

    /// Integers are an infinite domain: only a wildcard covers them.
    #[test]
    fn int_literals_alone_are_not_exhaustive() {
        assert!(check(&[arm(int_pat(1)), arm(int_pat(2))], &Type::Int32).is_some());
    }

    #[test]
    fn int_with_wildcard_is_exhaustive() {
        assert_eq!(
            check(&[arm(int_pat(1)), arm(TypedPattern::Wildcard)], &Type::Int32),
            None
        );
    }

    /// A guarded arm proves nothing about coverage.
    #[test]
    fn guarded_arms_do_not_count() {
        assert!(check(&[guarded(TypedPattern::Wildcard)], &Type::Bool).is_some());
        assert!(
            check(
                &[arm(bool_pat(true)), guarded(bool_pat(false))],
                &Type::Bool
            )
            .is_some()
        );
    }

    #[test]
    fn bool_pair_full_cross_product_is_exhaustive() {
        let tys = vec![Type::Bool, Type::Bool];
        let arms: Vec<TypedMatchArm> = [(true, true), (true, false), (false, true), (false, false)]
            .into_iter()
            .map(|(a, b)| arm(tuple_pat(vec![bool_pat(a), bool_pat(b)], tys.clone())))
            .collect();
        assert_eq!(check(&arms, &bool_pair()), None);
    }

    #[test]
    fn bool_pair_missing_one_combination_is_reported() {
        let tys = vec![Type::Bool, Type::Bool];
        let arms: Vec<TypedMatchArm> = [(true, true), (true, false), (false, true)]
            .into_iter()
            .map(|(a, b)| arm(tuple_pat(vec![bool_pat(a), bool_pat(b)], tys.clone())))
            .collect();
        let msg = check(&arms, &bool_pair()).expect("non-exhaustive");
        assert!(msg.contains("(false, false)"), "{msg}");
    }

    /// A diagonal covers neither off-diagonal cell.
    #[test]
    fn bool_pair_diagonal_is_not_exhaustive() {
        let tys = vec![Type::Bool, Type::Bool];
        let arms: Vec<TypedMatchArm> = [(true, true), (false, false)]
            .into_iter()
            .map(|(a, b)| arm(tuple_pat(vec![bool_pat(a), bool_pat(b)], tys.clone())))
            .collect();
        assert!(check(&arms, &bool_pair()).is_some());
    }

    /// A wildcard in one column still needs the other column covered.
    #[test]
    fn bool_pair_wildcard_column_is_exhaustive() {
        let tys = vec![Type::Bool, Type::Bool];
        let arms = vec![
            arm(tuple_pat(
                vec![bool_pat(true), TypedPattern::Wildcard],
                tys.clone(),
            )),
            arm(tuple_pat(
                vec![bool_pat(false), TypedPattern::Wildcard],
                tys.clone(),
            )),
        ];
        assert_eq!(check(&arms, &bool_pair()), None);
    }

    /// An infinite column can only be covered by a wildcard, at any depth.
    #[test]
    fn bool_and_int_needs_a_wildcard_in_the_int_column() {
        let tys = vec![Type::Bool, Type::Int32];
        let mixed = Type::Tuple(tys.clone(), MangledName::for_type(&fqn("a.T")));
        let covered = vec![
            arm(tuple_pat(
                vec![bool_pat(true), TypedPattern::Wildcard],
                tys.clone(),
            )),
            arm(tuple_pat(
                vec![bool_pat(false), TypedPattern::Wildcard],
                tys.clone(),
            )),
        ];
        assert_eq!(check(&covered, &mixed), None);

        let literal_only = vec![
            arm(tuple_pat(vec![bool_pat(true), int_pat(1)], tys.clone())),
            arm(tuple_pat(vec![bool_pat(false), int_pat(1)], tys.clone())),
        ];
        assert!(check(&literal_only, &mixed).is_some());
    }

    /// An unknown enum must degrade to silence, never a false positive.
    #[test]
    fn unresolvable_scrutinee_reports_nothing() {
        let unknown = Type::Enum(fqn("a.Missing"), MangledName::for_type(&fqn("a.Missing")));
        assert_eq!(check(&[], &unknown), None);
    }
}
