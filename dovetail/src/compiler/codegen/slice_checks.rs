//! Validate bounded views when recovering their flattened values from erased storage.

use wasm_encoder::{BlockType, HeapType, Instruction as I, RefType, ValType};

use super::Codegen;
use crate::common::types::Fqn;
use crate::typechecker::types::Type;

impl Codegen<'_> {
    pub(super) fn slice_offsets(&self, ty: &Type) -> Vec<(u32, Option<u32>)> {
        let mut offsets = Vec::new();
        self.collect_slice_offsets(ty, 0, &mut offsets);
        offsets
    }

    fn collect_slice_offsets(&self, ty: &Type, base: u32, offsets: &mut Vec<(u32, Option<u32>)>) {
        match ty {
            Type::GenericNewtype {
                fqn,
                type_args: type_params,
                ..
            } if ["standard.prelude.Slice", "standard.prelude.ReadonlySlice"]
                .iter()
                .any(|name| Some(fqn.clone()) == Fqn::from_dotted(name)) =>
            {
                let element = Self::array_element_type(&type_params[0].1);
                let storage = if matches!(
                    element,
                    Type::Any | Type::TypeVariable(..) | Type::GenericParam(..)
                ) {
                    None
                } else {
                    Some(self.array_type_index(element))
                };
                offsets.push((base, storage));
            }
            Type::Newtype(_, inner)
            | Type::GenericNewtype {
                concrete_inner_type: inner,
                ..
            } => self.collect_slice_offsets(inner, base, offsets),
            Type::Tuple(elements, _) => {
                let mut offset = base;
                for element in elements {
                    self.collect_slice_offsets(element, offset, offsets);
                    offset += self.flat_width(element);
                }
            }
            _ => {}
        }
    }

    /// A boolean predicate over already type-checked data/start/length locals.
    /// Subtraction may wrap for an invalid start, but the other conjuncts reject it.
    fn slice_bounds_instrs(
        array: u32,
        start: u32,
        length: u32,
        storage: Option<u32>,
    ) -> Vec<I<'static>> {
        let heap = storage
            .map(HeapType::Concrete)
            .unwrap_or(HeapType::Abstract {
                shared: false,
                ty: wasm_encoder::AbstractHeapType::Array,
            });
        vec![
            I::LocalGet(array),
            I::RefTestNonNull(heap),
            I::If(BlockType::Result(ValType::I32)),
            I::LocalGet(start),
            I::I32Const(0),
            I::I32GeS,
            I::LocalGet(length),
            I::I32Const(0),
            I::I32GeS,
            I::I32And,
            I::LocalGet(start),
            I::LocalGet(array),
            I::RefCastNonNull(heap),
            I::ArrayLen,
            I::I32LeU,
            I::I32And,
            I::LocalGet(length),
            I::LocalGet(array),
            I::RefCastNonNull(heap),
            I::ArrayLen,
            I::LocalGet(start),
            I::I32Sub,
            I::I32LeU,
            I::I32And,
            I::Else,
            I::I32Const(0),
            I::End,
        ]
    }

    fn spill_flat_values(base: u32, count: usize) -> impl Iterator<Item = I<'static>> {
        (0..count as u32).rev().map(move |i| I::LocalSet(base + i))
    }

    pub(super) fn validate_unboxed_slices(
        &self,
        ty: &Type,
        base: u32,
        instrs: &mut Vec<I<'static>>,
        temps: &mut Vec<ValType>,
    ) {
        let offsets = self.slice_offsets(ty);
        if offsets.is_empty() {
            return;
        }
        let values_base = base + temps.len() as u32;
        let valtypes = self.type_to_valtypes(ty);
        instrs.extend(Self::spill_flat_values(values_base, valtypes.len()));
        temps.extend(valtypes.iter().copied());
        for (offset, storage) in offsets {
            let array = values_base + offset;
            instrs.extend(Self::slice_bounds_instrs(
                array,
                array + 1,
                array + 2,
                storage,
            ));
            instrs.extend([I::I32Eqz, I::If(BlockType::Empty), I::Unreachable, I::End]);
        }
        instrs.extend((0..valtypes.len() as u32).map(|i| I::LocalGet(values_base + i)));
    }

    /// Nontrapping test for a tuple containing slices. Check every leaf's runtime
    /// representation before decoding it, then check the views' bounds. The block
    /// returns false at the first mismatch, so `is` and pattern matching agree.
    pub(super) fn tuple_slice_test_instrs(
        &self,
        ty: &Type,
        base: u32,
    ) -> (Vec<I<'static>>, Vec<ValType>) {
        let tuple_index = self.tuple_struct_index(ty);
        let tuple_heap = HeapType::Concrete(tuple_index);
        let mut instrs = vec![I::LocalSet(base), I::Block(BlockType::Result(ValType::I32))];
        Self::test_or_false(
            &mut instrs,
            [I::LocalGet(base), I::RefTestNonNull(tuple_heap)],
        );
        for (i, leaf) in self.flatten_to_leaf_types(ty).iter().enumerate() {
            match leaf {
                Type::Any | Type::TypeVariable(..) | Type::GenericParam(..) => continue,
                Type::Never | Type::Error => {
                    Self::test_or_false(&mut instrs, [I::I32Const(0)]);
                    continue;
                }
                _ => {}
            }
            Self::test_or_false(
                &mut instrs,
                [
                    I::LocalGet(base),
                    I::RefCastNonNull(tuple_heap),
                    I::StructGet {
                        struct_type_index: tuple_index,
                        field_index: i as u32,
                    },
                    I::RefTestNonNull(HeapType::Concrete(self.wasm_type_index_for_any_cast(leaf))),
                ],
            );
        }
        let (unbox, unbox_temps) = self.tuple_unbox_fields_instrs(ty, base + 1);
        let mut temps = vec![ValType::Ref(RefType {
            nullable: true,
            heap_type: HeapType::ANY,
        })];
        temps.extend(unbox_temps);
        instrs.extend([I::LocalGet(base), I::RefCastNonNull(tuple_heap)]);
        instrs.extend(unbox);
        let values_base = base + temps.len() as u32;
        let valtypes = self.type_to_valtypes(ty);
        instrs.extend(Self::spill_flat_values(values_base, valtypes.len()));
        temps.extend(valtypes);
        for (offset, storage) in self.slice_offsets(ty) {
            let array = values_base + offset;
            Self::test_or_false(
                &mut instrs,
                Self::slice_bounds_instrs(array, array + 1, array + 2, storage),
            );
        }
        instrs.extend([I::I32Const(1), I::End]);
        (instrs, temps)
    }

    fn test_or_false(
        instrs: &mut Vec<I<'static>>,
        condition: impl IntoIterator<Item = I<'static>>,
    ) {
        instrs.push(I::I32Const(0));
        instrs.extend(condition);
        instrs.extend([I::I32Eqz, I::BrIf(0), I::Drop]);
    }
}
