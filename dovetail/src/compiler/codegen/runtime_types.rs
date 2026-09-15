//! Module-local identities and subtype tables for erased nominal values.
use super::{
    Codegen,
    instance_key::{InstanceKey, instance_key},
};
use crate::common::types::MangledName;
use crate::typechecker::{
    subtyping,
    types::{Type, TypeDef, TypedExpr, TypedExprKind, TypedPattern},
};
use std::collections::{BTreeMap, BTreeSet};
use wasm_encoder::{ConstExpr, GlobalSection, GlobalType, Instruction, RefType, ValType};

#[derive(Default)]
pub(super) struct RuntimeTypes {
    classes: BTreeSet<MangledName>,
    ids: BTreeMap<InstanceKey, u32>,
    tables: BTreeMap<InstanceKey, RuntimeTypeTable>,
}

struct RuntimeTypeTable {
    expected_type: Type,
    membership: Vec<u8>,
    global_index: Option<u32>,
}

struct DiscoveredRuntimeTypes {
    concrete: BTreeMap<InstanceKey, Type>,
    expected: BTreeMap<InstanceKey, Type>,
}

impl Codegen<'_> {
    pub(super) fn id_prefix(&self, name: &MangledName) -> u32 {
        u32::from(match self.typed_module.types.get(name) {
            Some(TypeDef::Record(r)) => !r.type_params.is_empty(),
            Some(TypeDef::Enum(e)) => !e.type_params.is_empty(),
            Some(TypeDef::Class(_)) => self.runtime_types.classes.contains(name),
            _ => false,
        })
    }

    pub(super) fn class_vtable_field(&self, name: &MangledName) -> u32 {
        self.id_prefix(name)
    }
    pub(super) fn class_hash_field(&self, name: &MangledName) -> u32 {
        self.id_prefix(name) + 1
    }
    pub(super) fn class_header_size(&self, name: &MangledName) -> u32 {
        self.id_prefix(name) + 2
    }

    fn reified(&self, ty: &Type) -> bool {
        match ty {
            Type::GenericRecord { .. } | Type::GenericEnum { .. } | Type::GenericClass { .. } => {
                true
            }
            Type::Class(_, name) => self.id_prefix(name) != 0,
            _ => false,
        }
    }

    pub(super) fn runtime_id(&self, ty: &Type) -> Option<u32> {
        if !self.reified(ty) {
            return None;
        }
        Some(
            *self
                .runtime_types
                .ids
                .get(&instance_key(ty))
                .unwrap_or_else(|| panic!("unregistered runtime construction type: {ty}")),
        )
    }

    pub(super) fn runtime_check_table(&self, ty: &Type) -> Option<u32> {
        if !matches!(
            ty,
            Type::GenericRecord { .. } | Type::GenericEnum { .. } | Type::GenericClass { .. }
        ) {
            return None;
        }
        let table = self
            .runtime_types
            .tables
            .get(&instance_key(ty))
            .unwrap_or_else(|| panic!("unregistered runtime check type: {ty}"));
        Some(table.global_index.unwrap_or_else(|| {
            panic!(
                "runtime check table used before global emission: {}",
                table.expected_type
            )
        }))
    }

    pub(super) fn prepare_runtime_types(&mut self) {
        self.mark_reified_class_hierarchies();
        let discovered = self.discover_runtime_types();
        self.assign_runtime_type_ids(&discovered.concrete);
        self.build_runtime_check_tables(discovered);
    }

    fn mark_reified_class_hierarchies(&mut self) {
        // Mark whole connected hierarchies, including non-generic ancestors/siblings.
        let root = |name: &MangledName| {
            let mut name = name.clone();
            while let Some(TypeDef::Class(c)) = self.typed_module.types.get(&name) {
                match &c.parent_mangled_name {
                    Some(parent) => name = parent.clone(),
                    None => break,
                }
            }
            name
        };
        let roots: BTreeSet<_> = self
            .typed_module
            .types
            .iter()
            .filter_map(|(name, def)| match def {
                TypeDef::Class(c) if !c.type_params.is_empty() => Some(root(name)),
                _ => None,
            })
            .collect();
        self.runtime_types.classes = self
            .typed_module
            .types
            .iter()
            .filter(|(name, def)| matches!(def, TypeDef::Class(_)) && roots.contains(&root(name)))
            .map(|(name, _)| name.clone())
            .collect();
    }

    fn discover_runtime_types(&self) -> DiscoveredRuntimeTypes {
        let mut pending = Vec::new();
        self.for_each_module_type(&mut |ty| pending.push(ty.clone()));
        let mut expected = BTreeMap::new();
        self.visit_runtime_expressions(&mut |expr| {
            collect_checks(expr, &mut expected);
            pending.push(self.construction_type(expr));
        });
        pending.extend(expected.values().cloned());
        for name in &self.runtime_types.classes {
            if let TypeDef::Class(c) = &self.typed_module.types[name]
                && c.type_params.is_empty()
            {
                pending.push(Type::Class(c.fqn.clone(), name.clone()));
            }
        }
        let mut types = BTreeMap::new();
        let mut seen = BTreeSet::new();
        while let Some(ty) = pending.pop() {
            if ty.contains_type_parameter() || ty.is_error() || !seen.insert(instance_key(&ty)) {
                continue;
            }
            if self.reified(&ty) {
                types.insert(instance_key(&ty), ty.clone());
            }
            self.concrete_children(&ty, &mut pending);
        }
        DiscoveredRuntimeTypes {
            concrete: types,
            expected,
        }
    }

    fn assign_runtime_type_ids(&mut self, types: &BTreeMap<InstanceKey, Type>) {
        self.runtime_types.ids = types
            .keys()
            .enumerate()
            .map(|(i, key)| {
                (
                    key.clone(),
                    u32::try_from(i).expect("too many runtime types"),
                )
            })
            .collect();
    }

    fn build_runtime_check_tables(&mut self, discovered: DiscoveredRuntimeTypes) {
        self.runtime_types.tables = discovered
            .expected
            .into_iter()
            .filter(|(_, t)| {
                matches!(
                    t,
                    Type::GenericRecord { .. }
                        | Type::GenericEnum { .. }
                        | Type::GenericClass { .. }
                )
            })
            .map(|(key, ty)| {
                assert!(
                    !ty.contains_type_parameter(),
                    "unresolved runtime check type: {ty}"
                );
                let membership = discovered
                    .concrete
                    .values()
                    .map(|actual| u8::from(subtyping::is_subtype(self.registry, actual, &ty)))
                    .collect();
                (
                    key,
                    RuntimeTypeTable {
                        expected_type: ty,
                        membership,
                        global_index: None,
                    },
                )
            })
            .collect();
    }

    fn concrete_children(&self, ty: &Type, pending: &mut Vec<Type>) {
        match ty {
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. } => {
                pending.extend(type_args.iter().map(|(_, t)| t.clone()))
            }
            Type::Array(t) | Type::Newtype(_, t) => pending.push((**t).clone()),
            Type::GenericNewtype {
                concrete_inner_type,
                type_args,
                ..
            } => {
                pending.push((**concrete_inner_type).clone());
                pending.extend(type_args.iter().map(|(_, t)| t.clone()));
            }
            Type::Tuple(ts, _) => pending.extend(ts.iter().cloned()),
            Type::Function(ts, r) => {
                pending.extend(ts.iter().cloned());
                pending.push((**r).clone());
            }
            _ => {}
        }
    }

    fn specialized_class_constructor(&self, name: &MangledName) -> bool {
        let mut current = Some(name);
        while let Some(name) = current {
            let Some(TypeDef::Class(class)) = self.typed_module.types.get(name) else {
                break;
            };
            if !class.type_params.is_empty() {
                return true;
            }
            current = class.parent_mangled_name.as_ref();
        }
        false
    }

    fn visit_runtime_expressions(&self, visitor: &mut impl FnMut(&TypedExpr)) {
        fn walk(expr: &TypedExpr, visitor: &mut impl FnMut(&TypedExpr)) {
            visitor(expr);
            crate::monomorphize::visit_expr_children(expr, |child| walk(child, visitor));
        }
        for f in self.typed_module.functions.values() {
            walk(&f.body, visitor);
        }
        for g in self.typed_module.globals.values() {
            walk(&g.initializer, visitor);
        }
        for t in &self.typed_module.tests {
            walk(&t.body, visitor);
        }
        for d in self.typed_module.types.values() {
            if let TypeDef::Class(c) = d {
                // Generic-ancestry constructors were lowered into specialized
                // functions. Their retained declaration bodies are templates,
                // not additional executable initializers.
                if self.specialized_class_constructor(&c.mangled_name) {
                    continue;
                }
                for e in c.initializer.iter().chain(c.extends_args.iter().flatten()) {
                    walk(e, visitor);
                }
            }
        }
    }

    pub(super) fn emit_runtime_type_globals(&mut self, globals: &mut GlobalSection) {
        for table in self.runtime_types.tables.values_mut() {
            table.global_index = Some(globals.len());
            let mut instructions: Vec<_> = table
                .membership
                .iter()
                .map(|b| Instruction::I32Const(i32::from(*b)))
                .collect();
            instructions.push(Instruction::ArrayNewFixed {
                array_type_index: super::ARRAY_I8_TYPE_INDEX,
                array_size: table.membership.len() as u32,
            });
            globals.global(
                GlobalType {
                    val_type: ValType::Ref(RefType {
                        nullable: false,
                        heap_type: wasm_encoder::HeapType::Concrete(super::ARRAY_I8_TYPE_INDEX),
                    }),
                    mutable: false,
                    shared: false,
                },
                &ConstExpr::extended(instructions),
            );
        }
    }
}

fn collect_checks(expr: &TypedExpr, expected: &mut BTreeMap<InstanceKey, Type>) {
    fn pattern(p: &TypedPattern, expected: &mut BTreeMap<InstanceKey, Type>) {
        match p {
            TypedPattern::TypeAnnotated { ty, .. } => {
                expected.insert(instance_key(ty), ty.clone());
            }
            TypedPattern::Tuple {
                element_patterns, ..
            } => {
                for p in element_patterns {
                    pattern(p, expected);
                }
            }
            TypedPattern::EnumVariant {
                payload_patterns, ..
            } => {
                for p in payload_patterns {
                    pattern(p, expected);
                }
            }
            TypedPattern::Newtype { inner_pattern, .. } => pattern(inner_pattern, expected),
            TypedPattern::EnumVariantRecord { field_patterns, .. }
            | TypedPattern::Record {
                fields: field_patterns,
                ..
            } => {
                for f in field_patterns {
                    pattern(&f.pattern, expected);
                }
            }
            _ => {}
        }
    }
    match &expr.kind {
        TypedExprKind::TypeTest { target_type, .. }
        | TypedExprKind::TypeCast { target_type, .. } => {
            expected.insert(instance_key(target_type), target_type.clone());
        }
        TypedExprKind::Match { arms, .. } => {
            for arm in arms {
                pattern(&arm.pattern, expected);
            }
        }
        _ => {}
    }
}

pub(super) fn id_field() -> wasm_encoder::FieldType {
    wasm_encoder::FieldType {
        element_type: wasm_encoder::StorageType::Val(ValType::I32),
        mutable: false,
    }
}

impl Codegen<'_> {
    /// Constructor arguments survive contextual widening of the expression type.
    pub(super) fn construction_type(&self, expr: &TypedExpr) -> Type {
        let params = match &expr.kind {
            TypedExprKind::RecordCreate { type_params, .. }
            | TypedExprKind::EnumCreate { type_params, .. }
            | TypedExprKind::EnumVariantRecordCreate { type_params, .. }
            | TypedExprKind::ClassNew { type_params, .. }
            | TypedExprKind::ClassStructCreate { type_params, .. }
            | TypedExprKind::RecordWith { type_params, .. } => type_params,
            _ => return expr.ty.clone(),
        };
        let mut ty = expr.ty.clone();
        match &mut ty {
            Type::GenericRecord { type_args, .. }
            | Type::GenericEnum { type_args, .. }
            | Type::GenericClass { type_args, .. } => {
                assert_eq!(
                    type_args.len(),
                    params.len(),
                    "missing constructor type parameters"
                );
                for ((_, ty), param) in type_args.iter_mut().zip(params) {
                    *ty = param.clone();
                }
            }
            _ => {}
        }
        ty
    }

    pub(super) fn variant_payload_prefix(&self, index: u32) -> u32 {
        let ((name, _), _) = self
            .variant_type_indices
            .iter()
            .find(|(_, i)| **i == index)
            .expect("variant type index");
        self.id_prefix(name)
    }
}
