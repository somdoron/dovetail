//! Opaque three-slot readonly views. Primitive reads stay native until erasure.
use super::{ExprContext, FunctionEmitter};
use crate::typechecker::types::{IntrinsicKind, Type, TypedExpr};
use wasm_encoder::{BlockType, HeapType, Instruction as I, ValType};

impl FunctionEmitter<'_> {
    pub(super) fn emit_readonly_slice(
        &mut self,
        kind: &IntrinsicKind,
        args: &[TypedExpr],
        expr: &TypedExpr,
        context: ExprContext,
    ) {
        match kind {
            IntrinsicKind::ReadonlySliceMake => self.readonly_make(args),
            IntrinsicKind::ReadonlySliceLength => {
                let view = self.readonly_view(&args[0]);
                self.instruction(I::LocalGet(view + 2));
            }
            IntrinsicKind::ReadonlySliceGet => self.readonly_get(args, &expr.ty),
            IntrinsicKind::ReadonlySliceSlice => self.readonly_slice(args),
            IntrinsicKind::ReadonlySliceCopyTo => self.readonly_copy(args),
            _ => unreachable!("readonly slice intrinsic"),
        }
        self.drop_if_statement(context, &expr.ty);
    }

    fn readonly_make(&mut self, args: &[TypedExpr]) {
        let array = self.readonly_argument(&args[0]);
        let start = self.readonly_argument(&args[1]);
        let length = self.readonly_argument(&args[2]);
        self.readonly_bounds(array, start, length);
        for local in [array, start, length] {
            self.instruction(I::LocalGet(local));
        }
    }

    fn readonly_get(&mut self, args: &[TypedExpr], element: &Type) {
        let view = self.readonly_view(&args[0]);
        let index = self.readonly_argument(&args[1]);
        self.instruction(I::LocalGet(index));
        self.instruction(I::LocalGet(view + 2));
        self.instruction(I::I32LtU);
        self.readonly_require();
        self.instruction(I::LocalGet(index));
        self.instruction(I::LocalGet(view + 1));
        self.instruction(I::I32Add);
        self.instruction(I::LocalSet(index));
        self.readonly_read(view, index, element);
        if self.codegen.is_tuple(element) {
            self.emit_unbox_tuple(element);
        } else if self.codegen.is_uint128(element) {
            self.emit_unbox_uint128();
        }
    }

    fn readonly_slice(&mut self, args: &[TypedExpr]) {
        let view = self.readonly_view(&args[0]);
        let start = self.readonly_argument(&args[1]);
        let end = self.readonly_argument(&args[2]);
        for (left, right) in [(start, end), (end, view + 2)] {
            self.instruction(I::LocalGet(left));
            self.instruction(I::LocalGet(right));
            self.instruction(I::I32LeU);
            self.readonly_require();
        }
        self.instruction(I::LocalGet(view));
        self.instruction(I::LocalGet(view + 1));
        self.instruction(I::LocalGet(start));
        self.instruction(I::I32Add);
        self.instruction(I::LocalGet(end));
        self.instruction(I::LocalGet(start));
        self.instruction(I::I32Sub);
    }

    fn readonly_argument(&mut self, value: &TypedExpr) -> u32 {
        self.emit_expr(value, ExprContext::Value);
        let local = self.add_local(self.codegen.single_val_type(&value.ty));
        self.instruction(I::LocalSet(local));
        local
    }

    fn readonly_view(&mut self, value: &TypedExpr) -> u32 {
        self.emit_expr(value, ExprContext::Value);
        let base = self.add_local(self.codegen.single_val_type(&Type::Any));
        self.add_local(ValType::I32);
        self.add_local(ValType::I32);
        for offset in (0..3).rev() {
            self.instruction(I::LocalSet(base + offset));
        }
        base
    }

    fn readonly_require(&mut self) {
        self.instruction(I::I32Eqz);
        self.emit_if_block(BlockType::Empty);
        self.instruction(I::Unreachable);
        self.emit_end_block();
    }

    fn readonly_bounds(&mut self, array: u32, start: u32, length: u32) {
        for value in [start, length] {
            self.instruction(I::LocalGet(value));
            self.instruction(I::I32Const(0));
            self.instruction(I::I32GeS);
            self.readonly_require();
        }
        self.instruction(I::LocalGet(start));
        self.instruction(I::LocalGet(array));
        self.instruction(I::ArrayLen);
        self.instruction(I::I32LeU);
        self.readonly_require();
        self.instruction(I::LocalGet(length));
        self.instruction(I::LocalGet(array));
        self.instruction(I::ArrayLen);
        self.instruction(I::LocalGet(start));
        self.instruction(I::I32Sub);
        self.instruction(I::I32LeU);
        self.readonly_require();
    }

    /// Leaves the element in its single-slot representation, without boxing a
    /// known primitive. Tuple and Uint128 callers can then expand their results.
    fn readonly_read(&mut self, array: u32, index: u32, element: &Type) {
        let element = super::super::Codegen::array_element_type(element);
        if matches!(
            element,
            Type::Any | Type::TypeVariable(..) | Type::GenericParam(..)
        ) {
            self.readonly_read_erased(array, index);
            return;
        }
        let storage = self.codegen.array_type_index(element);
        self.instruction(I::LocalGet(array));
        self.instruction(I::RefCastNonNull(HeapType::Concrete(storage)));
        self.instruction(I::LocalGet(index));
        self.emit_array_element_get(element, storage);
    }

    fn readonly_read_erased(&mut self, array: u32, index: u32) {
        for (element, storage) in super::super::ARRAY_ELEMENT_TYPES {
            let storage = *storage;
            self.instruction(I::LocalGet(array));
            self.instruction(I::RefTestNonNull(HeapType::Concrete(storage)));
            self.emit_if_block(BlockType::Result(self.codegen.single_val_type(&Type::Any)));
            self.instruction(I::LocalGet(array));
            self.instruction(I::RefCastNonNull(HeapType::Concrete(storage)));
            self.instruction(I::LocalGet(index));
            self.emit_array_element_get(element, storage);
            if !matches!(element, Type::Any | Type::Uint128) {
                self.emit_box_to_any(element);
            }
            self.instruction(I::Else);
        }
        self.instruction(I::Unreachable);
        for _ in super::super::ARRAY_ELEMENT_TYPES {
            self.emit_end_block();
        }
    }

    fn readonly_copy(&mut self, args: &[TypedExpr]) {
        let source = self.readonly_view(&args[0]);
        let destination = self.readonly_view(&args[1]);
        let element = match &args[1].ty {
            Type::GenericNewtype {
                type_args: type_params,
                ..
            } => &type_params[0].1,
            _ => unreachable!("destination slice type"),
        };
        let storage = self.codegen.array_type_index(element);
        self.instruction(I::LocalGet(source + 2));
        self.instruction(I::LocalGet(destination + 2));
        self.instruction(I::I32LeU);
        self.readonly_require();
        self.instruction(I::LocalGet(source));
        self.instruction(I::RefTestNonNull(HeapType::Concrete(storage)));
        self.emit_if_block(BlockType::Empty);
        self.instruction(I::LocalGet(destination));
        self.instruction(I::RefCastNonNull(HeapType::Concrete(storage)));
        self.instruction(I::LocalGet(destination + 1));
        self.instruction(I::LocalGet(source));
        self.instruction(I::RefCastNonNull(HeapType::Concrete(storage)));
        self.instruction(I::LocalGet(source + 1));
        self.instruction(I::LocalGet(source + 2));
        self.instruction(I::ArrayCopy {
            array_type_index_dst: storage,
            array_type_index_src: storage,
        });
        self.instruction(I::Else);
        self.readonly_copy_converted(source, destination, element);
        self.emit_end_block();
        self.instruction(I::I32Const(0));
    }

    fn readonly_copy_converted(&mut self, source: u32, destination: u32, element: &Type) {
        let index = self.add_local(ValType::I32);
        let source_index = self.add_local(ValType::I32);
        let storage = self.codegen.array_type_index(element);
        self.instruction(I::I32Const(0));
        self.instruction(I::LocalSet(index));
        self.emit_while_block();
        self.instruction(I::LocalGet(index));
        self.instruction(I::LocalGet(source + 2));
        self.instruction(I::I32GeU);
        self.instruction(I::BrIf(1));
        self.instruction(I::LocalGet(source + 1));
        self.instruction(I::LocalGet(index));
        self.instruction(I::I32Add);
        self.instruction(I::LocalSet(source_index));
        self.instruction(I::LocalGet(destination));
        self.instruction(I::RefCastNonNull(HeapType::Concrete(storage)));
        self.instruction(I::LocalGet(destination + 1));
        self.instruction(I::LocalGet(index));
        self.instruction(I::I32Add);
        self.readonly_read(source, source_index, element);
        self.instruction(I::ArraySet(storage));
        self.instruction(I::LocalGet(index));
        self.instruction(I::I32Const(1));
        self.instruction(I::I32Add);
        self.instruction(I::LocalSet(index));
        self.instruction(I::Br(0));
        self.emit_end_while_block();
    }
}
