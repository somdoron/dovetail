//! Emission order for the module-wide rec group.
//!
//! Every GC type the compiler emits lives in a *single* WASM rec group (see
//! `docs/nominal-type-identity.md`). Within one group, members at different indices are
//! distinct types even when structurally identical — which is what gives Dovetail's types
//! nominal identity on top of WASM-GC's structural canonicalization — and field references
//! may point anywhere in the group, forward or backward.
//!
//! That removes the need for any dependency analysis: there is no SCC computation, no
//! topological sort over field references, and no forcing of same-shape siblings into a
//! shared group. The one rule that survives is WASM's requirement that a **declared
//! supertype must appear before its subtype**.

use std::collections::{BTreeMap, BTreeSet};

use crate::common::types::MangledName;
use crate::typechecker::types::TypeDef;

/// The order in which user-defined types are laid out in the module-wide rec group.
///
/// Key order of `types`, except that a class is preceded by its ancestor chain. That covers
/// every supertype relation between two *distinct* entries: a class struct declares its parent
/// class struct as supertype, and a class vtable struct declares the parent's vtable struct.
/// (Enum variants also declare a supertype — their enum base — but base and variants are slots
/// of one entry, always emitted base-first, so they need no ordering help here.)
///
/// Emitting a class after its parent is also what lets `create_class_vtable` inherit the
/// parent's already-registered slot func types.
pub fn emission_order(types: &BTreeMap<MangledName, TypeDef>) -> Vec<MangledName> {
    let mut order = Vec::with_capacity(types.len());
    let mut seen: BTreeSet<MangledName> = BTreeSet::new();

    for name in types.keys() {
        visit(name, types, &mut seen, &mut order);
    }

    order
}

/// Push `name` after its ancestor chain. Inserting into `seen` *before* recursing means a
/// malformed parent cycle terminates instead of overflowing the stack; recursion depth is
/// otherwise bounded by inheritance depth.
fn visit(
    name: &MangledName,
    types: &BTreeMap<MangledName, TypeDef>,
    seen: &mut BTreeSet<MangledName>,
    order: &mut Vec<MangledName>,
) {
    if !seen.insert(name.clone()) {
        return;
    }

    if let Some(TypeDef::Class(cls)) = types.get(name)
        && let Some(parent) = &cls.parent_mangled_name
        && types.contains_key(parent)
    {
        visit(parent, types, seen, order);
    }

    order.push(name.clone());
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::common::types::{Fqn, PackagePath, SymbolName};
    use crate::typechecker::types::{ClassTypeDef, RecordTypeDef, Type, TypeDef};

    fn fqn(name: &str) -> Fqn {
        Fqn {
            package: PackagePath(vec!["test".to_string()]),
            symbol: SymbolName(name.to_string()),
        }
    }

    fn span() -> crate::common::span::Span {
        crate::common::span::Span::point(std::sync::Arc::from(""), 1, 1)
    }

    fn make_record(name: &str, fields: Vec<(&str, Type)>) -> (MangledName, TypeDef) {
        let record_fqn = fqn(name);
        let mn = MangledName::for_type(&record_fqn);
        let rec = RecordTypeDef {
            fqn: record_fqn,
            mangled_name: mn.clone(),
            fields: fields
                .into_iter()
                .map(|(f, ty)| (f.to_string(), ty))
                .collect(),
            type_params: vec![],
            span: span(),
        };
        (mn, TypeDef::Record(rec))
    }

    fn record_type(name: &str) -> Type {
        let record_fqn = fqn(name);
        let mn = MangledName::for_type(&record_fqn);
        Type::Record(record_fqn, mn)
    }

    fn make_class(name: &str, parent: Option<&str>) -> (MangledName, TypeDef) {
        let class_fqn = fqn(name);
        let mn = MangledName::for_type(&class_fqn);
        let parent_mangled_name = parent.map(|p| MangledName::for_type(&fqn(p)));
        let hierarchy_root_mangled = parent_mangled_name.clone().unwrap_or_else(|| mn.clone());
        let cls = ClassTypeDef {
            fqn: class_fqn,
            mangled_name: mn.clone(),
            fields: vec![],
            is_final: false,
            is_abstract: false,
            is_sealed: false,
            parent_mangled_name,
            parent_type: None,
            vtable_methods: vec![],
            hierarchy_root_mangled,
            constructor_params: vec![],
            initializer: vec![],
            initializer_fields: vec![],
            extends_args: None,
            type_params: vec![],
            span: span(),
        };
        (mn, TypeDef::Class(cls))
    }

    fn position(order: &[MangledName], mn: &MangledName) -> usize {
        order.iter().position(|n| n == mn).expect("type in order")
    }

    #[test]
    fn every_type_is_emitted_once() {
        let mut types = BTreeMap::new();
        for (mn, td) in [
            make_record("A", vec![("b", record_type("B"))]),
            make_record("B", vec![("x", Type::Int32)]),
            make_record("C", vec![("a", record_type("A"))]),
        ] {
            types.insert(mn, td);
        }

        let order = emission_order(&types);
        assert_eq!(order.len(), 3);
        let unique: BTreeSet<_> = order.iter().collect();
        assert_eq!(unique.len(), 3);
    }

    #[test]
    fn record_dependencies_need_no_ordering() {
        // A references B, but both land in the same rec group, so field references may point
        // forward — the order is simply key order, with no dependency reshuffling.
        let mut types = BTreeMap::new();
        let (mn_a, td_a) = make_record("A", vec![("b", record_type("B"))]);
        let (mn_b, td_b) = make_record("B", vec![("x", Type::Int32)]);
        types.insert(mn_a.clone(), td_a);
        types.insert(mn_b.clone(), td_b);

        let order = emission_order(&types);
        assert_eq!(order, vec![mn_a, mn_b]);
    }

    #[test]
    fn mutually_recursive_records_are_fine() {
        let mut types = BTreeMap::new();
        let (mn_a, td_a) = make_record("A", vec![("b", record_type("B"))]);
        let (mn_b, td_b) = make_record("B", vec![("a", record_type("A"))]);
        types.insert(mn_a.clone(), td_a);
        types.insert(mn_b.clone(), td_b);

        let order = emission_order(&types);
        assert_eq!(order, vec![mn_a, mn_b]);
    }

    #[test]
    fn class_follows_its_parent() {
        // Keys sort as Base, Derived, Middle — so plain key order would put Middle after
        // Derived, violating the supertype-first rule. The ancestor walk fixes that.
        let mut types = BTreeMap::new();
        let (mn_base, td_base) = make_class("Base", None);
        let (mn_middle, td_middle) = make_class("Middle", Some("Base"));
        let (mn_derived, td_derived) = make_class("Derived", Some("Middle"));
        types.insert(mn_base.clone(), td_base);
        types.insert(mn_middle.clone(), td_middle);
        types.insert(mn_derived.clone(), td_derived);

        let order = emission_order(&types);
        assert_eq!(order.len(), 3);
        assert!(position(&order, &mn_base) < position(&order, &mn_middle));
        assert!(position(&order, &mn_middle) < position(&order, &mn_derived));
    }

    #[test]
    fn separate_hierarchies_are_not_merged() {
        // Two same-shape hierarchies stay separate entries; nominal distinctness now comes
        // from rec-group index, not from forcing them into a shared group.
        let mut types = BTreeMap::new();
        for (mn, td) in [
            make_class("ARoot", None),
            make_class("AChild", Some("ARoot")),
            make_class("BRoot", None),
            make_class("BChild", Some("BRoot")),
        ] {
            types.insert(mn, td);
        }

        let order = emission_order(&types);
        assert_eq!(order.len(), 4);
        let a_root = MangledName::for_type(&fqn("ARoot"));
        let a_child = MangledName::for_type(&fqn("AChild"));
        let b_root = MangledName::for_type(&fqn("BRoot"));
        let b_child = MangledName::for_type(&fqn("BChild"));
        assert!(position(&order, &a_root) < position(&order, &a_child));
        assert!(position(&order, &b_root) < position(&order, &b_child));
    }

    #[test]
    fn empty_module() {
        let types = BTreeMap::new();
        assert!(emission_order(&types).is_empty());
    }
}
