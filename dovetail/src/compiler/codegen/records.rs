use wasm_encoder::{FieldType, StorageType, StructType, SubType};

use crate::typechecker::types::RecordTypeDef;

use super::Codegen;

/// Build a WASM-GC SubType for a record type definition.
///
/// Each field is **spliced** into its flattened WASM value types (`type_to_valtypes`): a concrete
/// tuple field becomes N consecutive struct fields (one per leaf), stored unboxed in the record
/// struct. The boxed form of a tuple is the shared `$Tuple_N` struct (emitted separately); a record
/// never holds a `$Tuple_N` ref for a concrete tuple field — it splices the leaves directly.
pub fn build_record_subtype(rec: &RecordTypeDef, codegen: &Codegen) -> SubType {
    let mut fields: Vec<FieldType> = Vec::new();
    if codegen.id_prefix(&rec.mangled_name) != 0 {
        fields.push(super::runtime_types::id_field());
    }

    for (_, ty) in &rec.fields {
        for val_type in codegen.type_to_valtypes(ty) {
            fields.push(FieldType {
                element_type: StorageType::Val(val_type),
                mutable: false,
            });
        }
    }

    SubType {
        is_final: true,
        supertype_idx: None,
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
