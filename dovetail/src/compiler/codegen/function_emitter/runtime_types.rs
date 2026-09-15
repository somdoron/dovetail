use super::FunctionEmitter;
use crate::typechecker::types::{Type, TypeDef};
use wasm_encoder::{BlockType, HeapType, Instruction, RefType, ValType};

impl FunctionEmitter<'_> {
    pub(super) fn emit_type_id(&mut self, ty: &Type) {
        if let Some(id) = self.codegen.runtime_id(ty) {
            self.instruction(Instruction::I32Const(id as i32));
        }
    }

    /// Insert the immutable header before payload values already on the stack.
    pub(super) fn emit_nominal_struct_new(&mut self, ty: &Type, index: u32) {
        if self.codegen.runtime_id(ty).is_some() {
            let name = ty.mangled_name();
            let fields: Vec<ValType> = match &self.codegen.typed_module.types[&name] {
                TypeDef::Record(r) => r
                    .fields
                    .iter()
                    .flat_map(|(_, t)| self.codegen.type_to_valtypes(t))
                    .collect(),
                TypeDef::Enum(e) => {
                    let variant = e
                        .variants
                        .iter()
                        .find(|v| {
                            self.codegen.variant_type_indices[&(name.clone(), v.name.clone())]
                                == index
                        })
                        .expect("enum variant layout");
                    variant
                        .payload_types
                        .iter()
                        .flat_map(|t| self.codegen.type_to_valtypes(t))
                        .collect()
                }
                _ => panic!("payload constructor requires record or enum"),
            };
            let locals: Vec<_> = fields.into_iter().map(|t| self.add_local(t)).collect();
            for &local in locals.iter().rev() {
                self.instruction(Instruction::LocalSet(local));
            }
            self.emit_type_id(ty);
            for local in locals {
                self.instruction(Instruction::LocalGet(local));
            }
        }
        self.instruction(Instruction::StructNew(index));
    }

    /// Consume one reference and leave a Boolean; the nominal guard protects the ID read.
    pub(super) fn emit_reified_type_test(&mut self, target: &Type) -> bool {
        let Some(global) = self.codegen.runtime_check_table(target) else {
            return false;
        };
        let base = self.codegen.wasm_type_index_for_any_cast(target);
        let local = self.add_local(ValType::Ref(RefType::ANYREF));
        self.instruction(Instruction::LocalTee(local));
        self.instruction(Instruction::RefTestNonNull(HeapType::Concrete(base)));
        self.emit_if_block(BlockType::Result(ValType::I32));
        self.instruction(Instruction::GlobalGet(global));
        self.instruction(Instruction::LocalGet(local));
        self.instruction(Instruction::RefCastNonNull(HeapType::Concrete(base)));
        self.instruction(Instruction::StructGet {
            struct_type_index: base,
            field_index: 0,
        });
        self.instruction(Instruction::ArrayGetU(super::super::ARRAY_I8_TYPE_INDEX));
        self.instruction(Instruction::Else);
        self.instruction(Instruction::I32Const(0));
        self.emit_end_block();
        true
    }

    pub(super) fn enforce_reified_cast(&mut self, target: &Type) {
        if self.codegen.runtime_check_table(target).is_none() {
            return;
        }
        let local = self.add_local(ValType::Ref(RefType::ANYREF));
        self.instruction(Instruction::LocalTee(local));
        self.emit_reified_type_test(target);
        self.instruction(Instruction::I32Eqz);
        self.emit_if_block(BlockType::Empty);
        self.instruction(Instruction::Unreachable);
        self.emit_end_block();
        self.instruction(Instruction::LocalGet(local));
    }
}
