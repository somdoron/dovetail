//! Coherence: two implement blocks for the same trait may not overlap — no
//! type may match both (trait-design-appendix §6.2). Conservative Rust-style
//! rule: shapes that unify are an overlap regardless of where-bounds (bounds
//! only the built-in Tuple bound contributes structural arity information).
//!
//! Exact duplicates among non-generic blocks are already rejected at collect
//! time (and the duplicate is never registered), so this rule reports the
//! remaining cases: generic×generic and generic×concrete overlaps, including
//! cross-package ones.

use std::collections::BTreeMap;

use crate::common::diagnostics::Diagnostics;
use crate::common::types::{PackagePath, TypeParamName};
use crate::typechecker::registry::{ImplBlockSignature, Registry};
use crate::typechecker::types::Type;

/// Which side of the unification a variable belongs to. The same spelling on
/// both sides is two distinct variables.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Side {
    Left,
    Right,
}

struct Unifier<'a> {
    left_params: &'a [TypeParamName],
    right_params: &'a [TypeParamName],
    /// var (on its side) → (side the VALUE is interpreted on, value). The
    /// value's side is recorded explicitly: after chasing, both operands of a
    /// unification step can live on the SAME side, so "values always come
    /// from the opposite side" does not hold.
    bindings: BTreeMap<(Side, TypeParamName), (Side, Type)>,
}

impl<'a> Unifier<'a> {
    fn new(left_params: &'a [TypeParamName], right_params: &'a [TypeParamName]) -> Self {
        Self { left_params, right_params, bindings: BTreeMap::new() }
    }

    fn var_of(&self, side: Side, t: &Type) -> Option<TypeParamName> {
        let name = match t {
            Type::TypeVariable(n, _) | Type::GenericParam(n, _, _) => n,
            _ => return None,
        };
        let params = match side {
            Side::Left => self.left_params,
            Side::Right => self.right_params,
        };
        params.contains(name).then(|| name.clone())
    }

    /// Chase bindings until a non-variable type or an unbound variable.
    /// Returns the resolved type and the side it should be interpreted on.
    fn resolve(&self, side: Side, t: &Type) -> (Side, Type) {
        let mut side = side;
        let mut t = t.clone();
        // Bounded depth: each chase step consumes one existing binding.
        for _ in 0..=self.bindings.len() {
            let Some(name) = self.var_of(side, &t) else { break };
            let Some((bound_side, bound)) = self.bindings.get(&(side, name)) else { break };
            side = *bound_side;
            t = bound.clone();
        }
        (side, t)
    }

    /// Occurs check: does `var` (of `var_side`) appear in `t` (interpreted on
    /// `side`), chasing bindings? Binding a variable to a type containing
    /// itself has no finite solution — the blocks are disjoint, and without
    /// this check `witness` would recurse forever.
    fn occurs(&self, var_side: Side, var: &TypeParamName, side: Side, t: &Type) -> bool {
        let (side, t) = self.resolve(side, t);
        if let Some(name) = self.var_of(side, &t) {
            return side == var_side && name == *var;
        }
        let check_args = |args: &[(crate::common::types::Variance, Type)]| {
            args.iter().any(|(_, a)| self.occurs(var_side, var, side, a))
        };
        match &t {
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. }
            | Type::GenericNewtype { type_args, .. } => check_args(type_args),
            Type::Array(e) | Type::TupleProjection(e, _) => self.occurs(var_side, var, side, e),
            Type::AssociatedProjection(projection) => projection.types().any(|ty| self.occurs(var_side, var, side, ty)),
            Type::TupleExtend(a, b) => self.occurs(var_side, var, side, a) || self.occurs(var_side, var, side, b),
            Type::Tuple(ts, _) => ts.iter().any(|a| self.occurs(var_side, var, side, a)),
            Type::Function(ps, r) => {
                ps.iter().any(|a| self.occurs(var_side, var, side, a))
                    || self.occurs(var_side, var, side, r)
            }
            Type::Newtype(_, inner) => self.occurs(var_side, var, side, inner),
            Type::InterfaceObject { traits, .. } => traits.iter().any(|c| {
                c.trait_type_args.iter().any(|a| self.occurs(var_side, var, side, a))
            }),
            _ => false,
        }
    }

    fn unify(&mut self, left: &Type, right: &Type) -> bool {
        self.unify_side(Side::Left, left, Side::Right, right)
    }

    fn unify_side(&mut self, lside: Side, l: &Type, rside: Side, r: &Type) -> bool {
        let (lside, l) = self.resolve(lside, l);
        let (rside, r) = self.resolve(rside, r);
        if let Some(name) = self.var_of(lside, &l) {
            if let Some(rname) = self.var_of(rside, &r) {
                // var-var: binding a variable to itself (same side + name)
                // would loop; anything else records one side.
                if lside == rside && name == rname {
                    return true;
                }
            }
            if self.occurs(lside, &name, rside, &r) {
                return false;
            }
            self.bindings.insert((lside, name), (rside, r));
            return true;
        }
        if let Some(name) = self.var_of(rside, &r) {
            if self.occurs(rside, &name, lside, &l) {
                return false;
            }
            self.bindings.insert((rside, name), (lside, l));
            return true;
        }
        self.unify_concrete(lside, &l, rside, &r)
    }

    fn unify_concrete(&mut self, lside: Side, l: &Type, rside: Side, r: &Type) -> bool {
        match (l, r) {
            (Type::TupleExtend(a, b), Type::TupleExtend(c, d)) => {
                self.unify_side(lside, a, rside, c) && self.unify_side(lside, b, rside, d)
            }
            (Type::TupleExtend(a, b), Type::Tuple(..)) => {
                let Some((prefix, last)) = l.split_tuple_extension(r) else { return false };
                self.unify_side(lside, a, rside, &prefix) && self.unify_side(lside, b, rside, &last)
            }
            (Type::Tuple(..), Type::TupleExtend(..)) => self.unify_concrete(rside, r, lside, l),
            (
                Type::GenericRecord { fqn: f1, type_args: a1, .. },
                Type::GenericRecord { fqn: f2, type_args: a2, .. },
            )
            | (
                Type::GenericEnum { fqn: f1, type_args: a1, .. },
                Type::GenericEnum { fqn: f2, type_args: a2, .. },
            )
            | (
                Type::GenericClass { fqn: f1, type_args: a1, .. },
                Type::GenericClass { fqn: f2, type_args: a2, .. },
            ) => {
                f1 == f2
                    && a1.len() == a2.len()
                    && a1.iter().zip(a2.iter()).all(|((_, t1), (_, t2))| {
                        self.unify_side(lside, t1, rside, t2)
                    })
            }
            (
                Type::GenericNewtype { fqn: f1, type_args: a1, .. },
                Type::GenericNewtype { fqn: f2, type_args: a2, .. },
            ) => {
                f1 == f2
                    && a1.len() == a2.len()
                    && a1.iter().zip(a2.iter()).all(|((_, t1), (_, t2))| {
                        self.unify_side(lside, t1, rside, t2)
                    })
            }
            (Type::Array(e1), Type::Array(e2)) => self.unify_side(lside, e1, rside, e2),
            (Type::Tuple(ts1, _), Type::Tuple(ts2, _)) => {
                ts1.len() == ts2.len()
                    && ts1.iter().zip(ts2.iter()).all(|(t1, t2)| {
                        self.unify_side(lside, t1, rside, t2)
                    })
            }
            (Type::Function(p1, r1), Type::Function(p2, r2)) => {
                p1.len() == p2.len()
                    && p1.iter().zip(p2.iter()).all(|(t1, t2)| self.unify_side(lside, t1, rside, t2))
                    && self.unify_side(lside, r1, rside, r2)
            }
            (Type::Newtype(f1, i1), Type::Newtype(f2, i2)) => {
                f1 == f2 && self.unify_side(lside, i1, rside, i2)
            }
            // Interface-object for_types: same sorted component set, with the
            // components' trait args unified (they can hold block variables —
            // `implement <T> Tr for Producer<T>` vs `Tr for Producer<Int32>`).
            (
                Type::InterfaceObject { traits: t1, .. },
                Type::InterfaceObject { traits: t2, .. },
            ) => {
                t1.len() == t2.len()
                    && t1.iter().zip(t2.iter()).all(|(c1, c2)| {
                        c1.trait_fqn == c2.trait_fqn
                            && c1.trait_type_args.len() == c2.trait_type_args.len()
                            && c1
                                .trait_type_args
                                .iter()
                                .zip(c2.trait_type_args.iter())
                                .all(|(a1, a2)| self.unify_side(lside, a1, rside, a2))
                    })
            }
            // Everything else (primitives, plain records/enums/classes,
            // Never, Any, ...): structural equality. Never/Any are ordinary
            // shapes here — shape overlap, not subtyping.
            _ => l == r,
        }
    }

    /// Substitute the accumulated left-side bindings into a left-side type,
    /// producing the overlap witness. Unbound variables render as themselves.
    fn witness(&self, side: Side, t: &Type) -> Type {
        let (side, t) = self.resolve(side, t);
        match &t {
            Type::GenericRecord { fqn, mangled_name, type_args } => Type::GenericRecord {
                fqn: fqn.clone(),
                mangled_name: mangled_name.clone(),
                type_args: type_args.iter().map(|(v, a)| (*v, self.witness(side, a))).collect(),
            },
            Type::GenericEnum { fqn, mangled_name, type_args } => Type::GenericEnum {
                fqn: fqn.clone(),
                mangled_name: mangled_name.clone(),
                type_args: type_args.iter().map(|(v, a)| (*v, self.witness(side, a))).collect(),
            },
            Type::GenericClass { fqn, mangled_name, type_args } => Type::GenericClass {
                fqn: fqn.clone(),
                mangled_name: mangled_name.clone(),
                type_args: type_args.iter().map(|(v, a)| (*v, self.witness(side, a))).collect(),
            },
            Type::Array(e) => Type::Array(Box::new(self.witness(side, e))),
            Type::GenericNewtype { fqn, type_args, concrete_inner_type } => {
                Type::GenericNewtype {
                    fqn: fqn.clone(),
                    type_args: type_args
                        .iter()
                        .map(|(v, a)| (*v, self.witness(side, a)))
                        .collect(),
                    concrete_inner_type: Box::new(self.witness(side, concrete_inner_type)),
                }
            }
            Type::Newtype(f, inner) => {
                Type::Newtype(f.clone(), Box::new(self.witness(side, inner)))
            }
            Type::TupleExtend(a, b) => Type::tuple_extend(self.witness(side, a), self.witness(side, b)),
            Type::Tuple(ts, boxed) => Type::Tuple(
                ts.iter().map(|a| self.witness(side, a)).collect(),
                boxed.clone(),
            ),
            Type::Function(ps, r) => Type::Function(
                ps.iter().map(|a| self.witness(side, a)).collect(),
                Box::new(self.witness(side, r)),
            ),
            Type::InterfaceObject { traits, .. } => Type::interface_intersection(
                traits
                    .iter()
                    .map(|c| {
                        (
                            c.trait_fqn.clone(),
                            c.trait_type_args.iter().map(|a| self.witness(side, a)).collect(),
                        )
                    })
                    .collect(),
            ),
            _ => t.clone(),
        }
    }
}

/// If the two blocks' for-types (and trait type args) can both apply to some
/// type, return that type (the witness); otherwise `None`.
fn overlap_witness(a: &ImplBlockSignature, b: &ImplBlockSignature) -> Option<Type> {
    if a.trait_fqn != b.trait_fqn || a.trait_type_args.len() != b.trait_type_args.len() {
        return None;
    }
    let mut unifier = Unifier::new(&a.type_params, &b.type_params);
    if !unifier.unify(&a.for_type, &b.for_type) {
        return None;
    }
    for (ta, tb) in a.trait_type_args.iter().zip(b.trait_type_args.iter()) {
        if !unifier.unify(ta, tb) {
            return None;
        }
    }
    // Trait arguments may constrain a prefix after receiver matching. A scalar
    // binding cannot witness the built-in tuple constraint on that prefix.
    for (side, block) in [(Side::Left, a), (Side::Right, b)] {
        if let Type::TupleExtend(prefix, _) = &block.for_type {
            let resolved = unifier.witness(side, prefix);
            if !resolved.is_tuple() && !matches!(resolved, Type::TypeVariable(..) | Type::GenericParam(..)) {
                return None;
            }
        }
    }
    Some(unifier.witness(Side::Left, &a.for_type))
}

/// Report overlapping implement blocks. Runs over the merged registry so
/// cross-package overlaps are caught, but errors only on blocks owned by the
/// current package (dependency×dependency pairs were reported when the
/// dependency itself was compiled).
pub(super) fn check_coherence(
    merged_registry: &Registry,
    package_path: &PackagePath,
    diagnostics: &mut Diagnostics,
) {
    let blocks = merged_registry.all_implement_blocks();
    // Group indices by trait to keep the pairwise scan cheap.
    let mut by_trait: BTreeMap<&crate::common::types::Fqn, Vec<usize>> = BTreeMap::new();
    for (i, block) in blocks.iter().enumerate() {
        by_trait.entry(&block.trait_fqn).or_default().push(i);
    }
    for indices in by_trait.values() {
        for (pos, &j) in indices.iter().enumerate() {
            if blocks[j].package != *package_path {
                continue;
            }
            for &i in &indices[..pos] {
                // Skip pairs already rejected at collect time (exact
                // duplicates are never registered, so any surviving pair is
                // a genuine overlap to report).
                if let Some(witness) = overlap_witness(&blocks[i], &blocks[j]) {
                    diagnostics.error(
                        blocks[j].span.clone(),
                        format!(
                            "overlapping implementations of trait '{}': this block and the one at {}:{} can both apply to '{}'",
                            blocks[j].trait_fqn.symbol.0,
                            blocks[i].source_file,
                            blocks[i].span.line,
                            witness,
                        ),
                    );
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::span::Span;
    use crate::common::types::{Fqn, MangledName, PackagePath, SymbolName};
    use crate::common::types::Variance;
    use crate::typechecker::types::TraitBounds;

    fn fqn(pkg: &str, sym: &str) -> Fqn {
        Fqn { package: PackagePath(vec![pkg.to_string()]), symbol: SymbolName(sym.to_string()) }
    }

    fn block(trait_sym: &str, for_type: Type, params: &[&str], trait_args: Vec<Type>) -> ImplBlockSignature {
        let type_fqn = for_type.try_to_fqn().unwrap_or_else(|| fqn("a", "X"));
        ImplBlockSignature {
            trait_fqn: fqn("a", trait_sym),
            type_fqn,
            for_type,
            type_params: params.iter().map(|p| TypeParamName(p.to_string())).collect(),
            trait_type_args: trait_args,
            trait_bounds: TraitBounds::default(),
            methods: vec![],
            properties: vec![],
            associated_type_defs: Default::default(),
            span: Span::point("t.dove".into(), 1, 1),
            source_file: "t.dove".into(),
            package: PackagePath(vec!["a".to_string()]),
        }
    }

    fn list_of(t: Type) -> Type {
        Type::GenericRecord {
            fqn: fqn("std", "List"),
            mangled_name: MangledName::for_type(&fqn("std", "List")),
            type_args: vec![(Variance::Invariant, t)],
        }
    }

    fn var(n: &str) -> Type {
        Type::TypeVariable(TypeParamName(n.to_string()), vec![])
    }

    #[test]
    fn concrete_equal_overlap() {
        let a = block("Tr", list_of(Type::Int32), &[], vec![]);
        let b = block("Tr", list_of(Type::Int32), &[], vec![]);
        assert!(overlap_witness(&a, &b).is_some());
    }

    #[test]
    fn concrete_unequal_no_overlap() {
        let a = block("Tr", list_of(Type::Int32), &[], vec![]);
        let b = block("Tr", list_of(Type::String), &[], vec![]);
        assert!(overlap_witness(&a, &b).is_none());
    }

    #[test]
    fn blanket_vs_concrete_overlap_with_witness() {
        let a = block("Tr", list_of(var("T")), &["T"], vec![]);
        let b = block("Tr", list_of(Type::Int32), &[], vec![]);
        let w = overlap_witness(&a, &b).expect("overlap");
        assert_eq!(w, list_of(Type::Int32));
    }

    #[test]
    fn blanket_vs_blanket_overlap() {
        let a = block("Tr", list_of(var("T")), &["T"], vec![]);
        let b = block("Tr", list_of(var("U")), &["U"], vec![]);
        assert!(overlap_witness(&a, &b).is_some());
    }

    #[test]
    fn nested_partial_overlap() {
        // Pair<T, Int32> vs Pair<String, U> → overlap at Pair<String, Int32>
        let pair = |x: Type, y: Type| Type::GenericRecord {
            fqn: fqn("a", "Pair"),
            mangled_name: MangledName::for_type(&fqn("a", "Pair")),
            type_args: vec![(Variance::Invariant, x), (Variance::Invariant, y)],
        };
        let a = block("Tr", pair(var("T"), Type::Int32), &["T"], vec![]);
        let b = block("Tr", pair(Type::String, var("U")), &["U"], vec![]);
        let w = overlap_witness(&a, &b).expect("overlap");
        assert_eq!(w, pair(Type::String, Type::Int32));
    }

    #[test]
    fn nested_disjoint_no_overlap() {
        let pair = |x: Type, y: Type| Type::GenericRecord {
            fqn: fqn("a", "Pair"),
            mangled_name: MangledName::for_type(&fqn("a", "Pair")),
            type_args: vec![(Variance::Invariant, x), (Variance::Invariant, y)],
        };
        let a = block("Tr", pair(var("T"), Type::Int32), &["T"], vec![]);
        let b = block("Tr", pair(var("U"), Type::String), &["U"], vec![]);
        assert!(overlap_witness(&a, &b).is_none());
    }

    #[test]
    fn trait_args_gate_disjoint() {
        // From<Int32> for X vs From<String> for X — no overlap.
        let x = Type::Record(fqn("a", "X"), MangledName::for_type(&fqn("a", "X")));
        let a = block("From", x.clone(), &[], vec![Type::Int32]);
        let b = block("From", x, &[], vec![Type::String]);
        assert!(overlap_witness(&a, &b).is_none());
    }

    #[test]
    fn trait_args_gate_blanket() {
        // implement <T> From<T> for X vs From<Int32> for X — overlap, T := Int32.
        let x = Type::Record(fqn("a", "X"), MangledName::for_type(&fqn("a", "X")));
        let a = block("From", x.clone(), &["T"], vec![var("T")]);
        let b = block("From", x, &[], vec![Type::Int32]);
        assert!(overlap_witness(&a, &b).is_some());
    }

    #[test]
    fn occurs_check_mutual_recursion_disjoint() {
        // Pair<T, List<T>> vs Pair<List<U>, U>: T = List<U>, U = List<T> has
        // no finite solution — disjoint, and must not overflow the stack.
        let pair = |x: Type, y: Type| Type::GenericRecord {
            fqn: fqn("a", "Pair"),
            mangled_name: MangledName::for_type(&fqn("a", "Pair")),
            type_args: vec![(Variance::Invariant, x), (Variance::Invariant, y)],
        };
        let a = block("Tr", pair(var("T"), list_of(var("T"))), &["T"], vec![]);
        let b = block("Tr", pair(list_of(var("U")), var("U")), &["U"], vec![]);
        assert!(overlap_witness(&a, &b).is_none());
    }

    #[test]
    fn different_traits_never_compared() {
        let a = block("Tr", list_of(Type::Int32), &[], vec![]);
        let b = block("Other", list_of(Type::Int32), &[], vec![]);
        assert!(overlap_witness(&a, &b).is_none());
    }
}
