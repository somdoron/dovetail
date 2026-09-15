use wasm_encoder::{FieldType, StorageType, StructType, SubType};

use super::Codegen;
use crate::typechecker::types::EnumVariantDef;

/// Build a WASM-GC SubType for an enum base type (empty struct, not final).
/// Variants will subtype this.
pub fn build_enum_base_subtype(reified: bool) -> SubType {
    SubType {
        is_final: false,
        supertype_idx: None,
        composite_type: wasm_encoder::CompositeType {
            inner: wasm_encoder::CompositeInnerType::Struct(StructType {
                fields: if reified { vec![super::runtime_types::id_field()].into_boxed_slice() } else { Box::new([]) },
            }),
            shared: false,
            describes: None,
            descriptor: None,
        },
    }
}

/// Build a WASM-GC SubType for an enum variant (struct with payload fields, final, subtypes base).
pub fn build_enum_variant_subtype(
    variant: &EnumVariantDef,
    base_type_index: u32,
    reified: bool,
    codegen: &Codegen,
) -> SubType {
    // Each payload is spliced into its flattened WASM value types (`type_to_valtypes`), like record
    // fields: a tuple payload becomes N consecutive fields (one per leaf), everything else is one.
    let mut fields: Vec<FieldType> = variant
        .payload_types
        .iter()
        .flat_map(|ty| codegen.type_to_valtypes(ty))
        .map(|val_type| FieldType {
            element_type: StorageType::Val(val_type),
            mutable: false,
        })
        .collect();

    if reified { fields.insert(0,super::runtime_types::id_field()); }
    SubType {
        is_final: true,
        supertype_idx: Some(base_type_index),
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

