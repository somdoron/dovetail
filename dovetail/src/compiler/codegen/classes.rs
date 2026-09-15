use wasm_encoder::{FieldType, StorageType, StructType, SubType, ValType};

use crate::common::types::MangledName;
use crate::typechecker::types::{ClassTypeDef, TypeDef};

use super::Codegen;

/// Build a WASM-GC SubType for a class type definition.
///
/// Payload fields flatten through `type_to_valtypes`; their mutability comes
/// from `ClassFieldDef.mutable`. The optional immutable type ID precedes the
/// `(ref $vtable_type)` vtable reference,
/// emitted in the same rec group as the class struct to avoid circular dependencies.
/// WASM-GC covariance on immutable fields allows subclass structs to narrow this type.
pub fn build_class_subtype(cls: &ClassTypeDef, codegen: &Codegen, vtable_type_idx: u32) -> SubType {
    let mut fields: Vec<FieldType> = Vec::new();
    if codegen.id_prefix(&cls.mangled_name) != 0 { fields.push(super::runtime_types::id_field()); }

    // Vtable reference follows the optional immutable type ID.
    fields.push(FieldType {
        element_type: StorageType::Val(ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(vtable_type_idx),
        })),
        mutable: false,
    });

    // The common header has exactly one inherited, lazily assigned identity hash.
    fields.push(FieldType {
        element_type: StorageType::Val(ValType::I32),
        mutable: true,
    });

    // Each field is spliced into its flattened WASM value types (`type_to_valtypes`), exactly like
    // record fields: a tuple field becomes N consecutive fields (one per leaf), every other field
    // is one slot. Mutability comes from `ClassFieldDef.mutable` (each spliced sub-field inherits
    // it, so a mutable tuple field is reassigned leaf-by-leaf in place).
    for f in &cls.fields {
        for val_type in codegen.type_to_valtypes(&f.ty) {
            fields.push(FieldType {
                element_type: StorageType::Val(val_type),
                mutable: f.mutable,
            });
        }
    }

    let supertype_idx = cls
        .parent_mangled_name
        .as_ref()
        .map(|mn| codegen.type_indices[mn]);

    SubType {
        is_final: cls.is_final,
        supertype_idx,
        composite_type: wasm_encoder::CompositeType {
            inner: wasm_encoder::CompositeInnerType::Struct(StructType {
                fields: fields.into_boxed_slice(),
            }),
            shared: false,
            describes: None,
            descriptor: None,
        },
    }
}

impl Codegen<'_> {
    /// Build a class's vtable SubTypes: one func type per slot the class introduces, then the
    /// vtable struct itself.
    ///
    /// Purely a builder — every index it reads (this class's slot func types, the parent's
    /// vtable struct, the hierarchy root's class struct) was registered by `emit_type_section`'s
    /// pre-allocation pass, which also copied the parent's func types onto the inherited slots.
    pub(super) fn create_class_vtable(
        &self,
        cls: &ClassTypeDef,
        class_mn: &MangledName,
    ) -> Vec<SubType> {
        let mut subtypes = Vec::new();

        let parent_vtable_len = cls.parent_mangled_name.as_ref().map_or(0, |parent_mn| {
            if let TypeDef::Class(parent_cls) = &self.typed_module.types[parent_mn] {
                parent_cls.vtable_methods.len()
            } else {
                0
            }
        });

        // Build self ref type using hierarchy root
        let root_mn = &cls.hierarchy_root_mangled;
        let root_type_idx = self.type_indices[root_mn];
        let self_ref = ValType::Ref(wasm_encoder::RefType {
            nullable: false,
            heap_type: wasm_encoder::HeapType::Concrete(root_type_idx),
        });

        // Emit new func types for new slots (beyond parent's vtable). Tuple params flatten into a
        // sequence of WASM params (like direct calls); type-parameter leaves lower to anyref, producing
        // the erased slot signature shared across all instantiations of the class.
        for slot_idx in parent_vtable_len..cls.vtable_methods.len() {
            let slot = &cls.vtable_methods[slot_idx];
            let mut params = vec![self_ref];
            for pt in slot.param_types.iter().skip(1) {
                params.extend(self.type_to_valtypes(pt));
            }
            // Phase 2: a concrete tuple return flattens to multi-value results; the slot signature
            // matches the (multi-value) method function ref.func'd into the vtable.
            let results: Vec<ValType> = self.type_to_valtypes(&slot.return_type).into_vec();

            subtypes.push(SubType {
                is_final: true,
                supertype_idx: None,
                composite_type: wasm_encoder::CompositeType {
                    inner: wasm_encoder::CompositeInnerType::Func(
                        wasm_encoder::FuncType::new(params, results),
                    ),
                    shared: false,
                    describes: None,
                    descriptor: None,
                },
            });
        }

        // Build vtable struct type with funcref fields
        let mut vtable_fields = Vec::new();
        for slot_idx in 0..cls.vtable_methods.len() {
            let func_type_idx = self.class_vtable_slot_func_types
                [&(class_mn.clone(), slot_idx as u32)];
            vtable_fields.push(wasm_encoder::FieldType {
                element_type: wasm_encoder::StorageType::Val(ValType::Ref(
                    wasm_encoder::RefType {
                        nullable: false,
                        heap_type: wasm_encoder::HeapType::Concrete(func_type_idx),
                    },
                )),
                mutable: false,
            });
        }

        let parent_vtable_type_idx = cls.parent_mangled_name.as_ref()
            .and_then(|mn| self.class_vtable_type_indices.get(mn).copied());

        subtypes.push(SubType {
            is_final: cls.is_final,
            supertype_idx: parent_vtable_type_idx,
            composite_type: wasm_encoder::CompositeType {
                inner: wasm_encoder::CompositeInnerType::Struct(wasm_encoder::StructType {
                    fields: vtable_fields.into_boxed_slice(),
                }),
                shared: false,
                describes: None,
                descriptor: None,
            },
        });

        // virtual_method_func_types is populated by a separate pass after all type indices
        // are settled — see `register_virtual_method_func_types` in `mod.rs`.

        subtypes
    }

    /// Build vtable func types, vtable struct, and class struct subtypes into the module-wide
    /// rec group. All indices were pre-allocated by `emit_type_section`'s first pass.
    pub(super) fn build_class_vtable_subtypes(
        &self,
        cls: &ClassTypeDef,
        class_mn: &MangledName,
        subtypes: &mut Vec<SubType>,
    ) {
        // Build vtable func types and vtable struct subtype (indices pre-allocated)
        subtypes.extend(self.create_class_vtable(cls, class_mn));

        // Build the class struct type
        let vtable_type_idx = self.class_vtable_type_indices[class_mn];
        subtypes.push(build_class_subtype(cls, self, vtable_type_idx));
    }
}
